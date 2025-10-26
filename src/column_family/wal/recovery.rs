use super::journal::WALJournal;
use std::io;
use std::sync::Arc;

/// WAL recovery performs crash recovery by replaying WAL entries on database open.
///
/// This ensures that all committed transactions are applied to the main database,
/// even if the process crashed before a checkpoint could be performed.
pub(crate) struct WALRecovery {
    journal: Arc<WALJournal>,
}

impl WALRecovery {
    /// Creates a new WAL recovery instance.
    pub(crate) fn new(journal: Arc<WALJournal>) -> Self {
        Self { journal }
    }

    /// Performs crash recovery by replaying WAL entries.
    ///
    /// Returns the number of transactions recovered.
    ///
    /// This method:
    /// 1. Reads the WAL header to get the sequence range
    /// 2. Reads all entries from oldest_seq
    /// 3. Validates sequence numbers are monotonic
    /// 4. Returns valid entries for the caller to apply
    ///
    /// The caller is responsible for:
    /// - Applying entries to the database
    /// - Fsyncing the database
    /// - Truncating the WAL
    pub(crate) fn recover(&self) -> io::Result<usize> {
        // Read WAL header
        let header = self.journal.read_header()?;

        // If no entries, nothing to recover
        if header.latest_seq < header.oldest_seq {
            return Ok(0);
        }

        // Read all entries from oldest_seq
        let entries = self.journal.read_from(header.oldest_seq)?;

        // Validate sequence numbers are monotonic and within expected range
        let mut prev_seq = header.oldest_seq.saturating_sub(1);
        let mut valid_count = 0;

        for entry in &entries {
            // Check sequence number
            if entry.sequence != prev_seq + 1 {
                eprintln!(
                    "WAL sequence gap detected: expected {}, got {}",
                    prev_seq + 1,
                    entry.sequence
                );
                if entry.sequence <= header.latest_seq {
                    // Within range but out of order - corruption
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "WAL sequence corruption detected",
                    ));
                } else {
                    // Beyond latest_seq - partial write from crash, stop here
                    break;
                }
            }

            valid_count += 1;
            prev_seq = entry.sequence;
        }

        Ok(valid_count)
    }

    /// Gets all valid WAL entries for replay.
    ///
    /// This is separate from recover() to allow the caller to apply entries
    /// at their convenience.
    pub(crate) fn get_entries(&self) -> io::Result<Vec<super::entry::WALEntry>> {
        let header = self.journal.read_header()?;
        if header.latest_seq < header.oldest_seq {
            return Ok(vec![]);
        }
        self.journal.read_from(header.oldest_seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column_family::wal::entry::{WALEntry, WALTransactionPayload};
    use crate::Durability;
    use tempfile::NamedTempFile;

    #[test]
    fn test_recovery_empty_wal() {
        let temp_file = NamedTempFile::new().unwrap();
        let journal = Arc::new(WALJournal::open(temp_file.path()).unwrap());

        let recovery = WALRecovery::new(journal);
        let count = recovery.recover().unwrap();

        assert_eq!(count, 0);
    }

    #[test]
    fn test_recovery_with_entries() {
        let temp_file = NamedTempFile::new().unwrap();
        let journal = Arc::new(WALJournal::open(temp_file.path()).unwrap());

        // Write some entries
        for i in 0..5 {
            let payload = WALTransactionPayload {
                user_root: None,
                system_root: None,
                freed_pages: vec![],
                allocated_pages: vec![],
                durability: Durability::Immediate,
            };

            let mut entry = WALEntry::new(format!("cf_{i}"), i, payload);
            journal.append(&mut entry).unwrap();
        }
        journal.sync().unwrap();

        // Recovery should find all entries
        let recovery = WALRecovery::new(Arc::clone(&journal));
        let count = recovery.recover().unwrap();

        assert_eq!(count, 5);
    }

    #[test]
    fn test_get_entries() {
        let temp_file = NamedTempFile::new().unwrap();
        let journal = Arc::new(WALJournal::open(temp_file.path()).unwrap());

        // Write entries
        for i in 0..3 {
            let payload = WALTransactionPayload {
                user_root: None,
                system_root: None,
                freed_pages: vec![],
                allocated_pages: vec![],
                durability: Durability::Immediate,
            };

            let mut entry = WALEntry::new(format!("test_{i}"), i, payload);
            journal.append(&mut entry).unwrap();
        }
        journal.sync().unwrap();

        // Get entries
        let recovery = WALRecovery::new(journal);
        let entries = recovery.get_entries().unwrap();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].cf_name, "test_0");
        assert_eq!(entries[1].cf_name, "test_1");
        assert_eq!(entries[2].cf_name, "test_2");
    }
}
