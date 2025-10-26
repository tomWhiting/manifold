use super::entry::WALEntry;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Magic number for WAL file identification.
const WAL_MAGIC: &[u8; 8] = b"REDB-WAL";

/// Current WAL format version.
const WAL_VERSION: u8 = 1;

/// Size of the WAL file header in bytes.
const WAL_HEADER_SIZE: usize = 512;

/// The Write-Ahead Log journal manages durable logging of transactions.
///
/// The journal provides:
/// - Fast append-only writes with fsync
/// - CRC32 checksums for corruption detection
/// - Sequence numbers for ordering and replay
/// - Truncation for checkpoint cleanup
pub(crate) struct WALJournal {
    file: Mutex<File>,
    sequence_counter: Arc<AtomicU64>,
}

/// Header structure for the WAL file.
#[derive(Debug, Clone)]
pub(crate) struct WALHeader {
    magic: [u8; 8],
    version: u8,
    oldest_seq: u64,
    latest_seq: u64,
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
    /// Opens an existing WAL file or creates a new one.
    pub(crate) fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;

        // Check if file is new (empty)
        let metadata = file.metadata()?;
        let header = if metadata.len() == 0 {
            // New file - write initial header
            let header = WALHeader::new();
            file.write_all(&header.to_bytes())?;
            file.sync_all()?;
            header
        } else {
            // Existing file - read and validate header
            let mut header_buf = [0u8; WAL_HEADER_SIZE];
            file.seek(SeekFrom::Start(0))?;
            file.read_exact(&mut header_buf)?;
            WALHeader::from_bytes(&header_buf)?
        };

