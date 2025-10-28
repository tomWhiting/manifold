//! Integration traits for external analytics libraries.

use crate::aggregate::{Aggregate, Granularity};
use crate::encoding::TimestampEncoding;
use crate::timeseries::TimeSeriesTableRead;
use manifold::StorageError;

/// Trait for consuming time series data from external analytics libraries.
///
/// This trait provides a generic interface for analytics libraries to access
/// time series data efficiently without needing to understand Manifold's internal structure.
///
/// # Example
///
/// ```rust,no_run
/// use manifold_timeseries::{TimeSeriesSource, TimeSeriesTableRead, AbsoluteEncoding, Granularity};
/// use manifold::column_family::ColumnFamilyDatabase;
///
/// fn analyze_timeseries<'a, S: TimeSeriesSource<'a>>(source: &'a S, series_id: &str) {
///     // External library can iterate over raw data points
///     for point in source.iter_raw(series_id, 0, u64::MAX).unwrap() {
///         let (timestamp, value) = point.unwrap();
///         // Process data point...
///     }
/// }
/// ```
pub trait TimeSeriesSource<'a> {
    /// Iterator over raw time series data points.
    type RawIter: Iterator<Item = Result<(u64, f32), StorageError>>;

    /// Iterator over aggregate data points.
    type AggregateIter: Iterator<Item = Result<(u64, Aggregate), StorageError>>;

    /// Iterates over raw data points for a series within a time range.
    ///
    /// # Arguments
    ///
    /// * `series_id` - Series identifier
    /// * `start_ms` - Start timestamp (inclusive)
    /// * `end_ms` - End timestamp (exclusive)
    fn iter_raw(
        &'a self,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<Self::RawIter, StorageError>;

    /// Iterates over aggregate data points for a series within a time range.
    ///
    /// # Arguments
    ///
    /// * `granularity` - Which aggregation level to query
    /// * `series_id` - Series identifier
    /// * `start_ms` - Start timestamp (inclusive)
    /// * `end_ms` - End timestamp (exclusive)
    fn iter_aggregates(
        &'a self,
        granularity: Granularity,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<Self::AggregateIter, StorageError>;

    /// Returns the number of raw data points in a time range.
    fn count_raw(&self, series_id: &str, start_ms: u64, end_ms: u64)
        -> Result<usize, StorageError>;
}

/// Iterator over raw time series data points.
pub struct RawPointsIter<'a, E: TimestampEncoding> {
    #[allow(dead_code)]
    table_read: &'a TimeSeriesTableRead<E>,
    #[allow(dead_code)]
    series_id: String,
    #[allow(dead_code)]
    start_ms: u64,
    end_ms: u64,
    current_pos: u64,
}

impl<E: TimestampEncoding> Iterator for RawPointsIter<'_, E> {
    type Item = Result<(u64, f32), StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_pos >= self.end_ms {
            return None;
        }

        // Note: This is a placeholder iterator implementation.
        // In production, you'd want to wrap Manifold's actual range iterator.
        // For now, return None to satisfy the trait but mark fields as allowed dead_code
        // since they'll be used when we implement proper iteration.
        None
    }
}

/// Iterator over aggregate time series data points.
pub struct AggregatePointsIter<'a, E: TimestampEncoding> {
    #[allow(dead_code)]
    table_read: &'a TimeSeriesTableRead<E>,
    #[allow(dead_code)]
    granularity: Granularity,
    #[allow(dead_code)]
    series_id: String,
    #[allow(dead_code)]
    start_ms: u64,
    end_ms: u64,
    current_pos: u64,
}

impl<E: TimestampEncoding> Iterator for AggregatePointsIter<'_, E> {
    type Item = Result<(u64, Aggregate), StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_pos >= self.end_ms {
            return None;
        }

        // Note: This is a placeholder iterator implementation.
        // In production, you'd want to wrap Manifold's actual range iterator.
        // For now, return None to satisfy the trait but mark fields as allowed dead_code
        // since they'll be used when we implement proper iteration.
        None
    }
}

impl<'a, E: TimestampEncoding + 'a> TimeSeriesSource<'a> for TimeSeriesTableRead<E> {
    type RawIter = RawPointsIter<'a, E>;
    type AggregateIter = AggregatePointsIter<'a, E>;

    fn iter_raw(
        &'a self,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<Self::RawIter, StorageError> {
        Ok(RawPointsIter {
            table_read: self,
            series_id: series_id.to_string(),
            start_ms,
            end_ms,
            current_pos: start_ms,
        })
    }

    fn iter_aggregates(
        &'a self,
        granularity: Granularity,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<Self::AggregateIter, StorageError> {
        Ok(AggregatePointsIter {
            table_read: self,
            granularity,
            series_id: series_id.to_string(),
            start_ms,
            end_ms,
            current_pos: start_ms,
        })
    }

    fn count_raw(
        &self,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<usize, StorageError> {
        let start_key = (start_ms, series_id);
        let end_key = (end_ms, series_id);

        let mut count = 0;
        for item in self.raw_table().range(start_key..end_key)? {
            item?; // Propagate errors
            count += 1;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::AbsoluteEncoding;
    use manifold::column_family::ColumnFamilyDatabase;
    use tempfile::tempdir;

    #[test]
    fn test_timeseries_source_trait() {
        let dir = tempdir().unwrap();
        let db = ColumnFamilyDatabase::open(dir.path().join("test.db")).unwrap();
        let cf = db.column_family_or_create("metrics").unwrap();

        // Write some test data
        {
            let write_txn = cf.begin_write().unwrap();
            let mut ts =
                crate::timeseries::TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu")
                    .unwrap();

            ts.write("server1", 100_000, 10.0).unwrap();
            ts.write("server1", 200_000, 20.0).unwrap();
            ts.write("server1", 300_000, 30.0).unwrap();

            drop(ts);
            write_txn.commit().unwrap();
        }

        // Use TimeSeriesSource trait
        {
            let read_txn = cf.begin_read().unwrap();
            let ts_read = TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "cpu").unwrap();

            // Test count_raw through trait
            let count = ts_read.count_raw("server1", 0, u64::MAX).unwrap();
            assert_eq!(count, 3);
        }
    }
}
