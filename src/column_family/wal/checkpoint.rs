use super::config::CheckpointConfig;
use super::journal::WALJournal;
use crate::column_family::database::ColumnFamilyDatabase;
use crate::column_family::wal::entry::WALEntry;
use crate::tree_store::BtreeHeader;
use std::collections::BTreeSet;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Manages background checkpointing of WAL entries to the main database.
///
/// The CheckpointManager runs a background thread that periodically:
/// 1. Reads pending WAL entries
/// 2. Applies them to the main database using apply_wal_transaction()
/// 3. Fsyncs the database
/// 4. Truncates the WAL file
///
/// Checkpoints are triggered by:
/// - Time interval (default: 60 seconds)
/// - WAL size threshold (default: 64 MB)
/// - Manual checkpoint requests
pub(crate) struct CheckpointManager {
    // These fields are accessed via &self references in methods like checkpoint_now()
    #[allow(dead_code)]
    journal: Arc<WALJournal>,
    #[allow(dead_code)]
    database: Arc<ColumnFamilyDatabase>,
    #[allow(dead_code)]
    config: CheckpointConfig,
    pending_sequences: Arc<RwLock<BTreeSet<u64>>>,
    shutdown_signal: Arc<AtomicBool>,
    checkpoint_thread: Option<JoinHandle<()>>,
}

impl CheckpointManager {
    /// Creates and starts the checkpoint manager.
    pub(crate) fn start(
        journal: Arc<WALJournal>,
        database: Arc<ColumnFamilyDatabase>,
        config: CheckpointConfig,
    ) -> Self {
        let pending_sequences = Arc::new(RwLock::new(BTreeSet::new()));
        let shutdown_signal = Arc::new(AtomicBool::new(false));

        let thread_journal = Arc::clone(&journal);
        let thread_database = Arc::clone(&database);
        let thread_config = config.clone();
        let thread_pending = Arc::clone(&pending_sequences);
        let thread_shutdown = Arc::clone(&shutdown_signal);

        let checkpoint_thread = thread::spawn(move || {
            Self::checkpoint_loop(
                thread_journal,
                thread_database,
                thread_config,
                thread_pending,
                thread_shutdown,
            );
        });

        Self {
            journal,
            database,
            config,
            pending_sequences,
            shutdown_signal,
            checkpoint_thread: Some(checkpoint_thread),
        }
    }

    /// Registers a transaction sequence number as pending checkpoint.
    pub(crate) fn register_pending(&self, sequence: u64) {
        let mut pending = self.pending_sequences.write().unwrap();
        pending.insert(sequence);
    }

    /// Manually triggers a checkpoint (blocks until complete).
    ///
    /// This is used in tests and can be used by applications that need
    /// explicit control over checkpoint timing.
    #[allow(dead_code)]
    pub(crate) fn checkpoint_now(&self) -> io::Result<()> {
        Self::checkpoint_internal(&self.journal, &self.database, &self.pending_sequences)
    }

    /// Shuts down the checkpoint thread gracefully.
    pub(crate) fn shutdown(mut self) -> io::Result<()> {
        self.shutdown_signal.store(true, Ordering::Release);

        if let Some(handle) = self.checkpoint_thread.take() {
            handle
                .join()
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "checkpoint thread panicked"))?;
        }

        Ok(())
    }

    /// Background checkpoint loop.
    fn checkpoint_loop(
        journal: Arc<WALJournal>,
        database: Arc<ColumnFamilyDatabase>,
        config: CheckpointConfig,
        pending_sequences: Arc<RwLock<BTreeSet<u64>>>,
        shutdown_signal: Arc<AtomicBool>,
    ) {
        let mut last_checkpoint = std::time::Instant::now();

        loop {
            // Sleep in small increments to check shutdown signal
            let sleep_duration = Duration::from_millis(100);
            thread::sleep(sleep_duration);

            if shutdown_signal.load(Ordering::Acquire) {
                // Perform final checkpoint before shutdown
                let _ = Self::checkpoint_internal(&journal, &database, &pending_sequences);
                break;
            }

            // Check if checkpoint is needed
            let should_checkpoint = {
                // Time-based trigger
                let time_elapsed = last_checkpoint.elapsed() >= config.interval;

                // Size-based trigger
                let wal_size = journal.file_size().unwrap_or(0);
                let size_exceeded = wal_size >= config.max_wal_size;

                time_elapsed || size_exceeded
            };

            if should_checkpoint {
                match Self::checkpoint_internal(&journal, &database, &pending_sequences) {
                    Ok(()) => {
                        last_checkpoint = std::time::Instant::now();
                    }
                    Err(e) => {
                        eprintln!("Checkpoint failed: {e}");
                        // Continue running - retry on next interval
                    }
                }
            }
        }
    }

    /// Performs a checkpoint operation.
    fn checkpoint_internal(
        journal: &Arc<WALJournal>,
        database: &Arc<ColumnFamilyDatabase>,
        pending_sequences: &Arc<RwLock<BTreeSet<u64>>>,
    ) -> io::Result<()> {
        // Get snapshot of pending sequences
        let sequences = {
            let pending = pending_sequences.read().unwrap();
            if pending.is_empty() {
                return Ok(()); // Nothing to checkpoint
            }
            pending.clone()
        };

        let oldest_seq = *sequences.first().unwrap();
        let latest_seq = *sequences.last().unwrap();

        // Read all pending WAL entries
        let entries = journal.read_from(oldest_seq)?;

        if entries.is_empty() {
            // No entries found - clear pending and return
            pending_sequences.write().unwrap().clear();
            return Ok(());
        }

        // Apply each entry to the database
        for entry in &entries {
            Self::apply_wal_entry_to_database(database, entry)?;
        }

        // Fsync all column families to persist changes
        // This ensures the main database is durable before we truncate the WAL
        for cf_name in database.list_column_families() {
            if let Ok(cf) = database.column_family(&cf_name) {
                if let Ok(db) = cf.ensure_database() {
                    // Sync the database to persist checkpoint changes
                    // We access the underlying storage through a write transaction
                    // that we immediately commit with durability
                    let mut txn = db.begin_write().map_err(|e| {
                        io::Error::new(io::ErrorKind::Other, format!("begin write failed: {e}"))
                    })?;

                    txn.set_durability(crate::Durability::Immediate)
                        .map_err(|e| {
                            io::Error::new(
                                io::ErrorKind::Other,
                                format!("set durability failed: {e}"),
                            )
                        })?;

                    txn.commit().map_err(|e| {
                        io::Error::new(io::ErrorKind::Other, format!("commit failed: {e}"))
                    })?;
                }
            }
        }

        // Truncate WAL and reset sequence counter
        journal.truncate(latest_seq + 1)?;

        // Clear pending sequences
        pending_sequences.write().unwrap().clear();

        Ok(())
    }

    /// Applies a single WAL entry to the database.
    fn apply_wal_entry_to_database(
        database: &Arc<ColumnFamilyDatabase>,
        entry: &WALEntry,
    ) -> io::Result<()> {
        // Get the column family
        let cf = database.column_family(&entry.cf_name).map_err(|e| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("column family '{}' not found: {e}", entry.cf_name),
            )
        })?;

        // Get the underlying Database instance
        let db = cf.ensure_database().map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("failed to access database: {e}"),
            )
        })?;

        // Get the TransactionalMemory
        let mem = db.get_memory();

        // Convert WAL payload to BtreeHeader format
        let data_root = entry
            .payload
            .user_root
            .map(|(page_num, checksum, length)| BtreeHeader {
                root: page_num,
                checksum,
                length,
            });

        let system_root = entry
            .payload
            .system_root
            .map(|(page_num, checksum, length)| BtreeHeader {
                root: page_num,
                checksum,
                length,
            });

        // Apply the WAL transaction to the database
        mem.apply_wal_transaction(
            data_root,
            system_root,
            crate::transaction_tracker::TransactionId::new(entry.transaction_id),
        )
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("apply_wal_transaction failed: {e}"),
            )
        })?;

        Ok(())
    }
}

