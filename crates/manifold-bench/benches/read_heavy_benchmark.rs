//! Read-heavy workload benchmark for Manifold
//!
//! Tests read throughput with multiple concurrent readers, read performance
//! during concurrent writes, range scan performance, and iterator batch sizes.
//!
//! Phase 1, Task 1.4: Read-heavy workload benchmarks

use manifold::column_family::ColumnFamilyDatabase;
use manifold::{Durability, TableDefinition};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

const BENCHMARK_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

const WARMUP_ITERATIONS: usize = 2;
const BENCHMARK_ITERATIONS: usize = 5;

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

/// Benchmark: Multiple concurrent readers (no writes)
fn benchmark_concurrent_readers(num_readers: usize, num_entries: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());
    db.create_column_family("data", Some(100 * 1024 * 1024))
        .unwrap();

    // Populate data
    let cf = db.column_family("data").unwrap();
    let data = vec![0u8; 1024];
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
            for i in 0..num_entries as u64 {
                table.insert(&i, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    // Benchmark concurrent reads
    let start = Instant::now();
    let mut handles = vec![];

    for _ in 0..num_readers {
        let db_clone = db.clone();
        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family("data").unwrap();
            let txn = cf.begin_read().unwrap();
            let table = txn.open_table(BENCHMARK_TABLE).unwrap();

            for i in 0..num_entries as u64 {
                let _value = table.get(&i).unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    start.elapsed()
}

/// Benchmark: Reads during concurrent writes
fn benchmark_reads_during_writes(
    num_readers: usize,
    num_writers: usize,
    duration_secs: u64,
) -> (usize, usize) {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    // Create separate CFs for readers and writers to avoid blocking
    db.create_column_family("read_data", Some(100 * 1024 * 1024))
        .unwrap();
    db.create_column_family("write_data", Some(100 * 1024 * 1024))
        .unwrap();

    // Populate read data
    let read_cf = db.column_family("read_data").unwrap();
    let data = vec![0u8; 1024];
    {
        let txn = read_cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
            for i in 0..10000u64 {
                table.insert(&i, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    let stop_flag = Arc::new(AtomicBool::new(false));
    let mut handles = vec![];
    let total_reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let total_writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Spawn readers
    for _ in 0..num_readers {
        let db_clone = db.clone();
        let stop = stop_flag.clone();
        let reads = total_reads.clone();
        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family("read_data").unwrap();
            while !stop.load(Ordering::Relaxed) {
                let txn = cf.begin_read().unwrap();
                let table = txn.open_table(BENCHMARK_TABLE).unwrap();
                for i in 0..100u64 {
                    let _value = table.get(&i).unwrap();
                }
                reads.fetch_add(100, Ordering::Relaxed);
            }
        });
        handles.push(handle);
    }

    // Spawn writers
    for writer_id in 0..num_writers {
        let db_clone = db.clone();
        let stop = stop_flag.clone();
        let writes = total_writes.clone();
        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family("write_data").unwrap();
            let data = vec![0u8; 1024];
            let mut counter = 0u64;

            while !stop.load(Ordering::Relaxed) {
                let mut txn = cf.begin_write().unwrap();
                txn.set_durability(Durability::None).unwrap();
                {
                    let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
                    for i in 0..100 {
                        let key = (writer_id as u64 * 1_000_000) + counter + i;
                        table.insert(&key, data.as_slice()).unwrap();
                    }
                }
                txn.commit().unwrap();
                writes.fetch_add(100, Ordering::Relaxed);
                counter += 100;
            }
        });
        handles.push(handle);
    }

    // Run for specified duration
    std::thread::sleep(Duration::from_secs(duration_secs));
    stop_flag.store(true, Ordering::Relaxed);

    for handle in handles {
        handle.join().unwrap();
    }

    (
        total_reads.load(Ordering::Relaxed),
        total_writes.load(Ordering::Relaxed),
    )
}

/// Benchmark: Range scans
fn benchmark_range_scan(scan_size: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("data", Some(100 * 1024 * 1024))
        .unwrap();

    // Populate data
    let cf = db.column_family("data").unwrap();
    let data = vec![0u8; 1024];
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
            for i in 0..100000u64 {
                table.insert(&i, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    // Benchmark range scan
    let start = Instant::now();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(BENCHMARK_TABLE).unwrap();

    let mut count = 0;
    for entry in table.range(0u64..).unwrap() {
        let _ = entry.unwrap();
        count += 1;
        if count >= scan_size {
            break;
        }
    }

    start.elapsed()
}

/// Benchmark: Iterator batch performance
fn benchmark_iterator_batching(total_entries: usize, batch_size: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("data", Some(100 * 1024 * 1024))
        .unwrap();

    // Populate data
    let cf = db.column_family("data").unwrap();
    let data = vec![0u8; 512];
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
            for i in 0..total_entries as u64 {
                table.insert(&i, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    // Benchmark iteration with batching
    let start = Instant::now();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(BENCHMARK_TABLE).unwrap();

    let mut iter = table.range(0u64..).unwrap();
    let mut total_read = 0;

    loop {
        let mut batch_count = 0;
        for entry in iter.by_ref() {
            let _ = entry.unwrap();
            batch_count += 1;
            if batch_count >= batch_size {
                break;
            }
        }
        total_read += batch_count;
        if batch_count == 0 || total_read >= total_entries {
            break;
        }
    }

    start.elapsed()
}

fn main() {
    println!("\nManifold Read-Heavy Workload Benchmark Suite");
    println!("Phase 1, Task 1.4");
    println!("Warmup iterations: {WARMUP_ITERATIONS}");
    println!("Benchmark iterations: {BENCHMARK_ITERATIONS}");

    // ========================================================================
    // Test 1: Multiple concurrent readers (no writes)
    // ========================================================================
    print_section("Concurrent Readers (No Writes)");
    println!(
        "  {:<50} {:>12}  {:>15}",
        "Configuration", "Duration", "Throughput"
    );
    println!("  {}", "-".repeat(80));

    let num_entries = 10000;
    for num_readers in [1, 2, 4, 8, 16] {
        // Warmup
        for _ in 0..WARMUP_ITERATIONS {
            let _ = benchmark_concurrent_readers(num_readers, num_entries);
        }

        // Benchmark
        let mut times = vec![];
        for _ in 0..BENCHMARK_ITERATIONS {
            times.push(benchmark_concurrent_readers(num_readers, num_entries));
        }

        let avg_time: Duration = times.iter().sum::<Duration>() / times.len() as u32;
        let total_ops = num_readers * num_entries;
        print_result(
            &format!("{} readers, {} entries each", num_readers, num_entries),
            avg_time,
            total_ops,
        );
    }

    // ========================================================================
    // Test 2: Reads during concurrent writes
    // ========================================================================
    print_section("Reads During Concurrent Writes");
    println!("  Running for 3 seconds per configuration...\n");

    for (readers, writers) in [(4, 1), (8, 2), (12, 4)] {
        println!("  Configuration: {} readers, {} writers", readers, writers);

        // Warmup
        let _ = benchmark_reads_during_writes(readers, writers, 1);

        // Benchmark
        let (total_reads, total_writes) = benchmark_reads_during_writes(readers, writers, 3);
        let read_throughput = total_reads as f64 / 3.0;
        let write_throughput = total_writes as f64 / 3.0;

        println!(
            "    Read throughput:  {:.2}K ops/sec",
            read_throughput / 1000.0
        );
        println!(
            "    Write throughput: {:.2}K ops/sec",
            write_throughput / 1000.0
        );
    }

    // ========================================================================
    // Test 3: Range scan performance
    // ========================================================================
    print_section("Range Scan Performance");
    println!(
        "  {:<50} {:>12}  {:>15}",
        "Scan Size", "Duration", "Throughput"
    );
    println!("  {}", "-".repeat(80));

    for scan_size in [100, 1000, 10000, 50000] {
        // Warmup
        for _ in 0..WARMUP_ITERATIONS {
            let _ = benchmark_range_scan(scan_size);
        }

        // Benchmark
        let mut times = vec![];
        for _ in 0..BENCHMARK_ITERATIONS {
            times.push(benchmark_range_scan(scan_size));
        }

        let avg_time: Duration = times.iter().sum::<Duration>() / times.len() as u32;
        print_result(&format!("{} entries", scan_size), avg_time, scan_size);
    }

    // ========================================================================
    // Test 4: Iterator batch size optimization
    // ========================================================================
    print_section("Iterator Batch Size Performance");
    println!(
        "  {:<50} {:>12}  {:>15}",
        "Batch Size", "Duration", "Throughput"
    );
    println!("  {}", "-".repeat(80));

    let total_entries = 50000;
    for batch_size in [10, 100, 500, 1000, 5000] {
        // Warmup
        for _ in 0..WARMUP_ITERATIONS {
            let _ = benchmark_iterator_batching(total_entries, batch_size);
        }

        // Benchmark
        let mut times = vec![];
        for _ in 0..BENCHMARK_ITERATIONS {
            times.push(benchmark_iterator_batching(total_entries, batch_size));
        }

        let avg_time: Duration = times.iter().sum::<Duration>() / times.len() as u32;
        print_result(
            &format!("batch_size={}, total={}", batch_size, total_entries),
            avg_time,
            total_entries,
        );
    }

    print_section("Benchmark Complete");
    println!();
}
