//! Graph domain benchmark for Manifold
//!
//! Tests manifold-graph performance characteristics:
//! - Batch edge insertion (forward/reverse dual-table pattern)
//! - Bidirectional traversal (outgoing vs incoming by vertex degree)
//! - Full graph iteration performance
//! - Integration overhead (EdgeSource trait)
//! - Concurrent graph modifications
//! - Sustained high-volume stress tests
//!
//! Domain optimization benchmarks - Phase 2: Graph

use manifold::column_family::ColumnFamilyDatabase;
use manifold_graph::{GraphTable, GraphTableRead};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;
use uuid::Uuid;

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

/// Generate a batch of random edges for testing
fn generate_edges(count: usize, num_vertices: usize) -> Vec<(Uuid, &'static str, Uuid, bool, f32, u64)> {
    let vertices: Vec<Uuid> = (0..num_vertices).map(|_| Uuid::new_v4()).collect();
    let mut edges = Vec::with_capacity(count);
    let mut rng = 12345u64;
    let base_timestamp = 1000000000u64;

    for i in 0..count {
        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let source_idx = (rng as usize) % num_vertices;

        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let target_idx = (rng as usize) % num_vertices;

        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let edge_type = match rng % 4 {
            0 => "follows",
            1 => "likes",
            2 => "mentions",
            _ => "retweets",
        };

        let is_active = true;
        let weight = (rng % 100) as f32 / 100.0;
        let timestamp = base_timestamp + (i as u64);

        edges.push((
            vertices[source_idx],
            edge_type,
            vertices[target_idx],
            is_active,
            weight,
            timestamp,
        ));
    }

    edges
}

/// Benchmark: Individual edge insertions
fn benchmark_individual_inserts(num_edges: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("graph").unwrap();

    let edges = generate_edges(num_edges, num_edges / 10);

    let start = Instant::now();

    let txn = cf.begin_write().unwrap();
    {
        let mut graph = GraphTable::open(&txn, "social").unwrap();

        for (source, edge_type, target, is_active, weight, timestamp) in &edges {
            graph
                .add_edge(source, edge_type, target, *is_active, *weight, Some(*timestamp))
                .unwrap();
        }
    }
    txn.commit().unwrap();

    let elapsed = start.elapsed();

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Batch edge insertion (sorted vs unsorted)
fn benchmark_batch_insert(num_edges: usize, sorted: bool) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("graph").unwrap();

    let edges = generate_edges(num_edges, num_edges / 10);

    let start = Instant::now();

    let txn = cf.begin_write().unwrap();
    {
        let mut graph = GraphTable::open(&txn, "social").unwrap();
        graph.add_edges_batch(&edges, sorted).unwrap();
    }
    txn.commit().unwrap();

    let elapsed = start.elapsed();

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Outgoing edge traversal by vertex degree
fn benchmark_outgoing_traversal(num_vertices: usize, edges_per_vertex: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("graph").unwrap();

    let vertices: Vec<Uuid> = (0..num_vertices).map(|_| Uuid::new_v4()).collect();

    // Create edges with known degree
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut graph = GraphTable::open(&txn, "social").unwrap();

            for source in &vertices {
                for i in 0..edges_per_vertex {
                    let target = Uuid::new_v4();
                    graph
                        .add_edge(source, "follows", &target, true, i as f32, None)
                        .unwrap();
                }
            }
        }
        txn.commit().unwrap();
    }

    // Benchmark traversal
    let txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&txn, "social").unwrap();

    let start = Instant::now();

    for source in &vertices {
        let mut count = 0;
        for result in graph.outgoing_edges(source).unwrap() {
            let edge = result.unwrap();
            // Only count "follows" edges
            if edge.edge_type == "follows" {
                count += 1;
            }
        }
        assert_eq!(count, edges_per_vertex);
    }

    let elapsed = start.elapsed();

    drop(graph);
    drop(txn);
    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Incoming edge traversal by vertex degree
fn benchmark_incoming_traversal(num_vertices: usize, edges_per_vertex: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("graph").unwrap();

    let vertices: Vec<Uuid> = (0..num_vertices).map(|_| Uuid::new_v4()).collect();

    // Create edges with known degree
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut graph = GraphTable::open(&txn, "social").unwrap();

            for target in &vertices {
                for i in 0..edges_per_vertex {
                    let source = Uuid::new_v4();
                    graph
                        .add_edge(&source, "follows", target, true, i as f32, None)
                        .unwrap();
                }
            }
        }
        txn.commit().unwrap();
    }

    // Benchmark traversal
    let txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&txn, "social").unwrap();

    let start = Instant::now();

    for target in &vertices {
        let mut count = 0;
        for result in graph.incoming_edges(target).unwrap() {
            let edge = result.unwrap();
            // Only count "follows" edges
            if edge.edge_type == "follows" {
                count += 1;
            }
        }
        assert_eq!(count, edges_per_vertex);
    }

    let elapsed = start.elapsed();

    drop(graph);
    drop(txn);
    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Full graph iteration
