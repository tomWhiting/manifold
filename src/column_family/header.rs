use std::io;

/// Magic number identifying a column family database file.
///
/// The sequence includes DOS/Unix line ending detection bytes (0x1A, 0x0A) to help
/// detect text-mode corruption.
pub const MAGIC_NUMBER: [u8; 9] = *b"mnfd-cf\x1A\x0A";

/// Current format version for the master header.
/// Version 2 introduces segmented column families with free space tracking.
pub const FORMAT_VERSION: u8 = 2;

/// Size of one page in bytes (4KB).
///
/// The master header must fit within a single page.
pub(crate) const PAGE_SIZE: usize = 4096;

/// A contiguous segment of storage within the database file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Absolute byte offset where this segment begins in the file.
    pub offset: u64,
    /// Size of this segment in bytes.
    pub size: u64,
}

impl Segment {
    /// Creates a new segment.
    pub fn new(offset: u64, size: u64) -> Self {
        Self { offset, size }
    }

    /// Returns the end offset (exclusive) of this segment.
    pub fn end(&self) -> u64 {
        self.offset + self.size
    }

    /// Serializes this segment to bytes.
    ///
    /// Format: `offset` (u64) | `size` (u64)
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&self.offset.to_le_bytes());
        bytes.extend_from_slice(&self.size.to_le_bytes());
        bytes
    }

    /// Deserializes a segment from bytes.
    ///
    /// Returns (`segment`, `bytes_consumed`) on success.
    fn from_bytes(data: &[u8]) -> io::Result<(Self, usize)> {
        if data.len() < 16 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "insufficient data for segment",
            ));
        }

        let offset = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let size = u64::from_le_bytes(data[8..16].try_into().unwrap());

        Ok((Self { offset, size }, 16))
    }
}

/// A free (deleted/unused) segment that can be reclaimed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeSegment {
    /// Absolute byte offset where this free segment begins.
    pub offset: u64,
    /// Size of this free segment in bytes.
    pub size: u64,
}

impl FreeSegment {
    /// Creates a new free segment.
    pub fn new(offset: u64, size: u64) -> Self {
        Self { offset, size }
    }

    /// Serializes this free segment to bytes.
    ///
    /// Format: `offset` (u64) | `size` (u64)
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&self.offset.to_le_bytes());
        bytes.extend_from_slice(&self.size.to_le_bytes());
        bytes
    }

    /// Deserializes a free segment from bytes.
    ///
    /// Returns (`free_segment`, `bytes_consumed`) on success.
    fn from_bytes(data: &[u8]) -> io::Result<(Self, usize)> {
        if data.len() < 16 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "insufficient data for free segment",
            ));
        }

        let offset = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let size = u64::from_le_bytes(data[8..16].try_into().unwrap());

        Ok((Self { offset, size }, 16))
    }
}

/// Metadata describing a column family composed of one or more segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnFamilyMetadata {
    /// Name of the column family.
    pub name: String,
    /// Segments that make up this column family.
    /// Multiple segments enable non-contiguous growth without data movement.
    pub segments: Vec<Segment>,
}

impl ColumnFamilyMetadata {
    /// Creates a new column family metadata entry with a single segment.
    pub fn new(name: String, offset: u64, size: u64) -> Self {
        Self {
            name,
            segments: vec![Segment::new(offset, size)],
        }
    }

    /// Creates a new column family metadata entry with multiple segments.
    pub fn with_segments(name: String, segments: Vec<Segment>) -> Self {
        Self { name, segments }
    }

    /// Returns the total size of all segments.
    pub fn total_size(&self) -> u64 {
        self.segments.iter().map(|s| s.size).sum()
    }

