use manifold::column_family::ColumnFamilyDatabase;
use manifold::{Durability, TableDefinition};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n=== Column Family Write Isolation Benchmark ===\n");
    println!("Goal: Isolate actual write performance without fsync overhead\n");

    // Test 1: Single CF baseline - write only, no sync
    println!("Test 1: Single CF - Write Performance (no fsync)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("single", Some(10 * 1024 * 1024))
        .unwrap();

    let cf = db.column_family("single").unwrap();
    let data = vec![0u8; 1024];
    let num_writes = 10_000;

    let start = Instant::now();
    let mut txn = cf.begin_write().unwrap();
    txn.set_durability(Durability::None).unwrap();
    {
        let mut table = txn.open_table(TEST_TABLE).unwrap();
        for i in 0..num_writes {
            table.insert(&i, data.as_slice()).unwrap();
        }
    }
    txn.commit().unwrap(); // No fsync with Durability::None
    let elapsed = start.elapsed();

    let ops_per_sec = num_writes as f64 / elapsed.as_secs_f64();
    println!(
        "  {} writes in {:?} = {:.0} ops/sec",
        num_writes, elapsed, ops_per_sec
    );

    // Test 2: Two CFs concurrently - write only, no sync
    println!("\nTest 2: Two CFs Concurrent - Write Performance (no fsync)");
    let tmpfile2 = NamedTempFile::new().unwrap();
    let db2 = Arc::new(ColumnFamilyDatabase::open(tmpfile2.path()).unwrap());
    db2.create_column_family("cf1", Some(10 * 1024 * 1024))
        .unwrap();
    db2.create_column_family("cf2", Some(10 * 1024 * 1024))
        .unwrap();

    let start = Instant::now();
    let mut handles = vec![];

    for cf_name in ["cf1", "cf2"] {
        let db_clone = db2.clone();
        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family(cf_name).unwrap();
            let data = vec![0u8; 1024];
            let thread_start = Instant::now();

            let mut txn = cf.begin_write().unwrap();
            txn.set_durability(Durability::None).unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                for i in 0..num_writes {
                    table.insert(&i, data.as_slice()).unwrap();
                }
            }
            txn.commit().unwrap(); // No fsync

            thread_start.elapsed()
        });
        handles.push(handle);
    }

    let mut thread_times = vec![];
    for handle in handles {
        thread_times.push(handle.join().unwrap());
    }
    let wall_time = start.elapsed();

    let total_ops = num_writes * 2;
    let wall_ops_per_sec = total_ops as f64 / wall_time.as_secs_f64();
    let avg_thread_time: Duration = thread_times.iter().sum::<Duration>() / 2;
    let parallelism_efficiency = avg_thread_time.as_secs_f64() / wall_time.as_secs_f64();

    println!("  Wall time: {:?}", wall_time);
    println!("  Avg thread time: {:?}", avg_thread_time);
    println!(
        "  Parallelism efficiency: {:.1}%",
        parallelism_efficiency * 100.0
    );
    println!("  Throughput: {:.0} ops/sec", wall_ops_per_sec);
    println!(
        "  Speedup vs single CF: {:.2}x",
        wall_ops_per_sec / ops_per_sec
    );

    // Test 3: Four CFs concurrently
    println!("\nTest 3: Four CFs Concurrent - Write Performance (no fsync)");
    let tmpfile3 = NamedTempFile::new().unwrap();
    let db3 = Arc::new(ColumnFamilyDatabase::open(tmpfile3.path()).unwrap());
    for i in 0..4 {
        db3.create_column_family(&format!("cf{}", i), Some(10 * 1024 * 1024))
            .unwrap();
    }

    let start = Instant::now();
    let mut handles = vec![];

    for i in 0..4 {
        let db_clone = db3.clone();
        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family(&format!("cf{}", i)).unwrap();
            let data = vec![0u8; 1024];

            let mut txn = cf.begin_write().unwrap();
            txn.set_durability(Durability::None).unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                for j in 0..num_writes {
                    table.insert(&j, data.as_slice()).unwrap();
                }
            }
            txn.commit().unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
    let wall_time = start.elapsed();

    let total_ops = num_writes * 4;
    let wall_ops_per_sec = total_ops as f64 / wall_time.as_secs_f64();

    println!("  Wall time: {:?}", wall_time);
    println!("  Throughput: {:.0} ops/sec", wall_ops_per_sec);
    println!(
        "  Speedup vs single CF: {:.2}x (should be ~4x if fully parallel)",
        wall_ops_per_sec / ops_per_sec
    );

    // Test 4: Measure write lock acquisition overhead
    println!("\nTest 4: Write Lock Acquisition Overhead");
    let tmpfile4 = NamedTempFile::new().unwrap();
    let db4 = Arc::new(ColumnFamilyDatabase::open(tmpfile4.path()).unwrap());
    db4.create_column_family("lock_test", Some(10 * 1024 * 1024))
        .unwrap();

    let cf = db4.column_family("lock_test").unwrap();
    let iterations = 1000;

    let start = Instant::now();
    for _ in 0..iterations {
        let txn = cf.begin_write().unwrap();
        txn.abort().unwrap();
    }
    let elapsed = start.elapsed();

    println!(
        "  {} lock acquisitions in {:?} = {:.2}µs per lock",
        iterations,
        elapsed,
        elapsed.as_micros() as f64 / iterations as f64
    );

    // Test 5: Concurrent lock acquisition on different CFs
    println!("\nTest 5: Concurrent Lock Acquisition (Different CFs)");
    let tmpfile5 = NamedTempFile::new().unwrap();
    let db5 = Arc::new(ColumnFamilyDatabase::open(tmpfile5.path()).unwrap());
    db5.create_column_family("lock1", Some(10 * 1024 * 1024))
        .unwrap();
    db5.create_column_family("lock2", Some(10 * 1024 * 1024))
        .unwrap();

    let ready = Arc::new(AtomicBool::new(false));
    let start_barrier = ready.clone();

    let mut handles = vec![];
    for cf_name in ["lock1", "lock2"] {
        let db_clone = db5.clone();
        let ready_clone = ready.clone();
        let handle = std::thread::spawn(move || {
            while !ready_clone.load(Ordering::Relaxed) {
                std::hint::spin_loop();
            }

            let cf = db_clone.column_family(cf_name).unwrap();
            let thread_start = Instant::now();
            for _ in 0..iterations {
                let txn = cf.begin_write().unwrap();
                txn.abort().unwrap();
            }
            thread_start.elapsed()
        });
        handles.push(handle);
    }

    // Start all threads simultaneously
    std::thread::sleep(Duration::from_millis(10));
    start_barrier.store(true, Ordering::Release);

    let mut thread_times = vec![];
    for handle in handles {
        thread_times.push(handle.join().unwrap());
    }

    let max_time = thread_times.iter().max().unwrap();
    let avg_time: Duration = thread_times.iter().sum::<Duration>() / 2;

    println!("  Max thread time: {:?}", max_time);
    println!("  Avg thread time: {:?}", avg_time);
    println!(
        "  Per-lock avg: {:.2}µs",
        avg_time.as_micros() as f64 / iterations as f64
    );

    println!("\n=== Analysis ===");
    println!("If write performance is slow even without fsync, the bottleneck is elsewhere.");
    println!("If concurrent CFs don't speed up, there's lock contention we haven't found.");
    println!("Look for speedup close to N-way for N CFs to confirm true parallelism.\n");
}
