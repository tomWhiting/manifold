//! Fair comparison: What's the best each system can do?

use manifold::{Database, TableDefinition};
use manifold::column_family::ColumnFamilyDatabase;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("FAIR COMPARISON - Each System's Best Case");
    println!("{}\n", "=".repeat(80));

    let data = vec![0u8; 1024];

    // Test 1: Original redb - BEST CASE (single-threaded, large batches)
    println!("[1/3] Original redb Database - Single-threaded, large batches");
    println!("      (this is how redb is designed to be used)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Database::create(tmpfile.path()).unwrap();
    
    let num_batches = 100;
    let ops_per_batch = 1000;
    let total_ops = num_batches * ops_per_batch;
    
    let start = Instant::now();
    for batch in 0..num_batches {
        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..ops_per_batch {
                let key = (batch * ops_per_batch + i) as u64;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }
    let duration = start.elapsed();
    let redb_best = total_ops as f64 / duration.as_secs_f64();
    println!("      {} ops in {:.2}s = {:.0} ops/sec\n", total_ops, duration.as_secs_f64(), redb_best);

    // Test 2: ColumnFamily without WAL - BEST CASE (concurrent threads, different CFs)
    println!("[2/3] ColumnFamilyDatabase NO WAL - Concurrent threads with batches");
    println!("      (exploits parallelism across column families)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::builder().pool_size(0).open(tmpfile.path()).unwrap());

    let num_threads = 8;
    for i in 0..num_threads {
        db.create_column_family(&format!("cf_{}", i), Some(10 * 1024 * 1024)).unwrap();
    }

    let batches_per_thread = 100;
    let ops_per_batch = 100;
    let total_ops = num_threads * batches_per_thread * ops_per_batch;

    let start = Instant::now();
    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = db.clone();
        let data_clone = data.clone();

        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family(&format!("cf_{}", thread_id)).unwrap();

            for batch in 0..batches_per_thread {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    for i in 0..ops_per_batch {
                        let key = (batch * ops_per_batch + i) as u64;
                        table.insert(&key, data_clone.as_slice()).unwrap();
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

    let duration = start.elapsed();
    let cf_no_wal_best = total_ops as f64 / duration.as_secs_f64();
    println!("      {} ops in {:.2}s = {:.0} ops/sec\n", total_ops, duration.as_secs_f64(), cf_no_wal_best);

    // Test 3: ColumnFamily WITH WAL - BEST CASE (concurrent + batched)
    println!("[3/3] ColumnFamilyDatabase WITH WAL - Concurrent threads with batches");
    println!("      (parallelism + group commit)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    for i in 0..num_threads {
        db.create_column_family(&format!("cf_{}", i), Some(10 * 1024 * 1024)).unwrap();
    }

    let start = Instant::now();
    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = db.clone();
        let data_clone = data.clone();

        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family(&format!("cf_{}", thread_id)).unwrap();

            for batch in 0..batches_per_thread {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    for i in 0..ops_per_batch {
                        let key = (batch * ops_per_batch + i) as u64;
                        table.insert(&key, data_clone.as_slice()).unwrap();
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

    let duration = start.elapsed();
    let cf_wal_best = total_ops as f64 / duration.as_secs_f64();
    println!("      {} ops in {:.2}s = {:.0} ops/sec\n", total_ops, duration.as_secs_f64(), cf_wal_best);

    // Summary
    println!("{}", "=".repeat(80));
    println!("RESULTS (Best Case for Each):");
    println!("  Original redb (single-threaded):    {:8.0} ops/sec", redb_best);
    println!("  ColumnFamily without WAL (8 thread): {:8.0} ops/sec  ({:.1}x)", 
             cf_no_wal_best, cf_no_wal_best / redb_best);
    println!("  ColumnFamily with WAL (8 thread):   {:8.0} ops/sec  ({:.1}x)",
             cf_wal_best, cf_wal_best / redb_best);
    println!("\n{}", "=".repeat(80));
    println!("Analysis:");
    println!("  - Original redb: Optimized for single-threaded, large batches");
    println!("  - ColumnFamily: Enables parallel writes to different partitions");
    println!("  - WAL: Makes durable commits fast via group commit batching");
    
    if cf_wal_best > redb_best {
        println!("\n  ✅ For concurrent workloads, our system is {:.1}x faster!", 
                 cf_wal_best / redb_best);
    } else if cf_wal_best < redb_best * 0.8 {
        println!("\n  ⚠️  Our system is slower - may need optimization");
    } else {
        println!("\n  ≈  Performance is comparable");
    }
    println!("{}", "=".repeat(80));
}
