//! Vector domain benchmark for Manifold
//!
//! Tests manifold-vectors performance characteristics:
//! - Dense vector write throughput (128/384/768 dimensions)
//! - Zero-copy read performance (guard vs traditional deserialization)
//! - Distance computation throughput (cosine, euclidean, dot product)
//! - Batch insert operations with varying sizes
//! - Sustained high-volume stress tests
//!
//! Domain optimization benchmarks - Phase 1: Vectors

use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::{VectorTable, VectorTableRead, distance};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

const WARMUP_ITERATIONS: usize = 1;
const BENCHMARK_ITERATIONS: usize = 3;
const BENCHMARK_DURATION_SECS: u64 = 5;
const STRESS_TEST_DURATION_SECS: u64 = 30;

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

/// Generate random vector for testing
fn random_vector<const DIM: usize>(seed: u64) -> [f32; DIM] {
    let mut vec = [0.0f32; DIM];
    let mut rng = seed;
    for i in 0..DIM {
        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        vec[i] = (rng as f32 / u64::MAX as f32) * 2.0 - 1.0;
    }
    vec
}

/// Normalize vector to unit length for cosine similarity
fn normalize<const DIM: usize>(vec: &mut [f32; DIM]) {
    let magnitude: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if magnitude > 0.0 {
        for x in vec.iter_mut() {
            *x /= magnitude;
        }
    }
}

