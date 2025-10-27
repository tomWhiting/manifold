use super::entry::WALEntry;
use crate::StorageBackend;
#[cfg(not(target_arch = "wasm32"))]
use crate::tree_store::file_backend::FileBackend;
#[cfg(not(target_arch = "wasm32"))]
use std::fs::OpenOptions;
use std::io;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Magic number for WAL file identification.
const WAL_MAGIC: &[u8; 8] = b"REDB-WAL";

/// Current WAL format version.
const WAL_VERSION: u8 = 1;

/// Size of the WAL file header in bytes.
const WAL_HEADER_SIZE: usize = 512;

/// Batching window for leader-based group commit (microseconds)
/// Leader spins for this duration to collect additional transactions before fsync
/// Tuned for balance: 100μs = low latency, 300μs = balanced, 500μs+ = max batching
/// Set to 0 to disable batching window (immediate fsync when leader elected)
const GROUP_COMMIT_WINDOW_MICROS: u64 = 0;

/// The Write-Ahead Log journal manages durable logging of transactions.
///
/// The journal is **shared across all column families** in the database.
///
/// **Pipelined Leader-Based Group Commit:**
/// - First transaction becomes the "leader" and performs fsync for all pending transactions
/// - Leader spins briefly (~200μs) to collect additional transactions (batching window)
/// - While leader is fsyncing, new transactions accumulate for next batch
/// - Provides 30-50K+ ops/sec throughput with adaptive batching
///
/// Single transaction: ~200-300 ops/sec (limited by fsync ~3-5ms)
/// Concurrent transactions: 30-50K+ ops/sec (20-200 txns batched per fsync)
pub(crate) struct WALJournal {
    backend: Arc<dyn StorageBackend>,
    sequence_counter: Arc<AtomicU64>,
    /// Tracks the last sequence number that has been fsynced
    last_synced: Arc<(Mutex<u64>, Condvar)>,
    /// Leader election flag - true when a transaction is performing group sync
    sync_in_progress: AtomicBool,
    /// Mutex to ensure atomic append operations (len + write)
    append_lock: Mutex<()>,
}

/// Header structure for the WAL file.
#[derive(Debug, Clone)]
pub(crate) struct WALHeader {
    pub(crate) magic: [u8; 8],
    pub(crate) version: u8,
    pub(crate) oldest_seq: u64,
    pub(crate) latest_seq: u64,
}

impl WALHeader {
    fn new() -> Self {
        Self {
            magic: *WAL_MAGIC,
            version: WAL_VERSION,
            oldest_seq: 0,
            latest_seq: 0,
        }
    }

    fn to_bytes(&self) -> [u8; WAL_HEADER_SIZE] {
        let mut buf = [0u8; WAL_HEADER_SIZE];

        buf[0..8].copy_from_slice(&self.magic);
        buf[8] = self.version;
        buf[9..17].copy_from_slice(&self.oldest_seq.to_le_bytes());
        buf[17..25].copy_from_slice(&self.latest_seq.to_le_bytes());

        // Compute CRC32 of header fields (excluding the CRC field itself)
        let crc = crc32fast::hash(&buf[0..25]);
        buf[25..29].copy_from_slice(&crc.to_le_bytes());

        // Rest is padding (already zeros)
        buf
    }

    fn from_bytes(buf: &[u8; WAL_HEADER_SIZE]) -> io::Result<Self> {
        let mut magic = [0u8; 8];
        magic.copy_from_slice(&buf[0..8]);

        if &magic != WAL_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid WAL magic number",
            ));
        }

        let version = buf[8];
        if version != WAL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported WAL version: {version}"),
            ));
        }

        let oldest_seq = u64::from_le_bytes(buf[9..17].try_into().unwrap());
        let latest_seq = u64::from_le_bytes(buf[17..25].try_into().unwrap());
        let stored_crc = u32::from_le_bytes(buf[25..29].try_into().unwrap());

        // Verify CRC32
        let computed_crc = crc32fast::hash(&buf[0..25]);
        if computed_crc != stored_crc {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "WAL header CRC mismatch",
            ));
        }

        Ok(Self {
            magic,
            version,
            oldest_seq,
            latest_seq,
        })
    }
}

