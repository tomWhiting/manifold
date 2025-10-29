//! Time series domain benchmark for Manifold
//!
//! Tests manifold-timeseries performance characteristics:
//! - Raw data ingestion rate (absolute vs delta encoding)
//! - Range query performance across different time windows
//! - Downsampling throughput (raw → minute → hour → day)
//! - Multi-series concurrent writes
//! - Retention policy execution speed
//! - Sustained high-volume stress tests
//!
//! Domain optimization benchmarks - Phase 3: Time Series

use manifold::column_family::ColumnFamilyDatabase;
use manifold_timeseries::{
    AbsoluteEncoding, DeltaEncoding, Granularity, TimeSeriesTable, TimeSeriesTableRead,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;

const WARMUP_ITERATIONS: usize = 1;
const BENCHMARK_ITERATIONS: usize = 3;
const STRESS_TEST_DURATION_SECS: u64 = 30;
const BENCHMARK_DURATION_SECS: u64 = 5;

fn format_duration(d: Duration) -> String {
    let micros = d.as_micros();
    if micros < 1000 {
        format!("{micros}µs")
    } else if micros < 1_000_000 {
        format!("{:.2}ms", micros as f64 / 1000.0)
    } else {
        format!("{:.2}s", micros as f64 / 1_000_000.0)
    }
}

fn format_throughput(ops: usize, duration: Duration) -> String {
    let ops_per_sec = ops as f64 / duration.as_secs_f64();
    if ops_per_sec > 1_000_000.0 {
        format!("{:.2}M ops/sec", ops_per_sec / 1_000_000.0)
    } else if ops_per_sec > 1000.0 {
        format!("{:.2}K ops/sec", ops_per_sec / 1000.0)
    } else {
        format!("{:.2} ops/sec", ops_per_sec)
    }
}

fn print_section(title: &str) {
    println!("\n{}", "=".repeat(80));
    println!("{title}");
    println!("{}", "=".repeat(80));
}

fn print_result(name: &str, duration: Duration, ops: usize) {
    println!(
        "  {:<50} {:>12}  {:>15}",
        name,
        format_duration(duration),
        format_throughput(ops, duration)
    );
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Generate synthetic time series data
fn generate_data_points(count: usize, base_time: u64, interval_ms: u64) -> Vec<(String, u64, f32)> {
    let mut points = Vec::with_capacity(count);
    let mut rng = 12345u64;

    for i in 0..count {
        let series_id = format!("series_{}", i % 10);
        let timestamp = base_time + (i as u64 * interval_ms);

        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let value = (rng % 10000) as f32 / 100.0;

        points.push((series_id, timestamp, value));
    }

    points
}

/// Benchmark: Raw data ingestion with AbsoluteEncoding
fn benchmark_absolute_encoding_write(num_points: usize, batch_size: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("metrics").unwrap();

    let base_time = current_timestamp();
    let points = generate_data_points(num_points, base_time, 1000);

    let start = Instant::now();

    for chunk in points.chunks(batch_size) {
        let txn = cf.begin_write().unwrap();
        {
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&txn, "data").unwrap();

            let batch: Vec<(&str, u64, f32)> =
                chunk.iter().map(|(s, t, v)| (s.as_str(), *t, *v)).collect();

            ts.write_batch(&batch, false).unwrap();
        }
        txn.commit().unwrap();
    }

    let elapsed = start.elapsed();

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Raw data ingestion with DeltaEncoding
fn benchmark_delta_encoding_write(num_points: usize, batch_size: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("metrics").unwrap();

    let base_time = current_timestamp();
    let points = generate_data_points(num_points, base_time, 1000);

    let start = Instant::now();

    for chunk in points.chunks(batch_size) {
        let txn = cf.begin_write().unwrap();
        {
            let mut ts = TimeSeriesTable::<DeltaEncoding>::open(&txn, "data").unwrap();

            let batch: Vec<(&str, u64, f32)> =
                chunk.iter().map(|(s, t, v)| (s.as_str(), *t, *v)).collect();

            ts.write_batch(&batch, false).unwrap();
        }
        txn.commit().unwrap();
    }

    let elapsed = start.elapsed();

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Range query performance
fn benchmark_range_query(num_points: usize, range_hours: u64) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("metrics").unwrap();

    let base_time = current_timestamp();
    let points = generate_data_points(num_points, base_time, 1000);

    // Populate data
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&txn, "data").unwrap();

            let batch: Vec<(&str, u64, f32)> = points
                .iter()
                .map(|(s, t, v)| (s.as_str(), *t, *v))
                .collect();

            ts.write_batch(&batch, false).unwrap();
        }
        txn.commit().unwrap();
    }

    // Benchmark range queries
    let txn = cf.begin_read().unwrap();
    let ts = TimeSeriesTableRead::<AbsoluteEncoding>::open(&txn, "data").unwrap();

    let range_ms = range_hours * 60 * 60 * 1000;
    let start_time = base_time;
    let end_time = base_time + range_ms;

    let start = Instant::now();

    for result in ts.range("series_0", start_time, end_time).unwrap() {
        let (_timestamp, _value) = result.unwrap();
    }

    let elapsed = start.elapsed();

    drop(ts);
    drop(txn);
    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Downsampling performance
