//! Downsampling logic for time series aggregation.
//!
//! This module provides methods to compute aggregates from raw data or lower-granularity
//! aggregates and write them to higher-granularity tables.

use crate::aggregate::{Aggregate, Granularity};
use crate::encoding::TimestampEncoding;
use crate::timeseries::TimeSeriesTable;
use manifold::{ReadableTable, StorageError};
use std::collections::HashMap;

impl<E: TimestampEncoding> TimeSeriesTable<'_, E> {
    /// Downsamples raw data to minute-level aggregates.
    ///
    /// Reads raw data points for the specified series within the time range,
    /// groups them by minute, computes aggregates, and writes them to the minute table.
    ///
    /// # Arguments
    ///
    /// * `series_id` - Series identifier to downsample
    /// * `start_ms` - Start timestamp (inclusive)
    /// * `end_ms` - End timestamp (exclusive)
    ///
    /// # Returns
    ///
    /// Number of minute aggregates written
    pub fn downsample_to_minute(
        &mut self,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<usize, StorageError> {
        self.downsample_range(series_id, start_ms, end_ms, Granularity::Minute)
    }

    /// Downsamples minute-level aggregates to hour-level aggregates.
    ///
    /// # Arguments
    ///
    /// * `series_id` - Series identifier to downsample
    /// * `start_ms` - Start timestamp (inclusive)
    /// * `end_ms` - End timestamp (exclusive)
    ///
    /// # Returns
    ///
    /// Number of hour aggregates written
    pub fn downsample_minute_to_hour(
        &mut self,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<usize, StorageError> {
        self.downsample_aggregates(
            series_id,
            start_ms,
            end_ms,
            Granularity::Minute,
            Granularity::Hour,
        )
    }

    /// Downsamples hour-level aggregates to day-level aggregates.
    ///
    /// # Arguments
    ///
    /// * `series_id` - Series identifier to downsample
    /// * `start_ms` - Start timestamp (inclusive)
    /// * `end_ms` - End timestamp (exclusive)
    ///
    /// # Returns
    ///
    /// Number of day aggregates written
    pub fn downsample_hour_to_day(
        &mut self,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<usize, StorageError> {
        self.downsample_aggregates(
            series_id,
            start_ms,
            end_ms,
            Granularity::Hour,
            Granularity::Day,
        )
    }

    /// Internal helper: Downsamples raw data to a target granularity.
    fn downsample_range(
        &mut self,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
        target: Granularity,
    ) -> Result<usize, StorageError> {
        if target == Granularity::Raw {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Cannot downsample to Raw granularity",
            )));
        }

        // Group raw data points by target granularity windows
        let mut buckets: HashMap<u64, Aggregate> = HashMap::new();

        let start_key = (start_ms, series_id);
        let end_key = (end_ms, series_id);

        for item in self.raw.range(start_key..end_key)? {
            let (key_guard, value_guard) = item?;
            let (timestamp, sid) = key_guard.value();

            // Only process the requested series
            if sid != series_id {
                continue;
            }

            let value = value_guard.value();
            let bucket_ts = target.round_down(timestamp);

            buckets
                .entry(bucket_ts)
                .or_insert_with(Aggregate::empty)
                .accumulate(value);
        }

        // Write aggregates to the target table
        let target_table = match target {
            Granularity::Raw => unreachable!(),
            Granularity::Minute => &mut self.minute,
            Granularity::Hour => &mut self.hour,
            Granularity::Day => &mut self.day,
        };

        let count = buckets.len();
        for (bucket_ts, aggregate) in buckets {
            target_table.insert((bucket_ts, series_id), &aggregate)?;
        }

        Ok(count)
    }

    /// Internal helper: Downsamples from one aggregate granularity to a coarser one.
    fn downsample_aggregates(
        &mut self,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
        source: Granularity,
        target: Granularity,
    ) -> Result<usize, StorageError> {
        if source == Granularity::Raw {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Use downsample_to_minute for raw data",
            )));
        }

        if target == Granularity::Raw {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Cannot downsample to Raw granularity",
            )));
        }

        if target.duration_ms() <= source.duration_ms() {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Target granularity must be coarser than source",
            )));
        }

        // Group source aggregates by target granularity windows
        let mut buckets: HashMap<u64, Aggregate> = HashMap::new();

        let source_table = match source {
            Granularity::Raw => unreachable!(),
            Granularity::Minute => &self.minute,
            Granularity::Hour => &self.hour,
            Granularity::Day => &self.day,
        };

        let start_key = (start_ms, series_id);
        let end_key = (end_ms, series_id);

        for item in source_table.range(start_key..end_key)? {
            let (key_guard, agg_guard) = item?;
            let (timestamp, sid) = key_guard.value();

            // Only process the requested series
            if sid != series_id {
                continue;
            }

            let source_agg = agg_guard.value();
            let bucket_ts = target.round_down(timestamp);

            buckets
                .entry(bucket_ts)
                .or_insert_with(Aggregate::empty)
                .merge(&source_agg);
        }

        // Write aggregates to the target table
        let target_table = match target {
            Granularity::Raw => unreachable!(),
            Granularity::Minute => &mut self.minute,
            Granularity::Hour => &mut self.hour,
            Granularity::Day => &mut self.day,
        };

        let count = buckets.len();
        for (bucket_ts, aggregate) in buckets {
            target_table.insert((bucket_ts, series_id), &aggregate)?;
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
    fn test_downsample_to_minute() {
        let dir = tempdir().unwrap();
        let db = ColumnFamilyDatabase::open(dir.path().join("test.db")).unwrap();
        let cf = db.column_family_or_create("metrics").unwrap();

        // Write some raw data points across 2 minutes
        {
            let write_txn = cf.begin_write().unwrap();
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu").unwrap();

            // Minute 1: timestamps 60000-60999 (3 points)
            ts.write("server1", 60_000, 10.0).unwrap();
            ts.write("server1", 60_500, 20.0).unwrap();
            ts.write("server1", 60_999, 30.0).unwrap();

            // Minute 2: timestamps 120000-120999 (2 points)
            ts.write("server1", 120_000, 15.0).unwrap();
            ts.write("server1", 120_500, 25.0).unwrap();

            drop(ts);
            write_txn.commit().unwrap();
        }

        // Downsample to minutes
        {
            let write_txn = cf.begin_write().unwrap();
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu").unwrap();

            let count = ts.downsample_to_minute("server1", 0, 200_000).unwrap();
            assert_eq!(count, 2); // 2 minute buckets

            drop(ts);
            write_txn.commit().unwrap();
        }

        // Verify minute aggregates
        {
            let read_txn = cf.begin_read().unwrap();
            let ts_read =
                crate::timeseries::TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "cpu")
                    .unwrap();

            // Minute 1 aggregate (rounded to 60000)
            let agg1 = ts_read
                .get_aggregate(Granularity::Minute, "server1", 60_500)
                .unwrap()
                .unwrap();
            assert_eq!(agg1.min, 10.0);
            assert_eq!(agg1.max, 30.0);
            assert_eq!(agg1.sum, 60.0);
            assert_eq!(agg1.count, 3);
            assert_eq!(agg1.average(), 20.0);

            // Minute 2 aggregate (rounded to 120000)
            let agg2 = ts_read
                .get_aggregate(Granularity::Minute, "server1", 120_500)
                .unwrap()
                .unwrap();
            assert_eq!(agg2.min, 15.0);
            assert_eq!(agg2.max, 25.0);
            assert_eq!(agg2.sum, 40.0);
            assert_eq!(agg2.count, 2);
            assert_eq!(agg2.average(), 20.0);
        }
    }

    #[test]
    fn test_downsample_minute_to_hour() {
        let dir = tempdir().unwrap();
        let db = ColumnFamilyDatabase::open(dir.path().join("test.db")).unwrap();
        let cf = db.column_family_or_create("metrics").unwrap();

        // Manually write minute aggregates
        {
            let write_txn = cf.begin_write().unwrap();
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu").unwrap();

            // Hour 1: 3 minute aggregates
            ts.minute_table_mut()
                .insert(
                    (60_000, "server1"),
                    &Aggregate {
                        min: 10.0,
                        max: 20.0,
                        sum: 100.0,
                        count: 10,
                        last: 15.0,
                    },
                )
                .unwrap();

            ts.minute_table_mut()
                .insert(
                    (120_000, "server1"),
                    &Aggregate {
                        min: 15.0,
                        max: 25.0,
                        sum: 150.0,
                        count: 15,
                        last: 20.0,
                    },
                )
                .unwrap();

            drop(ts);
            write_txn.commit().unwrap();
        }

        // Downsample to hour
        {
            let write_txn = cf.begin_write().unwrap();
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu").unwrap();

            let count = ts
                .downsample_minute_to_hour("server1", 0, 3_600_000)
                .unwrap();
            assert_eq!(count, 1); // 1 hour bucket

            drop(ts);
            write_txn.commit().unwrap();
        }

        // Verify hour aggregate
        {
            let read_txn = cf.begin_read().unwrap();
            let ts_read =
                crate::timeseries::TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "cpu")
                    .unwrap();

            let agg = ts_read
                .get_aggregate(Granularity::Hour, "server1", 120_000)
                .unwrap()
                .unwrap();
            assert!((agg.min - 10.0).abs() < f32::EPSILON); // min of mins
            assert!((agg.max - 25.0).abs() < f32::EPSILON); // max of maxes
            assert!((agg.sum - 250.0).abs() < f32::EPSILON); // sum of sums
            assert_eq!(agg.count, 25); // sum of counts
            assert!((agg.last - 20.0).abs() < f32::EPSILON); // last of lasts
        }
    }
}
