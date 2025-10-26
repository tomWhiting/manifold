use crate::StorageBackend;
use crate::column_family::header::Segment;
use std::fmt::{Debug, Formatter};
use std::io;
use std::sync::{Arc, RwLock};

/// A storage backend that operates on one or more segments within an underlying storage backend.
///
/// This backend supports multi-segment column families where data can be stored in non-contiguous
/// regions of the file. This enables instant growth without moving existing data - new segments
/// are simply appended at the end of the file.
///
/// # Virtual Offset Translation
///
/// The backend presents a continuous virtual address space to the caller, mapping it to
/// physical segments transparently:
/// - Virtual offset 0-1GB might map to physical offset 4KB-1GB (segment 1)
/// - Virtual offset 1GB-1.5GB might map to physical offset 5GB-5.5GB (segment 2)
///
/// # Auto-Expansion
///
/// When a write would exceed the total capacity of all segments, the backend can automatically
/// request a new segment via the expansion callback. This makes growth transparent to the
/// Database instance.
///
/// # Example
///
/// ```ignore
/// use redb::column_family::{PartitionedStorageBackend, Segment};
/// use redb::backends::FileBackend;
/// use std::sync::Arc;
///
/// let file_backend = Arc::new(FileBackend::new(file)?);
///
/// let segments = vec![
///     Segment::new(4096, 1024 * 1024 * 1024),  // 1GB at 4KB
///     Segment::new(5 * 1024 * 1024 * 1024, 512 * 1024 * 1024), // 512MB at 5GB
/// ];
///
/// let backend = PartitionedStorageBackend::with_segments(
///     file_backend,
///     segments,
///     None, // No auto-expansion
/// );
/// ```
pub struct PartitionedStorageBackend {
    inner: Arc<dyn StorageBackend>,
    segments: Arc<RwLock<Vec<Segment>>>,
    expansion_callback: Option<Arc<dyn Fn(u64) -> io::Result<Segment> + Send + Sync>>,
}

impl PartitionedStorageBackend {
    /// Creates a new partitioned storage backend with a single segment (for backward compatibility).
    ///
    /// # Arguments
    ///
    /// * `inner` - The underlying storage backend (wrapped in Arc for sharing)
    /// * `partition_offset` - The absolute byte offset where this partition begins
    /// * `partition_size` - The size of this partition in bytes
    ///
    /// # Panics
    ///
    /// Panics if `partition_offset + partition_size` would overflow `u64`.
    pub fn new(inner: Arc<dyn StorageBackend>, partition_offset: u64, partition_size: u64) -> Self {
        // Verify no overflow in partition bounds
        partition_offset
            .checked_add(partition_size)
            .expect("partition_offset + partition_size overflows u64");

        Self {
            inner,
            segments: Arc::new(RwLock::new(vec![Segment::new(
                partition_offset,
                partition_size,
            )])),
            expansion_callback: None,
        }
    }

    /// Creates a new partitioned storage backend with multiple segments.
    ///
    /// # Arguments
    ///
    /// * `inner` - The underlying storage backend (wrapped in Arc for sharing)
    /// * `segments` - Vector of segments making up this partition
    /// * `expansion_callback` - Optional callback to request new segments for auto-expansion
    pub fn with_segments(
        inner: Arc<dyn StorageBackend>,
        segments: Vec<Segment>,
        expansion_callback: Option<Arc<dyn Fn(u64) -> io::Result<Segment> + Send + Sync>>,
    ) -> Self {
        Self {
            inner,
            segments: Arc::new(RwLock::new(segments)),
            expansion_callback,
        }
    }

    /// Returns the total size of all segments (virtual address space size).
    fn total_size(&self) -> u64 {
        let segments = self.segments.read().unwrap();
        segments.iter().map(|s| s.size).sum()
    }

    /// Maps a virtual offset to a physical offset in a specific segment.
    ///
    /// Returns `(physical_offset, remaining_in_segment)` on success.
    fn virtual_to_physical(&self, virtual_offset: u64) -> io::Result<(u64, u64)> {
        let segments = self.segments.read().unwrap();
        let mut current_virtual = 0u64;

        for segment in segments.iter() {
            let segment_end = current_virtual + segment.size;

            if virtual_offset < segment_end {
                // Found the segment containing this virtual offset
                let offset_in_segment = virtual_offset - current_virtual;
                let physical_offset = segment.offset + offset_in_segment;
                let remaining_in_segment = segment.size - offset_in_segment;
                return Ok((physical_offset, remaining_in_segment));
            }

            current_virtual = segment_end;
        }

        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("virtual offset {virtual_offset} exceeds total size {current_virtual}"),
        ))
    }

    /// Attempts to expand the partition by requesting a new segment.
    fn try_expand(&self, requested_size: u64) -> io::Result<()> {
        if let Some(callback) = &self.expansion_callback {
            let new_segment = callback(requested_size)?;
            let mut segments = self.segments.write().unwrap();
            segments.push(new_segment);
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot expand partition: no expansion callback configured",
            ))
        }
    }
}