fn benchmark_downsampling(num_raw_points: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("metrics").unwrap();

    let base_time = current_timestamp();
    let points = generate_data_points(num_raw_points, base_time, 1000);

    // Populate raw data
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&txn, "data").unwrap();

            let batch: Vec<(&str, u64, f32)> = points
                .iter()
                .map(|(s, t, v)| (s.as_str(), *t, *v))
                .collect();

            ts.write_batch(&batch, false).unwrap();
        }
        txn.commit().unwrap();
    }

    // Benchmark downsampling
    let start = Instant::now();

    let write_txn = cf.begin_write().unwrap();
    {
        let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "data").unwrap();

        // Downsample from raw to minute to hour to day
        ts.downsample_to_minute(
            "series_0",
            base_time,
            base_time + (num_raw_points as u64 * 1000),
        )
        .unwrap();
        ts.downsample_minute_to_hour(
            "series_0",
            base_time,
            base_time + (num_raw_points as u64 * 1000),
        )
        .unwrap();
        ts.downsample_hour_to_day(
            "series_0",
            base_time,
            base_time + (num_raw_points as u64 * 1000),
        )
        .unwrap();
    }
    write_txn.commit().unwrap();

    let elapsed = start.elapsed();

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Multi-series concurrent writes
fn benchmark_multi_series_writes(
    num_series: usize,
    points_per_series: usize,
    batch_size: usize,
) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("metrics").unwrap();

    let base_time = current_timestamp();

    let start = Instant::now();

    for batch_num in 0..(points_per_series / batch_size) {
        let txn = cf.begin_write().unwrap();
        {
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&txn, "data").unwrap();

            for series_id in 0..num_series {
                let series_name = format!("series_{}", series_id);

                for i in 0..batch_size {
                    let timestamp = base_time + ((batch_num * batch_size + i) as u64 * 1000);
                    let value = (series_id as f32 * 10.0) + (i as f32);

                    ts.write(&series_name, timestamp, value).unwrap();
                }
            }
        }
        txn.commit().unwrap();
    }

    let elapsed = start.elapsed();

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Retention policy execution
fn benchmark_retention_policy(total_points: usize, retention_hours: u64) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("metrics").unwrap();

    let base_time = current_timestamp();
    let points = generate_data_points(total_points, base_time, 1000);

    // Populate data
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&txn, "data").unwrap();

            let batch: Vec<(&str, u64, f32)> = points
                .iter()
                .map(|(s, t, v)| (s.as_str(), *t, *v))
                .collect();

            ts.write_batch(&batch, false).unwrap();
        }
        txn.commit().unwrap();
    }

    // Benchmark retention
    let retention_duration = Duration::from_secs(retention_hours * 60 * 60);

    let start = Instant::now();

    let write_txn = cf.begin_write().unwrap();
    {
        let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "data").unwrap();

        ts.apply_retention(Granularity::Raw, retention_duration)
            .unwrap();
    }
    write_txn.commit().unwrap();

    let elapsed = start.elapsed();

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Sustained write stress test
fn benchmark_sustained_writes(
    duration_secs: u64,
    num_threads: usize,
    batch_size: usize,
) -> (usize, Duration) {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    // Create separate CF per thread
    for i in 0..num_threads {
        db.create_column_family(&format!("thread_{}", i), Some(100 * 1024 * 1024))
            .unwrap();
    }

    let stop_flag = Arc::new(AtomicBool::new(false));
    let total_ops = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    let start = Instant::now();

    for thread_id in 0..num_threads {
        let db_clone = db.clone();
        let stop = stop_flag.clone();
        let ops = total_ops.clone();

        let handle = std::thread::spawn(move || {
            let cf = db_clone
                .column_family(&format!("thread_{}", thread_id))
                .unwrap();
            let base_time = current_timestamp();
            let mut counter = 0usize;

            while !stop.load(Ordering::Relaxed) {
                let points = generate_data_points(batch_size, base_time + counter as u64, 1000);

                let txn = cf.begin_write().unwrap();
                {
                    let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&txn, "data").unwrap();

                    let batch: Vec<(&str, u64, f32)> = points
                        .iter()
                        .map(|(s, t, v)| (s.as_str(), *t, *v))
                        .collect();

                    ts.write_batch(&batch, false).unwrap();
                }
                txn.commit().unwrap();

                ops.fetch_add(batch_size, Ordering::Relaxed);
                counter += batch_size;
            }
        });
        handles.push(handle);
    }

    std::thread::sleep(Duration::from_secs(duration_secs));
    stop_flag.store(true, Ordering::Relaxed);

    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start.elapsed();
    let total = total_ops.load(Ordering::Relaxed);

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    (total, elapsed)
}

