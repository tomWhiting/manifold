//! Vanilla redb comparison benchmark for Manifold
//!
//! Direct comparison between Manifold's column family implementation
//! and vanilla redb 2.6.0 for equivalent workloads.
//!
//! Phase 1, Task 1.8: Comparison benchmarks

use manifold::TableDefinition as ManifoldTableDef;
use manifold::column_family::ColumnFamilyDatabase;
use redb2_6::{Database as RedbDatabase, ReadableTable, TableDefinition as RedbTableDef};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

const MANIFOLD_TABLE: ManifoldTableDef<u64, &[u8]> = ManifoldTableDef::new("data");
const REDB_TABLE: RedbTableDef<u64, &[u8]> = RedbTableDef::new("data");

const WARMUP_ITERATIONS: usize = 2;
const BENCHMARK_ITERATIONS: usize = 5;
const BATCH_SIZE: usize = 1000;

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

fn print_comparison(name: &str, manifold_dur: Duration, redb_dur: Duration, ops: usize) {
    let speedup = redb_dur.as_secs_f64() / manifold_dur.as_secs_f64();
    let manifold_throughput = format_throughput(ops, manifold_dur);
    let redb_throughput = format_throughput(ops, redb_dur);

    println!(
        "  {:<45} {:>15} {:>15} {:>10}",
        name,
        manifold_throughput,
        redb_throughput,
        format!("{:.2}x", speedup)
    );
}

