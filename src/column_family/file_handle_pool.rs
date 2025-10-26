use crate::DatabaseError;
use crate::StorageBackend;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::unlocked_backend::UnlockedFileBackend;

/// Entry in the file handle pool tracking usage metadata.
struct PoolEntry {
    backend: Arc<dyn StorageBackend>,
    last_used: Instant,
}

impl PoolEntry {
    fn new(backend: Arc<dyn StorageBackend>) -> Self {
        Self {
            backend,
            last_used: Instant::now(),
        }
    }

    fn touch(&mut self) {
        self.last_used = Instant::now();
    }
}

/// Manages a pool of file handles for column families.
///
/// The pool maintains a fixed maximum number of open file descriptors and implements
/// LRU eviction when the pool is full. This allows unlimited column families while
/// keeping file descriptor usage bounded.
///
/// Each column family can acquire its own `FileBackend` to the same physical file,
/// enabling true concurrent writes through independent file descriptors.
pub struct FileHandlePool {
    path: PathBuf,
    max_size: usize,
    entries: Mutex<HashMap<String, PoolEntry>>,
}

impl FileHandlePool {
    /// Creates a new file handle pool.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the database file
    /// * `max_size` - Maximum number of file handles to keep open
    pub fn new(path: PathBuf, max_size: usize) -> Self {
        Self {
            path,
            max_size,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Acquires a file handle for the specified column family.
    ///
    /// If the column family already has an open handle, it is reused and its
    /// `last_used` timestamp is updated. If the pool is full, the least recently
    /// used handle is evicted.
    ///
    /// # Arguments
    ///
    /// * `cf_name` - Name of the column family requesting a handle
    ///
    /// # Returns
    ///
    /// An Arc-wrapped `StorageBackend` that the column family can use for I/O operations.
    pub fn acquire(&self, cf_name: &str) -> Result<Arc<dyn StorageBackend>, DatabaseError> {
        // Fast path: check if already exists (read-only, no eviction needed)
        {
            let mut entries = self.entries.lock().unwrap();
            if let Some(entry) = entries.get_mut(cf_name) {
                entry.touch();
                return Ok(entry.backend.clone());
            }
        }

        // Slow path: need to open a new file
        // Open file WITHOUT holding the lock to avoid serializing all threads
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)?;

        let backend: Arc<dyn StorageBackend> = Arc::new(UnlockedFileBackend::new(file)?);

        // Acquire lock only for the insert
        let mut entries = self.entries.lock().unwrap();

        // Double-check: another thread might have inserted while we were opening the file
        if let Some(entry) = entries.get_mut(cf_name) {
            entry.touch();
            return Ok(entry.backend.clone());
        }

        if entries.len() >= self.max_size {
            Self::evict_lru(&mut entries, cf_name);
        }

        entries.insert(cf_name.to_string(), PoolEntry::new(backend.clone()));
        Ok(backend)
    }

    /// Updates the `last_used` timestamp for a column family's handle.
    ///
    /// This should be called when a column family's `Database` is reused to prevent
    /// premature eviction.
    pub fn touch(&self, cf_name: &str) {
        // Minimize lock hold time - just update timestamp
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(cf_name) {
            entry.touch();
        }
        // Lock released immediately
    }

    /// Explicitly releases a column family's file handle.
    ///
    /// This is optional and allows manual control over when handles are released.
    /// If not called, handles are released automatically via LRU eviction.
    pub fn release(&self, cf_name: &str) {
        let mut entries = self.entries.lock().unwrap();
        entries.remove(cf_name);
    }

    /// Returns the current number of open file handles.
    pub fn len(&self) -> usize {
        let entries = self.entries.lock().unwrap();
        entries.len()
    }

    /// Returns true if the pool has no open handles.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the maximum pool size.
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Evicts the least recently used entry from the pool.
    ///
    /// The entry being acquired (`cf_name`) is never evicted, even if it would be
    /// the LRU candidate.
    fn evict_lru(entries: &mut HashMap<String, PoolEntry>, exclude: &str) {
        let mut lru_name: Option<String> = None;
        let mut lru_time = Instant::now();

        for (name, entry) in entries.iter() {
            if name == exclude {
                continue;
            }

            if lru_name.is_none() || entry.last_used < lru_time {
                lru_name = Some(name.clone());
                lru_time = entry.last_used;
            }
        }

        if let Some(name) = lru_name {
            entries.remove(&name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_pool_creation() {
        let tmpfile = NamedTempFile::new().unwrap();
        std::fs::write(tmpfile.path(), b"test").unwrap();

        let pool = FileHandlePool::new(tmpfile.path().to_path_buf(), 10);
        assert_eq!(pool.max_size(), 10);
        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());
    }

    #[test]
    fn test_acquire_creates_new_handle() {
        let tmpfile = NamedTempFile::new().unwrap();
        std::fs::write(tmpfile.path(), b"test").unwrap();

        let pool = FileHandlePool::new(tmpfile.path().to_path_buf(), 10);

        let handle1 = pool.acquire("cf1").unwrap();
        assert_eq!(pool.len(), 1);
        assert!(!pool.is_empty());

        let handle2 = pool.acquire("cf2").unwrap();
        assert_eq!(pool.len(), 2);

        assert!(!Arc::ptr_eq(&handle1, &handle2));
    }

    #[test]
    fn test_acquire_reuses_existing_handle() {
        let tmpfile = NamedTempFile::new().unwrap();
        std::fs::write(tmpfile.path(), b"test").unwrap();

        let pool = FileHandlePool::new(tmpfile.path().to_path_buf(), 10);

        let handle1 = pool.acquire("cf1").unwrap();
        let handle2 = pool.acquire("cf1").unwrap();

        assert_eq!(pool.len(), 1);
        assert!(Arc::ptr_eq(&handle1, &handle2));
    }

    #[test]
    fn test_touch_updates_timestamp() {
        let tmpfile = NamedTempFile::new().unwrap();
        std::fs::write(tmpfile.path(), b"test").unwrap();

        let pool = FileHandlePool::new(tmpfile.path().to_path_buf(), 10);

        pool.acquire("cf1").unwrap();

        let entries = pool.entries.lock().unwrap();
        let time1 = entries.get("cf1").unwrap().last_used;
        drop(entries);

        std::thread::sleep(std::time::Duration::from_millis(10));
        pool.touch("cf1");

        let entries = pool.entries.lock().unwrap();
        let time2 = entries.get("cf1").unwrap().last_used;

        assert!(time2 > time1);
    }

    #[test]
    fn test_release_removes_handle() {
        let tmpfile = NamedTempFile::new().unwrap();
        std::fs::write(tmpfile.path(), b"test").unwrap();

        let pool = FileHandlePool::new(tmpfile.path().to_path_buf(), 10);

        pool.acquire("cf1").unwrap();
        assert_eq!(pool.len(), 1);

        pool.release("cf1");
        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());
    }

    #[test]
    fn test_lru_eviction() {
        let tmpfile = NamedTempFile::new().unwrap();
        std::fs::write(tmpfile.path(), b"test").unwrap();

        let pool = FileHandlePool::new(tmpfile.path().to_path_buf(), 3);

        pool.acquire("cf1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        pool.acquire("cf2").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        pool.acquire("cf3").unwrap();
        assert_eq!(pool.len(), 3);

        pool.acquire("cf4").unwrap();
        assert_eq!(pool.len(), 3);

        let entries = pool.entries.lock().unwrap();
        assert!(!entries.contains_key("cf1"));
        assert!(entries.contains_key("cf2"));
        assert!(entries.contains_key("cf3"));
        assert!(entries.contains_key("cf4"));
    }

    #[test]
    fn test_lru_respects_touch() {
        let tmpfile = NamedTempFile::new().unwrap();
        std::fs::write(tmpfile.path(), b"test").unwrap();

        let pool = FileHandlePool::new(tmpfile.path().to_path_buf(), 3);

        pool.acquire("cf1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        pool.acquire("cf2").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        pool.acquire("cf3").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        pool.touch("cf1");

        pool.acquire("cf4").unwrap();

        let entries = pool.entries.lock().unwrap();
        assert!(entries.contains_key("cf1"));
        assert!(!entries.contains_key("cf2"));
        assert!(entries.contains_key("cf3"));
        assert!(entries.contains_key("cf4"));
    }

    #[test]
    fn test_concurrent_acquire() {
        use std::sync::Arc;
        use std::thread;

        let tmpfile = NamedTempFile::new().unwrap();
        std::fs::write(tmpfile.path(), b"test").unwrap();

        let pool = Arc::new(FileHandlePool::new(tmpfile.path().to_path_buf(), 10));

        let mut handles = vec![];
        for i in 0..5 {
            let pool_clone = pool.clone();
            let handle = thread::spawn(move || {
                pool_clone.acquire(&format!("cf{i}")).unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(pool.len(), 5);
    }
}
