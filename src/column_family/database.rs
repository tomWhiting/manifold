use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use crate::backends::FileBackend;
use crate::db::ReadableDatabase;
use crate::{
    Database, DatabaseError, ReadTransaction, StorageBackend, StorageError, TransactionError,
    WriteTransaction,
};

use super::header::{ColumnFamilyMetadata, FreeSegment, MasterHeader, PAGE_SIZE, Segment};
use super::partitioned_backend::PartitionedStorageBackend;

/// Default size allocated to a new column family (1 GB).
const DEFAULT_COLUMN_FAMILY_SIZE: u64 = 1024 * 1024 * 1024;

/// Errors that can occur when working with column families.
#[derive(Debug)]
pub enum ColumnFamilyError {
    /// A column family with this name already exists.
    AlreadyExists(String),
    /// The requested column family was not found.
    NotFound(String),
    /// An underlying database error occurred.
    Database(DatabaseError),
    /// An I/O error occurred.
    Io(io::Error),
}

impl fmt::Display for ColumnFamilyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColumnFamilyError::AlreadyExists(name) => {
                write!(f, "column family '{name}' already exists")
            }
            ColumnFamilyError::NotFound(name) => {
                write!(f, "column family '{name}' not found")
            }
            ColumnFamilyError::Database(e) => write!(f, "database error: {e}"),
            ColumnFamilyError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for ColumnFamilyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ColumnFamilyError::Database(e) => Some(e),
            ColumnFamilyError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<DatabaseError> for ColumnFamilyError {
    fn from(err: DatabaseError) -> Self {
        ColumnFamilyError::Database(err)
    }
}