impl WALJournal {
    /// Creates a new WAL journal with the given storage backend.
    ///
    /// This constructor accepts a pre-initialized `StorageBackend`, making it suitable
    /// for both native (`FileBackend`) and WASM (`WasmStorageBackend`) use cases.
    ///
    /// The backend should be exclusive to this WAL journal (not shared with the main database).
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Native: wrap a FileBackend
    /// let file = std::fs::File::create("database.wal")?;
    /// let backend = Arc::new(FileBackend::new(file)?);
    /// let journal = WALJournal::new(backend)?;
    ///
    /// // WASM: use WasmStorageBackend
    /// let backend = WasmStorageBackend::new("database.wal").await?;
    /// let journal = WALJournal::new(Arc::new(backend))?;
    /// ```
    pub(crate) fn new(backend: Arc<dyn StorageBackend>) -> io::Result<Self> {
        // Check if backend is new (empty)
        let backend_len = backend.len()?;
        let header = if backend_len == 0 {
            // New backend - write initial header
            let header = WALHeader::new();
            backend.write(0, &header.to_bytes())?;
            backend.sync_data()?;
            header
        } else {
            // Existing backend - read and validate header
            let mut header_buf = [0u8; WAL_HEADER_SIZE];
            backend.read(0, &mut header_buf)?;
            WALHeader::from_bytes(&header_buf)?
        };

        Ok(Self {
            backend,
            sequence_counter: Arc::new(AtomicU64::new(header.latest_seq)),
            last_synced: Arc::new((Mutex::new(header.latest_seq), Condvar::new())),
            sync_in_progress: AtomicBool::new(false),
            append_lock: Mutex::new(()),
        })
    }

