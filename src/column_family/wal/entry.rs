use crate::Durability;
use crate::tree_store::{BtreeHeader, Checksum, PageNumber};
use std::io;

/// A single entry in the Write-Ahead Log.
///
/// Each entry represents a committed transaction that has been durably written
/// to the WAL but may not yet have been applied to the main database file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WALEntry {
    /// Monotonic sequence number assigned by the journal.
    pub(crate) sequence: u64,

    /// Name of the column family this transaction belongs to.
    pub(crate) cf_name: String,

    /// Transaction ID from the underlying redb TransactionalMemory.
    pub(crate) transaction_id: u64,

    /// The serialized transaction payload.
    pub(crate) payload: WALTransactionPayload,
}

/// The payload of a WAL entry containing all information needed to replay a transaction.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WALTransactionPayload {
    /// Root of the user data B-tree after this transaction.
    pub(crate) user_root: Option<(PageNumber, Checksum)>,

    /// Root of the system B-tree after this transaction.
    pub(crate) system_root: Option<(PageNumber, Checksum)>,

    /// Pages freed by this transaction.
    pub(crate) freed_pages: Vec<PageNumber>,

    /// Pages allocated by this transaction.
    pub(crate) allocated_pages: Vec<PageNumber>,

    /// Original durability setting of the transaction.
    pub(crate) durability: Durability,
}

impl WALEntry {
    /// Creates a new WAL entry.
    ///
    /// The sequence number will be assigned by the journal during append.
    pub(crate) fn new(
        cf_name: String,
        transaction_id: u64,
        payload: WALTransactionPayload,
    ) -> Self {
        Self {
            sequence: 0, // Will be assigned by journal
            cf_name,
            transaction_id,
            payload,
        }
    }

    /// Serializes the entry to bytes using zero-cost manual serialization.
    ///
    /// Format:
    /// - sequence: u64 (8 bytes)
    /// - cf_name_len: u32 (4 bytes)
    /// - cf_name: [u8; cf_name_len] (variable)
    /// - transaction_id: u64 (8 bytes)
    /// - payload: serialized WALTransactionPayload (variable)
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Sequence number
        buf.extend_from_slice(&self.sequence.to_le_bytes());

        // CF name (length-prefixed string)
        let cf_name_bytes = self.cf_name.as_bytes();
        buf.extend_from_slice(&(cf_name_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(cf_name_bytes);

        // Transaction ID
        buf.extend_from_slice(&self.transaction_id.to_le_bytes());

        // Payload
        self.payload.serialize_into(&mut buf);

        buf
    }

    /// Deserializes an entry from bytes.
    ///
    /// Returns the entry and the number of bytes consumed.
    pub(crate) fn from_bytes(data: &[u8]) -> io::Result<(Self, usize)> {
        let mut offset = 0;

        // Read sequence
        if data.len() < offset + 8 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated sequence",
            ));
        }
        let sequence = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
        offset += 8;

        // Read CF name length
        if data.len() < offset + 4 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated cf_name length",
            ));
        }
        let cf_name_len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        // Read CF name
        if data.len() < offset + cf_name_len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated cf_name",
            ));
        }
        let cf_name =
            String::from_utf8(data[offset..offset + cf_name_len].to_vec()).map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("invalid UTF-8: {}", e))
            })?;
        offset += cf_name_len;

        // Read transaction ID
        if data.len() < offset + 8 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated transaction_id",
            ));
        }
        let transaction_id = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
        offset += 8;

        // Read payload
        let (payload, payload_len) = WALTransactionPayload::deserialize_from(&data[offset..])?;
        offset += payload_len;

        Ok((
            Self {
                sequence,
                cf_name,
                transaction_id,
                payload,
            },
            offset,
        ))
    }
}

impl WALTransactionPayload {
    /// Creates a new transaction payload.
    pub(crate) fn new(
        user_root: Option<BtreeHeader>,
        system_root: Option<BtreeHeader>,
        freed_pages: Vec<PageNumber>,
        allocated_pages: Vec<PageNumber>,
        durability: Durability,
    ) -> Self {
        Self {
            user_root: user_root.map(|h| (h.root, h.checksum)),
            system_root: system_root.map(|h| (h.root, h.checksum)),
            freed_pages,
            allocated_pages,
            durability,
        }
    }

    /// Serializes the payload into the given buffer.
    ///
    /// Format:
    /// - user_root_present: u8 (1 = present, 0 = None)
    /// - user_root: PageNumber (8 bytes) + Checksum (16 bytes) if present
    /// - system_root_present: u8
    /// - system_root: PageNumber + Checksum if present
    /// - freed_pages_count: u32 (4 bytes)
    /// - freed_pages: [PageNumber; count] (8 bytes each)
    /// - allocated_pages_count: u32 (4 bytes)
    /// - allocated_pages: [PageNumber; count] (8 bytes each)
    /// - durability: u8 (1 byte)
    fn serialize_into(&self, buf: &mut Vec<u8>) {
        // User root
        if let Some((page_num, checksum)) = self.user_root {
            buf.push(1);
            buf.extend_from_slice(&page_num.to_le_bytes());
            buf.extend_from_slice(&checksum.to_le_bytes());
        } else {
            buf.push(0);
        }

        // System root
        if let Some((page_num, checksum)) = self.system_root {
            buf.push(1);
            buf.extend_from_slice(&page_num.to_le_bytes());
            buf.extend_from_slice(&checksum.to_le_bytes());
        } else {
            buf.push(0);
        }

        // Freed pages
        buf.extend_from_slice(&(self.freed_pages.len() as u32).to_le_bytes());
        for page_num in &self.freed_pages {
            buf.extend_from_slice(&page_num.to_le_bytes());
        }

        // Allocated pages
        buf.extend_from_slice(&(self.allocated_pages.len() as u32).to_le_bytes());
        for page_num in &self.allocated_pages {
            buf.extend_from_slice(&page_num.to_le_bytes());
        }

        // Durability
        let durability_byte = match self.durability {
            Durability::None => 0,
            Durability::Immediate => 1,
        };
        buf.push(durability_byte);
    }

