//! Sparse vector storage using COO format.
use manifold::{
    ReadOnlyTable, ReadTransaction, ReadableTableMetadata, StorageError, Table, TableDefinition,
    TableError, WriteTransaction,
};
use uuid::Uuid;

/// A sparse vector represented as (index, value) pairs.
#[derive(Debug, Clone, PartialEq)]
pub struct SparseVector {
    /// Non-zero entries as (index, value) pairs
    pub entries: Vec<(u32, f32)>,
}

impl SparseVector {
    /// Creates a new sparse vector from entries
    pub fn new(mut entries: Vec<(u32, f32)>) -> Self {
        entries.sort_unstable_by_key(|(idx, _)| *idx);
        Self { entries }
    }

    /// Returns the number of non-zero entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if there are no entries
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Computes the dot product with another sparse vector
    pub fn dot(&self, other: &Self) -> f32 {
        let mut result = 0.0;
        let mut i = 0;
        let mut j = 0;
        while i < self.entries.len() && j < other.entries.len() {
            let (idx_a, val_a) = self.entries[i];
            let (idx_b, val_b) = other.entries[j];
            match idx_a.cmp(&idx_b) {
                std::cmp::Ordering::Equal => {
                    result += val_a * val_b;
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
            }
        }
        result
    }
}

/// Table for storing sparse vectors
pub struct SparseVectorTable<'txn> {
    table: Table<'txn, Uuid, Vec<(u32, f32)>>,
}

impl<'txn> SparseVectorTable<'txn> {
    /// Opens a sparse vector table for writing
    pub fn open(txn: &'txn WriteTransaction, name: &str) -> Result<Self, TableError> {
        let def: TableDefinition<Uuid, Vec<(u32, f32)>> = TableDefinition::new(name);
        let table = txn.open_table(def)?;
        Ok(Self { table })
    }

    /// Inserts a sparse vector
    pub fn insert(&mut self, key: &Uuid, vector: &SparseVector) -> Result<(), TableError> {
        self.table.insert(key, &vector.entries)?;
        Ok(())
    }

    /// Returns the number of vectors stored
    pub fn len(&self) -> Result<u64, StorageError> {
        self.table.len()
    }

    /// Returns true if the table is empty
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }
}

/// Read-only sparse vector table
pub struct SparseVectorTableRead {
    table: ReadOnlyTable<Uuid, Vec<(u32, f32)>>,
}

impl SparseVectorTableRead {
    /// Opens a sparse vector table for reading
    pub fn open(txn: &ReadTransaction, name: &str) -> Result<Self, StorageError> {
        let def: TableDefinition<Uuid, Vec<(u32, f32)>> = TableDefinition::new(name);
        let table = txn.open_table(def).map_err(|e| match e {
            TableError::Storage(s) => s,
            _ => StorageError::Io(std::io::Error::other(e)),
        })?;
        Ok(Self { table })
    }

    /// Retrieves a sparse vector by key
    pub fn get(&self, key: &Uuid) -> Result<Option<SparseVector>, StorageError> {
        Ok(self.table.get(key)?.map(|guard| SparseVector {
            entries: guard.value().clone(),
        }))
    }

    /// Returns the number of vectors stored
    pub fn len(&self) -> Result<u64, StorageError> {
        self.table.len()
    }

    /// Returns true if the table is empty
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }
}