    /// Opens an existing WAL file or creates a new one (native platforms only).
    ///
    /// This is a convenience method for native platforms that wraps a `FileBackend`.
    /// For WASM or custom backends, use `WALJournal::new()` directly.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path = path.as_ref();
        #[allow(clippy::suspicious_open_options)]
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let backend = Arc::new(FileBackend::new(file).map_err(|e| {
            io::Error::other(format!("Failed to create FileBackend: {e}"))
        })?);
        Self::new(backend)
    }

    /// Appends a transaction entry to the WAL (without fsync).
    ///
    /// Returns the assigned sequence number.
    /// Call `wait_for_sync(sequence)` to wait until this entry is durable.
    pub(crate) fn append(&self, entry: &mut WALEntry) -> io::Result<u64> {
        // Assign sequence number
        let seq = self.sequence_counter.fetch_add(1, Ordering::SeqCst) + 1;
        entry.sequence = seq;

        // Serialize entry using zero-cost manual serialization
        let entry_data = entry.to_bytes();

        // Compute CRC32 of entry data
        let crc = crc32fast::hash(&entry_data);

        // Build wire format: length (4) + data (variable) + crc (4)
        let total_len = 4 + entry_data.len() + 4;
        let mut wire_data = Vec::with_capacity(total_len);
        #[allow(clippy::cast_possible_truncation)]
        wire_data.extend_from_slice(&(total_len as u32).to_le_bytes());
        wire_data.extend_from_slice(&entry_data);
        wire_data.extend_from_slice(&crc.to_le_bytes());

        // Append to backend (buffered write, no fsync yet)
        // Note: We don't update the header here to allow concurrent appends.
        // The header will be updated during checkpoint/truncate operations.
        // Use append_lock to make len() + write() atomic
        let _guard = self.append_lock.lock().unwrap();
        let offset = self.backend.len()?;
        self.backend.write(offset, &wire_data)?;

        Ok(seq)
    }

    /// Waits until the specified sequence number has been synced to disk.
    ///
    /// **Pipelined Leader-Based Group Commit:**
    /// - First transaction becomes the leader and performs fsync for all pending
    /// - Leader spins briefly to collect additional transactions (batching window)
    /// - Other transactions wait as followers and get woken when leader completes
    /// - Provides adaptive batching: single txn gets immediate fsync, concurrent txns batch
    pub(crate) fn wait_for_sync(&self, sequence: u64) -> io::Result<()> {
        loop {
            // Fast path: check if already synced
            {
                let (lock, _) = &*self.last_synced;
                let synced = lock.lock().unwrap();
                if *synced >= sequence {
                    return Ok(());
                }
            }

            // Try to become the leader
            if self
                .sync_in_progress
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                // I'm the leader - perform group sync for all pending transactions
                self.perform_group_sync()?;

                // Check if my sequence is now synced (should be, but verify)
                let (lock, _) = &*self.last_synced;
                let synced = lock.lock().unwrap();
                if *synced >= sequence {
                    return Ok(());
                }
                // If not synced yet, loop and try again (shouldn't happen)
            } else {
                // I'm a follower - wait for the current leader to finish
                let (lock, cvar) = &*self.last_synced;
                let mut synced = lock.lock().unwrap();

                // Wait for leader to notify us
                // The leader will wake all waiters when it updates last_synced
                while *synced < sequence {
                    synced = cvar.wait(synced).unwrap();
                }

                // We're synced - done
                return Ok(());
            }
        }
    }

    /// Performs the actual group sync operation (called by the leader).
    ///
    /// This method:
    /// 1. Spins for a brief batching window to collect additional transactions
    /// 2. Fsyncs all pending writes in one operation
    /// 3. Wakes all waiting transactions
    /// 4. Releases the leader flag
    fn perform_group_sync(&self) -> io::Result<()> {
        // Optional batching window: spin briefly to collect more transactions
        // This increases batching under load while keeping latency low
        #[allow(clippy::absurd_extreme_comparisons)]
        if GROUP_COMMIT_WINDOW_MICROS > 0 {
            let batch_start = Instant::now();
            while batch_start.elapsed() < Duration::from_micros(GROUP_COMMIT_WINDOW_MICROS) {
                std::hint::spin_loop();
            }
        }

        // Fsync all pending writes
        self.backend.sync_data()?;

        // Update last_synced and wake all waiting followers
        let current_seq = self.sequence_counter.load(Ordering::Acquire);
        {
            let (lock, cvar) = &*self.last_synced;
            let mut synced = lock.lock().unwrap();
            *synced = current_seq;
            cvar.notify_all();
        }

        // Release leader flag so next transaction can become leader
        self.sync_in_progress.store(false, Ordering::Release);

        Ok(())
    }

    /// Syncs all pending writes to disk immediately (bypasses group commit).
    pub(crate) fn sync(&self) -> io::Result<()> {
        self.backend.sync_data()?;

        // Update last_synced to current sequence
        let current_seq = self.sequence_counter.load(Ordering::Acquire);
        let (lock, cvar) = &*self.last_synced;
        let mut synced = lock.lock().unwrap();
        *synced = current_seq;
        cvar.notify_all();

        Ok(())
    }

    /// Reads all entries with sequence numbers >= `start_seq`.
    pub(crate) fn read_from(&self, start_seq: u64) -> io::Result<Vec<WALEntry>> {
        // Note: We don't check header.latest_seq here because append() doesn't update
        // the header (for performance). Instead, we scan the backend until EOF.

        let backend_len = self.backend.len()?;
        let mut offset = WAL_HEADER_SIZE as u64;
        let mut entries = Vec::new();

        while offset < backend_len {
            // Read entry length header
            let mut len_buf = [0u8; 4];
            if offset + 4 > backend_len {
                break; // Not enough data for length header
            }
            self.backend.read(offset, &mut len_buf)?;
            offset += 4;

            let total_len = u32::from_le_bytes(len_buf) as usize;
            if total_len < 8 {
                break; // Invalid entry length
            }

            let data_len = total_len - 4 - 4;
            if offset + data_len as u64 + 4 > backend_len {
                break; // Not enough data for entry
            }

            // Read entry data
            let mut entry_data = vec![0u8; data_len];
            self.backend.read(offset, &mut entry_data)?;
            offset += data_len as u64;

            // Read CRC
            let mut crc_buf = [0u8; 4];
            self.backend.read(offset, &mut crc_buf)?;
            offset += 4;

            let stored_crc = u32::from_le_bytes(crc_buf);
            let computed_crc = crc32fast::hash(&entry_data);
            if computed_crc != stored_crc {
                eprintln!("WAL entry CRC mismatch - stopping replay");
                break;
            }

            let (entry, _) = WALEntry::from_bytes(&entry_data)?;

            if entry.sequence >= start_seq {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    /// Truncates the WAL and resets the sequence counter.
    pub(crate) fn truncate(&self, new_oldest_seq: u64) -> io::Result<()> {
        // Truncate backend to just the header size
        self.backend.set_len(WAL_HEADER_SIZE as u64)?;

        // Write new header
        let mut header = WALHeader::new();
        header.oldest_seq = new_oldest_seq;
        header.latest_seq = new_oldest_seq - 1;
        self.backend.write(0, &header.to_bytes())?;
        self.backend.sync_data()?;

        // Update internal state
        self.sequence_counter
            .store(new_oldest_seq - 1, Ordering::SeqCst);

        let (lock, cvar) = &*self.last_synced;
        let mut synced = lock.lock().unwrap();
        *synced = new_oldest_seq - 1;
        cvar.notify_all();

        Ok(())
    }

    /// Reads the WAL header.
    ///
    /// Used in tests and diagnostics to inspect WAL state.
    #[allow(dead_code)]
    pub(crate) fn read_header(&self) -> io::Result<WALHeader> {
        let mut header_buf = [0u8; WAL_HEADER_SIZE];
        self.backend.read(0, &mut header_buf)?;
        WALHeader::from_bytes(&header_buf)
    }

    /// Returns the current WAL backend size in bytes.
    pub(crate) fn file_size(&self) -> io::Result<u64> {
        self.backend.len()
    }

    /// Shuts down the WAL journal gracefully.
    pub(crate) fn shutdown(&self) -> io::Result<()> {
        // Perform final sync to ensure all writes are durable
        // Wait for any in-progress sync to complete
        while self.sync_in_progress.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }

        // Do final sync
        self.sync()
    }

    /// Updates the header's `latest_seq` field.
    ///
    /// This is intentionally not called during `append()` for performance reasons.
    /// Header updates happen only during checkpoint/truncate operations.
    /// Kept for potential future use in checkpoint optimizations.
    #[allow(dead_code)]
    fn update_header_latest_seq(&self, latest_seq: u64) -> io::Result<()> {
        let mut header_buf = [0u8; WAL_HEADER_SIZE];
        self.backend.read(0, &mut header_buf)?;

        let mut header = WALHeader::from_bytes(&header_buf)?;
        header.latest_seq = latest_seq;

        self.backend.write(0, &header.to_bytes())?;

        Ok(())
    }
}

impl Drop for WALJournal {
    fn drop(&mut self) {
        // Best effort shutdown on drop
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column_family::wal::entry::WALTransactionPayload;
    use tempfile::NamedTempFile;

    #[test]
    fn test_wal_create_and_open() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let wal = WALJournal::open(path).unwrap();
        let header = wal.read_header().unwrap();
        assert_eq!(header.magic, *WAL_MAGIC);
        assert_eq!(header.version, WAL_VERSION);
        assert_eq!(header.oldest_seq, 0);
        assert_eq!(header.latest_seq, 0);

        wal.shutdown().unwrap();
    }

    #[test]
    fn test_wal_group_commit() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let wal = Arc::new(WALJournal::open(path).unwrap());

        // Simulate concurrent writes from different column families
        let mut handles = vec![];
        for i in 0..10 {
            let wal_clone = wal.clone();
            let handle = std::thread::spawn(move || {
                let payload = WALTransactionPayload {
                    user_root: None,
                    system_root: None,
                    freed_pages: vec![],
                    allocated_pages: vec![],
                    durability: crate::Durability::Immediate,
                };

                let mut entry = WALEntry::new(format!("cf_{i}"), i, payload);
                let seq = wal_clone.append(&mut entry).unwrap();
                wal_clone.wait_for_sync(seq).unwrap();
                seq
            });
            handles.push(handle);
        }

        let sequences: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(sequences.len(), 10);

        let entries = wal.read_from(1).unwrap();
        assert_eq!(entries.len(), 10);

        wal.shutdown().unwrap();
    }

    #[test]
    fn test_wal_truncate() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let wal = WALJournal::open(path).unwrap();

        for i in 0..5 {
            let payload = WALTransactionPayload {
                user_root: None,
                system_root: None,
                freed_pages: vec![],
                allocated_pages: vec![],
                durability: crate::Durability::Immediate,
            };

            let mut entry = WALEntry::new(format!("cf_{i}"), i, payload);
            let seq = wal.append(&mut entry).unwrap();
            wal.wait_for_sync(seq).unwrap();
        }

        wal.truncate(6).unwrap();

        let entries = wal.read_from(6).unwrap();
        assert_eq!(entries.len(), 0);

        let payload = WALTransactionPayload {
            user_root: None,
            system_root: None,
            freed_pages: vec![],
            allocated_pages: vec![],
            durability: crate::Durability::Immediate,
        };

        let mut entry = WALEntry::new("cf_new".to_string(), 100, payload);
        let seq = wal.append(&mut entry).unwrap();
        assert_eq!(seq, 6);

        wal.shutdown().unwrap();
    }
}
