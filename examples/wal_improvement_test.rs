//! Measure the actual improvement WAL provides over default durability

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("WAL IMPROVEMENT MEASUREMENT");
    println!("{}\n", "=".repeat(80));

    let data = vec![0u8; 1024];
    let txns = 100;
    let ops_per_txn = 100;
    let total_ops = txns * ops_per_txn;

    // Test 1: WITHOUT WAL - Default Durability (Immediate) - fsyncs B-tree every commit
    println!("[1/2] WITHOUT WAL - Default Durability (fsyncs B-tree on every commit)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::builder()
        .pool_size(0)  // Disable WAL
        .open(tmpfile.path())
        .unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024)).unwrap();
    let cf = db.column_family("test").unwrap();
    
    let start = Instant::now();
    for txn_id in 0..txns {
        let txn = cf.begin_write().unwrap();
        // Default durability = Immediate (fsyncs B-tree)
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..ops_per_txn {
                let key = (txn_id * ops_per_txn + i) as u64;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }
    let without_wal = start.elapsed();
    let without_wal_ops_sec = total_ops as f64 / without_wal.as_secs_f64();
    println!("  {} txns × {} ops = {} total", txns, ops_per_txn, total_ops);
    println!("  Time: {:?}", without_wal);
    println!("  Throughput: {:.0} ops/sec\n", without_wal_ops_sec);

    // Test 2: WITH WAL - Default Durability (Immediate) - fsyncs WAL only
    println!("[2/2] WITH WAL - Default Durability (fsyncs WAL only, not B-tree)");
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap(); // WAL enabled by default
    db.create_column_family("test", Some(10 * 1024 * 1024)).unwrap();
    let cf = db.column_family("test").unwrap();
    
    let start = Instant::now();
    for txn_id in 0..txns {
        let txn = cf.begin_write().unwrap();
        // Default durability = Immediate (but WAL intercepts it)
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..ops_per_txn {
                let key = (txn_id * ops_per_txn + i) as u64;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }
    let with_wal = start.elapsed();
    let with_wal_ops_sec = total_ops as f64 / with_wal.as_secs_f64();
    println!("  {} txns × {} ops = {} total", txns, ops_per_txn, total_ops);
    println!("  Time: {:?}", with_wal);
    println!("  Throughput: {:.0} ops/sec\n", with_wal_ops_sec);

    // Calculate improvement
    let improvement = with_wal_ops_sec / without_wal_ops_sec;
    let speedup = without_wal.as_secs_f64() / with_wal.as_secs_f64();

    println!("{}", "=".repeat(80));
    println!("RESULTS:");
    println!("  Without WAL: {:.0} ops/sec", without_wal_ops_sec);
    println!("  With WAL:    {:.0} ops/sec", with_wal_ops_sec);
    println!("  Improvement: {:.1}x faster", improvement);
    println!("  Time saved:  {:.1}x speedup", speedup);
    println!("{}", "=".repeat(80));
}
