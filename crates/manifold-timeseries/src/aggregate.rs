//! Aggregate types and granularity levels for time series downsampling.

use manifold::{TypeName, Value};

/// Time series aggregate containing statistical summaries.
///
/// This is a fixed-width struct (24 bytes) that can be efficiently stored
/// and retrieved from Manifold without serialization overhead.
///
/// # Fields
///
/// - `min`: Minimum value in the time window
/// - `max`: Maximum value in the time window
/// - `sum`: Sum of all values (used to compute average)
/// - `count`: Number of data points aggregated
/// - `last`: Last value seen in the time window
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aggregate {
    /// Minimum value in the aggregation window.
    pub min: f32,
    /// Maximum value in the aggregation window.
    pub max: f32,
    /// Sum of all values in the aggregation window.
    pub sum: f32,
    /// Number of data points in the aggregation window.
    pub count: u64,
    /// Last value in the aggregation window.
    pub last: f32,
}

impl Aggregate {
    /// Creates a new aggregate from a single value.
    pub fn from_value(value: f32) -> Self {
        Self {
            min: value,
            max: value,
            sum: value,
            count: 1,
            last: value,
        }
    }

    /// Creates an empty aggregate (used as a starting point for accumulation).
    pub fn empty() -> Self {
        Self {
            min: f32::INFINITY,
            max: f32::NEG_INFINITY,
            sum: 0.0,
            count: 0,
            last: 0.0,
        }
    }

    /// Accumulates a value into this aggregate.
    pub fn accumulate(&mut self, value: f32) {
        if self.count == 0 {
            *self = Self::from_value(value);
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
            self.sum += value;
            self.count += 1;
            self.last = value;
        }
    }

    /// Merges another aggregate into this one.
    pub fn merge(&mut self, other: &Self) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = *other;
            return;
        }
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
        self.sum += other.sum;
        self.count += other.count;
        self.last = other.last;
    }

    /// Computes the average value.
    ///
    /// Returns `0.0` if count is zero.
    #[allow(clippy::cast_precision_loss)]
    pub fn average(&self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / (self.count as f32)
        }
    }

    /// Returns true if this aggregate contains no data points.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Implements Value trait for efficient Manifold storage.
impl Value for Aggregate {
    type SelfType<'a> = Self;
    type AsBytes<'a> = [u8; 24];

    fn fixed_width() -> Option<usize> {
        Some(24)
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        assert_eq!(data.len(), 24, "Aggregate must be exactly 24 bytes");

        let min = f32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let max = f32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let sum = f32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let count = u64::from_be_bytes([
            data[12], data[13], data[14], data[15], data[16], data[17], data[18], data[19],
        ]);
        let last = f32::from_be_bytes([data[20], data[21], data[22], data[23]]);

        Self {
            min,
            max,
            sum,
            count,
            last,
        }
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'a + 'b,
    {
        let mut bytes = [0u8; 24];
        bytes[0..4].copy_from_slice(&value.min.to_be_bytes());
        bytes[4..8].copy_from_slice(&value.max.to_be_bytes());
        bytes[8..12].copy_from_slice(&value.sum.to_be_bytes());
        bytes[12..20].copy_from_slice(&value.count.to_be_bytes());
        bytes[20..24].copy_from_slice(&value.last.to_be_bytes());
        bytes
    }

    fn type_name() -> TypeName {
        TypeName::new("manifold_timeseries::Aggregate")
    }
}

/// Time granularity levels for downsampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Granularity {
    /// Raw, full-resolution data.
    Raw,
    /// 1-minute aggregates (60 seconds).
    Minute,
    /// 1-hour aggregates (3600 seconds).
    Hour,
    /// 1-day aggregates (86400 seconds).
    Day,
}

