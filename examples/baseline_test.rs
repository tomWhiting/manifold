//! Baseline performance test - no WAL, just raw redb

use manifold::column_family::ColumnFamilyDatabase;
use manifold::{Durability, TableDefinition};
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("BASELINE PERFORMANCE TEST - Raw redb (no WAL optimizations)");
    println!("{}\n", "=".repeat(80));

    // Test 1: Default durability (Immediate - fsyncs every commit)
    println!("[1/3] Default Durability (Immediate) - fsyncs on every commit");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024)).unwrap();
    let cf = db.column_family("test").unwrap();
    
    let data = vec![0u8; 1024];
    let start = Instant::now();
    
    for i in 0..100u64 {
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }
    
    let duration = start.elapsed();
    println!("  100 transactions: {:?}", duration);
    println!("  Throughput: {:.0} ops/sec\n", 100.0 / duration.as_secs_f64());

    // Test 2: Durability::None (no fsyncs)
    println!("[2/3] Durability::None - no fsyncs");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024)).unwrap();
    let cf = db.column_family("test").unwrap();
    
    let start = Instant::now();
    
    for i in 0..5000u64 {
        let mut txn = cf.begin_write().unwrap();
        txn.set_durability(Durability::None).unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }
    
    let duration = start.elapsed();
    println!("  5000 transactions: {:?}", duration);
    println!("  Throughput: {:.0} ops/sec\n", 5000.0 / duration.as_secs_f64());

    // Test 3: Current WAL implementation
    println!("[3/3] Current WAL Implementation (pool_size > 0, so WAL enabled)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024)).unwrap();
    let cf = db.column_family("test").unwrap();
    
    let start = Instant::now();
    
    for i in 0..100u64 {
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }
    
    let duration = start.elapsed();
    println!("  100 transactions: {:?}", duration);
    println!("  Throughput: {:.0} ops/sec\n", 100.0 / duration.as_secs_f64());

    println!("{}", "=".repeat(80));
}