fn benchmark_full_iteration(num_edges: usize) -> (Duration, usize) {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("graph").unwrap();

    let edges = generate_edges(num_edges, num_edges / 10);

    // Populate graph
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut graph = GraphTable::open(&txn, "social").unwrap();
            graph.add_edges_batch(&edges, false).unwrap();
        }
        txn.commit().unwrap();
    }

    // Benchmark iteration
    let txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&txn, "social").unwrap();

    let start = Instant::now();

    let mut count = 0;
    for result in graph.all_edges().unwrap() {
        let _edge = result.unwrap();
        count += 1;
    }

    let elapsed = start.elapsed();

    // Note: count may be less than num_edges due to duplicate edges being overwritten
    // (same source + edge_type + target key). This is expected behavior.

    drop(graph);
    drop(txn);
    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    (elapsed, count)
}

/// Benchmark: Edge updates
fn benchmark_edge_updates(num_edges: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("graph").unwrap();

    let edges = generate_edges(num_edges, num_edges / 10);

    // Populate graph
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut graph = GraphTable::open(&txn, "social").unwrap();
            for (source, edge_type, target, is_active, weight, timestamp) in &edges {
                graph
                    .add_edge(source, edge_type, target, *is_active, *weight, Some(*timestamp))
                    .unwrap();
            }
        }
        txn.commit().unwrap();
    }

    // Benchmark updates
    let start = Instant::now();

    let txn = cf.begin_write().unwrap();
    {
        let mut graph = GraphTable::open(&txn, "social").unwrap();

        for (source, edge_type, target, _, weight, _) in &edges {
            graph
                .update_edge(source, edge_type, target, false, weight + 0.1)
                .unwrap();
        }
    }
    txn.commit().unwrap();

    let elapsed = start.elapsed();

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Edge deletions
fn benchmark_edge_deletions(num_edges: usize) -> Duration {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("graph").unwrap();

    let edges = generate_edges(num_edges, num_edges / 10);

    // Populate graph
    {
        let txn = cf.begin_write().unwrap();
        {
            let mut graph = GraphTable::open(&txn, "social").unwrap();
            for (source, edge_type, target, is_active, weight, timestamp) in &edges {
                graph
                    .add_edge(source, edge_type, target, *is_active, *weight, Some(*timestamp))
                    .unwrap();
            }
        }
        txn.commit().unwrap();
    }

    // Benchmark deletions
    let start = Instant::now();

    let txn = cf.begin_write().unwrap();
    {
        let mut graph = GraphTable::open(&txn, "social").unwrap();

        for (source, edge_type, target, _, _, _) in &edges {
            graph.remove_edge(source, edge_type, target).unwrap();
        }
    }
    txn.commit().unwrap();

    let elapsed = start.elapsed();

    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    drop(tmpfile);

    elapsed
}

