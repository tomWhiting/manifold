use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::backends::FileBackend;
use crate::db::ReadableDatabase;
use crate::tree_store::BtreeHeader;
use crate::{
    Database, DatabaseError, ReadTransaction, StorageBackend, StorageError, TransactionError,
    WriteTransaction,
};

use super::builder::ColumnFamilyDatabaseBuilder;
use super::file_handle_pool::FileHandlePool;
use super::header::{ColumnFamilyMetadata, FreeSegment, MasterHeader, PAGE_SIZE, Segment};
use super::state::ColumnFamilyState;
use super::wal::checkpoint::CheckpointManager;
use super::wal::config::CheckpointConfig;
use super::wal::journal::WALJournal;

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
/// Column families are lazily initialized, so creating many column families is cheap.
/// File descriptors are only acquired when a column family is first written to, and
/// the pool manages eviction to keep file descriptor usage bounded.
///
/// # Example
///
/// ```ignore
/// use manifold::column_family::ColumnFamilyDatabase;
///
/// let db = ColumnFamilyDatabase::builder()
///     .pool_size(64)
///     .open("my_database.manifold")?;
///
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
    header_backend: Arc<FileBackend>,
    handle_pool: Arc<FileHandlePool>,
    column_families: Arc<RwLock<HashMap<String, Arc<ColumnFamilyState>>>>,
    header: Arc<RwLock<MasterHeader>>,
    wal_journal: Option<Arc<WALJournal>>,
    checkpoint_manager: Option<Arc<crate::column_family::wal::checkpoint::CheckpointManager>>,
}

impl ColumnFamilyDatabase {
    /// Returns a builder for configuring and opening a column family database.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let db = ColumnFamilyDatabase::builder()
    ///     .pool_size(64)
    ///     .open("my_database.manifold")?;
    /// ```
    pub fn builder() -> ColumnFamilyDatabaseBuilder {
        ColumnFamilyDatabaseBuilder::new()
    }