/// Benchmark: Mixed read/write workload
fn benchmark_mixed_workload(
    duration_secs: u64,
    num_threads: usize,
    read_percentage: usize,
) -> (usize, usize, Duration) {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    // Create and populate CFs
    for i in 0..num_threads {
        db.create_column_family(&format!("thread_{}", i), Some(100 * 1024 * 1024))
            .unwrap();

        let cf = db.column_family(&format!("thread_{}", i)).unwrap();
        let base_time = current_timestamp();
        let points = generate_data_points(1000, base_time, 1000);

        let txn = cf.begin_write().unwrap();
        {
            let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&txn, "data").unwrap();

            let batch: Vec<(&str, u64, f32)> = points
                .iter()
                .map(|(s, t, v)| (s.as_str(), *t, *v))
                .collect();

            ts.write_batch(&batch, false).unwrap();
        }
        txn.commit().unwrap();
    }

    let stop_flag = Arc::new(AtomicBool::new(false));
    let total_reads = Arc::new(AtomicUsize::new(0));
    let total_writes = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    let start = Instant::now();

    for thread_id in 0..num_threads {
        let db_clone = db.clone();
        let stop = stop_flag.clone();
        let reads = total_reads.clone();
        let writes = total_writes.clone();

        let handle = std::thread::spawn(move || {
            let cf = db_clone
                .column_family(&format!("thread_{}", thread_id))
                .unwrap();
            let base_time = current_timestamp();
            let mut counter = 0u64;
            let mut rng = thread_id as u64;

            while !stop.load(Ordering::Relaxed) {
                rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                let do_read = (rng % 100) < read_percentage as u64;

                if do_read {
                    let txn = cf.begin_read().unwrap();
                    let ts = TimeSeriesTableRead::<AbsoluteEncoding>::open(&txn, "data").unwrap();

                    let start_time = base_time;
                    let end_time = base_time + 100_000;

                    let mut read_count = 0;
                    for result in ts.range("series_0", start_time, end_time).unwrap() {
                        let _ = result.unwrap();
                        read_count += 1;
                    }
                    reads.fetch_add(read_count, Ordering::Relaxed);
                } else {
                    let points = generate_data_points(50, base_time + counter, 1000);

                    let txn = cf.begin_write().unwrap();
                    {
                        let mut ts =
                            TimeSeriesTable::<AbsoluteEncoding>::open(&txn, "data").unwrap();

                        let batch: Vec<(&str, u64, f32)> = points
                            .iter()
                            .map(|(s, t, v)| (s.as_str(), *t, *v))
                            .collect();

                        ts.write_batch(&batch, false).unwrap();
                    }
                    txn.commit().unwrap();
                    writes.fetch_add(points.len(), Ordering::Relaxed);
                    counter += points.len() as u64;
                }
            }
        });
        handles.push(handle);
    }

    std::thread::sleep(Duration::from_secs(duration_secs));
    stop_flag.store(true, Ordering::Relaxed);

    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start.elapsed();

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    (
        total_reads.load(Ordering::Relaxed),
        total_writes.load(Ordering::Relaxed),
        elapsed,
    )
}

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("MANIFOLD TIME SERIES DOMAIN BENCHMARK");
    println!("Testing manifold-timeseries performance characteristics");
    println!("{}", "=".repeat(80));
    println!();

    // 1. Raw Data Ingestion - Absolute vs Delta Encoding
    print_section("1. Raw Data Ingestion (Batch Size: 1000)");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &count in &[10000, 50000, 100000] {
        // Absolute encoding
        let mut abs_durations = Vec::new();
        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_absolute_encoding_write(count, 1000);
            if i >= WARMUP_ITERATIONS {
                abs_durations.push(duration);
            }
        }
        let avg_abs = abs_durations.iter().sum::<Duration>() / abs_durations.len() as u32;
        print_result(&format!("{} points (Absolute)", count), avg_abs, count);

        // Delta encoding
        let mut delta_durations = Vec::new();
        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_delta_encoding_write(count, 1000);
            if i >= WARMUP_ITERATIONS {
                delta_durations.push(duration);
            }
        }
        let avg_delta = delta_durations.iter().sum::<Duration>() / delta_durations.len() as u32;
        print_result(&format!("{} points (Delta)", count), avg_delta, count);
    }

    // 2. Range Query Performance
    print_section("2. Range Query Performance (10K points)");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &hours in &[1, 6, 24, 168] {
        let mut durations = Vec::new();

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_range_query(10000, hours);
            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        let hours_label = if hours == 168 {
            "1 week".to_string()
        } else if hours == 24 {
            "1 day".to_string()
        } else {
            format!("{} hours", hours)
        };
        print_result(&format!("Range: {}", hours_label), avg_duration, 10000);
    }

    // 3. Downsampling Performance
    print_section("3. Downsampling Performance (Raw → Minute → Hour → Day)");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &count in &[1000, 5000, 10000] {
        let mut durations = Vec::new();

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_downsampling(count);
            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(
            &format!("{} raw points (3 levels)", count),
            avg_duration,
            count,
        );
    }

    // 4. Multi-Series Concurrent Writes
    print_section("4. Multi-Series Concurrent Writes");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &(series, points) in &[(10, 1000), (50, 1000), (100, 1000)] {
        let mut durations = Vec::new();
        let total_points = series * points;

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_multi_series_writes(series, points, 100);
            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(
            &format!("{} series × {} points", series, points),
            avg_duration,
            total_points,
        );
    }

    // 5. Retention Policy Execution
    print_section("5. Retention Policy Execution");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &(points, retention_hrs) in &[(10000, 24), (10000, 168), (50000, 168)] {
        let mut durations = Vec::new();

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_retention_policy(points, retention_hrs);
            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        let retention_label = if retention_hrs == 168 {
            "1 week".to_string()
        } else {
            format!("{} hours", retention_hrs)
        };
        print_result(
            &format!("{} points, {} retention", points, retention_label),
            avg_duration,
            points,
        );
    }

    // 6. Sustained Write Stress Test
    print_section("6. Sustained Write Stress Test (30 seconds)");
    println!(
        "  {:<50} {:>12}  {:>15}",
        "Configuration", "Duration", "Throughput"
    );
    println!("  {}", "-".repeat(80));

    for &(threads, batch) in &[(1, 100), (4, 100), (8, 100), (4, 1000)] {
        let (total_ops, duration) =
            benchmark_sustained_writes(STRESS_TEST_DURATION_SECS, threads, batch);

        print_result(
            &format!("{} threads, batch={}", threads, batch),
            duration,
            total_ops,
        );
    }

    // 7. Mixed Read/Write Workload
    print_section("7. Mixed Read/Write Workload (5 seconds)");
    println!(
        "  {:<50} {:>12}  {:>15}",
        "Configuration", "Total Ops", "Throughput"
    );
    println!("  {}", "-".repeat(80));

    for &(threads, read_pct) in &[(4, 80), (4, 50), (4, 20)] {
        let (reads, writes, duration) =
            benchmark_mixed_workload(BENCHMARK_DURATION_SECS, threads, read_pct);
        let total_ops = reads + writes;

        println!(
            "  {:<50} {:>12}  {:>15}",
            format!("{} threads, {}% reads", threads, read_pct),
            format!("{} ops", total_ops),
            format_throughput(total_ops, duration)
        );
        println!(
            "    Reads: {} ({:.1}%) | Writes: {} ({:.1}%)",
            reads,
            (reads as f64 / total_ops as f64) * 100.0,
            writes,
            (writes as f64 / total_ops as f64) * 100.0
        );
    }

    println!("\n{}", "=".repeat(80));
    println!("BENCHMARK COMPLETE");
    println!("{}", "=".repeat(80));
    println!();
}