impl Debug for PartitionedStorageBackend {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let segments = self.segments.read().unwrap();
        f.debug_struct("PartitionedStorageBackend")
            .field("segment_count", &segments.len())
            .field("total_size", &self.total_size())
            .finish_non_exhaustive()
    }
}

impl StorageBackend for PartitionedStorageBackend {
    fn len(&self) -> io::Result<u64> {
        // Return the actual allocated length across all segments
        let underlying_len = self.inner.len()?;
        let segments = self.segments.read().unwrap();

        let mut total_allocated = 0u64;

        for segment in segments.iter() {
            if underlying_len <= segment.offset {
                // This segment hasn't been allocated yet
                break;
            }

            let segment_allocated = (underlying_len - segment.offset).min(segment.size);
            total_allocated += segment_allocated;

            // If this segment isn't fully allocated, stop counting
            if segment_allocated < segment.size {
                break;
            }
        }

        Ok(total_allocated)
    }

    fn read(&self, offset: u64, out: &mut [u8]) -> io::Result<()> {
        let mut bytes_read = 0;
        let mut current_offset = offset;

        while bytes_read < out.len() {
            let (physical_offset, remaining_in_segment) =
                self.virtual_to_physical(current_offset)?;

            #[allow(clippy::cast_possible_truncation)]
            let bytes_to_read =
                (out.len() - bytes_read).min(remaining_in_segment.min(usize::MAX as u64) as usize);

            self.inner.read(
                physical_offset,
                &mut out[bytes_read..bytes_read + bytes_to_read],
            )?;

            bytes_read += bytes_to_read;
            current_offset += bytes_to_read as u64;
        }

        Ok(())
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        let current_total = self.total_size();

        // If requested length exceeds current capacity, try to expand
        if len > current_total {
            let needed = len - current_total;
            // Add 10% buffer to reduce frequent small expansions
            let allocation_size = needed + (needed / 10).max(1024 * 1024); // At least 1MB buffer
            self.try_expand(allocation_size)?;
        }

        // Calculate the maximum physical end we need to allocate
        let segments = self.segments.read().unwrap();
        let mut remaining = len;
        let mut max_physical_end = 0u64;

        for segment in segments.iter() {
            if remaining == 0 {
                break;
            }

            let used_in_segment = remaining.min(segment.size);
            let physical_end = segment.offset + used_in_segment;
            max_physical_end = max_physical_end.max(physical_end);

            remaining = remaining.saturating_sub(used_in_segment);
        }

        // Only grow the underlying storage if needed (no-shrink policy)
        let current_underlying_len = self.inner.len()?;
        if max_physical_end > current_underlying_len {
            self.inner.set_len(max_physical_end)?;
        }

        Ok(())
    }

    fn sync_data(&self) -> io::Result<()> {
        self.inner.sync_data()
    }

    fn write(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        let mut bytes_written = 0;
        let mut current_offset = offset;

        while bytes_written < data.len() {
            let (physical_offset, remaining_in_segment) =
                self.virtual_to_physical(current_offset)?;

            #[allow(clippy::cast_possible_truncation)]
            let bytes_to_write = (data.len() - bytes_written)
                .min(remaining_in_segment.min(usize::MAX as u64) as usize);

            self.inner.write(
                physical_offset,
                &data[bytes_written..bytes_written + bytes_to_write],
            )?;

            bytes_written += bytes_to_write;
            current_offset += bytes_to_write as u64;
        }

        Ok(())
    }

    fn close(&self) -> io::Result<()> {
        // Do not close the underlying storage, as other partitions may still be using it
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::InMemoryBackend;

    #[test]
    fn test_single_segment_len() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner.clone(), 1000, 5000);

        // Initially, partition has no allocated storage
        assert_eq!(backend.len().unwrap(), 0);

        // After setting length, it should return the allocated size
        backend.set_len(3000).unwrap();
        assert_eq!(backend.len().unwrap(), 3000);

