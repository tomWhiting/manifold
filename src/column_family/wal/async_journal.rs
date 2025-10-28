//! Asynchronous WAL journal with background sync thread.
//!
//! This module provides an optimized WAL implementation that decouples fsync
//! operations from the transaction commit path, allowing better throughput
//! through automatic batching of sync operations.
//!
//! # Architecture
//!
//! ```text
//! Transaction Thread 1: append() → return immediately
//! Transaction Thread 2: append() → return immediately
//! Transaction Thread 3: append() → return immediately
//!                           ↓
//!                    Pending Sync Queue
//!                           ↓
//!              Background Sync Thread
//!                  (batches fsyncs)
//!                           ↓
//!                   All threads notified
//! ```
//!
//! # Performance Benefits
//!
//! - Transactions don't block on fsync (return immediately after append)
//! - Background thread automatically batches multiple appends into single fsync
//! - Better CPU utilization (no threads blocking on I/O)
//! - Expected 1.5-2x throughput improvement over synchronous WAL

#![allow(dead_code)] // Phase 1 core implementation - will be used in integration

use super::entry::WALEntry;
use super::journal::{WAL_HEADER_SIZE, WALHeader};
use crate::StorageBackend;
use std::collections::BTreeSet;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// How often the sync thread wakes up to check for pending writes (microseconds).
/// Lower = better latency, higher = better batching.
const SYNC_POLL_INTERVAL_MICROS: u64 = 100;

/// Maximum time to wait before forcing a sync, even if queue is small (milliseconds).
const MAX_SYNC_DELAY_MILLIS: u64 = 1;

/// Asynchronous WAL journal with background sync thread.
///
/// Provides non-blocking WAL appends with automatic batching of fsync operations.
pub struct AsyncWALJournal {
    backend: Arc<dyn StorageBackend>,
    sequence_counter: Arc<AtomicU64>,

    /// Sequences that have been appended but not yet fsynced.
    pending_sync: Arc<Mutex<BTreeSet<u64>>>,

    /// Last sequence number that has been fsynced to disk.
    last_synced: Arc<AtomicU64>,

    /// Condition variable for notifying sync thread of new writes.
    sync_signal: Arc<Condvar>,

    /// Mutex for sync_signal (required by Condvar).
    sync_mutex: Arc<Mutex<()>>,

    /// Shutdown signal for background thread.
    shutdown: Arc<AtomicBool>,

    /// Background sync thread handle.
    #[cfg(not(target_arch = "wasm32"))]
    sync_thread: Option<JoinHandle<()>>,

    /// Mutex to ensure atomic append operations (len + write).
    append_lock: Mutex<()>,
}

impl AsyncWALJournal {
    /// Creates a new async WAL journal with the given storage backend.
    ///
    /// Starts a background thread that periodically fsyncs pending writes.
    ///
    /// # Arguments
    ///
    /// * `backend` - Storage backend for WAL file (must be exclusive to this WAL)
    ///
    /// # Returns
    ///
    /// A new `AsyncWALJournal` instance with background sync thread running.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(backend: Arc<dyn StorageBackend>) -> io::Result<Self> {
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

        let sequence_counter = Arc::new(AtomicU64::new(header.latest_seq));
        let last_synced = Arc::new(AtomicU64::new(header.latest_seq));
        let pending_sync = Arc::new(Mutex::new(BTreeSet::new()));
        let sync_signal = Arc::new(Condvar::new());
        let sync_mutex = Arc::new(Mutex::new(()));
        let shutdown = Arc::new(AtomicBool::new(false));

        // Spawn background sync thread
        let thread_backend = Arc::clone(&backend);
        let thread_pending = Arc::clone(&pending_sync);
        let thread_last_synced = Arc::clone(&last_synced);
        let thread_sync_signal = Arc::clone(&sync_signal);
        let thread_sync_mutex = Arc::clone(&sync_mutex);
        let thread_shutdown = Arc::clone(&shutdown);

        let sync_thread = thread::spawn(move || {
            Self::sync_thread_loop(
                thread_backend,
                thread_pending,
                thread_last_synced,
                thread_sync_signal,
                thread_sync_mutex,
                thread_shutdown,
            );
        });