    /// Opens or creates a column family database at the specified path with default settings.
    ///
    /// This is equivalent to `ColumnFamilyDatabase::builder().open(path)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or the header is invalid.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        Self::builder().open(path)
    }

    /// Internal implementation of open, called by the builder.
    pub(crate) fn open_with_builder(
        path: PathBuf,
        pool_size: usize,
    ) -> Result<Self, DatabaseError> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

        let header_backend = Arc::new(FileBackend::new(file)?);

        let is_new = header_backend
            .len()
            .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?
            == 0;

        let header = if is_new {
            let header = MasterHeader::new();
            let header_bytes = header
                .to_bytes()
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

            header_backend
                .write(0, &header_bytes)
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;
            header_backend
                .sync_data()
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

            header
        } else {
            let mut header_bytes = vec![0u8; PAGE_SIZE];
            header_backend
                .read(0, &mut header_bytes)
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

            MasterHeader::from_bytes(&header_bytes)
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?
        };

        let handle_pool = Arc::new(FileHandlePool::new(path.clone(), pool_size));
        let header = Arc::new(RwLock::new(header));

        let mut column_families = HashMap::new();
        for cf_meta in &header.read().unwrap().column_families {
            let state = ColumnFamilyState::new(cf_meta.name.clone(), cf_meta.segments.clone());
            column_families.insert(cf_meta.name.clone(), Arc::new(state));
        }

        // Initialize WAL journal and perform recovery if needed
        let wal_journal = if pool_size > 0 {
            let wal_path = path.with_extension("wal");
            let journal = WALJournal::open(&wal_path)
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

            // Perform WAL recovery by reading all entries from the journal
            let entries = journal
                .read_from(0)
                .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

            if !entries.is_empty() {
                #[cfg(feature = "logging")]
                log::info!("WAL recovery: applying {} transactions", entries.len());

                // Create temporary database instance for recovery (without WAL to avoid recursion)
                let temp_db = Self {
                    path: path.clone(),
                    header_backend: Arc::clone(&header_backend),
                    handle_pool: Arc::clone(&handle_pool),
                    column_families: Arc::new(RwLock::new(column_families.clone())),
                    header: Arc::clone(&header),
                    wal_journal: None, // Important: no WAL during recovery to avoid appending during replay
                    checkpoint_manager: None, // No checkpoint manager during recovery
                };

                // Apply each WAL entry to the database
                for entry in &entries {
                    // Get or create the column family
                    let cf = temp_db.column_family(&entry.cf_name).map_err(|e| {
                        DatabaseError::Storage(StorageError::from(io::Error::new(
                            io::ErrorKind::NotFound,
                            format!(
                                "column family '{}' not found during recovery: {}",
                                entry.cf_name, e
                            ),
                        )))
                    })?;

                    // Get the Database instance
                    let cf_db = cf.ensure_database()?;

                    // Get the TransactionalMemory
                    let mem = cf_db.get_memory();

                    // Convert WAL payload to BtreeHeader format
                    let data_root =
                        entry
                            .payload
                            .user_root
                            .map(|(page_num, checksum, length)| BtreeHeader {
                                root: page_num,
                                checksum,
                                length,
                            });

                    let system_root =
                        entry
                            .payload
                            .system_root
                            .map(|(page_num, checksum, length)| BtreeHeader {
                                root: page_num,
                                checksum,
                                length,
                            });

                    // Apply the WAL transaction
                    mem.apply_wal_transaction(
                        data_root,
                        system_root,
                        crate::transaction_tracker::TransactionId::new(entry.transaction_id),
                    )?;
                }

                // Sync all column families to persist recovery
                for cf_name in temp_db.list_column_families() {
                    if let Ok(cf) = temp_db.column_family(&cf_name) {
                        if let Ok(cf_db) = cf.ensure_database() {
                            // Commit an empty transaction with durability to fsync
                            let mut txn = cf_db.begin_write().map_err(|e| {
                                DatabaseError::Storage(StorageError::from(io::Error::new(
                                    io::ErrorKind::Other,
                                    format!("recovery fsync begin_write failed: {e}"),
                                )))
                            })?;
                            txn.set_durability(crate::Durability::Immediate)
                                .map_err(|e| {
                                    DatabaseError::Storage(StorageError::from(io::Error::new(
                                        io::ErrorKind::Other,
                                        format!("recovery set_durability failed: {e}"),
                                    )))
                                })?;
                            txn.commit().map_err(|e| {
                                DatabaseError::Storage(StorageError::from(io::Error::new(
                                    io::ErrorKind::Other,
                                    format!("recovery commit failed: {e}"),
                                )))
                            })?;
                        }
                    }
                }

                // Truncate WAL after successful recovery
                let latest_seq = entries.last().unwrap().sequence;
                journal
                    .truncate(latest_seq + 1)
                    .map_err(|e| DatabaseError::Storage(StorageError::from(e)))?;

                #[cfg(feature = "logging")]
                log::info!("WAL recovery completed successfully");
            }

            Some(Arc::new(journal))
        } else {
            None
        };

        // Start checkpoint manager if WAL is enabled
        let checkpoint_manager = if let Some(ref journal_arc) = wal_journal {
            let config = CheckpointConfig {
                interval: std::time::Duration::from_secs(60),
                max_wal_size: 64 * 1024 * 1024,
            };

            // Create database Arc for checkpoint manager (temporary, will be replaced by self)
            let db_arc = Arc::new(Self {
                path: path.clone(),
                header_backend: Arc::clone(&header_backend),
                handle_pool: Arc::clone(&handle_pool),
                column_families: Arc::new(RwLock::new(column_families.clone())),
                header: Arc::clone(&header),
                wal_journal: Some(Arc::clone(journal_arc)),
                checkpoint_manager: None, // Will be set after creation
            });

            let manager = CheckpointManager::start(Arc::clone(journal_arc), db_arc, config);

            Some(Arc::new(manager))
        } else {
            None
        };

        Ok(Self {
            path,
            header_backend,
            handle_pool,
            column_families: Arc::new(RwLock::new(column_families)),
            header,
            wal_journal,
            checkpoint_manager,
        })
    }

    /// Creates a new column family with the specified name and optional size.
    ///
    /// The column family is created cheaply with no file descriptor allocated.
    /// The Database instance and file handle are lazily initialized on first write.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the column family
    /// * `size` - Initial size in bytes. If None, defaults to 1GB.
    ///
    /// # Errors
    ///
    /// Returns an error if a column family with this name already exists or
    /// the header cannot be updated.
    pub fn create_column_family(
        &self,
        name: impl Into<String>,
        size: Option<u64>,
    ) -> Result<ColumnFamily, ColumnFamilyError> {
        let name = name.into();
        let size = size.unwrap_or(DEFAULT_COLUMN_FAMILY_SIZE);

        let mut cfs = self.column_families.write().unwrap();

        if cfs.contains_key(&name) {
            return Err(ColumnFamilyError::AlreadyExists(name));
        }

        let segments = {
            let mut header = self.header.write().unwrap();
            let offset = header.end_of_file();
            let metadata = ColumnFamilyMetadata::new(name.clone(), offset, size);

            header.column_families.push(metadata.clone());

            let header_bytes = header.to_bytes()?;
            self.header_backend.write(0, &header_bytes)?;
            self.header_backend.sync_data()?;

            metadata.segments
        };

        let state = Arc::new(ColumnFamilyState::new(name.clone(), segments));

        let cf_name = name.clone();
        let cf_name_for_callback = cf_name.clone();
        let header_clone = self.header.clone();
        let header_backend_clone = self.header_backend.clone();

        let state_clone = state.clone();

        let expansion_callback = Arc::new(move |requested_size: u64| -> io::Result<Segment> {
            Self::allocate_segment_internal(
                &cf_name_for_callback,
                requested_size,
                &header_clone,
                &header_backend_clone,
                &state_clone,
            )
        });

        state
            .ensure_database(&self.handle_pool, &self.path, expansion_callback)
            .map_err(|e| match e {
                DatabaseError::Storage(StorageError::Io(io_err)) => ColumnFamilyError::Io(io_err),
                DatabaseError::Storage(s) => {
                    ColumnFamilyError::Io(io::Error::other(format!("storage error: {s}")))
                }
                _ => ColumnFamilyError::Io(io::Error::other(format!(
                    "failed to initialize column family: {e}"
                ))),
            })?;

        cfs.insert(name.clone(), state.clone());

        Ok(ColumnFamily {
            name: cf_name.clone(),
            state,
            pool: self.handle_pool.clone(),
            path: self.path.clone(),
            header: self.header.clone(),
            header_backend: self.header_backend.clone(),
            wal_journal: self.wal_journal.clone(),
            checkpoint_manager: self.checkpoint_manager.clone(),
        })
    }

    /// Retrieves a handle to an existing column family.
    ///
    /// The returned handle is lightweight and can be cloned cheaply.
    ///
    /// # Errors
    ///
    /// Returns an error if no column family with the given name exists.
    pub fn column_family(&self, name: &str) -> Result<ColumnFamily, ColumnFamilyError> {
        let cfs = self.column_families.read().unwrap();

        match cfs.get(name) {
            Some(state) => Ok(ColumnFamily {
                name: name.to_string(),
                state: state.clone(),
                pool: self.handle_pool.clone(),
                path: self.path.clone(),
                header: self.header.clone(),
                header_backend: self.header_backend.clone(),
                wal_journal: self.wal_journal.clone(),
                checkpoint_manager: self.checkpoint_manager.clone(),
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

    /// Deletes a column family and adds its segments to the free list for reuse.
    ///
    /// # Errors
    ///
    /// Returns an error if the column family does not exist or the header
    /// cannot be updated.
    pub fn delete_column_family(&self, name: &str) -> Result<(), ColumnFamilyError> {
        let mut cfs = self.column_families.write().unwrap();

        if !cfs.contains_key(name) {
            return Err(ColumnFamilyError::NotFound(name.to_string()));
        }

        cfs.remove(name);

        let mut header = self.header.write().unwrap();

        let cf_idx = header
            .column_families
            .iter()
            .position(|cf| cf.name == name)
            .ok_or_else(|| ColumnFamilyError::NotFound(name.to_string()))?;

        let cf_meta = header.column_families.remove(cf_idx);
        for segment in cf_meta.segments {
            header
                .free_segments
                .push(FreeSegment::new(segment.offset, segment.size));
        }

        let header_bytes = header.to_bytes()?;
        self.header_backend.write(0, &header_bytes)?;
        self.header_backend.sync_data()?;

        Ok(())
    }

    /// Internal segment allocation function used by expansion callbacks.
    fn allocate_segment_internal(
        cf_name: &str,
        size: u64,
        header: &Arc<RwLock<MasterHeader>>,
        header_backend: &Arc<FileBackend>,
        state: &Arc<ColumnFamilyState>,
    ) -> io::Result<Segment> {
        // Allocate segment from free list or end of file - keep lock minimal
        let allocated_segment = {
            let mut hdr = header.write().unwrap();

            let mut best_fit_idx = None;
            let mut best_fit_size = u64::MAX;

            for (idx, free_seg) in hdr.free_segments.iter().enumerate() {
                if free_seg.size >= size && free_seg.size < best_fit_size {
                    best_fit_idx = Some(idx);
                    best_fit_size = free_seg.size;
                }
            }

            let allocated_segment = if let Some(idx) = best_fit_idx {
                let free_seg = hdr.free_segments.remove(idx);

                if free_seg.size == size {
                    Segment::new(free_seg.offset, free_seg.size)
                } else {
                    let allocated = Segment::new(free_seg.offset, size);
                    let remaining = FreeSegment::new(free_seg.offset + size, free_seg.size - size);
                    hdr.free_segments.push(remaining);
                    allocated
                }
            } else {
                let offset = hdr.end_of_file();
                let aligned_offset = offset.div_ceil(PAGE_SIZE as u64) * PAGE_SIZE as u64;
                Segment::new(aligned_offset, size)
            };

            if let Some(cf_meta) = hdr.column_families.iter_mut().find(|cf| cf.name == cf_name) {
                cf_meta.segments.push(allocated_segment.clone());
            }

            allocated_segment
        }; // Header lock released here - no disk I/O while holding lock

        // Update state outside of header lock
        let mut state_segments = state.segments.write().unwrap();
        state_segments.push(allocated_segment.clone());

        // Don't write/fsync header on every allocation - eliminates serialization bottleneck
        // Header persisted on clean shutdown or periodically
        // Trade-off: crash may lose segment allocations (wasted space, not data loss)

        Ok(allocated_segment)
    }
}

/// A handle to a column family within a [`ColumnFamilyDatabase`].
///
/// This is a lightweight structure that can be cheaply cloned and passed between threads.
/// The underlying Database instance is lazily initialized on first write, acquiring a
/// file handle from the pool.
#[derive(Clone)]
pub struct ColumnFamily {
    name: String,
    state: Arc<ColumnFamilyState>,
    pool: Arc<FileHandlePool>,
    path: PathBuf,
    header: Arc<RwLock<MasterHeader>>,
    header_backend: Arc<FileBackend>,
    wal_journal: Option<Arc<WALJournal>>,
    checkpoint_manager: Option<Arc<CheckpointManager>>,
}

impl ColumnFamily {
    /// Returns the name of this column family.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Begins a write transaction for this column family.
    ///
    /// On first call, this acquires a file handle from the pool and initializes
    /// the Database instance. Subsequent calls reuse the cached instance.
    ///
    /// # Errors
    ///
    /// Returns an error if a write transaction is already in progress for this
    /// column family or if the Database cannot be initialized.
    pub fn begin_write(&self) -> Result<WriteTransaction, TransactionError> {
        let db = self.ensure_database().map_err(|e| match e {
            DatabaseError::Storage(s) => TransactionError::Storage(s),
            _ => TransactionError::Storage(StorageError::from(io::Error::other(format!(
                "database initialization error: {e}"
            )))),
        })?;

        let mut txn = db.begin_write()?;

        // Inject WAL context if enabled
        if let Some(wal_journal) = &self.wal_journal {
            txn.set_wal_context(
                self.name.clone(),
                Arc::clone(wal_journal),
                self.checkpoint_manager.as_ref().map(Arc::clone),
            );
        }

        Ok(txn)
    }

    /// Begins a read transaction for this column family.
    ///
    /// Multiple read transactions may be active concurrently.
    pub fn begin_read(&self) -> Result<ReadTransaction, TransactionError> {
        let db = self.ensure_database().map_err(|e| match e {
            DatabaseError::Storage(s) => TransactionError::Storage(s),
            _ => TransactionError::Storage(StorageError::from(io::Error::other(format!(
                "database initialization error: {e}"
            )))),
        })?;
        db.begin_read()
    }

    /// Ensures the Database instance exists, creating it if necessary.
    pub(crate) fn ensure_database(&self) -> Result<Arc<Database>, DatabaseError> {
        let name = self.name.clone();
        let header = self.header.clone();
        let header_backend = self.header_backend.clone();

        let state = self.state.clone();

        let expansion_callback = Arc::new(move |requested_size: u64| -> io::Result<Segment> {
            ColumnFamilyDatabase::allocate_segment_internal(
                &name,
                requested_size,
                &header,
                &header_backend,
                &state,
            )
        });

        self.state
            .ensure_database(&self.pool, &self.path, expansion_callback)
    }
}

impl Drop for ColumnFamilyDatabase {
    fn drop(&mut self) {
        // Shutdown checkpoint manager if it exists
        if let Some(checkpoint_mgr) = self.checkpoint_manager.take() {
            // Try to unwrap the Arc - if we're the last owner, we can shutdown gracefully
            if let Ok(manager) = Arc::try_unwrap(checkpoint_mgr) {
                let _ = manager.shutdown();
            }
            // If Arc::try_unwrap fails, other references exist and Drop on CheckpointManager
            // will handle shutdown when they're dropped
        }

        // Close the header backend to release the file lock
        let _ = self.header_backend.close();
    }
}