    /// Serializes this metadata entry to bytes.
    ///
    /// Format: `name_len` (u32) | `name_bytes` | `segment_count` (u32) | segments
    fn to_bytes(&self) -> Vec<u8> {
        let name_bytes = self.name.as_bytes();
        let name_len =
            u32::try_from(name_bytes.len()).expect("column family name exceeds maximum length");
        let segment_count =
            u32::try_from(self.segments.len()).expect("too many segments in column family");

        let mut bytes = Vec::with_capacity(4 + name_bytes.len() + 4 + self.segments.len() * 16);
        bytes.extend_from_slice(&name_len.to_le_bytes());
        bytes.extend_from_slice(name_bytes);
        bytes.extend_from_slice(&segment_count.to_le_bytes());

        for segment in &self.segments {
            bytes.extend_from_slice(&segment.to_bytes());
        }

        bytes
    }

    /// Deserializes metadata from bytes.
    ///
    /// Returns (`metadata`, `bytes_consumed`) on success.
    fn from_bytes(data: &[u8]) -> io::Result<(Self, usize)> {
        if data.len() < 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "insufficient data for name length",
            ));
        }

        let name_len = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;

        if data.len() < 4 + name_len + 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "insufficient data for metadata entry: need at least {}, have {}",
                    4 + name_len + 4,
                    data.len()
                ),
            ));
        }

        let name = String::from_utf8(data[4..4 + name_len].to_vec()).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid UTF-8 in column family name: {e}"),
            )
        })?;

        let segment_count_start = 4 + name_len;
        let segment_count = u32::from_le_bytes(
            data[segment_count_start..segment_count_start + 4]
                .try_into()
                .unwrap(),
        ) as usize;

        let mut segments = Vec::with_capacity(segment_count);
        let mut offset = segment_count_start + 4;

        for _ in 0..segment_count {
            let (segment, consumed) = Segment::from_bytes(&data[offset..])?;
            segments.push(segment);
            offset += consumed;
        }

        let bytes_consumed = offset;

        Ok((Self { name, segments }, bytes_consumed))
    }
}

/// Master header describing the layout of all column families within a database file.
///
/// The master header occupies the first page (4KB) of the file and contains metadata
/// about all column families including their names and segments, plus a free list
/// for deleted/reclaimed space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasterHeader {
    /// Version of the header format.
    pub version: u8,
    /// Metadata for all column families in the database.
    pub column_families: Vec<ColumnFamilyMetadata>,
    /// Free segments available for reuse.
    pub free_segments: Vec<FreeSegment>,
}

impl MasterHeader {
    /// Creates a new empty master header.
    pub fn new() -> Self {
        Self {
            version: FORMAT_VERSION,
            column_families: Vec::new(),
            free_segments: Vec::new(),
        }
    }

    /// Creates a master header with the given column families.
    pub fn with_column_families(column_families: Vec<ColumnFamilyMetadata>) -> Self {
        Self {
            version: FORMAT_VERSION,
            column_families,
            free_segments: Vec::new(),
        }
    }

    /// Finds the end of the last allocated segment in the file.
    /// Returns `PAGE_SIZE` if no segments exist (first allocation starts after header).
    pub fn end_of_file(&self) -> u64 {
        let mut max_end = PAGE_SIZE as u64;

        for cf in &self.column_families {
            for segment in &cf.segments {
                max_end = max_end.max(segment.end());
            }
        }

        for free_seg in &self.free_segments {
            max_end = max_end.max(free_seg.offset + free_seg.size);
        }

        max_end
    }

    /// Serializes the master header to bytes that fit within one page.
    ///
    /// Format:
    /// - magic (9 bytes)
    /// - version (1 byte)
    /// - `cf_count` (u32)
    /// - metadata entries (variable)
    /// - `free_count` (u32)
    /// - free segment entries (variable)
    /// - CRC32 checksum (4 bytes) at `PAGE_SIZE - 4`
    /// - padding to page size
    ///
    /// Returns error if serialized size exceeds `PAGE_SIZE - 4` (need space for CRC).
    pub fn to_bytes(&self) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(PAGE_SIZE);

        // Magic number
        bytes.extend_from_slice(&MAGIC_NUMBER);

        // Version
        bytes.push(self.version);

        // Column family count
        let cf_count = u32::try_from(self.column_families.len()).expect("too many column families");
        bytes.extend_from_slice(&cf_count.to_le_bytes());

