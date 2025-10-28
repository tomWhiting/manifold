//! Time series table implementation with multi-granularity support.

use crate::aggregate::{Aggregate, Granularity};
use crate::encoding::TimestampEncoding;
use manifold::{
    ReadOnlyTable, ReadTransaction, ReadableTableMetadata, StorageError, Table, TableDefinition,
    TableError, WriteTransaction,
};
use std::marker::PhantomData;

/// A table storing time series data with multi-granularity support.
///
/// This table maintains four internal tables (raw, minute, hour, day) to enable
/// efficient queries at different time scales. All tables are updated within
/// the same write transaction.
///
/// # Type Parameters
///
/// - `E`: The timestamp encoding strategy (`AbsoluteEncoding` or `DeltaEncoding`)
pub struct TimeSeriesTable<'txn, E: TimestampEncoding> {
    pub(crate) raw: Table<'txn, (u64, &'static str), f32>,
    pub(crate) minute: Table<'txn, (u64, &'static str), Aggregate>,
    pub(crate) hour: Table<'txn, (u64, &'static str), Aggregate>,
    pub(crate) day: Table<'txn, (u64, &'static str), Aggregate>,
    _encoding: PhantomData<E>,
}

impl<'txn, E: TimestampEncoding> TimeSeriesTable<'txn, E> {
    /// Opens a time series table for writing.
    ///
    /// Creates four internal tables: `{name}_raw`, `{name}_minute`, `{name}_hour`, `{name}_day`.
    pub fn open(txn: &'txn WriteTransaction, name: &str) -> Result<Self, TableError> {
        let raw_name = format!("{name}_raw");
        let minute_name = format!("{name}_minute");
        let hour_name = format!("{name}_hour");
        let day_name = format!("{name}_day");

        let raw_def: TableDefinition<(u64, &str), f32> = TableDefinition::new(&raw_name);
        let minute_def: TableDefinition<(u64, &str), Aggregate> =
            TableDefinition::new(&minute_name);
        let hour_def: TableDefinition<(u64, &str), Aggregate> = TableDefinition::new(&hour_name);
        let day_def: TableDefinition<(u64, &str), Aggregate> = TableDefinition::new(&day_name);

        let raw = txn.open_table(raw_def)?;
        let minute = txn.open_table(minute_def)?;
        let hour = txn.open_table(hour_def)?;
        let day = txn.open_table(day_def)?;

        Ok(Self {
            raw,
            minute,
            hour,
            day,
            _encoding: PhantomData,
        })
    }

    /// Writes a single data point to the raw table.
    ///
    /// # Arguments
    ///
    /// * `series_id` - Series identifier (e.g., `"cpu.usage"`, `"sensor_42.temp"`)
    /// * `timestamp_ms` - Timestamp in milliseconds since epoch
    /// * `value` - Metric value
    pub fn write(
        &mut self,
        series_id: &str,
        timestamp_ms: u64,
        value: f32,
    ) -> Result<(), TableError> {
        self.raw.insert((timestamp_ms, series_id), &value)?;
        Ok(())
    }

    /// Writes multiple data points in a batch operation.
    ///
    /// # Arguments
    ///
    /// * `points` - Slice of (`series_id`, `timestamp_ms`, `value`) tuples
    /// * `sorted` - Whether the points are pre-sorted by (`timestamp`, `series_id`)
    pub fn write_batch(
        &mut self,
        points: &[(&str, u64, f32)],
        sorted: bool,
    ) -> Result<(), StorageError> {
        let items: Vec<((u64, &str), f32)> = points
            .iter()
            .map(|(series_id, timestamp_ms, value)| ((*timestamp_ms, *series_id), *value))
            .collect();

        self.raw.insert_bulk(items, sorted)?;
        Ok(())
    }

    /// Returns the number of raw data points stored.
    pub fn len(&self) -> Result<u64, StorageError> {
        self.raw.len()
    }

    /// Returns `true` if the raw table contains no data points.
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }

    /// Returns a reference to the raw data table.
    pub fn raw_table(&self) -> &Table<'txn, (u64, &'static str), f32> {
        &self.raw
    }

    /// Returns a reference to the minute aggregate table.
    pub fn minute_table(&self) -> &Table<'txn, (u64, &'static str), Aggregate> {
        &self.minute
    }

    /// Returns a mutable reference to the minute aggregate table.
    pub fn minute_table_mut(&mut self) -> &mut Table<'txn, (u64, &'static str), Aggregate> {
        &mut self.minute
    }

    /// Returns a reference to the hour aggregate table.
    pub fn hour_table(&self) -> &Table<'txn, (u64, &'static str), Aggregate> {
        &self.hour
    }

    /// Returns a reference to the day aggregate table.
    pub fn day_table(&self) -> &Table<'txn, (u64, &'static str), Aggregate> {
        &self.day
    }
}

