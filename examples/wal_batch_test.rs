//! Test WAL performance with BATCHED operations (the redb way)

use manifold::column_family::ColumnFamilyDatabase;
use manifold::{Durability, TableDefinition};
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("WAL PERFORMANCE WITH BATCHED OPERATIONS");
    println!("{}\n", "=".repeat(80));

    let data = vec![0u8; 1024];

    // Test 1: ColumnFamily WITHOUT WAL (pool_size = 0)
    println!("[1/3] ColumnFamily NO WAL - Batched (1000 ops/txn), Durability::None");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::builder()
        .pool_size(0)  // Disable WAL
        .open(tmpfile.path())
        .unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024)).unwrap();
    let cf = db.column_family("test").unwrap();
    
    let start = Instant::now();
    for batch in 0..10 {
        let mut txn = cf.begin_write().unwrap();
        txn.set_durability(Durability::None).unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..1000u64 {
                let key = batch * 1000 + i;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }
    let duration = start.elapsed();
    println!("  10 txns × 1000 ops = 10000 total: {:?}", duration);
    println!("  Throughput: {:.0} ops/sec\n", 10000.0 / duration.as_secs_f64());

    // Test 2: ColumnFamily WITH WAL - Batched
    println!("[2/3] ColumnFamily WITH WAL - Batched (1000 ops/txn), Default Durability");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap(); // WAL enabled by default
    db.create_column_family("test", Some(10 * 1024 * 1024)).unwrap();
    let cf = db.column_family("test").unwrap();
    
    let start = Instant::now();
    for batch in 0..10 {
        let txn = cf.begin_write().unwrap();
        // Default durability - should use WAL
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..1000u64 {
                let key = batch * 1000 + i;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }
    let duration = start.elapsed();
    println!("  10 txns × 1000 ops = 10000 total: {:?}", duration);
    println!("  Throughput: {:.0} ops/sec\n", 10000.0 / duration.as_secs_f64());

    // Test 3: Original redb for comparison
    println!("[3/3] Original redb Database - Batched (1000 ops/txn), Durability::None");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = manifold::Database::create(tmpfile.path()).unwrap();
    
    let start = Instant::now();
    for batch in 0..10 {
        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..1000u64 {
                let key = batch * 1000 + i;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }
    let duration = start.elapsed();
    println!("  10 txns × 1000 ops = 10000 total: {:?}", duration);
    println!("  Throughput: {:.0} ops/sec\n", 10000.0 / duration.as_secs_f64());

    println!("{}", "=".repeat(80));
}
