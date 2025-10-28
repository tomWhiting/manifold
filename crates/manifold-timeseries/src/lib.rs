//! # manifold-timeseries
//!
//! Time-series storage optimizations for the Manifold embedded database.
//!
//! This crate provides ergonomic, type-safe wrappers around Manifold's core primitives
//! for storing and querying time-series data with multi-granularity downsampling and
//! retention policies.
//!
//! ## Features
//!
//! - **Dual encoding strategies**: Absolute (default) or delta encoding for timestamps
//! - **Multi-granularity tables**: Raw, minute, hour, and day aggregates
//! - **Manual downsampling**: Compute aggregates (min, max, avg, sum, count)
//! - **Retention policies**: Time-based cleanup of old data
//! - **High performance**: Leverages Manifold's WAL group commit and ordered key-value storage
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use manifold::column_family::ColumnFamilyDatabase;
//! use manifold_timeseries::{TimeSeriesTable, AbsoluteEncoding};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let db = ColumnFamilyDatabase::open("my.db")?;
//! let cf = db.column_family_or_create("metrics")?;
//!
//! // Write time series data
//! {
//!     let write_txn = cf.begin_write()?;
//!     let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu")?;
//!     
//!     let timestamp = 1609459200000; // 2021-01-01 00:00:00 UTC
//!     ts.write("server1.cpu.usage", timestamp, 42.5)?;
//!     
//!     drop(ts);
//!     write_txn.commit()?;
//! }
//!
//! // Read time series data
//! let read_txn = cf.begin_read()?;
//! let ts_read = manifold_timeseries::TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "cpu")?;
//!
//! let start = 1609459200000;
//! let end = 1609459260000;
//! for point in ts_read.range("server1.cpu.usage", start, end)? {
//!     let (timestamp, value) = point?;
//!     println!("{}: {}", timestamp, value);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! This crate does **not** implement time-series analytics (forecasting, anomaly detection, etc.).
//! Instead, it focuses on efficient storage and provides integration traits for external
//! libraries.

#![deny(missing_docs)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

pub mod aggregate;
pub mod encoding;
pub mod timeseries;
pub mod downsampling;
pub mod retention;
pub mod integration;

pub use aggregate::{Aggregate, Granularity};
pub use encoding::{AbsoluteEncoding, DeltaEncoding, EncodingError, TimestampEncoding};
pub use timeseries::{TimeSeriesTable, TimeSeriesTableRead};
pub use integration::TimeSeriesSource;