        Ok(Self {
            backend,
            sequence_counter,
            pending_sync,
            last_synced,
            sync_signal,
            sync_mutex,
            shutdown,
            sync_thread: Some(sync_thread),
            append_lock: Mutex::new(()),
        })
    }

    /// Opens an existing WAL file or creates a new one (native platforms only).
    ///
    /// This is a convenience method for native platforms.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        use crate::backends::FileBackend;
        use std::fs::OpenOptions;

        let path = path.as_ref();
        #[allow(clippy::suspicious_open_options)]
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let backend = Arc::new(
            FileBackend::new(file)
                .map_err(|e| io::Error::other(format!("Failed to create FileBackend: {e}")))?,
        );
        Self::new(backend)
    }

    /// Appends a transaction entry to the WAL (non-blocking).
    ///
    /// Returns immediately after writing to the WAL file buffer. The actual
    /// fsync happens asynchronously in the background thread.
    ///
    /// # Arguments
    ///
    /// * `entry` - WAL entry to append (will be assigned a sequence number)
    ///
    /// # Returns
    ///
    /// The assigned sequence number. Call `wait_for_sync(sequence)` to wait
    /// for this entry to be fsynced to disk.
    pub fn append(&self, entry: &mut WALEntry) -> io::Result<u64> {
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
        // Use append_lock to make len() + write() atomic
        let _guard = self.append_lock.lock().unwrap();
        let offset = self.backend.len()?;
        self.backend.write(offset, &wire_data)?;
        drop(_guard);

        // Add to pending sync queue
        {
            let mut pending = self.pending_sync.lock().unwrap();
            pending.insert(seq);
        }

        // Signal sync thread that there's new data
        {
            let _lock = self.sync_mutex.lock().unwrap();
            self.sync_signal.notify_one();
        }

        Ok(seq)
    }

    /// Waits until the specified sequence number has been synced to disk.
    ///
    /// This method blocks until the background sync thread has fsynced all
    /// entries up to and including the given sequence number.
    ///
    /// # Arguments
    ///
    /// * `sequence` - Sequence number to wait for
    ///
    /// # Returns
    ///
    /// `Ok(())` when the sequence has been synced, or an error if sync failed.
    pub fn wait_for_sync(&self, sequence: u64) -> io::Result<()> {
        // Fast path: already synced
        if self.last_synced.load(Ordering::Acquire) >= sequence {
            return Ok(());
        }

        // Wait for background thread to sync this sequence
        // We use a simple spin-wait with yield for efficiency
        loop {
            let last = self.last_synced.load(Ordering::Acquire);
            if last >= sequence {
                return Ok(());
            }

            // Yield to other threads and wait a bit
            thread::yield_now();
            thread::sleep(Duration::from_micros(10));
        }
    }

    /// Background sync thread loop.
    ///
    /// Runs continuously, waking up periodically to fsync pending writes.
    /// Batches multiple writes into a single fsync for efficiency.
    #[allow(clippy::too_many_arguments)]
    fn sync_thread_loop(
        backend: Arc<dyn StorageBackend>,
        pending_sync: Arc<Mutex<BTreeSet<u64>>>,
        last_synced: Arc<AtomicU64>,
        sync_signal: Arc<Condvar>,
        sync_mutex: Arc<Mutex<()>>,
        shutdown: Arc<AtomicBool>,
    ) {
        let mut last_sync_time = std::time::Instant::now();

        loop {
            // Check shutdown signal
            if shutdown.load(Ordering::Acquire) {
                // Perform final sync before exiting
                let _ = Self::perform_sync(&backend, &pending_sync, &last_synced);
                break;
            }

            // Wait for signal or timeout
            {
                let lock = sync_mutex.lock().unwrap();
                let _result = sync_signal
                    .wait_timeout(lock, Duration::from_micros(SYNC_POLL_INTERVAL_MICROS))
                    .unwrap();
            }

            // Check if we should sync
            let should_sync = {
                let pending = pending_sync.lock().unwrap();
                let has_pending = !pending.is_empty();
                let timeout_elapsed =
                    last_sync_time.elapsed() >= Duration::from_millis(MAX_SYNC_DELAY_MILLIS);

                has_pending && timeout_elapsed
            };

            if should_sync {
                if Self::perform_sync(&backend, &pending_sync, &last_synced).is_ok() {
                    last_sync_time = std::time::Instant::now();
                }
            }
        }
    }

    /// Performs a single sync operation.
    ///
    /// Fsyncs the backend and updates last_synced to reflect all pending writes.
    fn perform_sync(
        backend: &Arc<dyn StorageBackend>,
        pending_sync: &Arc<Mutex<BTreeSet<u64>>>,
        last_synced: &Arc<AtomicU64>,
    ) -> io::Result<()> {
        // Fsync the backend
        backend.sync_data()?;

        // Update last_synced and clear pending queue
        let synced_seq = {
            let mut pending = pending_sync.lock().unwrap();
            if pending.is_empty() {
                return Ok(());
            }

            let max_seq = *pending.iter().max().unwrap();
            pending.clear();
            max_seq
        };

        last_synced.store(synced_seq, Ordering::Release);

        Ok(())
    }

    /// Manually triggers an immediate sync (blocks until complete).
    ///
    /// This is useful for testing or when explicit durability is required.
    #[allow(dead_code)]
    pub fn sync_now(&self) -> io::Result<()> {
        Self::perform_sync(&self.backend, &self.pending_sync, &self.last_synced)
    }

    /// Returns the last sequence number that has been synced to disk.
    #[allow(dead_code)]
    pub fn last_synced_sequence(&self) -> u64 {
        self.last_synced.load(Ordering::Acquire)
    }

    /// Shuts down the background sync thread gracefully.
    ///
    /// Performs a final sync before stopping.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn shutdown(mut self) -> io::Result<()> {
        self.shutdown.store(true, Ordering::Release);

        // Wake up sync thread
        {
            let _lock = self.sync_mutex.lock().unwrap();
            self.sync_signal.notify_one();
        }

        // Wait for thread to finish
        if let Some(handle) = self.sync_thread.take() {
            handle
                .join()
                .map_err(|_| io::Error::other("sync thread panicked"))?;
        }

        Ok(())
    }

    /// Returns the current file size.
    pub fn file_size(&self) -> io::Result<u64> {
        self.backend.len()
    }

    /// Reads WAL entries starting from the given sequence number.
    ///
    /// This is used during recovery to replay the WAL.
    pub fn read_from(&self, start_sequence: u64) -> io::Result<Vec<WALEntry>> {
        // This is the same implementation as the synchronous WAL journal
        // We can reuse the logic by reading directly from the backend
        super::journal::WALJournal::read_entries_from_backend(&self.backend, start_sequence)
    }

    /// Truncates the WAL file to remove entries before the given sequence.
    ///
    /// This is called after checkpoint to reclaim space.
    pub fn truncate(&self, oldest_sequence: u64) -> io::Result<()> {
        // Ensure all pending writes are synced first
        self.sync_now()?;

        // For now, we just update the header to reflect the new oldest sequence
        // A full implementation would copy remaining entries to a new file
        let mut header_buf = [0u8; WAL_HEADER_SIZE];
        self.backend.read(0, &mut header_buf)?;
        let mut header = WALHeader::from_bytes(&header_buf)?;

        header.oldest_seq = oldest_sequence;

        self.backend.write(0, &header.to_bytes())?;
        self.backend.sync_data()?;

        Ok(())
    }
}