/// Benchmark: Sustained edge insertion stress test
fn benchmark_sustained_inserts(
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
            while !stop.load(Ordering::Relaxed) {
                let edges = generate_edges(batch_size, batch_size / 2);

                let txn = cf.begin_write().unwrap();
                {
                    let mut graph = GraphTable::open(&txn, "social").unwrap();

                    for (source, edge_type, target, is_active, weight, timestamp) in &edges {
                        graph
                            .add_edge(source, edge_type, target, *is_active, *weight, Some(*timestamp))
                            .unwrap();
                    }
                }
                txn.commit().unwrap();

                ops.fetch_add(batch_size, Ordering::Relaxed);
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

/// Benchmark: Mixed read/write graph workload
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

        // Pre-populate with some edges
        let edges = generate_edges(1000, 100);
        let txn = cf.begin_write().unwrap();
        {
            let mut graph = GraphTable::open(&txn, "social").unwrap();
            graph.add_edges_batch(&edges, false).unwrap();
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
            let mut rng = thread_id as u64;

            while !stop.load(Ordering::Relaxed) {
                rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                let do_read = (rng % 100) < read_percentage as u64;

                if do_read {
                    let txn = cf.begin_read().unwrap();
                    let graph = GraphTableRead::open(&txn, "social").unwrap();

                    let mut read_count = 0;
                    for result in graph.all_edges().unwrap() {
                        let _edge = result.unwrap();
                        read_count += 1;
                        if read_count >= 100 {
                            break;
                        }
                    }
                    reads.fetch_add(read_count, Ordering::Relaxed);
                } else {
                    let edges = generate_edges(50, 25);

                    let txn = cf.begin_write().unwrap();
                    {
                        let mut graph = GraphTable::open(&txn, "social").unwrap();

                        for (source, edge_type, target, is_active, weight, timestamp) in &edges {
                            graph
                                .add_edge(source, edge_type, target, *is_active, *weight, Some(*timestamp))
                                .unwrap();
                        }
                    }
                    txn.commit().unwrap();
                    writes.fetch_add(edges.len(), Ordering::Relaxed);
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
    println!("MANIFOLD GRAPH DOMAIN BENCHMARK");
    println!("Testing manifold-graph performance characteristics");
    println!("{}", "=".repeat(80));
    println!();

    // 1. Individual Edge Insertions
    print_section("1. Individual Edge Insertions");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &count in &[1000, 5000, 10000] {
        let mut durations = Vec::new();

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_individual_inserts(count);

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(&format!("{} edges", count), avg_duration, count);
    }

    // 2. Batch Edge Insertions
    print_section("2. Batch Edge Insertions");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &count in &[10000, 50000] {
        for &sorted in &[false, true] {
            let mut durations = Vec::new();

            for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
                let duration = benchmark_batch_insert(count, sorted);

                if i >= WARMUP_ITERATIONS {
                    durations.push(duration);
                }
            }

            let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
            let sort_label = if sorted { "sorted" } else { "unsorted" };
            print_result(
                &format!("{} edges ({})", count, sort_label),
                avg_duration,
                count,
            );
        }
    }

    // 3. Outgoing Edge Traversal by Degree
    print_section("3. Outgoing Edge Traversal (by vertex degree)");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &(vertices, degree) in &[(100, 10), (100, 50), (100, 100)] {
        let mut durations = Vec::new();
        let total_edges = vertices * degree;

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_outgoing_traversal(vertices, degree);

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(
            &format!("{} vertices, degree {}", vertices, degree),
            avg_duration,
            total_edges,
        );
    }

    // 4. Incoming Edge Traversal by Degree
    print_section("4. Incoming Edge Traversal (by vertex degree)");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &(vertices, degree) in &[(100, 10), (100, 50), (100, 100)] {
        let mut durations = Vec::new();
        let total_edges = vertices * degree;

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_incoming_traversal(vertices, degree);

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(
            &format!("{} vertices, degree {}", vertices, degree),
            avg_duration,
            total_edges,
        );
    }

    // 5. Full Graph Iteration
    print_section("5. Full Graph Iteration");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &count in &[10000, 50000, 100000] {
        let mut durations = Vec::new();
        let mut actual_count = 0;

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let (duration, edges_counted) = benchmark_full_iteration(count);

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
                actual_count = edges_counted; // Use last iteration's count
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(
            &format!("{} edges (actual: {})", count, actual_count),
            avg_duration,
            actual_count,
        );
    }

    // 6. Edge Updates
    print_section("6. Edge Updates");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &count in &[1000, 5000, 10000] {
        let mut durations = Vec::new();

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_edge_updates(count);

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(&format!("{} edges", count), avg_duration, count);
    }

    // 7. Edge Deletions
    print_section("7. Edge Deletions");
    println!("  {:<50} {:>12}  {:>15}", "Test", "Duration", "Throughput");
    println!("  {}", "-".repeat(80));

    for &count in &[1000, 5000, 10000] {
        let mut durations = Vec::new();

        for i in 0..WARMUP_ITERATIONS + BENCHMARK_ITERATIONS {
            let duration = benchmark_edge_deletions(count);

            if i >= WARMUP_ITERATIONS {
                durations.push(duration);
            }
        }

        let avg_duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        print_result(&format!("{} edges", count), avg_duration, count);
    }

    // 8. Sustained Insert Stress Test
    print_section("8. Sustained Insert Stress Test (30 seconds)");
    println!(
        "  {:<50} {:>12}  {:>15}",
        "Configuration", "Duration", "Throughput"
    );
    println!("  {}", "-".repeat(80));

    for &(threads, batch) in &[(1, 100), (4, 100), (8, 100), (4, 1000)] {
        let (total_ops, duration) =
            benchmark_sustained_inserts(STRESS_TEST_DURATION_SECS, threads, batch);

        print_result(
            &format!("{} threads, batch={}", threads, batch),
            duration,
            total_ops,
        );
    }

    // 9. Mixed Read/Write Workload
    print_section("9. Mixed Read/Write Workload (5 seconds)");
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
