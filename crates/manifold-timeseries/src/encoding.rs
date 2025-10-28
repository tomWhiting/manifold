//! Timestamp encoding strategies for time series keys.

use std::io;

/// Error type for encoding/decoding operations.
#[derive(Debug)]
pub enum EncodingError {
    /// Invalid encoded data.
    InvalidData(String),
    /// IO error during encoding/decoding.
    Io(io::Error),
}

impl std::fmt::Display for EncodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidData(msg) => write!(f, "Invalid encoded data: {}", msg),
            Self::Io(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for EncodingError {}

impl From<io::Error> for EncodingError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Trait for encoding timestamps into sortable byte representations.
///
/// Different encoding strategies optimize for different use cases:
/// - `AbsoluteEncoding`: Simple, fast, supports random access
/// - `DeltaEncoding`: Space-efficient for dense regular-interval data
pub trait TimestampEncoding: Send + Sync {
    /// Encodes a timestamp (milliseconds since epoch) into a sortable byte representation.
    fn encode(timestamp: u64) -> Vec<u8>;
    
    /// Decodes a timestamp from its byte representation.
    fn decode(bytes: &[u8]) -> Result<u64, EncodingError>;
    
    /// Returns true if this encoding supports direct seeking to arbitrary timestamps.
    fn supports_random_access() -> bool;
}

/// Absolute timestamp encoding using big-endian u64.
///
/// **Characteristics:**
/// - Fixed 8-byte encoding
/// - Lexicographically sortable
/// - Fast encoding/decoding (essentially a memcpy)
/// - Supports random access (direct seeking to any timestamp)
///
/// **Best for:**
/// - Sparse, irregular time series
/// - Workloads requiring random access
/// - When simplicity and speed matter more than storage space
#[derive(Debug, Clone, Copy)]
pub struct AbsoluteEncoding;

impl TimestampEncoding for AbsoluteEncoding {
    fn encode(timestamp: u64) -> Vec<u8> {
        timestamp.to_be_bytes().to_vec()
    }
    
    fn decode(bytes: &[u8]) -> Result<u64, EncodingError> {
        if bytes.len() != 8 {
            return Err(EncodingError::InvalidData(format!(
                "Expected 8 bytes for AbsoluteEncoding, got {}",
                bytes.len()
            )));
        }
        let array: [u8; 8] = bytes.try_into().map_err(|_| {
            EncodingError::InvalidData("Failed to convert bytes to array".to_string())
        })?;
        Ok(u64::from_be_bytes(array))
    }
    
    fn supports_random_access() -> bool {
        true
    }
}

/// Delta encoding with varint compression and periodic checkpoints.
///
/// **Encoding format:**
/// - Checkpoint points stored every N entries (default: 1000)
/// - Each checkpoint: 8-byte absolute timestamp
/// - Between checkpoints: varint-encoded deltas
///
/// **Characteristics:**
/// - Variable-length encoding (1-9 bytes per delta)
/// - Space-efficient for regular intervals
/// - Requires checkpoint lookup for range queries
///
/// **Best for:**
/// - Dense, regular-interval data (e.g., 1-second IoT sensors)
/// - Storage-constrained environments
/// - Sequential scan workloads
///
/// **Not recommended for:**
/// - Sparse, irregular data
/// - Heavy random access patterns
#[derive(Debug, Clone, Copy)]
pub struct DeltaEncoding;

/// Checkpoint interval for delta encoding (number of points between checkpoints).
pub const DELTA_CHECKPOINT_INTERVAL: usize = 1000;

impl DeltaEncoding {
    /// Encodes a delta using unsigned varint encoding.
    pub fn encode_varint(value: u64, buf: &mut Vec<u8>) {
        let mut n = value;
        while n >= 0x80 {
            buf.push((n as u8) | 0x80);
            n >>= 7;
        }
        buf.push(n as u8);
    }
    
    /// Decodes a varint from bytes, returning (value, bytes_consumed).
    pub fn decode_varint(bytes: &[u8]) -> Result<(u64, usize), EncodingError> {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        
        for (i, &byte) in bytes.iter().enumerate() {
            if shift >= 64 {
                return Err(EncodingError::InvalidData(
                    "Varint overflow".to_string()
                ));
            }
            
            let value = u64::from(byte & 0x7F);
            result |= value << shift;
            
            if (byte & 0x80) == 0 {
                return Ok((result, i + 1));
            }
            
            shift += 7;
        }
        
        Err(EncodingError::InvalidData(
            "Incomplete varint".to_string()
        ))
    }
}

impl TimestampEncoding for DeltaEncoding {
    fn encode(timestamp: u64) -> Vec<u8> {
        // For single-point encoding, we just store the absolute value
        // In practice, delta encoding works best when encoding sequences,
        // but we need to support the trait interface
        timestamp.to_be_bytes().to_vec()
    }
    
    fn decode(bytes: &[u8]) -> Result<u64, EncodingError> {
        if bytes.len() != 8 {
            return Err(EncodingError::InvalidData(format!(
                "Expected 8 bytes for DeltaEncoding checkpoint, got {}",
                bytes.len()
            )));
        }
        let array: [u8; 8] = bytes.try_into().map_err(|_| {
            EncodingError::InvalidData("Failed to convert bytes to array".to_string())
        })?;
        Ok(u64::from_be_bytes(array))
    }
    
    fn supports_random_access() -> bool {
        // Delta encoding requires checkpoint lookup, not true random access
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_absolute_encoding() {
        let timestamp = 1_609_459_200_000u64; // 2021-01-01 00:00:00 UTC in milliseconds
        
        let encoded = AbsoluteEncoding::encode(timestamp);
        assert_eq!(encoded.len(), 8);
        
        let decoded = AbsoluteEncoding::decode(&encoded).unwrap();
        assert_eq!(decoded, timestamp);
    }

    #[test]
    fn test_absolute_encoding_sortable() {
        let ts1 = 1_000_000_000u64;
        let ts2 = 2_000_000_000u64;
        let ts3 = 3_000_000_000u64;
        
        let enc1 = AbsoluteEncoding::encode(ts1);
        let enc2 = AbsoluteEncoding::encode(ts2);
        let enc3 = AbsoluteEncoding::encode(ts3);
        
        // Lexicographic ordering should match timestamp ordering
        assert!(enc1 < enc2);
        assert!(enc2 < enc3);
        assert!(enc1 < enc3);
    }

    #[test]
    fn test_absolute_encoding_invalid_length() {
        let bytes = vec![1, 2, 3]; // Too short
        assert!(AbsoluteEncoding::decode(&bytes).is_err());
    }

    #[test]
    fn test_delta_encoding_basic() {
        let timestamp = 1_609_459_200_000u64;
        
        let encoded = DeltaEncoding::encode(timestamp);
        assert_eq!(encoded.len(), 8);
        
        let decoded = DeltaEncoding::decode(&encoded).unwrap();
        assert_eq!(decoded, timestamp);
    }

    #[test]
    fn test_varint_encoding() {
        let test_cases = vec![
            0u64,
            127,
            128,
            16383,
            16384,
            2_097_151,
            2_097_152,
            268_435_455,
            268_435_456,
        ];
        
        for value in test_cases {
            let mut buf = Vec::new();
            DeltaEncoding::encode_varint(value, &mut buf);
            let (decoded, consumed) = DeltaEncoding::decode_varint(&buf).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(consumed, buf.len());
        }
    }

    #[test]
    fn test_encoding_capabilities() {
        assert!(AbsoluteEncoding::supports_random_access());
        assert!(!DeltaEncoding::supports_random_access());
    }
}