        Ok(Self {
            file: Mutex::new(file),
            sequence_counter: Arc::new(AtomicU64::new(header.latest_seq)),
        })
    }

    /// Appends a transaction entry to the WAL.
    ///
    /// Returns the assigned sequence number.
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
        wire_data.extend_from_slice(&(total_len as u32).to_le_bytes());
        wire_data.extend_from_slice(&entry_data);
        wire_data.extend_from_slice(&crc.to_le_bytes());

        // Append to file
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::End(0))?;
        file.write_all(&wire_data)?;

        // Update header with latest sequence
        self.update_header_latest_seq(&mut file, seq)?;

        Ok(seq)
    }

    /// Syncs all pending writes to disk.
    pub(crate) fn sync(&self) -> io::Result<()> {
        let file = self.file.lock().unwrap();
        file.sync_all()
    }

    /// Reads all entries with sequence numbers >= start_seq.
    pub(crate) fn read_from(&self, start_seq: u64) -> io::Result<Vec<WALEntry>> {
        let mut file = self.file.lock().unwrap();

        // Read header to get latest_seq
        file.seek(SeekFrom::Start(0))?;
        let mut header_buf = [0u8; WAL_HEADER_SIZE];
        file.read_exact(&mut header_buf)?;
        let header = WALHeader::from_bytes(&header_buf)?;

        if start_seq > header.latest_seq {
            // No entries to read
            return Ok(vec![]);
        }

        // Read entries sequentially from beginning
        file.seek(SeekFrom::Start(WAL_HEADER_SIZE as u64))?;
        let mut entries = Vec::new();

        loop {
            // Read length field
            let mut len_buf = [0u8; 4];
            match file.read_exact(&mut len_buf) {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }

            let total_len = u32::from_le_bytes(len_buf) as usize;
            if total_len < 8 {
                // Invalid length (must be at least length field + CRC)
                break;
            }

            // Read entry data and CRC
            let data_len = total_len - 4 - 4; // Subtract length field and CRC field
            let mut entry_data = vec![0u8; data_len];
            file.read_exact(&mut entry_data)?;

            let mut crc_buf = [0u8; 4];
            file.read_exact(&mut crc_buf)?;
            let stored_crc = u32::from_le_bytes(crc_buf);

            // Verify CRC32
            let computed_crc = crc32fast::hash(&entry_data);
            if computed_crc != stored_crc {
                // CRC mismatch - stop reading (partial write or corruption)
                eprintln!("WAL entry CRC mismatch - stopping replay");
                break;
            }

            // Deserialize entry
            let (entry, _) = WALEntry::from_bytes(&entry_data)?;

            // Only include entries with sequence >= start_seq
            if entry.sequence >= start_seq {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    /// Truncates the WAL and resets the sequence counter.
    ///
    /// This should be called after a successful checkpoint to clear processed entries.
    pub(crate) fn truncate(&self, new_oldest_seq: u64) -> io::Result<()> {
        let mut file = self.file.lock().unwrap();

        // Truncate file to just the header
        file.set_len(WAL_HEADER_SIZE as u64)?;

        // Update header with new oldest and latest (resetting)
        file.seek(SeekFrom::Start(0))?;
        let mut header = WALHeader::new();
        header.oldest_seq = new_oldest_seq;
        header.latest_seq = new_oldest_seq - 1; // No entries yet
        file.write_all(&header.to_bytes())?;
        file.sync_all()?;

        // Reset sequence counter
        self.sequence_counter
            .store(new_oldest_seq - 1, Ordering::SeqCst);

        Ok(())
    }

    /// Reads the WAL header.
    #[cfg(test)]
    pub(crate) fn read_header(&self) -> io::Result<WALHeader> {
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(0))?;
        let mut header_buf = [0u8; WAL_HEADER_SIZE];
        file.read_exact(&mut header_buf)?;
        WALHeader::from_bytes(&header_buf)
    }

    /// Returns the current WAL file size in bytes.
    #[cfg(test)]
    pub(crate) fn file_size(&self) -> io::Result<u64> {
        let file = self.file.lock().unwrap();
        Ok(file.metadata()?.len())
    }

    /// Closes the WAL file.
    #[cfg(test)]
    pub(crate) fn close(self) -> io::Result<()> {
        let file = self.file.into_inner().unwrap();
        file.sync_all()
    }

    /// Updates the header with the latest sequence number.
    fn update_header_latest_seq(&self, file: &mut File, latest_seq: u64) -> io::Result<()> {
        file.seek(SeekFrom::Start(0))?;
        let mut header_buf = [0u8; WAL_HEADER_SIZE];
        file.read_exact(&mut header_buf)?;

        let mut header = WALHeader::from_bytes(&header_buf)?;
        header.latest_seq = latest_seq;

        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header.to_bytes())?;

        Ok(())
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

        // Create new WAL
        let wal = WALJournal::open(path).unwrap();
        let header = wal.read_header().unwrap();
        assert_eq!(header.magic, *WAL_MAGIC);
        assert_eq!(header.version, WAL_VERSION);
        assert_eq!(header.oldest_seq, 0);
        assert_eq!(header.latest_seq, 0);
    }

    #[test]
    fn test_wal_append_and_read() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let wal = WALJournal::open(path).unwrap();

        // Create test entry
        let payload = WALTransactionPayload {
            user_root: None,
            system_root: None,
            freed_pages: vec![],
            allocated_pages: vec![],
            durability: crate::Durability::Immediate,
        };

        let mut entry = WALEntry::new("test_cf".to_string(), 1, payload);

        // Append entry
        let seq = wal.append(&mut entry).unwrap();
        assert_eq!(seq, 1);
        assert_eq!(entry.sequence, 1);

        // Sync
        wal.sync().unwrap();

        // Read back
        let entries = wal.read_from(1).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[0].cf_name, "test_cf");
        assert_eq!(entries[0].transaction_id, 1);
    }

    #[test]
    fn test_wal_multiple_entries() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let wal = WALJournal::open(path).unwrap();

        // Append multiple entries
        for i in 0..10 {
            let payload = WALTransactionPayload {
                user_root: None,
                system_root: None,
                freed_pages: vec![],
                allocated_pages: vec![],
                durability: crate::Durability::Immediate,
            };

            let mut entry = WALEntry::new(format!("cf_{i}"), i, payload);
            wal.append(&mut entry).unwrap();
        }

        wal.sync().unwrap();

        // Read all entries
        let entries = wal.read_from(1).unwrap();
        assert_eq!(entries.len(), 10);

        // Read from middle
        let entries = wal.read_from(5).unwrap();
        assert_eq!(entries.len(), 6); // Entries 5-10
        assert_eq!(entries[0].sequence, 5);
    }

    #[test]
    fn test_wal_truncate() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let wal = WALJournal::open(path).unwrap();

        // Append entries
        for i in 0..5 {
            let payload = WALTransactionPayload {
                user_root: None,
                system_root: None,
                freed_pages: vec![],
                allocated_pages: vec![],
                durability: crate::Durability::Immediate,
            };

            let mut entry = WALEntry::new(format!("cf_{i}"), i, payload);
            wal.append(&mut entry).unwrap();
        }

        wal.sync().unwrap();

        // Truncate
        wal.truncate(6).unwrap();

        // Read should return empty
        let entries = wal.read_from(6).unwrap();
        assert_eq!(entries.len(), 0);

        // Header should reflect truncation
        let header = wal.read_header().unwrap();
        assert_eq!(header.oldest_seq, 6);
        assert_eq!(header.latest_seq, 5); // No new entries yet

        // Append new entry should start from sequence 6
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
    }

    #[test]
    fn test_wal_persistence() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Create WAL and append entries
        {
            let wal = WALJournal::open(&path).unwrap();

            for i in 0..3 {
                let payload = WALTransactionPayload {
                    user_root: None,
                    system_root: None,
                    freed_pages: vec![],
                    allocated_pages: vec![],
                    durability: crate::Durability::Immediate,
                };

                let mut entry = WALEntry::new(format!("cf_{i}"), i, payload);
                wal.append(&mut entry).unwrap();
            }

            wal.sync().unwrap();
        } // WAL closed

        // Reopen and verify entries persisted
        {
            let wal = WALJournal::open(&path).unwrap();
            let entries = wal.read_from(1).unwrap();
            assert_eq!(entries.len(), 3);
            assert_eq!(entries[0].sequence, 1);
            assert_eq!(entries[2].sequence, 3);
        }
    }
}
