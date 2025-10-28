//! Dense fixed-dimension vector storage with efficient access.

use manifold::{
    AccessGuard, ReadOnlyTable, ReadTransaction, ReadableTable, ReadableTableMetadata, 
    StorageError, Table, TableDefinition, TableError, WriteTransaction,
};
use std::ops::Deref;

/// A table storing fixed-dimension dense vectors.
pub struct VectorTable<'txn, const DIM: usize> {
    table: Table<'txn, &'static str, [f32; DIM]>,
}

impl<'txn, const DIM: usize> VectorTable<'txn, DIM> {
    /// Opens a vector table for writing.
    pub fn open(txn: &'txn WriteTransaction, name: &str) -> Result<Self, TableError> {
        let def: TableDefinition<&str, [f32; DIM]> = TableDefinition::new(name);
        let table = txn.open_table(def)?;
        Ok(Self { table })
    }

    /// Inserts a vector with the given key.
    pub fn insert(&mut self, key: &str, vector: &[f32; DIM]) -> Result<(), TableError> {
        self.table.insert(key, vector)?;
        Ok(())
    }

    /// Inserts multiple vectors in a single batch operation.
    pub fn insert_batch(&mut self, items: Vec<(&str, [f32; DIM])>, sorted: bool) -> Result<(), StorageError> {
        self.table.insert_bulk(items, sorted)?;
        Ok(())
    }

    /// Returns the number of vectors stored in this table.
    pub fn len(&self) -> Result<u64, StorageError> {
        self.table.len()
    }

    /// Returns `true` if the table contains no vectors.
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }
}

/// Read-only vector table providing efficient access.
///
/// This table leverages Manifold's fixed-width Value trait for arrays,
/// which deserializes directly from memory-mapped pages.
pub struct VectorTableRead<const DIM: usize> {
    table: ReadOnlyTable<&'static str, [f32; DIM]>,
}

impl<const DIM: usize> VectorTableRead<DIM> {
    /// Opens a vector table for reading.
    pub fn open(txn: &ReadTransaction, name: &str) -> Result<Self, StorageError> {
        let def: TableDefinition<&str, [f32; DIM]> = TableDefinition::new(name);
        let table = txn.open_table(def).map_err(|e| match e {
            TableError::Storage(s) => s,
            _ => StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)),
        })?;
        Ok(Self { table })
    }

    /// Retrieves a vector by key.
    ///
    /// Returns a guard that holds the vector data cached from deserialization.
    /// The vector is deserialized once when the guard is created.
    pub fn get(&self, key: &str) -> Result<Option<VectorGuard<'_, DIM>>, StorageError> {
        Ok(self.table.get(key)?.map(VectorGuard::new))
    }

    /// Returns the number of vectors stored in this table.
    pub fn len(&self) -> Result<u64, StorageError> {
        self.table.len()
    }

    /// Returns `true` if the table contains no vectors.
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }

    /// Iterates over all vectors in the table.
    pub fn iter(&self) -> Result<VectorIter<'_, DIM>, StorageError> {
        Ok(VectorIter { inner: self.table.iter()? })
    }
}

/// A guard providing access to a stored vector.
///
/// The vector data is deserialized once when the guard is created,
/// then cached for subsequent accesses.
pub struct VectorGuard<'a, const DIM: usize> {
    value_cached: [f32; DIM],
    _guard: AccessGuard<'a, [f32; DIM]>,
}

impl<'a, const DIM: usize> VectorGuard<'a, DIM> {
    fn new(guard: AccessGuard<'a, [f32; DIM]>) -> Self {
        let value_cached = guard.value();
        Self { value_cached, _guard: guard }
    }

    /// Returns a reference to the vector data.
    pub fn value(&self) -> &[f32; DIM] {
        &self.value_cached
    }

    /// Returns the vector data as a slice.
    pub fn as_slice(&self) -> &[f32] {
        &self.value_cached
    }
}

impl<'a, const DIM: usize> Deref for VectorGuard<'a, DIM> {
    type Target = [f32; DIM];

    fn deref(&self) -> &Self::Target {
        &self.value_cached
    }
}

/// Iterator over vectors in a `VectorTableRead`.
pub struct VectorIter<'a, const DIM: usize> {
    inner: manifold::Range<'a, &'static str, [f32; DIM]>,
}

impl<'a, const DIM: usize> Iterator for VectorIter<'a, DIM> {
    type Item = Result<(String, VectorGuard<'a, DIM>), StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|result| {
            result.map(|(key_guard, value_guard)| {
                (key_guard.value().to_string(), VectorGuard::new(value_guard))
            })
        })
    }
}