/// Benchmark: Single-threaded sequential writes (Manifold)
fn benchmark_manifold_sequential_writes(num_batches: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("data", Some(100 * 1024 * 1024))
        .unwrap();

    let cf = db.column_family("data").unwrap();
    let data = vec![0u8; 1024];

    let start = Instant::now();

    for batch in 0..num_batches {
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(MANIFOLD_TABLE).unwrap();
            for i in 0..BATCH_SIZE as u64 {
                let key = (batch as u64 * BATCH_SIZE as u64) + i;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    start.elapsed()
}

/// Benchmark: Single-threaded sequential writes (redb)
fn benchmark_redb_sequential_writes(num_batches: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = RedbDatabase::create(tmpfile.path()).unwrap();

    let data = vec![0u8; 1024];

    let start = Instant::now();

    for batch in 0..num_batches {
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(REDB_TABLE).unwrap();
            for i in 0..BATCH_SIZE as u64 {
                let key = (batch as u64 * BATCH_SIZE as u64) + i;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        write_txn.commit().unwrap();
    }

    start.elapsed()
}

/// Benchmark: Concurrent writes (Manifold - multiple CFs)
fn benchmark_manifold_concurrent_writes(num_threads: usize, batches_per_thread: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    let start = Instant::now();
    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = db.clone();
        let handle = std::thread::spawn(move || {
            let cf = db_clone
                .column_family_or_create(&format!("cf_{}", thread_id))
                .unwrap();
            let data = vec![0u8; 1024];

            for batch in 0..batches_per_thread {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(MANIFOLD_TABLE).unwrap();
                    for i in 0..BATCH_SIZE as u64 {
                        let key = (batch as u64 * BATCH_SIZE as u64) + i;
                        table.insert(&key, data.as_slice()).unwrap();
                    }
                }
                txn.commit().unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    start.elapsed()
}

/// Benchmark: Concurrent writes (redb - serialized)
fn benchmark_redb_concurrent_writes(num_threads: usize, batches_per_thread: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(RedbDatabase::create(tmpfile.path()).unwrap());

    let start = Instant::now();
    let mut handles = vec![];

    for _thread_id in 0..num_threads {
        let db_clone = db.clone();
        let handle = std::thread::spawn(move || {
            let data = vec![0u8; 1024];

            for batch in 0..batches_per_thread {
                let write_txn = db_clone.begin_write().unwrap();
                {
                    let mut table = write_txn.open_table(REDB_TABLE).unwrap();
                    for i in 0..BATCH_SIZE as u64 {
                        let key = (batch as u64 * BATCH_SIZE as u64) + i;
                        table.insert(&key, data.as_slice()).unwrap();
                    }
                }
                write_txn.commit().unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    start.elapsed()
}

/// Benchmark: Read performance (Manifold)
fn benchmark_manifold_reads(num_entries: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("data", Some(100 * 1024 * 1024))
        .unwrap();

    let cf = db.column_family("data").unwrap();
    let data = vec![0u8; 1024];

    // Populate
    let txn = cf.begin_write().unwrap();
    {
        let mut table = txn.open_table(MANIFOLD_TABLE).unwrap();
        for i in 0..num_entries as u64 {
            table.insert(&i, data.as_slice()).unwrap();
        }
    }
    txn.commit().unwrap();

    // Benchmark reads
    let start = Instant::now();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(MANIFOLD_TABLE).unwrap();
    for i in 0..num_entries as u64 {
        let _value = table.get(&i).unwrap();
    }
    start.elapsed()
}

/// Benchmark: Read performance (redb)
fn benchmark_redb_reads(num_entries: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = RedbDatabase::create(tmpfile.path()).unwrap();

    let data = vec![0u8; 1024];

    // Populate
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(REDB_TABLE).unwrap();
        for i in 0..num_entries as u64 {
            table.insert(&i, data.as_slice()).unwrap();
        }
    }
    write_txn.commit().unwrap();

    // Benchmark reads
    let start = Instant::now();
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(REDB_TABLE).unwrap();
    for i in 0..num_entries as u64 {
        let _value = table.get(&i).unwrap();
    }
    start.elapsed()
}

/// Benchmark: Range scan (Manifold)
fn benchmark_manifold_range_scan(num_entries: usize, scan_size: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("data", Some(100 * 1024 * 1024))
        .unwrap();

    let cf = db.column_family("data").unwrap();
    let data = vec![0u8; 1024];

    // Populate
    let txn = cf.begin_write().unwrap();
    {
        let mut table = txn.open_table(MANIFOLD_TABLE).unwrap();
        for i in 0..num_entries as u64 {
            table.insert(&i, data.as_slice()).unwrap();
        }
    }
    txn.commit().unwrap();

    // Benchmark range scan
    let start = Instant::now();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(MANIFOLD_TABLE).unwrap();
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

/// Benchmark: Range scan (redb)
fn benchmark_redb_range_scan(num_entries: usize, scan_size: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = RedbDatabase::create(tmpfile.path()).unwrap();

    let data = vec![0u8; 1024];

    // Populate
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(REDB_TABLE).unwrap();
        for i in 0..num_entries as u64 {
            table.insert(&i, data.as_slice()).unwrap();
        }
    }
    write_txn.commit().unwrap();

    // Benchmark range scan
    let start = Instant::now();
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(REDB_TABLE).unwrap();
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

fn main() {
    println!("\nManifold vs Vanilla redb 2.6.0 Comparison Benchmark");
    println!("Phase 1, Task 1.8");
    println!("Warmup iterations: {WARMUP_ITERATIONS}");
    println!("Benchmark iterations: {BENCHMARK_ITERATIONS}");
    println!("Batch size: {BATCH_SIZE} writes per transaction\n");

    // ========================================================================
    // Test 1: Single-threaded sequential writes
    // ========================================================================
    print_section("Single-Threaded Sequential Writes");
    println!(
        "  {:<45} {:>15} {:>15} {:>10}",
        "Test", "Manifold", "redb 2.6", "Speedup"
    );
    println!("  {}", "-".repeat(88));

    let num_batches = 100;
    let total_ops = num_batches * BATCH_SIZE;

    // Warmup
    for _ in 0..WARMUP_ITERATIONS {
        let _ = benchmark_manifold_sequential_writes(num_batches);
        let _ = benchmark_redb_sequential_writes(num_batches);
    }

    // Benchmark
    let mut manifold_times = vec![];
    let mut redb_times = vec![];
    for _ in 0..BENCHMARK_ITERATIONS {
        manifold_times.push(benchmark_manifold_sequential_writes(num_batches));
        redb_times.push(benchmark_redb_sequential_writes(num_batches));
    }

    let avg_manifold: Duration =
        manifold_times.iter().sum::<Duration>() / manifold_times.len() as u32;
    let avg_redb: Duration = redb_times.iter().sum::<Duration>() / redb_times.len() as u32;

    print_comparison(
        &format!("{} batches × {} writes", num_batches, BATCH_SIZE),
        avg_manifold,
        avg_redb,
        total_ops,
    );

    // ========================================================================
    // Test 2: Concurrent writes
    // ========================================================================
    print_section("Concurrent Writes (Multiple Threads)");
    println!(
        "  {:<45} {:>15} {:>15} {:>10}",
        "Configuration", "Manifold", "redb 2.6", "Speedup"
    );
    println!("  {}", "-".repeat(88));

    for num_threads in [2, 4, 8] {
        let batches_per_thread = 50;
        let total_ops = num_threads * batches_per_thread * BATCH_SIZE;

        // Warmup
        for _ in 0..WARMUP_ITERATIONS {
            let _ = benchmark_manifold_concurrent_writes(num_threads, batches_per_thread);
            let _ = benchmark_redb_concurrent_writes(num_threads, batches_per_thread);
        }

        // Benchmark
        let mut manifold_times = vec![];
        let mut redb_times = vec![];
        for _ in 0..BENCHMARK_ITERATIONS {
            manifold_times.push(benchmark_manifold_concurrent_writes(
                num_threads,
                batches_per_thread,
            ));
            redb_times.push(benchmark_redb_concurrent_writes(
                num_threads,
                batches_per_thread,
            ));
        }

        let avg_manifold: Duration =
            manifold_times.iter().sum::<Duration>() / manifold_times.len() as u32;
        let avg_redb: Duration = redb_times.iter().sum::<Duration>() / redb_times.len() as u32;

        print_comparison(
            &format!("{} threads × {} batches", num_threads, batches_per_thread),
            avg_manifold,
            avg_redb,
            total_ops,
        );
    }

    // ========================================================================
    // Test 3: Read performance
    // ========================================================================
    print_section("Read Performance (Single Transaction)");
    println!(
        "  {:<45} {:>15} {:>15} {:>10}",
        "Test", "Manifold", "redb 2.6", "Speedup"
    );
    println!("  {}", "-".repeat(88));

    let num_entries = 10000;

    // Warmup
    for _ in 0..WARMUP_ITERATIONS {
        let _ = benchmark_manifold_reads(num_entries);
        let _ = benchmark_redb_reads(num_entries);
    }

    // Benchmark
    let mut manifold_times = vec![];
    let mut redb_times = vec![];
    for _ in 0..BENCHMARK_ITERATIONS {
        manifold_times.push(benchmark_manifold_reads(num_entries));
        redb_times.push(benchmark_redb_reads(num_entries));
    }

    let avg_manifold: Duration =
        manifold_times.iter().sum::<Duration>() / manifold_times.len() as u32;
    let avg_redb: Duration = redb_times.iter().sum::<Duration>() / redb_times.len() as u32;

    print_comparison(
        &format!("{} sequential reads", num_entries),
        avg_manifold,
        avg_redb,
        num_entries,
    );

    // ========================================================================
    // Test 4: Range scan performance
    // ========================================================================
    print_section("Range Scan Performance");
    println!(
        "  {:<45} {:>15} {:>15} {:>10}",
        "Test", "Manifold", "redb 2.6", "Speedup"
    );
    println!("  {}", "-".repeat(88));

    for scan_size in [1000, 10000] {
        let num_entries = 50000;

        // Warmup
        for _ in 0..WARMUP_ITERATIONS {
            let _ = benchmark_manifold_range_scan(num_entries, scan_size);
            let _ = benchmark_redb_range_scan(num_entries, scan_size);
        }

        // Benchmark
        let mut manifold_times = vec![];
        let mut redb_times = vec![];
        for _ in 0..BENCHMARK_ITERATIONS {
            manifold_times.push(benchmark_manifold_range_scan(num_entries, scan_size));
            redb_times.push(benchmark_redb_range_scan(num_entries, scan_size));
        }

        let avg_manifold: Duration =
            manifold_times.iter().sum::<Duration>() / manifold_times.len() as u32;
        let avg_redb: Duration = redb_times.iter().sum::<Duration>() / redb_times.len() as u32;

        print_comparison(
            &format!("Scan {} of {} entries", scan_size, num_entries),
            avg_manifold,
            avg_redb,
            scan_size,
        );
    }

    print_section("Benchmark Complete");
    println!("\nNote: Manifold uses column families for true concurrent writes.");
    println!("      redb 2.6 serializes all write transactions.");
    println!("      Speedup shows Manifold performance relative to vanilla redb.\n");
}