impl Drop for AsyncWALJournal {
    fn drop(&mut self) {
        // Signal shutdown
        self.shutdown.store(true, Ordering::Release);

        // Wake up sync thread
        {
            let _lock = self.sync_mutex.lock().unwrap();
            self.sync_signal.notify_one();
        }

        // Wait for thread to finish (don't panic on error in drop)
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(handle) = self.sync_thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::InMemoryBackend;

    #[test]
    fn test_async_journal_create() {
        let backend = Arc::new(InMemoryBackend::new());
        let journal = AsyncWALJournal::new(backend).unwrap();

        assert_eq!(journal.last_synced_sequence(), 0);
    }

    #[test]
    fn test_async_append_and_sync() {
        use super::super::entry::{WALEntry, WALTransactionPayload};
        use crate::Durability;

        let backend = Arc::new(InMemoryBackend::new());
        let journal = AsyncWALJournal::new(backend).unwrap();

        let payload = WALTransactionPayload {
            user_root: None,
            system_root: None,
            freed_pages: vec![],
            allocated_pages: vec![],
            durability: Durability::Immediate,
        };

        let mut entry = WALEntry::new("test_cf".to_string(), 1, payload);
        let seq = journal.append(&mut entry).unwrap();

        assert_eq!(seq, 1);

        // Wait for sync
        journal.wait_for_sync(seq).unwrap();

        assert!(journal.last_synced_sequence() >= seq);
    }

    #[test]
    fn test_async_multiple_appends() {
        use super::super::entry::{WALEntry, WALTransactionPayload};
        use crate::Durability;

        let backend = Arc::new(InMemoryBackend::new());
        let journal = AsyncWALJournal::new(backend).unwrap();

        let mut sequences = vec![];

        // Append multiple entries
        for i in 0..10 {
            let payload = WALTransactionPayload {
                user_root: None,
                system_root: None,
                freed_pages: vec![],
                allocated_pages: vec![],
                durability: Durability::Immediate,
            };

            let mut entry = WALEntry::new(format!("cf_{}", i), i as u64, payload);
            let seq = journal.append(&mut entry).unwrap();
            sequences.push(seq);
        }

        // Wait for all to sync
        for seq in sequences {
            journal.wait_for_sync(seq).unwrap();
        }

        assert_eq!(journal.last_synced_sequence(), 10);
    }

    #[test]
    fn test_async_shutdown() {
        let backend = Arc::new(InMemoryBackend::new());
        let journal = AsyncWALJournal::new(backend).unwrap();

        // Shutdown should complete without hanging
        journal.shutdown().unwrap();
    }
}
