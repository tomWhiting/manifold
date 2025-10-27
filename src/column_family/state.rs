use std::io;
use std::sync::{Arc, RwLock};

use crate::{Database, DatabaseError, StorageBackend};

#[cfg(not(target_arch = "wasm32"))]
use super::file_handle_pool::FileHandlePool;
use super::header::Segment;
use super::partitioned_backend::PartitionedStorageBackend;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// Internal state for a column family, supporting lazy Database initialization.
///
/// Column families are cheap to create (just header metadata), but Database instances
/// are only created when first accessed for writing. This allows unlimited column
/// families while keeping file descriptor usage bounded by the pool.
pub(crate) struct ColumnFamilyState {
    /// Name of this column family.
    pub name: String,
    /// Segments that make up this column family's storage.
    pub segments: Arc<RwLock<Vec<Segment>>>,
    /// Lazily initialized Database instance.
    pub db: Arc<RwLock<Option<Arc<Database>>>>,
}

impl ColumnFamilyState {
    /// Creates a new column family state with no Database instance.
    pub fn new(name: String, segments: Vec<Segment>) -> Self {
        Self {
            name,
            segments: Arc::new(RwLock::new(segments)),
            db: Arc::new(RwLock::new(None)),
        }
    }

    /// Ensures the Database instance exists, creating it if necessary (native platforms).
    ///
    /// This acquires a file handle from the pool and initializes the Database
    /// on first call. Subsequent calls reuse the cached instance and touch the
    /// pool to prevent eviction.
    ///
    /// # Arguments
    ///
    /// * `pool` - File handle pool to acquire backend from
    /// * `path` - Path to database file
    /// * `expansion_callback` - Callback to request new segments when needed
    ///
    /// # Returns
    ///
    /// An Arc-wrapped Database instance ready for use.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn ensure_database(
        &self,
        pool: &FileHandlePool,
        _path: &Path,
        expansion_callback: Arc<dyn Fn(u64) -> io::Result<Segment> + Send + Sync>,
    ) -> Result<Arc<Database>, DatabaseError> {
        {
            let db_guard = self.db.read().unwrap();
            if let Some(db) = db_guard.as_ref() {
                pool.touch(&self.name);
                return Ok(db.clone());
            }
        }

        let mut db_guard = self.db.write().unwrap();

        if let Some(db) = db_guard.as_ref() {
            pool.touch(&self.name);
            return Ok(db.clone());
        }

        let backend = pool.acquire(&self.name)?;
        let segments = self.segments.read().unwrap().clone();

        let partition_backend =
            PartitionedStorageBackend::with_segments(backend, segments, Some(expansion_callback));

        let db = Arc::new(Database::builder().create_with_backend(partition_backend)?);
        *db_guard = Some(db.clone());

        Ok(db)
    }

    /// Ensures the Database instance exists, creating it if necessary (WASM).
    ///
    /// This creates a Database using the provided WASM backend on first call.
    /// Subsequent calls reuse the cached instance.
    ///
    /// # Arguments
    ///
    /// * `backend` - WASM storage backend to use
    /// * `expansion_callback` - Callback to request new segments when needed
    ///
    /// # Returns
    ///
    /// An Arc-wrapped Database instance ready for use.
    #[cfg(target_arch = "wasm32")]
    pub fn ensure_database_wasm(
        &self,
        backend: &Arc<dyn StorageBackend>,
        expansion_callback: Arc<dyn Fn(u64) -> io::Result<Segment> + Send + Sync>,
    ) -> Result<Arc<Database>, DatabaseError> {
        {
            let db_guard = self.db.read().unwrap();
            if let Some(db) = db_guard.as_ref() {
                return Ok(db.clone());
            }
        }

        let mut db_guard = self.db.write().unwrap();

        if let Some(db) = db_guard.as_ref() {
            return Ok(db.clone());
        }

        let segments = self.segments.read().unwrap().clone();

        let partition_backend = PartitionedStorageBackend::with_segments(
            Arc::clone(backend),
            segments,
            Some(expansion_callback),
        );

        let db = Arc::new(Database::builder().create_with_backend(partition_backend)?);
        *db_guard = Some(db.clone());

        Ok(db)
    }

    /// Drops the Database instance, releasing its file handle back to the pool.
    ///
    /// This is called by the pool during LRU eviction. The next access will
    /// re-acquire a handle and recreate the Database.
    #[allow(dead_code)]
    pub fn evict_database(&self) {
        let mut db_guard = self.db.write().unwrap();
        *db_guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_creation() {
        let segments = vec![Segment::new(4096, 1024 * 1024)];
        let state = ColumnFamilyState::new("test_cf".to_string(), segments.clone());

        assert_eq!(state.name, "test_cf");
        assert_eq!(state.segments.read().unwrap().len(), 1);
        assert!(state.db.read().unwrap().is_none());
    }

    #[test]
    fn test_eviction() {
        let segments = vec![Segment::new(4096, 1024 * 1024)];
        let state = ColumnFamilyState::new("test_cf".to_string(), segments);

        assert!(state.db.read().unwrap().is_none());

        state.evict_database();

        assert!(state.db.read().unwrap().is_none());
    }
}
