use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::backends::FileBackend;
use crate::db::ReadableDatabase;
use crate::{
    Database, DatabaseError, ReadTransaction, StorageBackend, StorageError, TransactionError,
    WriteTransaction,
};

use super::header::{ColumnFamilyMetadata, MasterHeader, PAGE_SIZE};
use super::partitioned_backend::PartitionedStorageBackend;

/// Default size allocated to a new column family (1 GB).
const DEFAULT_COLUMN_FAMILY_SIZE: u64 = 1024 * 1024 * 1024;

/// A database that manages multiple independent column families within a single file.
///
/// Each column family operates as a complete redb database with its own transaction
/// isolation, enabling concurrent writes to different column families while maintaining
/// ACID guarantees.
///
/// # Example
///
/// ```ignore
/// use redb::column_family::ColumnFamilyDatabase;
///
/// let db = ColumnFamilyDatabase::open("my_database.redb")?;
/// db.create_column_family("users", None)?;
/// db.create_column_family("products", None)?;
///
/// let users_cf = db.column_family("users")?;
/// let products_cf = db.column_family("products")?;
///
/// // Concurrent writes to different column families
/// std::thread::scope(|s| {
///     s.spawn(|| {
///         let txn = users_cf.begin_write()?;
///         // ... write user data
///         txn.commit()
///     });
///
///     s.spawn(|| {
///         let txn = products_cf.begin_write()?;
///         // ... write product data
///         txn.commit()
///     });
/// });
/// ```
pub struct ColumnFamilyDatabase {
    path: PathBuf,
    file_backend: Arc<dyn StorageBackend>,
    column_families: Arc<RwLock<HashMap<String, Arc<Database>>>>,
    header: Arc<RwLock<MasterHeader>>,
}

impl ColumnFamilyDatabase {
    /// Opens or creates a column family database at the specified path.
    ///
    /// If the file does not exist, it will be created with an empty master header.
    /// If the file exists, all column families defined in the master header will be opened.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, the header is invalid, or any
    /// column family cannot be initialized.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = path.as_ref().to_path_buf();

        // Open or create the file
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

        let file_backend =
            FileBackend::new(file)?;
        let file_backend: Arc<dyn StorageBackend> = Arc::new(file_backend);

        // Check if file is new (empty)
        let is_new = file_backend
            .len()
            .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?
            == 0;

        let header = if is_new {
            // Initialize new file with empty master header
            let header = MasterHeader::new();
            let header_bytes = header
                .to_bytes()
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

            // Write header to file
            file_backend
                .write(0, &header_bytes)
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;
            file_backend
                .sync_data()
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

            header
        } else {
            // Read existing header
            let mut header_bytes = vec![0u8; PAGE_SIZE];
            file_backend
                .read(0, &mut header_bytes)
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

            MasterHeader::from_bytes(&header_bytes)
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?
        };

        // Initialize column families from header
        let mut column_families = HashMap::new();
        for cf_meta in &header.column_families {
            let partition_backend =
                PartitionedStorageBackend::new(file_backend.clone(), cf_meta.offset, cf_meta.size);

            let db = Database::builder().create_with_backend(partition_backend)?;

            column_families.insert(cf_meta.name.clone(), Arc::new(db));
        }

