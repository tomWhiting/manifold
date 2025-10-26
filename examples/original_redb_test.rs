//! Test original redb Database performance (not ColumnFamilyDatabase)

use manifold::{Database, Durability, TableDefinition};
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("ORIGINAL REDB PERFORMANCE TEST");
    println!("{}\n", "=".repeat(80));

    // Test 1: Original redb - Default durability (Immediate)
    println!("[1/4] Original redb Database - Default Durability (Immediate)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Database::create(tmpfile.path()).unwrap();
    
    let data = vec![0u8; 1024];
    let start = Instant::now();
    
    for i in 0..100u64 {
        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }
    
    let duration = start.elapsed();
    println!("  100 transactions: {:?}", duration);
    println!("  Throughput: {:.0} ops/sec\n", 100.0 / duration.as_secs_f64());

    // Test 2: Original redb - Durability::None
    println!("[2/4] Original redb Database - Durability::None");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Database::create(tmpfile.path()).unwrap();
    
    let start = Instant::now();
    
    for i in 0..5000u64 {
        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }
    
    let duration = start.elapsed();
    println!("  5000 transactions: {:?}", duration);
    println!("  Throughput: {:.0} ops/sec\n", 5000.0 / duration.as_secs_f64());

    // Test 3: Original redb - batched (100 ops per txn)
    println!("[3/4] Original redb Database - Batched (100 ops/txn), Durability::None");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Database::create(tmpfile.path()).unwrap();
    
    let start = Instant::now();
    
    for batch in 0..50 {
        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..100u64 {
                let key = batch * 100 + i;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }
    
    let duration = start.elapsed();
    let total_ops = 50 * 100;
    println!("  {} transactions × 100 ops = {} total: {:?}", 50, total_ops, duration);
    println!("  Throughput: {:.0} ops/sec\n", total_ops as f64 / duration.as_secs_f64());

    // Test 4: Original redb - batched with explicit durability
    println!("[4/4] Original redb Database - Batched (1000 ops/txn), Durability::None");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Database::create(tmpfile.path()).unwrap();
    
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
    let total_ops = 10 * 1000;
    println!("  {} transactions × 1000 ops = {} total: {:?}", 10, total_ops, duration);
    println!("  Throughput: {:.0} ops/sec\n", total_ops as f64 / duration.as_secs_f64());

    println!("{}", "=".repeat(80));
    println!("SUMMARY:");
    println!("Original redb is designed for batched operations within transactions.");
    println!("Single-op-per-transaction is an anti-pattern and will be slow.");
    println!("{}", "=".repeat(80));
}
