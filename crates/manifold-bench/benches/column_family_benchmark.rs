use manifold::column_family::ColumnFamilyDatabase;
use manifold::{Durability, TableDefinition};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

const BENCHMARK_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

const WARMUP_ITERATIONS: usize = 3;
const BENCHMARK_ITERATIONS: usize = 10;

// Production-realistic batch size - write many records, sync periodically
const WRITES_PER_BATCH: usize = 1000;

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

fn calculate_percentiles(mut durations: Vec<Duration>) -> (Duration, Duration, Duration) {
    durations.sort();
    let len = durations.len();
    let p50 = durations[len / 2];
    let p95 = durations[(len * 95) / 100];
    let p99 = durations[(len * 99) / 100];
    (p50, p95, p99)
}

fn print_section(title: &str) {
    println!("\n{}", "=".repeat(80));
    println!("{title}");
    println!("{}", "=".repeat(80));
}

fn print_result(name: &str, duration: Duration, ops: usize) {
    println!(
        "  {:<45} {:>12}  {:>15}",
        name,
        format_duration(duration),
        format_throughput(ops, duration)
    );
}

fn benchmark_single_cf_sequential_writes(num_writes: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    // Use small initial allocation - auto-expansion will handle growth
    db.create_column_family("data", Some(1024 * 1024)).unwrap();

    let cf = db.column_family("data").unwrap();
    let data = vec![0u8; 1024];

    let start = Instant::now();

    // Production pattern: batch writes with periodic sync
    let batches = num_writes / WRITES_PER_BATCH;
    for batch in 0..batches {
        let mut txn = cf.begin_write().unwrap();
        // Use Durability::None for all but the last batch
        if batch < batches - 1 {
            txn.set_durability(Durability::None).unwrap();
        }
        {
            let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
            for i in 0..WRITES_PER_BATCH as u64 {
                let key = (batch as u64 * WRITES_PER_BATCH as u64) + i;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    start.elapsed()
}

fn benchmark_multi_cf_concurrent_writes(
    num_cfs: usize,
    writes_per_cf: usize,
) -> (Duration, Vec<Duration>) {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    // Create all CFs upfront with small initial allocations
    for i in 0..num_cfs {
        db.create_column_family(&format!("cf_{i}"), Some(1024 * 1024))
            .unwrap();
    }

    let start = Instant::now();
    let mut handles = vec![];

    for i in 0..num_cfs {
        let db_clone = db.clone();
        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family(&format!("cf_{i}")).unwrap();
            let data = vec![0u8; 1024];
            let thread_start = Instant::now();

            // Production pattern: batch writes with periodic sync
            let batches = writes_per_cf / WRITES_PER_BATCH;
            for batch in 0..batches {
                let mut txn = cf.begin_write().unwrap();
                // Only sync on last batch
                if batch < batches - 1 {
                    txn.set_durability(Durability::None).unwrap();
                }
                {
                    let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
                    for j in 0..WRITES_PER_BATCH as u64 {
                        let key = (batch as u64 * WRITES_PER_BATCH as u64) + j;
                        table.insert(&key, data.as_slice()).unwrap();
                    }
                }
                txn.commit().unwrap();
            }

            thread_start.elapsed()
        });
        handles.push(handle);
    }

    let thread_durations: Vec<Duration> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let total_duration = start.elapsed();
    (total_duration, thread_durations)
}

fn benchmark_cf_operations() {
    print_section("Column Family Operations Benchmark");

    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();

    let mut create_times = vec![];
    for i in 0..BENCHMARK_ITERATIONS {
        let start = Instant::now();
        db.create_column_family(&format!("cf_{i}"), Some(1024 * 1024))
            .unwrap();
        create_times.push(start.elapsed());
    }

    let (p50, p95, p99) = calculate_percentiles(create_times);
    println!("  CF Creation (1MB allocation):");
    println!(
        "    p50: {}  p95: {}  p99: {}",
        format_duration(p50),
        format_duration(p95),
        format_duration(p99)
    );

    let mut delete_times = vec![];
    for i in 0..BENCHMARK_ITERATIONS {
        let start = Instant::now();
        db.delete_column_family(&format!("cf_{i}")).unwrap();
        delete_times.push(start.elapsed());
    }

    let (p50, p95, p99) = calculate_percentiles(delete_times);
    println!("  CF Deletion:");
    println!(
        "    p50: {}  p95: {}  p99: {}",
        format_duration(p50),
        format_duration(p95),
        format_duration(p99)
    );
}

fn benchmark_concurrent_scaling() {
    print_section("Concurrent Write Scaling Benchmark");

    println!(
        "  {:<45} {:>12}  {:>15}",
        "Configuration", "Duration", "Throughput"
    );
    println!("  {}", "-".repeat(78));

    let writes_per_thread = 10_000;

    for _ in 0..WARMUP_ITERATIONS {
        let _ = benchmark_single_cf_sequential_writes(writes_per_thread);
    }

    let mut baseline_times = vec![];
    for _ in 0..BENCHMARK_ITERATIONS {
        baseline_times.push(benchmark_single_cf_sequential_writes(writes_per_thread));
    }
    let baseline_avg: Duration =
        baseline_times.iter().sum::<Duration>() / baseline_times.len() as u32;
    print_result(
        "1 CF, Sequential (baseline)",
        baseline_avg,
        writes_per_thread,
    );

    for num_threads in [2, 4, 8] {
        for _ in 0..WARMUP_ITERATIONS {
            let _ = benchmark_multi_cf_concurrent_writes(num_threads, writes_per_thread);
        }

        let mut times = vec![];
        for _ in 0..BENCHMARK_ITERATIONS {
            let (total_duration, _) =
                benchmark_multi_cf_concurrent_writes(num_threads, writes_per_thread);
            times.push(total_duration);
        }

        let avg_time: Duration = times.iter().sum::<Duration>() / times.len() as u32;
        let total_ops = num_threads * writes_per_thread;
        let speedup = baseline_avg.as_secs_f64() / avg_time.as_secs_f64();

        print_result(
            &format!("{num_threads} CFs, Concurrent ({speedup:.2}x speedup)"),
            avg_time,
            total_ops,
        );
    }
}

fn benchmark_multi_table_access() {
    print_section("Multi-Table Access Pattern Benchmark");

    const META_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("metadata");
    const VECTOR_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("vectors");
    const COMBINED_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("combined");

    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024))
        .unwrap();

    let cf = db.column_family("test").unwrap();

    let num_entries = 10000;
    let metadata = vec![0u8; 256];
    let vector_384d = vec![0u8; 384 * 4];
    let combined = vec![0u8; 256 + 384 * 4];

    println!("\n  Writing {num_entries} entries...");

    let write_txn = cf.begin_write().unwrap();
    {
        let mut meta_table = write_txn.open_table(META_TABLE).unwrap();
        let mut vec_table = write_txn.open_table(VECTOR_TABLE).unwrap();
        let mut combined_table = write_txn.open_table(COMBINED_TABLE).unwrap();

        for i in 0..num_entries {
            meta_table.insert(&i, metadata.as_slice()).unwrap();
            vec_table.insert(&i, vector_384d.as_slice()).unwrap();
            combined_table.insert(&i, combined.as_slice()).unwrap();
        }
    }
    write_txn.commit().unwrap();

    println!("\n  Reading patterns:");
    println!(
        "  {:<45} {:>12}  {:>15}",
        "Pattern", "Duration", "Throughput"
    );
    println!("  {}", "-".repeat(78));

    for _ in 0..WARMUP_ITERATIONS {
        let read_txn = cf.begin_read().unwrap();
        let vec_table = read_txn.open_table(VECTOR_TABLE).unwrap();
        for i in 0..num_entries {
            let _ = vec_table.get(&i).unwrap();
        }
    }

    let mut separate_times = vec![];
    for _ in 0..BENCHMARK_ITERATIONS {
        let start = Instant::now();
        let read_txn = cf.begin_read().unwrap();
        let vec_table = read_txn.open_table(VECTOR_TABLE).unwrap();
        for i in 0..num_entries {
            let _ = vec_table.get(&i).unwrap();
        }
        separate_times.push(start.elapsed());
    }
    let separate_avg: Duration =
        separate_times.iter().sum::<Duration>() / separate_times.len() as u32;
    print_result(
        "Vector table only (separate)",
        separate_avg,
        num_entries as usize,
    );

    for _ in 0..WARMUP_ITERATIONS {
        let read_txn = cf.begin_read().unwrap();
        let combined_table = read_txn.open_table(COMBINED_TABLE).unwrap();
        for i in 0..num_entries {
            let _ = combined_table.get(&i).unwrap();
        }
    }

    let mut combined_times = vec![];
    for _ in 0..BENCHMARK_ITERATIONS {
        let start = Instant::now();
        let read_txn = cf.begin_read().unwrap();
        let combined_table = read_txn.open_table(COMBINED_TABLE).unwrap();
        for i in 0..num_entries {
            let _ = combined_table.get(&i).unwrap();
        }
        combined_times.push(start.elapsed());
    }
    let combined_avg: Duration =
        combined_times.iter().sum::<Duration>() / combined_times.len() as u32;
    let speedup = combined_avg.as_secs_f64() / separate_avg.as_secs_f64();
    print_result(
        &format!("Combined table ({speedup:.2}x slower)"),
        combined_avg,
        num_entries as usize,
    );
}

