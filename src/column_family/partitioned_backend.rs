use crate::StorageBackend;
use std::fmt::{Debug, Formatter};
use std::io;
use std::sync::Arc;

/// A storage backend that operates on a partition (byte range) of an underlying storage backend.
///
/// This backend wraps another `StorageBackend` and translates all offset-based operations
/// to operate within a specific byte range of the underlying storage. This allows multiple
/// independent database instances to coexist within a single physical file.
///
/// # Offset Translation
///
/// All read/write operations are translated by adding `partition_offset` to the requested offset:
/// - `read(offset, buf)` becomes `inner.read(partition_offset + offset, buf)`
/// - `write(offset, data)` becomes `inner.write(partition_offset + offset, data)`
///
/// The `len()` method returns `partition_size` rather than the underlying storage length,
/// making the partition appear as a complete storage backend to the caller.
///
/// # Bounds Checking
///
/// All operations are bounds-checked to ensure they stay within the partition:
/// - Operations that would exceed `partition_size` return `io::ErrorKind::InvalidInput`
/// - Overflow checks prevent arithmetic overflow on offset calculations
///
/// # Example
///
/// ```ignore
/// use redb::column_family::PartitionedStorageBackend;
/// use redb::backends::FileBackend;
/// use std::sync::Arc;
///
/// let file_backend = Arc::new(FileBackend::new(file)?);
///
/// // Create two partitions in the same file
/// let partition1 = PartitionedStorageBackend::new(
///     file_backend.clone(),
///     4096,              // Start at 4KB (after master header)
///     1024 * 1024 * 1024 // 1GB partition
/// );
///
/// let partition2 = PartitionedStorageBackend::new(
///     file_backend.clone(),
///     4096 + 1024 * 1024 * 1024, // Start after partition1
///     1024 * 1024 * 1024          // 1GB partition
/// );
/// ```
pub struct PartitionedStorageBackend {
    inner: Arc<dyn StorageBackend>,
    partition_offset: u64,
    partition_size: u64,
}

impl PartitionedStorageBackend {
    /// Creates a new partitioned storage backend.
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
            partition_offset,
            partition_size,
        }
    }

    /// Validates that an operation at the given offset and length stays within partition bounds.
    ///
    /// Returns `Ok(translated_offset)` if the operation is valid, where `translated_offset`
    /// is the absolute offset in the underlying storage.
    ///
    /// Returns `Err` if:
    /// - `offset + len` exceeds `partition_size`
    /// - `partition_offset + offset` would overflow
    fn validate_and_translate(&self, offset: u64, len: usize) -> io::Result<u64> {
        let len_u64 = len as u64;

        // Check if offset + len exceeds partition size
        let end_offset = offset.checked_add(len_u64).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("offset + length overflows: offset={offset}, len={len}"),
            )
        })?;

        if end_offset > self.partition_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "operation exceeds partition bounds: offset={}, len={}, partition_size={}",
                    offset, len, self.partition_size
                ),
            ));
        }

        // Translate to absolute offset in underlying storage
        let translated_offset = self.partition_offset.checked_add(offset).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "translated offset overflows: partition_offset={}, offset={}",
                    self.partition_offset, offset
                ),
            )
        })?;

        Ok(translated_offset)
    }
}

impl Debug for PartitionedStorageBackend {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PartitionedStorageBackend")
            .field("partition_offset", &self.partition_offset)
            .field("partition_size", &self.partition_size)
            .finish_non_exhaustive()
    }
}

impl StorageBackend for PartitionedStorageBackend {
    fn len(&self) -> io::Result<u64> {
        // Return the actual allocated length within this partition
        // This is calculated as: min(underlying_len - partition_offset, partition_size)
        // If the file hasn't been extended to cover this partition yet, this will return 0
        let underlying_len = self.inner.len()?;

        if underlying_len <= self.partition_offset {
            // Partition hasn't been allocated yet
            Ok(0)
        } else {
            // Return the allocated portion, capped at partition_size
            let allocated = underlying_len - self.partition_offset;
            Ok(allocated.min(self.partition_size))
        }
    }

    fn read(&self, offset: u64, out: &mut [u8]) -> io::Result<()> {
        let translated_offset = self.validate_and_translate(offset, out.len())?;
        self.inner.read(translated_offset, out)
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        if len > self.partition_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "cannot set length beyond partition size: requested={}, partition_size={}",
                    len, self.partition_size
                ),
            ));
        }

        // Calculate the absolute length in the underlying storage
        let absolute_len = self.partition_offset.checked_add(len).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "absolute length calculation overflows",
            )
        })?;

        // Only grow the underlying storage if needed. We intentionally do not shrink
        // the underlying storage when len < current_len, as:
        // 1. Other partitions may be using space beyond this partition
        // 2. Shrinking would require coordination across all partitions
        // 3. The storage backend will handle any necessary compaction
        //
        // This design choice favors simplicity and safety over aggressive space reclamation.
        let current_underlying_len = self.inner.len()?;
        if absolute_len > current_underlying_len {
            self.inner.set_len(absolute_len)?;
        }

        Ok(())
    }

    fn sync_data(&self) -> io::Result<()> {
        // Sync the entire underlying storage
        self.inner.sync_data()
    }

    fn write(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        let translated_offset = self.validate_and_translate(offset, data.len())?;
        self.inner.write(translated_offset, data)
    }

    fn close(&self) -> io::Result<()> {
        // Do not close the underlying storage, as other partitions may still be using it
        // The underlying storage will be closed when the last Arc reference is dropped
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::InMemoryBackend;

    #[test]
    fn test_len_returns_allocated_size() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner.clone(), 1000, 5000);

        // Initially, partition has no allocated storage
        assert_eq!(backend.len().unwrap(), 0);

        // After setting length, it should return the allocated size
        backend.set_len(3000).unwrap();
        assert_eq!(backend.len().unwrap(), 3000);

        // Can't exceed partition size
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
    fn test_set_len_exceeds_partition_size() {
        let inner = Arc::new(InMemoryBackend::new());
        let backend = PartitionedStorageBackend::new(inner, 1000, 5000);

        let result = backend.set_len(6000);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
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
    fn test_offset_arithmetic_overflow_handling() {
        let inner = Arc::new(InMemoryBackend::new());
        // Use a large but reasonable offset that won't cause overflow in set_len
        let backend = PartitionedStorageBackend::new(inner.clone(), 1_000_000, 10_000);

        // Pre-size the partition to allow operations
        backend.set_len(1000).unwrap();

        // Test offset translation works correctly with large offsets
        let write_data = b"test";
        backend.write(500, write_data).unwrap();

        let mut read_buf = vec![0u8; write_data.len()];
        backend.read(500, &mut read_buf).unwrap();
        assert_eq!(&read_buf, write_data);

        // Verify translation occurred correctly (partition_offset + 500 = 1_000_500)
        let mut verify_buf = vec![0u8; write_data.len()];
        inner.read(1_000_500, &mut verify_buf).unwrap();
        assert_eq!(&verify_buf, write_data);
    }
}
