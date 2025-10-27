//! Mixed workload benchmark for Manifold
//!
//! Tests realistic mixed read/write patterns (80/20, 50/50, 20/80),
//! concurrent operations across multiple column families, contention
//! and scalability, and Zipfian distribution access patterns.
//!
//! Phase 1, Task 1.5: Mixed workload benchmarks

use manifold::TableDefinition;
use manifold::column_family::ColumnFamilyDatabase;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

const BENCHMARK_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

// Consistent batch size across all benchmarks
const BATCH_SIZE: usize = 1000;

const WARMUP_DURATION_SECS: u64 = 2;
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

/// Simple Zipfian distribution for realistic access patterns
/// Hot items (lower keys) are accessed more frequently
fn zipfian_key(counter: u64, num_items: u64) -> u64 {
    // Simple approximation: first 20% of keys get 80% of accesses
    let hot_threshold = num_items / 5;
    let selector = counter % 100;

    if selector < 80 {
        // 80% of accesses go to first 20% of keys
        (counter / 100) % hot_threshold
    } else {
        // 20% of accesses go to remaining 80% of keys
        hot_threshold + ((counter / 100) % (num_items - hot_threshold))
    }
}

/// Benchmark: Mixed read/write workload with consistent batching
/// Each thread gets its own CF for true parallel execution
/// Uses 1000-op batches with default durability (WAL enabled) for fair comparison
fn benchmark_mixed_workload(
    num_threads: usize,
    read_percentage: usize,
    duration_secs: u64,
) -> (usize, usize, Duration) {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    // Create separate CF per thread for parallel execution
    for i in 0..num_threads {
        db.create_column_family(&format!("thread_{}", i), Some(100 * 1024 * 1024))
            .unwrap();

        // Pre-populate with 50K entries per CF
        let cf = db.column_family(&format!("thread_{}", i)).unwrap();
        let data = vec![0u8; 1024];
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
            for j in 0..50000u64 {
                table.insert(&j, data.as_slice()).unwrap();
            }
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
            let data = vec![0u8; 1024];
            let mut counter = 0u64;
            let mut rng = thread_id as u64;

            while !stop.load(Ordering::Relaxed) {
                rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                let do_reads = (rng % 100) < read_percentage as u64;

                if do_reads {
                    // Batch BATCH_SIZE reads in one read transaction
                    let txn = cf.begin_read().unwrap();
                    let table = txn.open_table(BENCHMARK_TABLE).unwrap();
                    for _ in 0..BATCH_SIZE {
                        let key = zipfian_key(counter, 50000);
                        let _value = table.get(&key).unwrap();
                        counter += 1;
                    }
                    reads.fetch_add(BATCH_SIZE, Ordering::Relaxed);
                } else {
                    // Batch BATCH_SIZE writes in one write transaction
                    // Use default durability to let WAL group commit work
                    let txn = cf.begin_write().unwrap();
                    {
                        let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
                        for _ in 0..BATCH_SIZE {
                            let key = zipfian_key(counter, 50000);
                            table.insert(&key, data.as_slice()).unwrap();
                            counter += 1;
                        }
                    }
                    txn.commit().unwrap();
                    writes.fetch_add(BATCH_SIZE, Ordering::Relaxed);
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
    (
        total_reads.load(Ordering::Relaxed),
        total_writes.load(Ordering::Relaxed),
        elapsed,
    )
}

/// Benchmark: Multi-column family mixed workload with consistent batching
fn benchmark_multi_cf_mixed(
    num_cfs: usize,
    threads_per_cf: usize,
    read_percentage: usize,
    duration_secs: u64,
) -> (usize, usize, Duration) {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    // Create and populate column families
    for i in 0..num_cfs {
        db.create_column_family(&format!("cf_{}", i), Some(100 * 1024 * 1024))
            .unwrap();

        let cf = db.column_family(&format!("cf_{}", i)).unwrap();
        let data = vec![0u8; 1024];
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
            for j in 0..50000u64 {
                table.insert(&j, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    let stop_flag = Arc::new(AtomicBool::new(false));
    let total_reads = Arc::new(AtomicUsize::new(0));
    let total_writes = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    let start = Instant::now();

    for cf_id in 0..num_cfs {
        for thread_id in 0..threads_per_cf {
            let db_clone = db.clone();
            let stop = stop_flag.clone();
            let reads = total_reads.clone();
            let writes = total_writes.clone();

            let handle = std::thread::spawn(move || {
                let cf = db_clone.column_family(&format!("cf_{}", cf_id)).unwrap();
                let data = vec![0u8; 1024];
                let mut counter = 0u64;
                let mut rng = (cf_id * 1000 + thread_id) as u64;

                while !stop.load(Ordering::Relaxed) {
                    rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                    let do_reads = (rng % 100) < read_percentage as u64;

                    if do_reads {
                        // Batch BATCH_SIZE reads in one read transaction
                        let txn = cf.begin_read().unwrap();
                        let table = txn.open_table(BENCHMARK_TABLE).unwrap();
                        for _ in 0..BATCH_SIZE {
                            let key = counter % 50000;
                            let _value = table.get(&key).unwrap();
                            counter += 1;
                        }
                        reads.fetch_add(BATCH_SIZE, Ordering::Relaxed);
                    } else {
                        // Batch BATCH_SIZE writes in one write transaction
                        // Use default durability for WAL group commit
                        let txn = cf.begin_write().unwrap();
                        {
                            let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
                            for _ in 0..BATCH_SIZE {
                                let key = counter % 50000;
                                table.insert(&key, data.as_slice()).unwrap();
                                counter += 1;
                            }
                        }
                        txn.commit().unwrap();
                        writes.fetch_add(BATCH_SIZE, Ordering::Relaxed);
                    }
                }
            });
            handles.push(handle);
        }
    }

    std::thread::sleep(Duration::from_secs(duration_secs));
    stop_flag.store(true, Ordering::Relaxed);

    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start.elapsed();
    (
        total_reads.load(Ordering::Relaxed),
        total_writes.load(Ordering::Relaxed),
        elapsed,
    )
}

/// Benchmark: Thread scalability with mixed workload
fn benchmark_scalability(num_threads: usize, duration_secs: u64) -> (usize, usize, Duration) {
    benchmark_mixed_workload(num_threads, 50, duration_secs)
}

fn main() {
    println!("\nManifold Mixed Workload Benchmark Suite");
    println!("Phase 1, Task 1.5");
    println!("Warmup duration: {}s", WARMUP_DURATION_SECS);
    println!("Benchmark duration: {}s", BENCHMARK_DURATION_SECS);

    // ========================================================================
    // Test 1: Read/Write ratio variations (80/20, 50/50, 20/80)
    // ========================================================================
    print_section("Read/Write Ratio Comparison (8 threads)");
    println!(
        "  {:<30} {:>15} {:>15} {:>15}",
        "Configuration", "Reads/sec", "Writes/sec", "Total ops/sec"
    );
    println!("  {}", "-".repeat(80));

    for (label, read_pct) in [
        ("80% Reads, 20% Writes", 80),
        ("50% Reads, 50% Writes", 50),
        ("20% Reads, 80% Writes", 20),
    ] {
        // Warmup
        let _ = benchmark_mixed_workload(8, read_pct, WARMUP_DURATION_SECS);

        // Benchmark
        let (reads, writes, duration) =
            benchmark_mixed_workload(8, read_pct, BENCHMARK_DURATION_SECS);

        let total_throughput = (reads + writes) as f64 / duration.as_secs_f64();

        println!(
            "  {:<30} {:>15} {:>15} {:>15}",
            label,
            format_throughput(reads, duration),
            format_throughput(writes, duration),
            format!("{:.2}K ops/sec", total_throughput / 1000.0)
        );
    }

    // ========================================================================
    // Test 2: Multi-column family contention
    // ========================================================================
    print_section("Multi-Column Family Mixed Workload (50/50 R/W)");
    println!(
        "  {:<30} {:>15} {:>15} {:>15}",
        "Configuration", "Reads/sec", "Writes/sec", "Total ops/sec"
    );
    println!("  {}", "-".repeat(80));

    for (num_cfs, threads_per_cf) in [(2, 2), (4, 2), (8, 1), (4, 4)] {
        let total_threads = num_cfs * threads_per_cf;
        let label = format!("{} CFs × {} threads", num_cfs, threads_per_cf);

        // Warmup
        let _ = benchmark_multi_cf_mixed(num_cfs, threads_per_cf, 50, WARMUP_DURATION_SECS);

        // Benchmark
        let (reads, writes, duration) =
            benchmark_multi_cf_mixed(num_cfs, threads_per_cf, 50, BENCHMARK_DURATION_SECS);

        let total_throughput = (reads + writes) as f64 / duration.as_secs_f64();

        println!(
            "  {:<30} {:>15} {:>15} {:>15}",
            format!("{} ({} total)", label, total_threads),
            format_throughput(reads, duration),
            format_throughput(writes, duration),
            format!("{:.2}K ops/sec", total_throughput / 1000.0)
        );
    }

    // ========================================================================
    // Test 3: Thread scalability (1-16 threads)
    // ========================================================================
    print_section("Thread Scalability (50/50 R/W, Zipfian distribution)");
    println!(
        "  {:<30} {:>15} {:>15} {:>15}",
        "Threads", "Reads/sec", "Writes/sec", "Total ops/sec"
    );
    println!("  {}", "-".repeat(80));

    for num_threads in [1, 2, 4, 8, 12, 16] {
        // Warmup
        let _ = benchmark_scalability(num_threads, WARMUP_DURATION_SECS);

        // Benchmark
        let (reads, writes, duration) = benchmark_scalability(num_threads, BENCHMARK_DURATION_SECS);

        let total_throughput = (reads + writes) as f64 / duration.as_secs_f64();

        println!(
            "  {:<30} {:>15} {:>15} {:>15}",
            format!("{} threads", num_threads),
            format_throughput(reads, duration),
            format_throughput(writes, duration),
            format!("{:.2}K ops/sec", total_throughput / 1000.0)
        );
    }

    // ========================================================================
    // Test 4: Zipfian vs Uniform access patterns
    // ========================================================================
    print_section("Access Pattern Comparison (8 threads, 80/20 R/W)");
    println!("\n  Note: All tests use Zipfian (80/20 hot/cold) distribution");
    println!("        This simulates realistic workloads where some keys are");
    println!("        accessed much more frequently than others.\n");

    let (reads, writes, duration) = benchmark_mixed_workload(8, 80, BENCHMARK_DURATION_SECS);

    let total_ops = reads + writes;
    println!(
        "  Total operations: {} ({} reads, {} writes)",
        total_ops, reads, writes
    );
    println!("  Duration: {}", format_duration(duration));
    println!("  Throughput: {}", format_throughput(total_ops, duration));

    print_section("Benchmark Complete");
    println!();
}