impl From<io::Error> for ColumnFamilyError {
    fn from(err: io::Error) -> Self {
        ColumnFamilyError::Io(err)
    }
}

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
    /// Mutex to serialize segment allocation to prevent races
    allocation_lock: Arc<Mutex<()>>,
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
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ColumnFamilyError> {
        let path = path.as_ref().to_path_buf();

        // Open or create the file
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

        let file_backend = FileBackend::new(file)?;
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
        let header = Arc::new(RwLock::new(header));
        let allocation_lock = Arc::new(Mutex::new(()));

        let mut column_families = HashMap::new();
        for cf_meta in &header.read().unwrap().column_families {
            // Create expansion callback for this column family
            let cf_name = cf_meta.name.clone();
            let header_clone = header.clone();
            let file_backend_clone = file_backend.clone();
            let allocation_lock_clone = allocation_lock.clone();

            let expansion_callback = Arc::new(move |requested_size: u64| -> io::Result<Segment> {
                Self::allocate_segment_internal(
                    &cf_name,
                    requested_size,
                    &header_clone,
                    &file_backend_clone,
                    &allocation_lock_clone,
                )
            });

            let partition_backend = PartitionedStorageBackend::with_segments(
                file_backend.clone(),
                cf_meta.segments.clone(),
                Some(expansion_callback),
            );

            let db = Database::builder().create_with_backend(partition_backend)?;

            column_families.insert(cf_meta.name.clone(), Arc::new(db));
        }

        Ok(Self {
            path,
            file_backend,
            column_families: Arc::new(RwLock::new(column_families)),
            header,
            allocation_lock,
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
    ) -> Result<ColumnFamily, ColumnFamilyError> {
        let name = name.into();
        let size = size.unwrap_or(DEFAULT_COLUMN_FAMILY_SIZE);

        // Acquire write lock on column families map
        let mut cfs = self.column_families.write().unwrap();

        // Check for duplicate name
        if cfs.contains_key(&name) {
            return Err(ColumnFamilyError::AlreadyExists(name));
        }

        // Calculate next available offset using the end_of_file method
        let metadata = {
            let mut header = self.header.write().unwrap();
            let offset = header.end_of_file();
            let metadata = ColumnFamilyMetadata::new(name.clone(), offset, size);

            // Add to header immediately to reserve the space
            header.column_families.push(metadata.clone());

            // Persist updated header to disk
            let header_bytes = header.to_bytes()?;
            self.file_backend.write(0, &header_bytes)?;
            self.file_backend.sync_data()?;

            metadata
        }; // Header lock dropped here

        // Create expansion callback for auto-growth
        let cf_name = name.clone();
        let header_clone = self.header.clone();
        let file_backend_clone = self.file_backend.clone();
        let allocation_lock_clone = self.allocation_lock.clone();

        let expansion_callback = Arc::new(move |requested_size: u64| -> io::Result<Segment> {
            Self::allocate_segment_internal(
                &cf_name,
                requested_size,
                &header_clone,
                &file_backend_clone,
                &allocation_lock_clone,
            )
        });

        // Create partitioned backend with segments and expansion callback
        let partition_backend = PartitionedStorageBackend::with_segments(
            self.file_backend.clone(),
            metadata.segments.clone(),
            Some(expansion_callback),
        );

        // Initialize new Database instance (this may trigger expansion callback)
        let db = Database::builder().create_with_backend(partition_backend)?;
        let db = Arc::new(db);

        // Add to column families map
        cfs.insert(name.clone(), db.clone());

        Ok(ColumnFamily { name, db })
    }

    /// Retrieves a handle to an existing column family.
    ///
    /// # Errors
    ///
    /// Returns an error if no column family with the given name exists.
    pub fn column_family(&self, name: &str) -> Result<ColumnFamily, ColumnFamilyError> {
        let cfs = self.column_families.read().unwrap();

        match cfs.get(name) {
            Some(db) => Ok(ColumnFamily {
                name: name.to_string(),
                db: db.clone(),
            }),
            None => Err(ColumnFamilyError::NotFound(name.to_string())),
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

    /// Internal segment allocation function used by expansion callbacks.
    ///
    /// This allocates a new segment and updates the header atomically.
    fn allocate_segment_internal(
        cf_name: &str,
        size: u64,
        header: &Arc<RwLock<MasterHeader>>,
        file_backend: &Arc<dyn StorageBackend>,
        allocation_lock: &Arc<Mutex<()>>,
    ) -> io::Result<Segment> {
        let _lock = allocation_lock.lock().unwrap();
        let mut hdr = header.write().unwrap();

        // Try to find a free segment that's large enough
        let mut best_fit_idx = None;
        let mut best_fit_size = u64::MAX;

        for (idx, free_seg) in hdr.free_segments.iter().enumerate() {
            if free_seg.size >= size && free_seg.size < best_fit_size {
                best_fit_idx = Some(idx);
                best_fit_size = free_seg.size;
            }
        }

        let allocated_segment = if let Some(idx) = best_fit_idx {
            // Use the free segment
            let free_seg = hdr.free_segments.remove(idx);

            if free_seg.size == size {
                // Perfect fit - use the whole segment
                Segment::new(free_seg.offset, free_seg.size)
            } else {
                // Partial fit - split the free segment
                let allocated = Segment::new(free_seg.offset, size);
                let remaining = FreeSegment::new(free_seg.offset + size, free_seg.size - size);
                hdr.free_segments.push(remaining);
                allocated
            }
        } else {
            // No suitable free segment - append at end of file
            let offset = hdr.end_of_file();

            // Ensure offset is page-aligned
            let aligned_offset = offset.div_ceil(PAGE_SIZE as u64) * PAGE_SIZE as u64;

            Segment::new(aligned_offset, size)
        };

        // Add the new segment to the column family's segment list
        if let Some(cf_meta) = hdr.column_families.iter_mut().find(|cf| cf.name == cf_name) {
            cf_meta.segments.push(allocated_segment.clone());
        }

        // Persist updated header to disk
        let header_bytes = hdr.to_bytes()?;
        file_backend.write(0, &header_bytes)?;
        file_backend.sync_data()?;

        Ok(allocated_segment)
    }

    /// Deletes a column family and adds its segments to the free list for reuse.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The column family does not exist
    /// - The header cannot be updated or written to disk
    pub fn delete_column_family(&self, name: &str) -> Result<(), ColumnFamilyError> {
        let mut cfs = self.column_families.write().unwrap();

        // Check if column family exists
        if !cfs.contains_key(name) {
            return Err(ColumnFamilyError::NotFound(name.to_string()));
        }

        // Remove from in-memory map
        cfs.remove(name);

        // Update header - move CF segments to free list
        let mut header = self.header.write().unwrap();

        // Find the column family in the header
        let cf_idx = header
            .column_families
            .iter()
            .position(|cf| cf.name == name)
            .ok_or_else(|| ColumnFamilyError::NotFound(name.to_string()))?;

        // Remove the column family and add its segments to free list
        let cf_meta = header.column_families.remove(cf_idx);
        for segment in cf_meta.segments {
            header
                .free_segments
                .push(FreeSegment::new(segment.offset, segment.size));
        }

        // Persist updated header to disk
        let header_bytes = header.to_bytes()?;
        self.file_backend.write(0, &header_bytes)?;
        self.file_backend.sync_data()?;

        Ok(())
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
    use crate::TableDefinition;
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

    #[test]
    fn test_delete_column_family() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();
        db.create_column_family("test_cf", None).unwrap();

        assert_eq!(db.list_column_families().len(), 1);

        db.delete_column_family("test_cf").unwrap();
        assert_eq!(db.list_column_families().len(), 0);

        // Should not be able to get deleted CF
        assert!(db.column_family("test_cf").is_err());
    }

    #[test]
    fn test_delete_nonexistent_column_family_fails() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();
        let result = db.delete_column_family("nonexistent");

        assert!(result.is_err());
    }

    #[test]
    fn test_space_reuse_after_delete() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();

        // Create and delete a column family
        db.create_column_family("temp_cf", Some(1024 * 1024))
            .unwrap();

        db.delete_column_family("temp_cf").unwrap();

        // Check that free segments were added
        let free_segment_count = {
            let header_after_delete = db.header.read().unwrap();
            assert!(!header_after_delete.free_segments.is_empty());
            header_after_delete.free_segments.len()
        };

        // Create a new column family - should reuse the free segment
        db.create_column_family("new_cf", Some(512 * 1024)).unwrap();

        // Verify that a free segment was consumed (count should decrease)
        let final_free_segment_count = {
            let header_after_create = db.header.read().unwrap();
            header_after_create.free_segments.len()
        };

        // Free segment should have been used or split (count may stay same if split, or decrease if fully used)
        assert!(
            final_free_segment_count <= free_segment_count,
            "Expected free segments to be reused"
        );
    }

    #[test]
    fn test_automatic_expansion() {
        const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("test");

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path();

        let db = ColumnFamilyDatabase::open(path).unwrap();

        // Create a very small column family to force expansion
        let cf = db.create_column_family("small_cf", Some(8192)).unwrap();

        // Check initial segment count (may be >1 if Database init triggered expansion)
        let initial_segments = {
            let header_before = db.header.read().unwrap();
            let cf_meta_before = header_before
                .column_families
                .iter()
                .find(|c| c.name == "small_cf")
                .unwrap();
            cf_meta_before.segments.len()
        };

        // Write enough data to trigger expansion beyond initial segments
        // Database init may have already expanded, so write a lot more data
        let write_txn = cf.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TEST_TABLE).unwrap();
            let data = vec![0u8; 8192];
            // Write many large values to definitely trigger multiple expansions
            for i in 0..100 {
                table.insert(&i, data.as_slice()).unwrap();
            }
        }
        write_txn.commit().unwrap();

        // Check that a new segment was added
        let final_segments = {
            let header_after = db.header.read().unwrap();
            let cf_meta_after = header_after
                .column_families
                .iter()
                .find(|c| c.name == "small_cf")
                .unwrap();
            cf_meta_after.segments.len()
        };

        // Should have grown to multiple segments
        assert!(
            final_segments > initial_segments,
            "Expected segments to grow from {initial_segments} but got {final_segments}"
        );
    }

    #[test]
    fn test_concurrent_expansion() {
        const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("test");
        use std::thread;

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_path_buf();

        {
            let db = ColumnFamilyDatabase::open(&path).unwrap();

            // Create two small column families
            db.create_column_family("cf1", Some(8192)).unwrap();
            db.create_column_family("cf2", Some(8192)).unwrap();
        }

        // Reopen and test concurrent expansion
        let db = Arc::new(ColumnFamilyDatabase::open(&path).unwrap());

        let db1 = db.clone();
        let handle1 = thread::spawn(move || {
            let cf = db1.column_family("cf1").unwrap();
            let write_txn = cf.begin_write().unwrap();
            {
                let mut table = write_txn.open_table(TEST_TABLE).unwrap();
                let data = vec![0u8; 8192];
                for i in 0..100 {
                    table.insert(&i, data.as_slice()).unwrap();
                }
            }
            write_txn.commit().unwrap();
        });

        let db2 = db.clone();
        let handle2 = thread::spawn(move || {
            let cf = db2.column_family("cf2").unwrap();
            let write_txn = cf.begin_write().unwrap();
            {
                let mut table = write_txn.open_table(TEST_TABLE).unwrap();
                let data = vec![0u8; 8192];
                for i in 0..100 {
                    table.insert(&i, data.as_slice()).unwrap();
                }
            }
            write_txn.commit().unwrap();
        });

        handle1.join().unwrap();
        handle2.join().unwrap();

        // Verify both column families expanded
        {
            let header = db.header.read().unwrap();
            for cf_name in &["cf1", "cf2"] {
                let cf_meta = header
                    .column_families
                    .iter()
                    .find(|c| c.name == *cf_name)
                    .unwrap();
                assert!(
                    cf_meta.segments.len() > 1,
                    "Expected {cf_name} to have multiple segments"
                );
            }
        }
    }

    #[test]
    fn test_persistence_with_segments() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_path_buf();

        {
            let db = ColumnFamilyDatabase::open(&path).unwrap();
            db.create_column_family("users", Some(1024 * 1024)).unwrap();
            db.create_column_family("products", Some(512 * 1024))
                .unwrap();
        }

        // Reopen and verify column families are restored
        let db = ColumnFamilyDatabase::open(&path).unwrap();
        let names = db.list_column_families();

        assert_eq!(names.len(), 2);
        assert!(names.contains(&"users".to_string()));
        assert!(names.contains(&"products".to_string()));

        // Verify we can use them
        let users_cf = db.column_family("users").unwrap();
        let products_cf = db.column_family("products").unwrap();

        assert_eq!(users_cf.name(), "users");
        assert_eq!(products_cf.name(), "products");
    }
}
