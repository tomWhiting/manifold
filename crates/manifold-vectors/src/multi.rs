//! Multi-vector storage for token-level embeddings.
use manifold::{
    ReadOnlyTable, ReadTransaction, ReadableTableMetadata, StorageError, Table, TableDefinition,
    TableError, WriteTransaction,
};
use uuid::Uuid;

/// Table for storing multi-vectors (sequences of vectors)
pub struct MultiVectorTable<'txn, const DIM: usize> {
    table: Table<'txn, Uuid, Vec<[f32; DIM]>>,
}

impl<'txn, const DIM: usize> MultiVectorTable<'txn, DIM> {
    /// Opens a multi-vector table for writing
    pub fn open(txn: &'txn WriteTransaction, name: &str) -> Result<Self, TableError> {
        let def: TableDefinition<Uuid, Vec<[f32; DIM]>> = TableDefinition::new(name);
        let table = txn.open_table(def)?;
        Ok(Self { table })
    }

    /// Inserts a sequence of vectors
    pub fn insert(&mut self, key: &Uuid, vectors: &[[f32; DIM]]) -> Result<(), TableError> {
        self.table.insert(key, &vectors.to_vec())?;
        Ok(())
    }

    /// Returns the number of entries stored
    pub fn len(&self) -> Result<u64, StorageError> {
        self.table.len()
    }

    /// Returns true if the table is empty
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }
}

/// Read-only multi-vector table
pub struct MultiVectorTableRead<const DIM: usize> {
    table: ReadOnlyTable<Uuid, Vec<[f32; DIM]>>,
}

impl<const DIM: usize> MultiVectorTableRead<DIM> {
    /// Opens a multi-vector table for reading
    pub fn open(txn: &ReadTransaction, name: &str) -> Result<Self, StorageError> {
        let def: TableDefinition<Uuid, Vec<[f32; DIM]>> = TableDefinition::new(name);
        let table = txn.open_table(def).map_err(|e| match e {
            TableError::Storage(s) => s,
            _ => StorageError::Io(std::io::Error::other(e)),
        })?;
        Ok(Self { table })
    }

    /// Retrieves a sequence of vectors by key
    pub fn get(&self, key: &Uuid) -> Result<Option<Vec<[f32; DIM]>>, StorageError> {
        Ok(self.table.get(key)?.map(|guard| guard.value().clone()))
    }

    /// Returns the number of entries stored
    pub fn len(&self) -> Result<u64, StorageError> {
        self.table.len()
    }

    /// Returns true if the table is empty
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }
}
