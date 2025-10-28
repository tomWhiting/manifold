# manifold-timeseries

Time-series storage optimizations for the [Manifold](https://github.com/cberner/redb) embedded database.

[![Crates.io](https://img.shields.io/crates/v/manifold-timeseries.svg)](https://crates.io/crates/manifold-timeseries)
[![Documentation](https://docs.rs/manifold-timeseries/badge.svg)](https://docs.rs/manifold-timeseries)

## Overview

`manifold-timeseries` provides ergonomic, type-safe wrappers around Manifold's core primitives for storing and querying time-series data with multi-granularity downsampling and retention policies. It does **not** implement time-series analytics (forecasting, anomaly detection) - instead, it focuses on efficient persistent storage and provides integration traits for external analytics libraries.

## Features

- **Dual encoding strategies** - Absolute (default) or delta encoding for timestamps
- **Multi-granularity tables** - Raw, minute, hour, and day aggregates
- **Manual downsampling** - Compute aggregates (min, max, avg, sum, count, last)
- **Retention policies** - Time-based cleanup of old data
- **High performance** - Leverages Manifold's WAL group commit and ordered key-value storage
- **Integration ready** - `TimeSeriesSource` trait for external analytics libraries

## Quick Start

### Basic Usage

```rust
use manifold::column_family::ColumnFamilyDatabase;
use manifold_timeseries::{TimeSeriesTable, TimeSeriesTableRead, AbsoluteEncoding};

// Open database and column family
let db = ColumnFamilyDatabase::open("my.db")?;
let cf = db.column_family_or_create("metrics")?;

// Write time series data
{
    let write_txn = cf.begin_write()?;
    let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu")?;
    
    let timestamp = 1609459200000; // 2021-01-01 00:00:00 UTC (milliseconds)
    ts.write("server1.cpu.usage", timestamp, 42.5)?;
    ts.write("server1.cpu.usage", timestamp + 1000, 43.2)?;
    ts.write("server1.cpu.usage", timestamp + 2000, 41.8)?;
    
    drop(ts);
    write_txn.commit()?;
}

// Read time series data
let read_txn = cf.begin_read()?;
let ts = TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "cpu")?;

// Query a specific point
if let Some(value) = ts.get("server1.cpu.usage", timestamp)? {
    println!("CPU usage at {}: {}%", timestamp, value);
}

// Range query
let start = 1609459200000;
let end = 1609459260000;
for point in ts.range("server1.cpu.usage", start, end)? {
    let (timestamp, value) = point?;
    println!("{}: {}", timestamp, value);
}
```

### Batch Operations

For high-throughput metric ingestion:

```rust
let points = vec![
    ("server1.cpu.usage", 1609459200000, 42.5),
    ("server1.cpu.usage", 1609459201000, 43.2),
    ("server1.mem.usage", 1609459200000, 78.3),
    ("server2.cpu.usage", 1609459200000, 55.1),
];

let write_txn = cf.begin_write()?;
let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "metrics")?;

// Batch write for better performance
ts.write_batch(&points, false)?;

drop(ts);
write_txn.commit()?;
```

## Timestamp Encoding Strategies

### Absolute Encoding (Default)

Stores timestamps as 8-byte big-endian u64 values:

```rust
use manifold_timeseries::AbsoluteEncoding;

let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "metrics")?;
```

**Best for:**
- Sparse or irregular time series
- Random access patterns
- Ad-hoc queries

**Storage:** 8 bytes per timestamp (fixed)

### Delta Encoding

Stores timestamps as varint-compressed deltas with periodic checkpoints:

```rust
use manifold_timeseries::DeltaEncoding;

let mut ts = TimeSeriesTable::<DeltaEncoding>::open(&write_txn, "metrics")?;
```

**Best for:**
- Dense, regular-interval data (e.g., 1-second IoT sensors)
- Storage-constrained environments
- Sequential scan workloads

**Storage:** 1-9 bytes per timestamp (variable, typically 1-2 bytes for regular intervals)

## Multi-Granularity Support

Each `TimeSeriesTable` maintains four internal tables for efficient queries at different time scales:

- **Raw** - Original data points (per-second or per-millisecond)
- **Minute** - Aggregated per-minute data
- **Hour** - Aggregated per-hour data
- **Day** - Aggregated per-day data

### Aggregates

Each aggregate contains:
- `min: f32` - Minimum value in the window
- `max: f32` - Maximum value in the window
- `sum: f32` - Sum of all values
- `count: u64` - Number of data points
- `last: f32` - Most recent value

### Downsampling

Convert raw data to aggregates:

```rust
use manifold_timeseries::Granularity;

let write_txn = cf.begin_write()?;
let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu")?;

// Downsample last hour of raw data to minute aggregates
let start_ms = now_ms - (60 * 60 * 1000); // 1 hour ago
let count = ts.downsample_to_minute("server1.cpu.usage", start_ms, now_ms)?;
println!("Created {} minute aggregates", count);

// Downsample minute data to hour aggregates
let count = ts.downsample_minute_to_hour("server1.cpu.usage", start_ms, now_ms)?;
println!("Created {} hour aggregates", count);

drop(ts);
write_txn.commit()?;
```

Query aggregates:

```rust
let read_txn = cf.begin_read()?;
let ts = TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "cpu")?;

// Get minute-level aggregate
let agg = ts.get_aggregate(Granularity::Minute, "server1.cpu.usage", timestamp)?;
if let Some(agg) = agg {
    println!("Minute stats: min={}, max={}, avg={}", agg.min, agg.max, agg.average());
}

// Range query over hour aggregates
for result in ts.range_aggregates(Granularity::Hour, "server1.cpu.usage", start, end)? {
    let (timestamp, agg) = result?;
    println!("{}: avg={}", timestamp, agg.average());
}
```

## Retention Policies

Delete old data to manage storage:

```rust
use std::time::Duration;

let write_txn = cf.begin_write()?;
let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "metrics")?;

// Keep only last 7 days of raw data
ts.apply_retention(
    Granularity::Raw,
    Duration::from_secs(7 * 24 * 60 * 60)
)?;

// Apply multiple retention policies at once
ts.apply_all_retentions(&[
    (Granularity::Raw, Duration::from_secs(7 * 24 * 60 * 60)),      // 7 days
    (Granularity::Minute, Duration::from_secs(30 * 24 * 60 * 60)),  // 30 days
    (Granularity::Hour, Duration::from_secs(90 * 24 * 60 * 60)),    // 90 days
    (Granularity::Day, Duration::from_secs(365 * 24 * 60 * 60)),    // 1 year
])?;

drop(ts);
write_txn.commit()?;
```

## Architecture

### Storage Layout

Each time series table creates four internal Manifold tables:

```
{name}_raw     → (timestamp: u64, series_id: &str) → value: f32
{name}_minute  → (timestamp: u64, series_id: &str) → aggregate: Aggregate
{name}_hour    → (timestamp: u64, series_id: &str) → aggregate: Aggregate
{name}_day     → (timestamp: u64, series_id: &str) → aggregate: Aggregate
```

All tables share the same composite key structure for efficient range queries.

### Performance Characteristics

- **Write (single point)**: O(log n) B-tree insert
- **Write (batch)**: Amortized O(log n) with WAL group commit
- **Read (single point)**: O(log n) lookup
- **Range query**: O(log n) + O(k) where k = points in range
- **Downsampling**: O(k) scan + O(m log n) aggregate writes where m = buckets
- **Key size**: 8 bytes (timestamp) + series_id length
- **Value size**: 4 bytes (raw) or 24 bytes (aggregate)

### Aggregate Storage Format

Aggregates are stored as fixed-width 24-byte values:

```
[min: f32][max: f32][sum: f32][count: u64][last: f32]
  4 bytes   4 bytes   4 bytes    8 bytes    4 bytes
```

## Examples

The crate includes comprehensive examples demonstrating real-world usage:

### 1. Metrics Collection (`examples/metrics_collection.rs`)
Real system metrics collection:
- CPU usage tracking
- Memory monitoring
- Using sysinfo crate for real data
- Time series storage patterns

```bash
cargo run --example metrics_collection -p manifold-timeseries
```

### 2. IoT Sensors (`examples/iot_sensors.rs`)
IoT sensor data simulation:
- Multiple sensor types (temperature, humidity, pressure)
- Batch ingestion
- Range queries
- Statistics computation

```bash
cargo run --example iot_sensors -p manifold-timeseries
```

### 3. Downsampling Lifecycle (`examples/downsampling_lifecycle.rs`)
Complete downsampling workflow:
- Raw data ingestion
- Multi-level downsampling (raw → minute → hour → day)
- Retention policy application
- Query optimization strategies

```bash
cargo run --example downsampling_lifecycle -p manifold-timeseries
```

## Use Cases

- **Application monitoring** - System metrics, performance counters
- **IoT data** - Sensor readings, telemetry
- **Financial data** - Stock prices, trading volumes
- **Analytics** - User activity, event tracking
- **DevOps** - Infrastructure monitoring, alerting
- **Scientific data** - Experimental measurements, logging

## Combining with Other Domain Layers

`manifold-timeseries` works seamlessly with other manifold domain layers:

```rust
// Store entity embeddings in vectors
let vectors_cf = db.column_family_or_create("embeddings")?;
let mut vectors = VectorTable::<768>::open(&vectors_txn, "entities")?;
vectors.insert("server_1", &embedding)?;

// Store entity relationships in graph
let graph_cf = db.column_family_or_create("infrastructure")?;
let mut graph = GraphTable::open(&graph_txn, "network")?;
graph.add_edge(&server_1, "connects_to", &server_2, true, 1.0)?;

// Store entity metrics in time series
let metrics_cf = db.column_family_or_create("monitoring")?;
let mut ts = TimeSeriesTable::open(&metrics_txn, "server_metrics")?;
ts.write("server_1.cpu", timestamp, 45.2)?;
```

## Integration with Analytics Libraries

The `TimeSeriesSource` trait enables integration with external time-series analytics libraries:

```rust
use manifold_timeseries::TimeSeriesSource;

let read_txn = cf.begin_read()?;
let ts = TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "metrics")?;

// Use the trait to integrate with external libraries
let points: Vec<_> = ts.range_raw("cpu.usage", start, end)?
    .collect::<Result<Vec<_>, _>>()?;

// Pass to analytics library for forecasting, anomaly detection, etc.
// (example with hypothetical library)
let forecast = analytics::forecast(&points, 24)?;
```

## Requirements

- Rust 1.70+ (for const generics)
- `manifold` version 3.1+

## Performance Tips

1. **Use batch operations** for bulk ingestion - reduces transaction overhead
2. **Downsample regularly** - Query aggregates instead of raw data when possible
3. **Apply retention policies** - Delete old raw data after downsampling
4. **Pre-sort batch data** when possible - set `sorted: true` for better performance
5. **Use appropriate granularity** - Don't query raw data for long time ranges
6. **Choose the right encoding**:
   - Absolute: Default, supports random access
   - Delta: Better compression for regular-interval data

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Contributing

Contributions are welcome! This crate follows the patterns established in the manifold domain layer architecture.

## Related Crates

- [manifold](https://crates.io/crates/manifold) - Core embedded database
- [manifold-vectors](https://crates.io/crates/manifold-vectors) - Vector storage for embeddings
- [manifold-graph](https://crates.io/crates/manifold-graph) - Graph storage for relationships