        // Serialize each column family metadata
        for cf in &self.column_families {
            bytes.extend_from_slice(&cf.to_bytes());
        }

        // Free segment count
        let free_count = u32::try_from(self.free_segments.len()).expect("too many free segments");
        bytes.extend_from_slice(&free_count.to_le_bytes());

        // Serialize each free segment
        for free_seg in &self.free_segments {
            bytes.extend_from_slice(&free_seg.to_bytes());
        }

        // Check size constraint (reserve 4 bytes for CRC at the end)
        if bytes.len() > PAGE_SIZE - 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "master header size ({} bytes) exceeds page size minus CRC ({} bytes)",
                    bytes.len(),
                    PAGE_SIZE - 4
                ),
            ));
        }

        // Pad to PAGE_SIZE - 4 with zeros (leaving space for CRC)
        bytes.resize(PAGE_SIZE - 4, 0);

        // Compute CRC32 over all data before the checksum
        let crc = crc32fast::hash(&bytes);
        
        // Append CRC32 at the end
        bytes.extend_from_slice(&crc.to_le_bytes());

        assert_eq!(bytes.len(), PAGE_SIZE);

        Ok(bytes)
    }

    /// Deserializes a master header from bytes.
    ///
    /// Validates magic number, CRC32 checksum, version, and metadata integrity.
    pub fn from_bytes(data: &[u8]) -> io::Result<Self> {
        if data.len() < PAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("data too short: need {}, have {}", PAGE_SIZE, data.len()),
            ));
        }

        // Validate magic number
        if data[0..9] != MAGIC_NUMBER {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid magic number",
            ));
        }

        // Validate CRC32 checksum BEFORE parsing
        // CRC is stored in the last 4 bytes
        let stored_crc = u32::from_le_bytes(data[PAGE_SIZE - 4..PAGE_SIZE].try_into().unwrap());
        let computed_crc = crc32fast::hash(&data[0..PAGE_SIZE - 4]);

        if stored_crc != computed_crc {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("header checksum mismatch: expected {stored_crc:#x}, got {computed_crc:#x}"),
            ));
        }

        // Check version
        let version = data[9];
        if version != FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported format version: {version}"),
            ));
        }

        // Read column family count
        let cf_count = u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize;

        // Deserialize column family metadata entries
        let mut column_families = Vec::with_capacity(cf_count);
        let mut offset = 14;

        for _ in 0..cf_count {
            let (cf_meta, consumed) = ColumnFamilyMetadata::from_bytes(&data[offset..])?;
            column_families.push(cf_meta);
            offset += consumed;
        }

        // Read free segment count
        if offset + 4 > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "insufficient data for free segment count",
            ));
        }
        let free_count = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        // Deserialize free segments
        let mut free_segments = Vec::with_capacity(free_count);
        for _ in 0..free_count {
            let (free_seg, consumed) = FreeSegment::from_bytes(&data[offset..])?;
            free_segments.push(free_seg);
            offset += consumed;
        }

        let header = Self {
            version,
            column_families,
            free_segments,
        };

        // Validate the header
        header.validate()?;

        Ok(header)
    }

    /// Validates the master header for consistency.
    ///
    /// Checks:
    /// - Column family names are non-empty and unique
    /// - Segment offsets are page-aligned
    /// - Segment sizes are positive
    /// - No overlapping segments (CF or free)
    pub fn validate(&self) -> io::Result<()> {
        // Check for empty or duplicate names
        let mut seen_names = std::collections::HashSet::new();
        for cf in &self.column_families {
            if cf.name.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "column family name cannot be empty",
                ));
            }

            if !seen_names.insert(&cf.name) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("duplicate column family name: {}", cf.name),
                ));
            }

            // Validate each segment
            for segment in &cf.segments {
                // Offset should be page-aligned
                if segment.offset % PAGE_SIZE as u64 != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "column family '{}' segment offset {} is not page-aligned",
                            cf.name, segment.offset
                        ),
                    ));
                }

                // Size must be positive
                if segment.size == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("column family '{}' has segment with zero size", cf.name),
                    ));
                }

                // Check for overflow
                segment.offset.checked_add(segment.size).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "column family '{}' segment offset + size overflows",
                            cf.name
                        ),
                    )
                })?;
            }
        }

        // Validate free segments
        for free_seg in &self.free_segments {
            if free_seg.offset % PAGE_SIZE as u64 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "free segment offset {} is not page-aligned",
                        free_seg.offset
                    ),
                ));
            }

            if free_seg.size == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "free segment has zero size",
                ));
            }

            free_seg.offset.checked_add(free_seg.size).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "free segment offset + size overflows",
                )
            })?;
        }

        // Collect all segments (CF and free) and check for overlaps
        let mut all_segments: Vec<(u64, u64, String)> = Vec::new();

        for cf in &self.column_families {
            for segment in &cf.segments {
                all_segments.push((segment.offset, segment.end(), cf.name.clone()));
            }
        }

        for (i, free_seg) in self.free_segments.iter().enumerate() {
            all_segments.push((
                free_seg.offset,
                free_seg.offset + free_seg.size,
                format!("free#{i}"),
            ));
        }

        all_segments.sort_by_key(|(start, _, _)| *start);

        for i in 0..all_segments.len() {
            for j in i + 1..all_segments.len() {
                let (start1, end1, name1) = &all_segments[i];
                let (start2, end2, name2) = &all_segments[j];

                // Check if segments overlap
                if start1 < end2 && start2 < end1 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("segments '{name1}' and '{name2}' overlap"),
                    ));
                }
            }
        }

        Ok(())
    }
}

