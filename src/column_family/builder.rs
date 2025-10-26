use std::path::Path;

use super::database::ColumnFamilyDatabase;
use crate::DatabaseError;

/// Default file handle pool size.
const DEFAULT_POOL_SIZE: usize = 32;

/// Builder for configuring and opening a column family database.
///
/// # Example
///
/// ```ignore
/// use manifold::column_family::ColumnFamilyDatabase;
///
/// let db = ColumnFamilyDatabase::builder()
///     .pool_size(64)
///     .open("my_database.manifold")?;
/// ```
pub struct ColumnFamilyDatabaseBuilder {
    pool_size: usize,
}

impl ColumnFamilyDatabaseBuilder {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self {
            pool_size: DEFAULT_POOL_SIZE,
        }
    }

    /// Sets the maximum number of file handles in the pool.
    ///
    /// The pool manages file descriptors for column families. A larger pool allows
    /// more column families to have active writes concurrently without eviction.
    ///
    /// Default: 32
    ///
    /// # Arguments
    ///
    /// * `size` - Maximum number of file handles to keep open
    #[must_use]
    pub fn pool_size(mut self, size: usize) -> Self {
        self.pool_size = size;
        self
    }

    /// Opens or creates a column family database at the specified path.
    ///
    /// If the file does not exist, it will be created with an empty master header.
    /// If the file exists, column families defined in the master header will be
    /// available for lazy initialization.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or the header is invalid.
    pub fn open(self, path: impl AsRef<Path>) -> Result<ColumnFamilyDatabase, DatabaseError> {
        let path = path.as_ref().to_path_buf();
        ColumnFamilyDatabase::open_with_builder(path, self.pool_size)
    }
}

impl Default for ColumnFamilyDatabaseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_builder_default() {
        let builder = ColumnFamilyDatabaseBuilder::new();
        assert_eq!(builder.pool_size, DEFAULT_POOL_SIZE);
    }

    #[test]
    fn test_builder_custom_pool_size() {
        let builder = ColumnFamilyDatabaseBuilder::new().pool_size(64);
        assert_eq!(builder.pool_size, 64);
    }

    #[test]
    fn test_builder_open() {
        let tmpfile = NamedTempFile::new().unwrap();
        let db = ColumnFamilyDatabaseBuilder::new()
            .pool_size(16)
            .open(tmpfile.path())
            .unwrap();

        assert_eq!(db.list_column_families().len(), 0);
    }
}