    /// Deserializes the payload from bytes.
    ///
    /// Returns the payload and the number of bytes consumed.
    fn deserialize_from(data: &[u8]) -> io::Result<(Self, usize)> {
        let mut offset = 0;

        // User root
        if data.len() < offset + 1 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated user_root flag",
            ));
        }
        let user_root = if data[offset] == 1 {
            offset += 1;
            if data.len() < offset + 8 + 16 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated user_root",
                ));
            }
            let page_num = PageNumber::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
            offset += 8;
            let checksum = u128::from_le_bytes(data[offset..offset + 16].try_into().unwrap());
            offset += 16;
            Some((page_num, checksum))
        } else {
            offset += 1;
            None
        };

        // System root
        if data.len() < offset + 1 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated system_root flag",
            ));
        }
        let system_root = if data[offset] == 1 {
            offset += 1;
            if data.len() < offset + 8 + 16 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated system_root",
                ));
            }
            let page_num = PageNumber::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
            offset += 8;
            let checksum = u128::from_le_bytes(data[offset..offset + 16].try_into().unwrap());
            offset += 16;
            Some((page_num, checksum))
        } else {
            offset += 1;
            None
        };

        // Freed pages
        if data.len() < offset + 4 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated freed_pages count",
            ));
        }
        let freed_pages_count =
            u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        let mut freed_pages = Vec::with_capacity(freed_pages_count);
        for _ in 0..freed_pages_count {
            if data.len() < offset + 8 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated freed_page",
                ));
            }
            let page_num = PageNumber::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
            offset += 8;
            freed_pages.push(page_num);
        }

        // Allocated pages
        if data.len() < offset + 4 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated allocated_pages count",
            ));
        }
        let allocated_pages_count =
            u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        let mut allocated_pages = Vec::with_capacity(allocated_pages_count);
        for _ in 0..allocated_pages_count {
            if data.len() < offset + 8 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated allocated_page",
                ));
            }
            let page_num = PageNumber::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
            offset += 8;
            allocated_pages.push(page_num);
        }

        // Durability
        if data.len() < offset + 1 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated durability",
            ));
        }
        let durability = match data[offset] {
            0 => Durability::None,
            1 => Durability::Immediate,
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid durability value: {}", other),
                ));
            }
        };
        offset += 1;

        Ok((
            Self {
                user_root,
                system_root,
                freed_pages,
                allocated_pages,
                durability,
            },
            offset,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_serialization_round_trip() {
        let payload = WALTransactionPayload {
            user_root: Some((PageNumber::new(0, 1, 0), 0x1234567890abcdef)),
            system_root: None,
            freed_pages: vec![PageNumber::new(0, 2, 0), PageNumber::new(0, 3, 0)],
            allocated_pages: vec![PageNumber::new(0, 4, 0)],
            durability: Durability::Immediate,
        };

        let entry = WALEntry {
            sequence: 42,
            cf_name: "test_cf".to_string(),
            transaction_id: 100,
            payload,
        };

        let bytes = entry.to_bytes();
        let (decoded, len) = WALEntry::from_bytes(&bytes).unwrap();

        assert_eq!(len, bytes.len());
        assert_eq!(decoded.sequence, entry.sequence);
        assert_eq!(decoded.cf_name, entry.cf_name);
        assert_eq!(decoded.transaction_id, entry.transaction_id);
        assert_eq!(decoded.payload, entry.payload);
    }

    #[test]
    fn test_payload_serialization_round_trip() {
        let payload = WALTransactionPayload {
            user_root: Some((PageNumber::new(1, 10, 2), 0xdeadbeef)),
            system_root: Some((PageNumber::new(2, 20, 1), 0xcafebabe)),
            freed_pages: vec![],
            allocated_pages: vec![
                PageNumber::new(0, 5, 0),
                PageNumber::new(0, 6, 0),
                PageNumber::new(0, 7, 0),
            ],
            durability: Durability::None,
        };

        let mut buf = Vec::new();
        payload.serialize_into(&mut buf);

        let (decoded, len) = WALTransactionPayload::deserialize_from(&buf).unwrap();

        assert_eq!(len, buf.len());
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_empty_payload() {
        let payload = WALTransactionPayload {
            user_root: None,
            system_root: None,
            freed_pages: vec![],
            allocated_pages: vec![],
            durability: Durability::Immediate,
        };

        let mut buf = Vec::new();
        payload.serialize_into(&mut buf);

        let (decoded, _) = WALTransactionPayload::deserialize_from(&buf).unwrap();

        assert_eq!(decoded, payload);
    }
}