impl Drop for CheckpointManager {
    fn drop(&mut self) {
        // Signal shutdown
        self.shutdown_signal.store(true, Ordering::Release);

        // Wait for thread to finish
        if let Some(handle) = self.checkpoint_thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Durability;
    use crate::column_family::database::ColumnFamilyDatabase;
    use crate::column_family::wal::entry::{WALEntry, WALTransactionPayload};
    use tempfile::TempDir;

    #[test]
    fn test_checkpoint_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Arc::new(
            ColumnFamilyDatabase::builder()
                .pool_size(1)
                .open(&db_path)
                .unwrap(),
        );

        let wal_path = db_path.with_extension("wal");
        let journal = Arc::new(WALJournal::open(&wal_path).unwrap());

        let config = CheckpointConfig {
            interval: Duration::from_secs(60),
            max_wal_size: 64 * 1024 * 1024,
        };

        let manager = CheckpointManager::start(journal, db, config);
        manager.shutdown().unwrap();
    }

    #[test]
    fn test_register_pending() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Arc::new(
            ColumnFamilyDatabase::builder()
                .pool_size(1)
                .open(&db_path)
                .unwrap(),
        );

        let wal_path = db_path.with_extension("wal");
        let journal = Arc::new(WALJournal::open(&wal_path).unwrap());

        let config = CheckpointConfig {
            interval: Duration::from_secs(3600), // Long interval
            max_wal_size: 1024 * 1024 * 1024,    // Large threshold
        };

        let manager = CheckpointManager::start(journal, db, config);

        // Register some sequences
        manager.register_pending(1);
        manager.register_pending(2);
        manager.register_pending(3);

        let pending = manager.pending_sequences.read().unwrap();
        assert_eq!(pending.len(), 3);
        assert!(pending.contains(&1));
        assert!(pending.contains(&2));
        assert!(pending.contains(&3));

        drop(pending);
        manager.shutdown().unwrap();
    }

    #[test]
    fn test_manual_checkpoint() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Arc::new(
            ColumnFamilyDatabase::builder()
                .pool_size(1)
                .open(&db_path)
                .unwrap(),
        );

        // Create a column family
        db.create_column_family("test_cf", None).unwrap();

        let wal_path = db_path.with_extension("wal");
        let journal = Arc::new(WALJournal::open(&wal_path).unwrap());

        // Write some WAL entries
        for i in 0..3 {
            let payload = WALTransactionPayload {
                user_root: None,
                system_root: None,
                freed_pages: vec![],
                allocated_pages: vec![],
                durability: Durability::Immediate,
            };

            let mut entry = WALEntry::new("test_cf".to_string(), i, payload);
            journal.append(&mut entry).unwrap();
        }
        journal.sync().unwrap();

        let config = CheckpointConfig {
            interval: Duration::from_secs(3600),
            max_wal_size: 1024 * 1024 * 1024,
        };

        let manager = CheckpointManager::start(Arc::clone(&journal), db, config);

        // Register sequences
        manager.register_pending(1);
        manager.register_pending(2);
        manager.register_pending(3);

        // Trigger manual checkpoint
        manager.checkpoint_now().unwrap();

        // Pending sequences should be cleared
        {
            let pending = manager.pending_sequences.read().unwrap();
            assert_eq!(pending.len(), 0);
        }

        manager.shutdown().unwrap();
    }
}
