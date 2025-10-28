//! Retention policy implementation for automatic data cleanup.

use crate::aggregate::Granularity;
use crate::encoding::TimestampEncoding;
use crate::timeseries::TimeSeriesTable;
use manifold::{ReadableTable, StorageError};
use std::time::Duration;

impl<'txn, E: TimestampEncoding> TimeSeriesTable<'txn, E> {
    /// Applies a retention policy to delete data older than the specified duration.
    ///
    /// # Arguments
    ///
    /// * `granularity` - Which table to apply retention to (Raw, Minute, Hour, or Day)
    /// * `keep_duration` - How far back to keep data
    ///
    /// # Returns
    ///
    /// Number of data points deleted
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use manifold_timeseries::{TimeSeriesTable, AbsoluteEncoding, Granularity};
    /// use std::time::Duration;
    /// # use manifold::column_family::ColumnFamilyDatabase;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let db = ColumnFamilyDatabase::open("test.db")?;
    /// # let cf = db.column_family_or_create("metrics")?;
    /// # let write_txn = cf.begin_write()?;
    /// # let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu")?;
    ///
    /// // Keep only the last 7 days of raw data
    /// let deleted = ts.apply_retention(
    ///     Granularity::Raw,
    ///     Duration::from_secs(7 * 24 * 60 * 60)
    /// )?;
    /// println!("Deleted {} old raw data points", deleted);
    /// # Ok(())
    /// # }
    /// ```
    pub fn apply_retention(
        &mut self,
        granularity: Granularity,
        keep_duration: Duration,
    ) -> Result<usize, StorageError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| {
                StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("System time error: {}", e),
                ))
            })?
            .as_millis() as u64;

        let keep_duration_ms = keep_duration.as_millis() as u64;
        if keep_duration_ms == 0 {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Retention duration must be positive",
            )));
        }

        let cutoff_ms = now_ms.saturating_sub(keep_duration_ms);

        self.delete_before(granularity, cutoff_ms)
    }

    /// Deletes all data points before the specified timestamp for a given granularity.
    ///
    /// # Arguments
    ///
    /// * `granularity` - Which table to delete from
    /// * `cutoff_ms` - Delete all data with timestamps before this value
    ///
    /// # Returns
    ///
    /// Number of data points deleted
    pub fn delete_before(
        &mut self,
        granularity: Granularity,
        cutoff_ms: u64,
    ) -> Result<usize, StorageError> {
        let mut keys_to_delete = Vec::new();
        let start_key = (0u64, "");
        let end_key = (cutoff_ms, "\u{10FFFF}");

        // Collect keys based on granularity
        match granularity {
            Granularity::Raw => {
                for item in self.raw.range(start_key..end_key)? {
                    let (key_guard, _) = item?;
                    let (timestamp, series_id) = key_guard.value();
                    if timestamp < cutoff_ms {
                        keys_to_delete.push((timestamp, series_id.to_string()));
                    }
                }
            }
            Granularity::Minute => {
                for item in self.minute.range(start_key..end_key)? {
                    let (key_guard, _) = item?;
                    let (timestamp, series_id) = key_guard.value();
                    if timestamp < cutoff_ms {
                        keys_to_delete.push((timestamp, series_id.to_string()));
                    }
                }
            }
            Granularity::Hour => {
                for item in self.hour.range(start_key..end_key)? {
                    let (key_guard, _) = item?;
                    let (timestamp, series_id) = key_guard.value();
                    if timestamp < cutoff_ms {
                        keys_to_delete.push((timestamp, series_id.to_string()));
                    }
                }
            }
            Granularity::Day => {
                for item in self.day.range(start_key..end_key)? {
                    let (key_guard, _) = item?;
                    let (timestamp, series_id) = key_guard.value();
                    if timestamp < cutoff_ms {
                        keys_to_delete.push((timestamp, series_id.to_string()));
                    }
                }
            }
        }

        // Delete collected keys
        let count = keys_to_delete.len();
        for (timestamp, series_id) in keys_to_delete {
            match granularity {
                Granularity::Raw => {
                    self.raw.remove((timestamp, series_id.as_str()))?;
                }
                Granularity::Minute => {
                    self.minute.remove((timestamp, series_id.as_str()))?;
                }
                Granularity::Hour => {
                    self.hour.remove((timestamp, series_id.as_str()))?;
                }
                Granularity::Day => {
                    self.day.remove((timestamp, series_id.as_str()))?;
                }
            }
        }

        Ok(count)
    }

    /// Applies retention policies to all granularities at once.
    ///
    /// This is a convenience method for applying different retention policies
    /// to each granularity level in a single call.
    ///
    /// # Arguments
    ///
    /// * `raw_duration` - Retention for raw data (None to skip)
    /// * `minute_duration` - Retention for minute aggregates (None to skip)
    /// * `hour_duration` - Retention for hour aggregates (None to skip)
    /// * `day_duration` - Retention for day aggregates (None to skip)
    ///
    /// # Returns
    ///
    /// Total number of data points deleted across all granularities
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use manifold_timeseries::{TimeSeriesTable, AbsoluteEncoding};
    /// use std::time::Duration;
    /// # use manifold::column_family::ColumnFamilyDatabase;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let db = ColumnFamilyDatabase::open("test.db")?;
    /// # let cf = db.column_family_or_create("metrics")?;
    /// # let write_txn = cf.begin_write()?;
    /// # let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu")?;
    ///
    /// let total_deleted = ts.apply_all_retentions(
    ///     Some(Duration::from_secs(7 * 24 * 60 * 60)),    // 7 days of raw data
    ///     Some(Duration::from_secs(30 * 24 * 60 * 60)),   // 30 days of minute aggregates
    ///     Some(Duration::from_secs(90 * 24 * 60 * 60)),   // 90 days of hour aggregates
    ///     Some(Duration::from_secs(365 * 24 * 60 * 60)),  // 1 year of day aggregates
    /// )?;
    /// println!("Total deleted: {}", total_deleted);
    /// # Ok(())
    /// # }
    /// ```
    pub fn apply_all_retentions(
        &mut self,
        raw_duration: Option<Duration>,
        minute_duration: Option<Duration>,
        hour_duration: Option<Duration>,
        day_duration: Option<Duration>,
    ) -> Result<usize, StorageError> {
        let mut total = 0;

        if let Some(duration) = raw_duration {
            total += self.apply_retention(Granularity::Raw, duration)?;
        }

        if let Some(duration) = minute_duration {
            total += self.apply_retention(Granularity::Minute, duration)?;
        }

        if let Some(duration) = hour_duration {
            total += self.apply_retention(Granularity::Hour, duration)?;
        }

        if let Some(duration) = day_duration {
            total += self.apply_retention(Granularity::Day, duration)?;
        }

        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::AbsoluteEncoding;
    use crate::integration::TimeSeriesSource;
    use manifold::column_family::ColumnFamilyDatabase;
    use tempfile::tempdir;

    #[test]
    fn test_apply_retention() {
        let dir = tempdir().unwrap();
        let db = ColumnFamilyDatabase::open(dir.path().join("test.db")).unwrap();
        let cf = db.column_family_or_create("metrics").unwrap();

        // Get current time
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Write data at different ages
        {
            let write_txn = cf.begin_write().unwrap();
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu").unwrap();

            // Old data (10 days ago)
            let old_ts = now_ms - (10 * 24 * 60 * 60 * 1000);
            ts.write("server1", old_ts, 10.0).unwrap();
            ts.write("server1", old_ts + 1000, 11.0).unwrap();

            // Recent data (1 day ago)
            let recent_ts = now_ms - (1 * 24 * 60 * 60 * 1000);
            ts.write("server1", recent_ts, 20.0).unwrap();
            ts.write("server1", recent_ts + 1000, 21.0).unwrap();

            drop(ts);
            write_txn.commit().unwrap();
        }

        // Apply 7-day retention (should delete old data)
        {
            let write_txn = cf.begin_write().unwrap();
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu").unwrap();

            let deleted = ts
                .apply_retention(Granularity::Raw, Duration::from_secs(7 * 24 * 60 * 60))
                .unwrap();

            assert_eq!(deleted, 2); // Should delete 2 old points

            drop(ts);
            write_txn.commit().unwrap();
        }

        // Verify only recent data remains
        {
            let read_txn = cf.begin_read().unwrap();
            let ts_read =
                crate::timeseries::TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "cpu")
                    .unwrap();

            let count = ts_read.count_raw("server1", 0, u64::MAX).unwrap();
            assert_eq!(count, 2); // Only 2 recent points remain
        }
    }

    #[test]
    fn test_delete_before() {
        let dir = tempdir().unwrap();
        let db = ColumnFamilyDatabase::open(dir.path().join("test.db")).unwrap();
        let cf = db.column_family_or_create("metrics").unwrap();

        // Write data at specific timestamps
        {
            let write_txn = cf.begin_write().unwrap();
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu").unwrap();

            ts.write("server1", 100_000, 10.0).unwrap();
            ts.write("server1", 200_000, 20.0).unwrap();
            ts.write("server1", 300_000, 30.0).unwrap();
            ts.write("server1", 400_000, 40.0).unwrap();

            drop(ts);
            write_txn.commit().unwrap();
        }

        // Delete everything before 250,000
        {
            let write_txn = cf.begin_write().unwrap();
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu").unwrap();

            let deleted = ts.delete_before(Granularity::Raw, 250_000).unwrap();
            assert_eq!(deleted, 2); // Should delete first 2 points

            drop(ts);
            write_txn.commit().unwrap();
        }

        // Verify only later data remains
        {
            let read_txn = cf.begin_read().unwrap();
            let ts_read =
                crate::timeseries::TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "cpu")
                    .unwrap();

            let count = ts_read.count_raw("server1", 0, u64::MAX).unwrap();
            assert_eq!(count, 2); // Only 2 points remain
        }
    }
}