/// Read-only time series table providing efficient access.
pub struct TimeSeriesTableRead<E: TimestampEncoding> {
    raw: ReadOnlyTable<(u64, &'static str), f32>,
    minute: ReadOnlyTable<(u64, &'static str), Aggregate>,
    hour: ReadOnlyTable<(u64, &'static str), Aggregate>,
    day: ReadOnlyTable<(u64, &'static str), Aggregate>,
    _encoding: PhantomData<E>,
}

impl<E: TimestampEncoding> TimeSeriesTableRead<E> {
    /// Opens a time series table for reading.
    pub fn open(txn: &ReadTransaction, name: &str) -> Result<Self, TableError> {
        let raw_name = format!("{name}_raw");
        let minute_name = format!("{name}_minute");
        let hour_name = format!("{name}_hour");
        let day_name = format!("{name}_day");

        let raw_def: TableDefinition<(u64, &str), f32> = TableDefinition::new(&raw_name);
        let minute_def: TableDefinition<(u64, &str), Aggregate> =
            TableDefinition::new(&minute_name);
        let hour_def: TableDefinition<(u64, &str), Aggregate> = TableDefinition::new(&hour_name);
        let day_def: TableDefinition<(u64, &str), Aggregate> = TableDefinition::new(&day_name);

        let raw = txn.open_table(raw_def)?;
        let minute = txn.open_table(minute_def)?;
        let hour = txn.open_table(hour_def)?;
        let day = txn.open_table(day_def)?;

        Ok(Self {
            raw,
            minute,
            hour,
            day,
            _encoding: PhantomData,
        })
    }

    /// Gets a single data point from the raw table.
    pub fn get(&self, series_id: &str, timestamp_ms: u64) -> Result<Option<f32>, StorageError> {
        self.raw
            .get((timestamp_ms, series_id))
            .map(|opt| opt.map(|guard| guard.value()))
    }

    /// Returns an iterator over raw data points in a time range.
    ///
    /// # Arguments
    ///
    /// * `series_id` - Series identifier to query
    /// * `start_ms` - Start timestamp (inclusive)
    /// * `end_ms` - End timestamp (exclusive)
    pub fn range(
        &self,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<RangeIter<'_>, StorageError> {
        let start_key = (start_ms, series_id);
        let end_key = (end_ms, series_id);

        let iter = self.raw.range(start_key..end_key)?;

        Ok(RangeIter {
            inner: iter,
            series_id: series_id.to_string(),
        })
    }

    /// Gets an aggregate from the specified granularity table.
    pub fn get_aggregate(
        &self,
        granularity: Granularity,
        series_id: &str,
        timestamp_ms: u64,
    ) -> Result<Option<Aggregate>, StorageError> {
        let rounded_ts = granularity.round_down(timestamp_ms);

        let table = match granularity {
            Granularity::Raw => {
                return Err(StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Cannot get aggregate for Raw granularity",
                )))
            }
            Granularity::Minute => &self.minute,
            Granularity::Hour => &self.hour,
            Granularity::Day => &self.day,
        };

        table
            .get((rounded_ts, series_id))
            .map(|opt| opt.map(|guard| guard.value()))
    }

    /// Returns an iterator over aggregates in a time range.
    pub fn range_aggregates(
        &self,
        granularity: Granularity,
        series_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<AggregateRangeIter<'_>, StorageError> {
        let table = match granularity {
            Granularity::Raw => {
                return Err(StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Cannot iterate aggregates for Raw granularity",
                )))
            }
            Granularity::Minute => &self.minute,
            Granularity::Hour => &self.hour,
            Granularity::Day => &self.day,
        };

        let start_key = (start_ms, series_id);
        let end_key = (end_ms, series_id);

        let iter = table.range(start_key..end_key)?;

        Ok(AggregateRangeIter {
            inner: iter,
            series_id: series_id.to_string(),
        })
    }

    /// Returns the number of raw data points stored.
    pub fn len(&self) -> Result<u64, StorageError> {
        self.raw.len()
    }

    /// Returns `true` if the raw table contains no data points.
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }

    /// Returns a reference to the raw data table.
    ///
    /// This is useful for integration with external libraries that need
    /// direct access to the underlying table.
    pub fn raw_table(&self) -> &ReadOnlyTable<(u64, &'static str), f32> {
        &self.raw
    }
}

/// Iterator over raw time series data points in a range.
pub struct RangeIter<'a> {
    inner: manifold::Range<'a, (u64, &'static str), f32>,
    series_id: String,
}

impl Iterator for RangeIter<'_> {
    type Item = Result<(u64, f32), StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next()? {
                Ok((key_guard, value_guard)) => {
                    let (timestamp, sid) = key_guard.value();
                    // Filter to only the requested series_id
                    if sid == self.series_id {
                        return Some(Ok((timestamp, value_guard.value())));
                    }
                    // Continue if different series_id
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

/// Iterator over aggregate data points in a range.
pub struct AggregateRangeIter<'a> {
    inner: manifold::Range<'a, (u64, &'static str), Aggregate>,
    series_id: String,
}

impl Iterator for AggregateRangeIter<'_> {
    type Item = Result<(u64, Aggregate), StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next()? {
                Ok((key_guard, value_guard)) => {
                    let (timestamp, sid) = key_guard.value();
                    // Filter to only the requested series_id
                    if sid == self.series_id {
                        return Some(Ok((timestamp, value_guard.value())));
                    }
                    // Continue if different series_id
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}