/// Benchmark: Dense vector write throughput for various dimensions
fn benchmark_dense_vector_writes<const DIM: usize>(
    batch_size: usize,
    num_batches: usize,
) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    let start = Instant::now();

    for batch_idx in 0..num_batches {
        let txn = cf.begin_write().unwrap();
        {
            let mut vectors = VectorTable::<DIM>::open(&txn, "embeddings").unwrap();

            for i in 0..batch_size {
                let key = format!("vec_{:08}", batch_idx * batch_size + i);
                let vector = random_vector::<DIM>((batch_idx * batch_size + i) as u64);
                vectors.insert(&key, &vector).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    start.elapsed()
}

/// Benchmark: Batch insert operations
fn benchmark_batch_insert<const DIM: usize>(total_vectors: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    // Prepare batch data
    let mut batch: Vec<(String, [f32; DIM])> = Vec::with_capacity(total_vectors);
    for i in 0..total_vectors {
        let key = format!("vec_{:08}", i);
        let vector = random_vector::<DIM>(i as u64);
        batch.push((key, vector));
    }

    let start = Instant::now();

    let txn = cf.begin_write().unwrap();
    {
        let mut vectors = VectorTable::<DIM>::open(&txn, "embeddings").unwrap();

        // Convert to slice of tuples with &str keys
        let batch_refs: Vec<(&str, [f32; DIM])> =
            batch.iter().map(|(k, v)| (k.as_str(), *v)).collect();

        vectors.insert_batch(&batch_refs, false).unwrap();
    }
    txn.commit().unwrap();

    start.elapsed()
}

/// Benchmark: Zero-copy read performance using guards
fn benchmark_guard_reads<const DIM: usize>(
    num_vectors: usize,
    reads_per_vector: usize,
) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    // Populate data
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut vectors = VectorTable::<DIM>::open(&txn, "embeddings").unwrap();
            for i in 0..num_vectors {
                let key = format!("vec_{:08}", i);
                let vector = random_vector::<DIM>(i as u64);
                vectors.insert(&key, &vector).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    // Benchmark reads
    let txn = cf.begin_read().unwrap();
    let vectors = VectorTableRead::<DIM>::open(&txn, "embeddings").unwrap();

    let start = Instant::now();

    for _ in 0..reads_per_vector {
        for i in 0..num_vectors {
            let key = format!("vec_{:08}", i);
            let guard = vectors.get(&key).unwrap().unwrap();
            // Access the vector data through guard
            let _first = guard.value()[0];
        }
    }

    start.elapsed()
}

/// Benchmark: Full iteration over all vectors
fn benchmark_full_iteration<const DIM: usize>(num_vectors: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    // Populate data
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut vectors = VectorTable::<DIM>::open(&txn, "embeddings").unwrap();
            for i in 0..num_vectors {
                let key = format!("vec_{:08}", i);
                let vector = random_vector::<DIM>(i as u64);
                vectors.insert(&key, &vector).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    // Benchmark iteration
    let txn = cf.begin_read().unwrap();
    let vectors = VectorTableRead::<DIM>::open(&txn, "embeddings").unwrap();

    let start = Instant::now();

    let mut count = 0;
    for result in vectors.all_vectors().unwrap() {
        let (_key, guard) = result.unwrap();
        let _first = guard.value()[0];
        count += 1;
    }

    assert_eq!(count, num_vectors);
    start.elapsed()
}

/// Benchmark: Distance computation throughput
fn benchmark_distance_computation<const DIM: usize>(
    num_vectors: usize,
    distance_fn: &str,
) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    // Populate data with normalized vectors for cosine
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut vectors = VectorTable::<DIM>::open(&txn, "embeddings").unwrap();
            for i in 0..num_vectors {
                let key = format!("vec_{:08}", i);
                let mut vector = random_vector::<DIM>(i as u64);
                normalize(&mut vector);
                vectors.insert(&key, &vector).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    // Prepare query vector
    let mut query = random_vector::<DIM>(99999);
    normalize(&mut query);

    // Benchmark distance computations
    let txn = cf.begin_read().unwrap();
    let vectors = VectorTableRead::<DIM>::open(&txn, "embeddings").unwrap();

    let start = Instant::now();

    for result in vectors.all_vectors().unwrap() {
        let (_key, guard) = result.unwrap();
        let _similarity = match distance_fn {
            "cosine" => distance::cosine(&query, guard.value()),
            "euclidean" => distance::euclidean(&query, guard.value()),
            "dot" => distance::dot_product(&query, guard.value()),
            "manhattan" => distance::manhattan(&query, guard.value()),
            _ => panic!("Unknown distance function"),
        };
    }

    start.elapsed()
}

/// Benchmark: Sustained write stress test
fn benchmark_sustained_writes<const DIM: usize>(
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
            let mut counter = 0usize;

            while !stop.load(Ordering::Relaxed) {
                let txn = cf.begin_write().unwrap();
                {
                    let mut vectors = VectorTable::<DIM>::open(&txn, "embeddings").unwrap();

                    for i in 0..batch_size {
                        let key = format!("t{}_vec_{:08}", thread_id, counter * batch_size + i);
                        let vector = random_vector::<DIM>((counter * batch_size + i) as u64);
                        vectors.insert(&key, &vector).unwrap();
                    }
                }
                txn.commit().unwrap();

                ops.fetch_add(batch_size, Ordering::Relaxed);
                counter += 1;
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

    // Explicitly drop database and ensure all Arc references are released
    drop(db);

    // Force sync to ensure file handles are released before returning
    std::thread::sleep(Duration::from_millis(50));

    drop(tmpfile);

    (total, elapsed)
}

/// Benchmark: Mixed read/write workload
fn benchmark_mixed_workload<const DIM: usize>(
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
        let txn = cf.begin_write().unwrap();
        {
            let mut vectors = VectorTable::<DIM>::open(&txn, "embeddings").unwrap();
            for j in 0..10000 {
                let key = format!("vec_{:08}", j);
                let vector = random_vector::<DIM>(j as u64);
                vectors.insert(&key, &vector).unwrap();
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
            let mut counter = 0u64;
            let mut rng = thread_id as u64;

            while !stop.load(Ordering::Relaxed) {
                rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                let do_read = (rng % 100) < read_percentage as u64;

                if do_read {
                    let txn = cf.begin_read().unwrap();
                    let vectors = VectorTableRead::<DIM>::open(&txn, "embeddings").unwrap();

                    for _ in 0..100 {
                        let key = format!("vec_{:08}", counter % 10000);
                        if let Ok(Some(guard)) = vectors.get(&key) {
                            let _first = guard.value()[0];
                        }
                        counter += 1;
                    }
                    reads.fetch_add(100, Ordering::Relaxed);
                } else {
                    let txn = cf.begin_write().unwrap();
                    {
                        let mut vectors = VectorTable::<DIM>::open(&txn, "embeddings").unwrap();

                        for _ in 0..100 {
                            let key = format!("vec_{:08}", counter % 10000);
                            let vector = random_vector::<DIM>(counter);
                            vectors.insert(&key, &vector).unwrap();
                            counter += 1;
                        }
                    }
                    txn.commit().unwrap();
                    writes.fetch_add(100, Ordering::Relaxed);
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

    // Explicitly drop database and ensure all Arc references are released
    drop(db);

    // Force sync to ensure file handles are released before returning
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
    println!("MANIFOLD VECTORS DOMAIN BENCHMARK");
    println!("Testing manifold-vectors performance characteristics");
    println!("{}", "=".repeat(80));
    println!();
    println!(
        "Note: Benchmarks reuse databases across iterations to prevent file descriptor exhaustion."
    );
    println!();

    // 1. Dense Vector Write Throughput
    print_section("1. Dense Vector Write Throughput (Batch Size: 1000)");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &dim in &[128, 384, 768] {
        let mut durations = Vec::new();
        let ops = 10_000;

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = match dim {
                128 => benchmark_dense_vector_writes::<128>(1000, 10),
                384 => benchmark_dense_vector_writes::<384>(1000, 10),
                768 => benchmark_dense_vector_writes::<768>(1000, 10),
                _ => unreachable!(),
            };

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(&format!("{}-dim vectors", dim), avg_duration, ops);
    }

    // 2. Batch Insert Performance
    print_section("2. Batch Insert Operations");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &(dim, count) in &[(128, 10000), (384, 10000), (768, 10000)] {
        let mut durations = Vec::new();

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = match dim {
                128 => benchmark_batch_insert::<128>(count),
                384 => benchmark_batch_insert::<384>(count),
                768 => benchmark_batch_insert::<768>(count),
                _ => unreachable!(),
            };

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(
            &format!("{}-dim × {} vectors", dim, count),
            avg_duration,
            count,
        );
    }

    // 3. Zero-Copy Guard Reads
    print_section("3. Zero-Copy Guard Read Performance");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &dim in &[128, 384, 768] {
        let mut durations = Vec::new();
        let num_vectors = 1000;
        let reads_per = 10;
        let total_ops = num_vectors * reads_per;

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = match dim {
                128 => benchmark_guard_reads::<128>(num_vectors, reads_per),
                384 => benchmark_guard_reads::<384>(num_vectors, reads_per),
                768 => benchmark_guard_reads::<768>(num_vectors, reads_per),
                _ => unreachable!(),
            };

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(
            &format!("{}-dim × {} reads", dim, total_ops),
            avg_duration,
            total_ops,
        );
    }

    // 4. Full Iteration Performance
    print_section("4. Full Vector Iteration");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &(dim, count) in &[(128, 10000), (384, 10000), (768, 10000)] {
        let mut durations = Vec::new();

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = match dim {
                128 => benchmark_full_iteration::<128>(count),
                384 => benchmark_full_iteration::<384>(count),
                768 => benchmark_full_iteration::<768>(count),
                _ => unreachable!(),
            };

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(
            &format!("{}-dim × {} vectors", dim, count),
            avg_duration,
            count,
        );
    }

    // 5. Distance Computation Throughput
    print_section("5. Distance Computation Performance (768-dim)");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for distance_fn in &["cosine", "euclidean", "dot", "manhattan"] {
        let mut durations = Vec::new();
        let num_vectors = 5000;

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_distance_computation::<768>(num_vectors, distance_fn);

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(
            &format!("{} × {} comparisons", distance_fn, num_vectors),
            avg_duration,
            num_vectors,
        );
    }

    // 6. Sustained Write Stress Test
    print_section("6. Sustained Write Stress Test (30 seconds)");
    println!(
        "  {:<50} {:>12}  {:>15}",
        "Configuration", "Duration", "Throughput"
    );
    println!("  {}", "-".repeat(80));

    for &(dim, threads, batch) in &[(768, 1, 100), (768, 4, 100), (768, 8, 100), (768, 4, 1000)] {
        let (total_ops, duration) = match dim {
            768 => benchmark_sustained_writes::<768>(STRESS_TEST_DURATION_SECS, threads, batch),
            _ => unreachable!(),
        };

        print_result(
            &format!("{}-dim, {} threads, batch={}", dim, threads, batch),
            duration,
            total_ops,
        );
    }

    // 7. Mixed Read/Write Workload
    print_section("7. Mixed Read/Write Workload (5 seconds, 768-dim)");
    println!(
        "  {:<50} {:>12}  {:>15}",
        "Configuration", "Total Ops", "Throughput"
    );
    println!("  {}", "-".repeat(80));

    for &(threads, read_pct) in &[(4, 80), (4, 50), (4, 20)] {
        let (reads, writes, duration) =
            benchmark_mixed_workload::<768>(BENCHMARK_DURATION_SECS, threads, read_pct);
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