        backend.set_len(5000).unwrap();
        assert_eq!(backend.len().unwrap(), 5000);
    }

    #[test]
    fn test_read_write_with_offset_translation() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner.clone(), 1000, 5000);

        // Pre-size the underlying storage to accommodate writes
        backend.set_len(200).unwrap();

        // Write data at partition offset 100
        let write_data = b"hello world";
        backend.write(100, write_data).unwrap();

        // Verify it was written to absolute offset 1100 in underlying storage
        let mut verify_buf = vec![0u8; write_data.len()];
        inner.read(1100, &mut verify_buf).unwrap();
        assert_eq!(&verify_buf, write_data);

        // Read back through the partition
        let mut read_buf = vec![0u8; write_data.len()];
        backend.read(100, &mut read_buf).unwrap();
        assert_eq!(&read_buf, write_data);
    }

    #[test]
    fn test_read_at_offset_zero() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner.clone(), 1000, 5000);

        // Pre-size the underlying storage
        backend.set_len(100).unwrap();

        let write_data = b"start";
        backend.write(0, write_data).unwrap();

        // Verify translation to absolute offset 1000
        let mut verify_buf = vec![0u8; write_data.len()];
        inner.read(1000, &mut verify_buf).unwrap();
        assert_eq!(&verify_buf, write_data);
    }

    #[test]
    fn test_read_at_partition_end() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner.clone(), 1000, 5000);

        // Pre-size to full partition
        backend.set_len(5000).unwrap();

        // Write exactly at the end of the partition (offset 4995, length 5)
        let write_data = b"end!_";
        backend.write(4995, write_data).unwrap();

        let mut read_buf = vec![0u8; write_data.len()];
        backend.read(4995, &mut read_buf).unwrap();
        assert_eq!(&read_buf, write_data);
    }

    #[test]
    fn test_write_exceeds_partition_size() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner, 1000, 5000);

        // Try to write beyond partition size
        let write_data = b"overflow";
        let result = backend.write(4996, write_data);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_read_exceeds_partition_size() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner, 1000, 5000);

        let mut buf = vec![0u8; 100];
        let result = backend.read(4950, &mut buf);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_set_len_within_partition() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner.clone(), 1000, 5000);

        backend.set_len(3000).unwrap();

        // Underlying storage should be extended to 1000 + 3000 = 4000
        assert_eq!(inner.len().unwrap(), 4000);
    }

    #[test]
    fn test_set_len_without_expansion_callback() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner, 1000, 5000);

        // Without expansion callback, cannot exceed initial size
        let result = backend.set_len(6000);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_partitions_same_storage() {
        let inner = Arc::new(InMemoryBackend::new());

        let partition1 = PartitionedStorageBackend::new(inner.clone(), 0, 1000);
        let partition2 = PartitionedStorageBackend::new(inner.clone(), 1000, 1000);

        // Pre-size both partitions
        partition1.set_len(200).unwrap();
        partition2.set_len(200).unwrap();

        // Write to partition 1
        partition1.write(100, b"partition1").unwrap();

        // Write to partition 2
        partition2.write(100, b"partition2").unwrap();

        // Read back from partition 1
        let mut buf1 = vec![0u8; 10];
        partition1.read(100, &mut buf1).unwrap();
        assert_eq!(&buf1, b"partition1");

        // Read back from partition 2
        let mut buf2 = vec![0u8; 10];
        partition2.read(100, &mut buf2).unwrap();
        assert_eq!(&buf2, b"partition2");

        // Verify they're at different absolute offsets
        let mut verify1 = vec![0u8; 10];
        inner.read(100, &mut verify1).unwrap();
        assert_eq!(&verify1, b"partition1");

        let mut verify2 = vec![0u8; 10];
        inner.read(1100, &mut verify2).unwrap();
        assert_eq!(&verify2, b"partition2");
    }

    #[test]
    fn test_partition_isolation() {
        let inner = Arc::new(InMemoryBackend::new());

        let partition1 = PartitionedStorageBackend::new(inner.clone(), 0, 1000);
        let partition2 = PartitionedStorageBackend::new(inner.clone(), 1000, 1000);

        // Each partition initially reports 0 length
        assert_eq!(partition1.len().unwrap(), 0);
        assert_eq!(partition2.len().unwrap(), 0);

        // Grow partition 1
        partition1.set_len(800).unwrap();

        // Partition 1 should report its allocated size
        assert_eq!(partition1.len().unwrap(), 800);

        // Partition 2 should still report 0 (not affected by partition 1)
        assert_eq!(partition2.len().unwrap(), 0);
    }

    #[test]
    fn test_sync_delegates_to_inner() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner, 1000, 5000);

        // sync_data should not error (InMemoryBackend sync is no-op)
        assert!(backend.sync_data().is_ok());
    }

    #[test]
    fn test_close_does_not_close_inner() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner.clone(), 1000, 5000);

        // Close the partition
        backend.close().unwrap();

        // Inner should still be usable (pre-size it first for the read test)
        inner.set_len(100).unwrap();
        let mut buf = vec![0u8; 10];
        assert!(inner.read(0, &mut buf).is_ok());
    }

    #[test]
    #[should_panic(expected = "partition_offset + partition_size overflows u64")]
    fn test_construction_overflow_panics() {
        let inner = Arc::new(InMemoryBackend::new());
        PartitionedStorageBackend::new(inner, u64::MAX, 1);
    }

    #[test]
    fn test_multi_segment_read_write() {
        let inner = Arc::new(InMemoryBackend::new());

        let segments = vec![
            Segment::new(1000, 1000),  // Virtual 0-1000 -> Physical 1000-2000
            Segment::new(5000, 1000),  // Virtual 1000-2000 -> Physical 5000-6000
            Segment::new(10000, 1000), // Virtual 2000-3000 -> Physical 10000-11000
        ];

        let backend = PartitionedStorageBackend::with_segments(inner.clone(), segments, None);
        backend.set_len(3000).unwrap();

        // Write data that spans multiple segments
        // Create 200 bytes of data so it definitely spans segments
        let mut data = Vec::new();
        for i in 0u8..200 {
            data.push(i);
        }

        // Write starting at virtual offset 900 (100 bytes before segment boundary)
        backend.write(900, &data).unwrap();

        // Read it back
        let mut read_buf = vec![0u8; data.len()];
        backend.read(900, &mut read_buf).unwrap();
        assert_eq!(&read_buf, &data);

        // Verify it was written to correct physical locations
        // Virtual 900-1000 (100 bytes) -> Physical 1900-2000 (end of segment 1)
        // Virtual 1000-1100 (100 bytes) -> Physical 5000-5100 (start of segment 2)
        let first_segment_bytes = 100; // bytes from virtual 900-1000

        let mut verify1 = vec![0u8; first_segment_bytes];
        inner.read(1900, &mut verify1).unwrap(); // Physical 1000 + 900 = 1900
        assert_eq!(&verify1, &data[..first_segment_bytes]);

        let mut verify2 = vec![0u8; 100]; // Next 100 bytes in segment 2
        inner.read(5000, &mut verify2).unwrap(); // Start of segment 2
        assert_eq!(
            &verify2,
            &data[first_segment_bytes..first_segment_bytes + 100]
        );
    }

    #[test]
    fn test_multi_segment_total_size() {
        let inner = Arc::new(InMemoryBackend::new());

        let segments = vec![
            Segment::new(1000, 1024),
            Segment::new(5000, 2048),
            Segment::new(10000, 512),
        ];

        let backend = PartitionedStorageBackend::with_segments(inner, segments, None);
        assert_eq!(backend.total_size(), 1024 + 2048 + 512);
    }

    #[test]
    fn test_virtual_to_physical_mapping() {
        let inner = Arc::new(InMemoryBackend::new());

        let segments = vec![Segment::new(4096, 1000), Segment::new(8192, 500)];

        let backend = PartitionedStorageBackend::with_segments(inner, segments, None);

        // Virtual offset 0 -> Physical 4096
        let (phys, rem) = backend.virtual_to_physical(0).unwrap();
        assert_eq!(phys, 4096);
        assert_eq!(rem, 1000);

        // Virtual offset 999 -> Physical 5095 (end of first segment)
        let (phys, rem) = backend.virtual_to_physical(999).unwrap();
        assert_eq!(phys, 5095);
        assert_eq!(rem, 1);

        // Virtual offset 1000 -> Physical 8192 (start of second segment)
        let (phys, rem) = backend.virtual_to_physical(1000).unwrap();
        assert_eq!(phys, 8192);
        assert_eq!(rem, 500);

        // Virtual offset 1499 -> Physical 8691 (end of second segment)
        let (phys, rem) = backend.virtual_to_physical(1499).unwrap();
        assert_eq!(phys, 8691);
        assert_eq!(rem, 1);

        // Virtual offset beyond capacity should fail
        assert!(backend.virtual_to_physical(1500).is_err());
    }
}