        Ok(Self {
            path,
            file_backend,
            column_families: Arc::new(RwLock::new(column_families)),
            header: Arc::new(RwLock::new(header)),
        })
    }

    /// Creates a new column family with the specified name and optional size.
    ///
    /// The size parameter specifies the number of bytes to allocate for this column family.
    /// If `None`, a default size of 1GB is used.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A column family with this name already exists
    /// - The header cannot be updated or written to disk
    /// - The new Database instance cannot be initialized
    pub fn create_column_family(
        &self,
        name: impl Into<String>,
        size: Option<u64>,
    ) -> Result<ColumnFamily, DatabaseError> {
        let name = name.into();
        let size = size.unwrap_or(DEFAULT_COLUMN_FAMILY_SIZE);

        // Acquire write lock on column families map
        let mut cfs = self.column_families.write().unwrap();

        // Check for duplicate name
        if cfs.contains_key(&name) {
            return Err(DatabaseError::Storage(StorageError::Io(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("column family '{}' already exists", name),
            ))));
        }

        // Calculate next available offset
        let mut header = self.header.write().unwrap();
        let offset = if header.column_families.is_empty() {
            // First column family starts after the master header page
            PAGE_SIZE as u64
        } else {
            // Find the maximum end offset of existing column families
            header
                .column_families
                .iter()
                .map(|cf| cf.offset + cf.size)
                .max()
                .unwrap()
        };

        // Create metadata for new column family
        let metadata = ColumnFamilyMetadata::new(name.clone(), offset, size);

        // Create partitioned backend
        let partition_backend =
            PartitionedStorageBackend::new(self.file_backend.clone(), offset, size);

        // Initialize new Database instance
        let db = Database::builder().create_with_backend(partition_backend)?;
        let db = Arc::new(db);

        // Update master header
        header.column_families.push(metadata);

        // Persist updated header to disk
        let header_bytes = header
            .to_bytes()
            .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;
        self.file_backend
            .write(0, &header_bytes)
            .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;
        self.file_backend
            .sync_data()
            .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

        // Add to column families map
        cfs.insert(name.clone(), db.clone());

        Ok(ColumnFamily { name, db })
    }

    /// Retrieves a handle to an existing column family.
    ///
    /// # Errors
    ///
    /// Returns an error if no column family with the given name exists.
    pub fn column_family(&self, name: &str) -> Result<ColumnFamily, DatabaseError> {
        let cfs = self.column_families.read().unwrap();

        match cfs.get(name) {
            Some(db) => Ok(ColumnFamily {
                name: name.to_string(),
                db: db.clone(),
            }),
            None => Err(DatabaseError::Storage(StorageError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("column family '{}' not found", name),
            )))),
        }
    }

    /// Returns a list of all column family names in the database.
    pub fn list_column_families(&self) -> Vec<String> {
        let header = self.header.read().unwrap();
        header
            .column_families
            .iter()
            .map(|cf| cf.name.clone())
            .collect()
    }

    /// Returns the path to the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// A handle to a column family within a [`ColumnFamilyDatabase`].
///
/// This is a lightweight wrapper around a redb [`Database`] instance that can be
/// cheaply cloned and passed between threads. All clones refer to the same underlying
/// database instance.
///
/// Use [`begin_write`](Self::begin_write) and [`begin_read`](Self::begin_read) to
/// create transactions.
#[derive(Clone)]
pub struct ColumnFamily {
    name: String,
    db: Arc<Database>,
}

impl ColumnFamily {
    /// Returns the name of this column family.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Begins a write transaction for this column family.
    ///
    /// Only one write transaction may be active at a time per column family.
    /// Multiple write transactions to different column families can proceed concurrently.
    ///
    /// # Errors
    ///
    /// Returns an error if a write transaction is already in progress for this column family.
    pub fn begin_write(&self) -> Result<WriteTransaction, TransactionError> {
        self.db.begin_write()
    }

    /// Begins a read transaction for this column family.
    ///
    /// Multiple read transactions may be active concurrently, even with active write
    /// transactions, thanks to MVCC snapshot isolation.
    pub fn begin_read(&self) -> Result<ReadTransaction, TransactionError> {
        self.db.begin_read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_create_and_open_database() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();
        assert_eq!(db.list_column_families().len(), 0);
    }

    #[test]
    fn test_create_column_family() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();

        let cf = db.create_column_family("test_cf", None).unwrap();
        assert_eq!(cf.name(), "test_cf");

        let names = db.list_column_families();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "test_cf");
    }

    #[test]
    fn test_duplicate_column_family_fails() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();

        db.create_column_family("test_cf", None).unwrap();
        let result = db.create_column_family("test_cf", None);

        assert!(result.is_err());
    }

    #[test]
    fn test_get_column_family() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();
        db.create_column_family("test_cf", None).unwrap();

        let cf = db.column_family("test_cf").unwrap();
        assert_eq!(cf.name(), "test_cf");
    }

    #[test]
    fn test_get_nonexistent_column_family_fails() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();
        let result = db.column_family("nonexistent");

        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_column_families() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();

        db.create_column_family("users", Some(512 * 1024 * 1024))
            .unwrap();
        db.create_column_family("products", Some(256 * 1024 * 1024))
            .unwrap();
        db.create_column_family("orders", None).unwrap();

        let names = db.list_column_families();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"users".to_string()));
        assert!(names.contains(&"products".to_string()));
        assert!(names.contains(&"orders".to_string()));
    }

    #[test]
    fn test_reopen_database_preserves_column_families() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_path_buf();

        {
            let db = ColumnFamilyDatabase::open(&path).unwrap();
            db.create_column_family("persistent_cf", None).unwrap();
        }

        // Reopen the database
        let db = ColumnFamilyDatabase::open(&path).unwrap();
        let names = db.list_column_families();

        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "persistent_cf");

        // Should be able to get the column family
        let cf = db.column_family("persistent_cf").unwrap();
        assert_eq!(cf.name(), "persistent_cf");
    }

    #[test]
    fn test_column_family_clone() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();
        let cf1 = db.create_column_family("test_cf", None).unwrap();

        let cf2 = cf1.clone();
        assert_eq!(cf1.name(), cf2.name());
    }
}
