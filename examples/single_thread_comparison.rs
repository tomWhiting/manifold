//! Direct comparison: single thread, same workload

use manifold::{Database, TableDefinition};
use manifold::column_family::ColumnFamilyDatabase;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("SINGLE-THREADED PERFORMANCE COMPARISON");
    println!("{}\n", "=".repeat(80));

    let data = vec![0u8; 1024];
    let num_batches = 100;
    let ops_per_batch = 1000;
    let total_ops = num_batches * ops_per_batch;

    // Test 1: Original redb
    println!("[1/2] Original redb Database");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Database::create(tmpfile.path()).unwrap();
    
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
    let redb_time = start.elapsed();
    let redb_throughput = total_ops as f64 / redb_time.as_secs_f64();
    
    println!("  Time: {:.3}s", redb_time.as_secs_f64());
    println!("  Throughput: {:.0} ops/sec", redb_throughput);
    println!("  Per batch: {:.3}ms\n", redb_time.as_secs_f64() * 1000.0 / num_batches as f64);

    // Test 2: ColumnFamily (no WAL to isolate overhead)
    println!("[2/2] ColumnFamilyDatabase (pool_size=0, no WAL)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::builder()
        .pool_size(0)
        .open(tmpfile.path())
        .unwrap();
    db.create_column_family("test", Some(100 * 1024 * 1024)).unwrap();
    let cf = db.column_family("test").unwrap();
    
    let start = Instant::now();
    for batch in 0..num_batches {
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..ops_per_batch {
                let key = (batch * ops_per_batch + i) as u64;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }
    let cf_time = start.elapsed();
    let cf_throughput = total_ops as f64 / cf_time.as_secs_f64();
    
    println!("  Time: {:.3}s", cf_time.as_secs_f64());
    println!("  Throughput: {:.0} ops/sec", cf_throughput);
    println!("  Per batch: {:.3}ms\n", cf_time.as_secs_f64() * 1000.0 / num_batches as f64);

    // Analysis
    println!("{}", "=".repeat(80));
    let overhead = (cf_time.as_secs_f64() - redb_time.as_secs_f64()) / redb_time.as_secs_f64() * 100.0;
    let slowdown = redb_throughput / cf_throughput;
    
    println!("ANALYSIS:");
    println!("  Original redb:        {:.0} ops/sec", redb_throughput);
    println!("  ColumnFamily:         {:.0} ops/sec", cf_throughput);
    println!("  Slowdown:             {:.2}x", slowdown);
    println!("  Overhead:             {:.1}%", overhead);
    println!("\nThis overhead is from ColumnFamily wrapper layers:");
    println!("  - PartitionedStorageBackend (segment management)");
    println!("  - FileHandlePool (even with pool_size=0)");
    println!("  - ColumnFamilyState (lazy initialization)");
    println!("  - Extra indirection layers");
    println!("{}", "=".repeat(80));
}
