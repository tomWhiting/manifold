//! Compare original redb vs our ColumnFamily implementation

use manifold::{Database, Durability, TableDefinition};
use manifold::column_family::ColumnFamilyDatabase;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("ORIGINAL REDB vs COLUMN FAMILY DATABASE COMPARISON");
    println!("{}\n", "=".repeat(80));

    let data = vec![0u8; 1024];
    let num_threads = 8;
    let txns_per_thread = 20;
    let ops_per_txn = 50;
    let total_ops = num_threads * txns_per_thread * ops_per_txn;

    // Test 1: Original redb Database - concurrent threads, same database
    println!("[1/3] Original redb Database - {} threads writing to SAME database", num_threads);
    println!("      (redb serializes write transactions - only 1 can run at a time)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(Database::create(tmpfile.path()).unwrap());
    
    let start = Instant::now();
    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = db.clone();
        let data_clone = data.clone();

        let handle = std::thread::spawn(move || {
            for txn_id in 0..txns_per_thread {
                let txn = db_clone.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    for i in 0..ops_per_txn {
                        let key = (thread_id * txns_per_thread * ops_per_txn + txn_id * ops_per_txn + i) as u64;
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
    let redb_ops_sec = total_ops as f64 / duration.as_secs_f64();
    println!("      Time: {:.2}s", duration.as_secs_f64());
    println!("      Throughput: {:.0} ops/sec\n", redb_ops_sec);

    // Test 2: ColumnFamilyDatabase WITHOUT WAL - concurrent threads, separate CFs
    println!("[2/3] ColumnFamilyDatabase NO WAL - {} threads, each with own CF", num_threads);
    println!("      (allows parallel writes to different CFs)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::builder().pool_size(0).open(tmpfile.path()).unwrap());

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

            for txn_id in 0..txns_per_thread {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    for i in 0..ops_per_txn {
                        let key = (txn_id * ops_per_txn + i) as u64;
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
    let cf_no_wal_ops_sec = total_ops as f64 / duration.as_secs_f64();
    println!("      Time: {:.2}s", duration.as_secs_f64());
    println!("      Throughput: {:.0} ops/sec\n", cf_no_wal_ops_sec);

    // Test 3: ColumnFamilyDatabase WITH WAL - concurrent threads, separate CFs
    println!("[3/3] ColumnFamilyDatabase WITH WAL - {} threads, each with own CF", num_threads);
    println!("      (parallel writes + group commit)");
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

            for txn_id in 0..txns_per_thread {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    for i in 0..ops_per_txn {
                        let key = (txn_id * ops_per_txn + i) as u64;
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
    let cf_wal_ops_sec = total_ops as f64 / duration.as_secs_f64();
    println!("      Time: {:.2}s", duration.as_secs_f64());
    println!("      Throughput: {:.0} ops/sec\n", cf_wal_ops_sec);

    // Summary
    println!("{}", "=".repeat(80));
    println!("SUMMARY:");
    println!("  Original redb (serialized):      {:8.0} ops/sec", redb_ops_sec);
    println!("  ColumnFamily without WAL:        {:8.0} ops/sec  ({:.1}x vs redb)", 
             cf_no_wal_ops_sec, cf_no_wal_ops_sec / redb_ops_sec);
    println!("  ColumnFamily with WAL:           {:8.0} ops/sec  ({:.1}x vs redb)",
             cf_wal_ops_sec, cf_wal_ops_sec / redb_ops_sec);
    println!("\nConclusion:");
    if cf_wal_ops_sec > redb_ops_sec {
        println!("  ✅ Our implementation is {:.1}x FASTER than original redb!", 
                 cf_wal_ops_sec / redb_ops_sec);
        println!("  Column families enable parallel writes that redb cannot do.");
    } else {
        println!("  ❌ Our implementation is slower than original redb.");
        println!("  Investigation needed.");
    }
    println!("{}", "=".repeat(80));
}