impl Granularity {
    /// Returns the duration of this granularity in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        match self {
            Self::Raw => 0, // Raw has no fixed duration
            Self::Minute => 60_000,
            Self::Hour => 3_600_000,
            Self::Day => 86_400_000,
        }
    }

    /// Returns the table suffix for this granularity.
    pub fn table_suffix(&self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
        }
    }

    /// Rounds a timestamp down to the start of the granularity window.
    ///
    /// For example, with `Granularity::Minute`, timestamp `1609459261500` (01:01:01.500)
    /// would be rounded down to `1609459260000` (01:01:00.000).
    pub fn round_down(&self, timestamp_ms: u64) -> u64 {
        if *self == Self::Raw {
            timestamp_ms // No rounding for raw data
        } else {
            let duration = self.duration_ms();
            (timestamp_ms / duration) * duration
        }
    }

    /// Rounds a timestamp up to the end of the granularity window.
    ///
    /// For example, with `Granularity::Minute`, timestamp `1609459261500` (01:01:01.500)
    /// would be rounded up to `1609459320000` (01:02:00.000).
    pub fn round_up(&self, timestamp_ms: u64) -> u64 {
        if *self == Self::Raw {
            timestamp_ms // No rounding for raw data
        } else {
            let duration = self.duration_ms();
            timestamp_ms.div_ceil(duration) * duration
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregate_from_value() {
        let agg = Aggregate::from_value(42.5);
        assert_eq!(agg.min, 42.5);
        assert_eq!(agg.max, 42.5);
        assert_eq!(agg.sum, 42.5);
        assert_eq!(agg.count, 1);
        assert_eq!(agg.last, 42.5);
        assert_eq!(agg.average(), 42.5);
    }

    #[test]
    fn test_aggregate_accumulate() {
        let mut agg = Aggregate::from_value(10.0);
        agg.accumulate(20.0);
        agg.accumulate(5.0);
        agg.accumulate(15.0);

        assert_eq!(agg.min, 5.0);
        assert_eq!(agg.max, 20.0);
        assert_eq!(agg.sum, 50.0);
        assert_eq!(agg.count, 4);
        assert_eq!(agg.last, 15.0);
        assert_eq!(agg.average(), 12.5);
    }

    #[test]
    fn test_aggregate_merge() {
        let mut agg1 = Aggregate::from_value(10.0);
        agg1.accumulate(20.0);

        let mut agg2 = Aggregate::from_value(5.0);
        agg2.accumulate(25.0);

        agg1.merge(&agg2);

        assert_eq!(agg1.min, 5.0);
        assert_eq!(agg1.max, 25.0);
        assert_eq!(agg1.sum, 60.0);
        assert_eq!(agg1.count, 4);
        assert_eq!(agg1.last, 25.0);
    }

    #[test]
    fn test_aggregate_value_trait() {
        let agg = Aggregate {
            min: 1.5,
            max: 10.5,
            sum: 42.0,
            count: 7,
            last: 8.5,
        };

        let bytes = Aggregate::as_bytes(&agg);
        assert_eq!(bytes.len(), 24);

        let decoded = Aggregate::from_bytes(&bytes);
        assert_eq!(decoded, agg);
    }

    #[test]
    fn test_granularity_duration() {
        assert_eq!(Granularity::Raw.duration_ms(), 0);
        assert_eq!(Granularity::Minute.duration_ms(), 60_000);
        assert_eq!(Granularity::Hour.duration_ms(), 3_600_000);
        assert_eq!(Granularity::Day.duration_ms(), 86_400_000);
    }

    #[test]
    fn test_granularity_round_down() {
        let ts = 1_609_459_261_500u64; // 2021-01-01 00:01:01.500 UTC

        assert_eq!(Granularity::Raw.round_down(ts), ts);
        assert_eq!(Granularity::Minute.round_down(ts), 1_609_459_260_000); // 00:01:00.000
        assert_eq!(Granularity::Hour.round_down(ts), 1_609_459_200_000); // 00:00:00.000
        assert_eq!(Granularity::Day.round_down(ts), 1_609_459_200_000); // 00:00:00.000
    }

    #[test]
    fn test_granularity_round_up() {
        let ts = 1_609_459_261_500u64; // 2021-01-01 00:01:01.500 UTC

        assert_eq!(Granularity::Raw.round_up(ts), ts);
        assert_eq!(Granularity::Minute.round_up(ts), 1_609_459_320_000); // 00:02:00.000
        assert_eq!(Granularity::Hour.round_up(ts), 1_609_462_800_000); // 01:00:00.000
        assert_eq!(Granularity::Day.round_up(ts), 1_609_545_600_000); // Next day 00:00:00.000
    }

    #[test]
    fn test_aggregate_empty() {
        let agg = Aggregate::empty();
        assert!(agg.is_empty());
        assert_eq!(agg.count, 0);
        assert_eq!(agg.average(), 0.0);
    }
}