impl Default for MasterHeader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_header_round_trip() {
        let header = MasterHeader::new();
        let bytes = header.to_bytes().unwrap();

        assert_eq!(bytes.len(), PAGE_SIZE);
        assert_eq!(&bytes[0..9], &MAGIC_NUMBER);
        assert_eq!(bytes[9], FORMAT_VERSION);

        let decoded = MasterHeader::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, header);
    }

    #[test]
    fn test_single_column_family() {
        let cf = ColumnFamilyMetadata::new("users".to_string(), PAGE_SIZE as u64, 1024 * 1024);
        let header = MasterHeader::with_column_families(vec![cf.clone()]);

        let bytes = header.to_bytes().unwrap();
        let decoded = MasterHeader::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.column_families.len(), 1);
        assert_eq!(decoded.column_families[0], cf);
        assert_eq!(decoded.free_segments.len(), 0);
    }

    #[test]
    fn test_multiple_column_families() {
        let cf1 = ColumnFamilyMetadata::new("users".to_string(), PAGE_SIZE as u64, 1024 * 1024);
        let cf2 = ColumnFamilyMetadata::new(
            "products".to_string(),
            PAGE_SIZE as u64 + 1024 * 1024,
            2048 * 1024,
        );
        let cf3 = ColumnFamilyMetadata::new(
            "orders".to_string(),
            PAGE_SIZE as u64 + 1024 * 1024 + 2048 * 1024,
            512 * 1024,
        );

        let header = MasterHeader::with_column_families(vec![cf1, cf2, cf3]);
        let bytes = header.to_bytes().unwrap();
        let decoded = MasterHeader::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.column_families.len(), 3);
        assert_eq!(decoded.column_families[0].name, "users");
        assert_eq!(decoded.column_families[1].name, "products");
        assert_eq!(decoded.column_families[2].name, "orders");
    }

    #[test]
    fn test_long_column_family_names() {
        let long_name = "a".repeat(200);
        let cf = ColumnFamilyMetadata::new(long_name.clone(), PAGE_SIZE as u64, 1024);
        let header = MasterHeader::with_column_families(vec![cf]);

        let bytes = header.to_bytes().unwrap();
        let decoded = MasterHeader::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.column_families[0].name, long_name);
    }

    #[test]
    fn test_invalid_magic_number() {
        let mut bytes = vec![0u8; PAGE_SIZE];
        bytes[0..8].copy_from_slice(b"badmagic");
        bytes[8] = FORMAT_VERSION;

        let result = MasterHeader::from_bytes(&bytes);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_invalid_version() {
        let mut bytes = vec![0u8; PAGE_SIZE];
        bytes[0..9].copy_from_slice(&MAGIC_NUMBER);
        bytes[9] = 255; // Unsupported version

        let result = MasterHeader::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_too_short_data() {
        let bytes = vec![0u8; 100];
        let result = MasterHeader::from_bytes(&bytes);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_validate_empty_name() {
        let cf = ColumnFamilyMetadata::new(String::new(), PAGE_SIZE as u64, 1024);
        let header = MasterHeader::with_column_families(vec![cf]);

        let result = header.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_duplicate_names() {
        let cf1 = ColumnFamilyMetadata::new("users".to_string(), PAGE_SIZE as u64, 1024);
        let cf2 = ColumnFamilyMetadata::new("users".to_string(), PAGE_SIZE as u64 + 2048, 1024);
        let header = MasterHeader::with_column_families(vec![cf1, cf2]);

        let result = header.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_unaligned_offset() {
        let cf = ColumnFamilyMetadata::new("users".to_string(), 1000, 1024);
        let header = MasterHeader::with_column_families(vec![cf]);

        let result = header.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not page-aligned"));
    }

    #[test]
    fn test_validate_zero_size() {
        let cf = ColumnFamilyMetadata::new("users".to_string(), PAGE_SIZE as u64, 0);
        let header = MasterHeader::with_column_families(vec![cf]);

        let result = header.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("zero size"));
    }

    #[test]
    fn test_validate_overlapping_ranges() {
        let cf1 =
            ColumnFamilyMetadata::new("users".to_string(), PAGE_SIZE as u64, PAGE_SIZE as u64 * 2);
        let cf2 = ColumnFamilyMetadata::new(
            "products".to_string(),
            (PAGE_SIZE * 2) as u64,
            PAGE_SIZE as u64 * 2,
        );
        let header = MasterHeader::with_column_families(vec![cf1, cf2]);

        let result = header.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("overlap"));
    }

    #[test]
    fn test_validate_adjacent_ranges_ok() {
        let cf1 =
            ColumnFamilyMetadata::new("users".to_string(), PAGE_SIZE as u64, PAGE_SIZE as u64);
        let cf2 = ColumnFamilyMetadata::new(
            "products".to_string(),
            (PAGE_SIZE * 2) as u64,
            PAGE_SIZE as u64 * 2,
        );
        let header = MasterHeader::with_column_families(vec![cf1, cf2]);

        assert!(header.validate().is_ok());
    }

    #[test]
    fn test_header_too_large() {
        // Create many column families with long names to exceed page size
        let mut cfs = Vec::new();
        for i in 0..100_u64 {
            let name = format!("column_family_with_very_long_name_{i}");
            let offset = PAGE_SIZE as u64 + (i * 1024 * 1024);
            cfs.push(ColumnFamilyMetadata::new(name, offset, 1024 * 1024));
        }

        let header = MasterHeader::with_column_families(cfs);
        let result = header.to_bytes();

        assert!(result.is_err());
    }

    #[test]
    fn test_metadata_serialization() {
        let cf = ColumnFamilyMetadata::new("test".to_string(), 4096, 1024);
        let bytes = cf.to_bytes();

        let (decoded, consumed) = ColumnFamilyMetadata::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, cf);
        assert_eq!(consumed, bytes.len());
    }

    #[test]
    fn test_multi_segment_column_family() {
        let segments = vec![
            Segment::new(PAGE_SIZE as u64, 1024 * 1024),
            Segment::new((PAGE_SIZE as u64) + 2 * 1024 * 1024, 512 * 1024),
            Segment::new((PAGE_SIZE as u64) + 3 * 1024 * 1024, 256 * 1024),
        ];
        let cf = ColumnFamilyMetadata::with_segments("users".to_string(), segments.clone());

        assert_eq!(cf.segments.len(), 3);
        assert_eq!(cf.total_size(), 1024 * 1024 + 512 * 1024 + 256 * 1024);

        let bytes = cf.to_bytes();
        let (decoded, _) = ColumnFamilyMetadata::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.segments, segments);
    }

    #[test]
    fn test_free_segments_serialization() {
        let cf = ColumnFamilyMetadata::new("users".to_string(), PAGE_SIZE as u64, 1024 * 1024);
        let mut header = MasterHeader::with_column_families(vec![cf]);

        header.free_segments = vec![
            FreeSegment::new(PAGE_SIZE as u64 + 2 * 1024 * 1024, 512 * 1024),
            FreeSegment::new(PAGE_SIZE as u64 + 4 * 1024 * 1024, 256 * 1024),
        ];

        let bytes = header.to_bytes().unwrap();
        let decoded = MasterHeader::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.free_segments.len(), 2);
        assert_eq!(
            decoded.free_segments[0].offset,
            PAGE_SIZE as u64 + 2 * 1024 * 1024
        );
        assert_eq!(decoded.free_segments[0].size, 512 * 1024);
        assert_eq!(
            decoded.free_segments[1].offset,
            PAGE_SIZE as u64 + 4 * 1024 * 1024
        );
        assert_eq!(decoded.free_segments[1].size, 256 * 1024);
    }

    #[test]
    fn test_end_of_file_calculation() {
        let mut header = MasterHeader::new();

        // Empty header should return PAGE_SIZE
        assert_eq!(header.end_of_file(), PAGE_SIZE as u64);

        // Add a column family
        let cf1 = ColumnFamilyMetadata::new("users".to_string(), PAGE_SIZE as u64, 1024 * 1024);
        header.column_families.push(cf1);
        assert_eq!(header.end_of_file(), PAGE_SIZE as u64 + 1024 * 1024);

        // Add another column family with multiple segments
        let cf2 = ColumnFamilyMetadata::with_segments(
            "products".to_string(),
            vec![
                Segment::new(PAGE_SIZE as u64 + 2 * 1024 * 1024, 512 * 1024),
                Segment::new(PAGE_SIZE as u64 + 4 * 1024 * 1024, 256 * 1024),
            ],
        );
        header.column_families.push(cf2);
        assert_eq!(
            header.end_of_file(),
            PAGE_SIZE as u64 + 4 * 1024 * 1024 + 256 * 1024
        );

        // Add a free segment beyond current EOF
        header.free_segments.push(FreeSegment::new(
            PAGE_SIZE as u64 + 6 * 1024 * 1024,
            128 * 1024,
        ));
        assert_eq!(
            header.end_of_file(),
            PAGE_SIZE as u64 + 6 * 1024 * 1024 + 128 * 1024
        );
    }

    #[test]
    fn test_segment_overlap_detection() {
        let cf1 = ColumnFamilyMetadata::with_segments(
            "users".to_string(),
            vec![Segment::new(PAGE_SIZE as u64, 1024 * 1024)],
        );
        let cf2 = ColumnFamilyMetadata::with_segments(
            "products".to_string(),
            vec![Segment::new(PAGE_SIZE as u64 + 512 * 1024, 1024 * 1024)],
        );

        let header = MasterHeader::with_column_families(vec![cf1, cf2]);
        let result = header.validate();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("overlap"));
    }

    #[test]
    fn test_free_segment_validation() {
        let cf = ColumnFamilyMetadata::new("users".to_string(), PAGE_SIZE as u64, 1024 * 1024);
        let mut header = MasterHeader::with_column_families(vec![cf]);

        // Test unaligned free segment
        header.free_segments.push(FreeSegment::new(1000, 1024));
        assert!(header.validate().is_err());

        // Test zero-size free segment
        header.free_segments.clear();
        header
            .free_segments
            .push(FreeSegment::new(PAGE_SIZE as u64, 0));
        assert!(header.validate().is_err());
    }

    #[test]
    fn test_segment_end() {
        let segment = Segment::new(4096, 1024);
        assert_eq!(segment.end(), 5120);
    }
}