fn benchmark_read_write_concurrency() {
    print_section("Read/Write Concurrency Benchmark");

    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    db.create_column_family("rw_stress", Some(10 * 1024 * 1024))
        .unwrap();

    let cf = db.column_family("rw_stress").unwrap();
    let data = vec![0u8; 1024];

    let write_txn = cf.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(BENCHMARK_TABLE).unwrap();
        for i in 0..10000u64 {
            table.insert(&i, data.as_slice()).unwrap();
        }
    }
    write_txn.commit().unwrap();

    println!("\n  Concurrent readers while writing:");
    println!(
        "  {:<45} {:>12}  {:>15}",
        "Configuration", "Duration", "Throughput"
    );
    println!("  {}", "-".repeat(78));

    for num_readers in [1, 4, 8, 16] {
        let db_clone = db.clone();
        let cf_name = cf.name().to_string();

        let start = Instant::now();
        let mut handles = vec![];

        for _ in 0..num_readers {
            let db_ref = db_clone.clone();
            let name = cf_name.clone();
            let handle = std::thread::spawn(move || {
                let cf = db_ref.column_family(&name).unwrap();
                let read_txn = cf.begin_read().unwrap();
                let table = read_txn.open_table(BENCHMARK_TABLE).unwrap();
                let mut sum = 0u64;
                for i in 0..10000 {
                    if let Some(val) = table.get(&i).unwrap() {
                        sum += val.value().len() as u64;
                    }
                }
                sum
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let duration = start.elapsed();
        print_result(
            &format!("{num_readers} concurrent readers"),
            duration,
            num_readers * 10000,
        );
    }
}

fn benchmark_expansion_overhead() {
    print_section("Auto-Expansion Overhead Benchmark");

    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();

    db.create_column_family("small", Some(16 * 1024)).unwrap();

    let cf = db.column_family("small").unwrap();
    let data = vec![0u8; 4096];

    println!("\n  Measuring expansion overhead with small initial allocation (16KB):");

    let mut first_expansion_detected = false;
    let mut write_times = vec![];

    for batch in 0..20 {
        let start = Instant::now();
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(BENCHMARK_TABLE).unwrap();
            for i in 0..10 {
                let key = (batch * 10 + i) as u64;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
        let duration = start.elapsed();

        write_times.push(duration);

        if !first_expansion_detected && batch > 0 && duration > write_times[0] * 2 {
            first_expansion_detected = true;
            println!(
                "    First expansion detected at batch {batch}: {}",
                format_duration(duration)
            );
        }
    }

    let (p50, p95, p99) = calculate_percentiles(write_times.clone());
    println!("    Latency percentiles across 20 batches:");
    println!(
        "      p50: {}  p95: {}  p99: {}",
        format_duration(p50),
        format_duration(p95),
        format_duration(p99)
    );

    let first_5_avg: Duration = write_times[0..5].iter().sum::<Duration>() / 5;
    let last_5_avg: Duration = write_times[15..20].iter().sum::<Duration>() / 5;
    println!(
        "    First 5 batches avg: {}  Last 5 batches avg: {}",
        format_duration(first_5_avg),
        format_duration(last_5_avg)
    );
}

fn main() {
    println!("\nManifold Column Family Benchmark Suite");
    println!("Version: 3.1.0");
    println!("Warmup iterations: {WARMUP_ITERATIONS}");
    println!("Benchmark iterations: {BENCHMARK_ITERATIONS}");
    println!("Batch size: {} writes per transaction\n", WRITES_PER_BATCH);
    println!(
        "NOTE: Production-realistic pattern - batch {} writes, sync periodically",
        WRITES_PER_BATCH
    );
    println!("      Small initial CF allocations (1MB) with auto-expansion\n");

    benchmark_cf_operations();
    benchmark_concurrent_scaling();
    benchmark_multi_table_access();
    benchmark_read_write_concurrency();
    benchmark_expansion_overhead();

    print_section("Benchmark Complete");
    println!();
}
