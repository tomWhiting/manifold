//! Compare WAL vs no-WAL with concurrent writes to different column families

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn test_scenario(name: &str, with_wal: bool, num_threads: usize) -> f64 {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = if with_wal {
        Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap())
    } else {
        Arc::new(ColumnFamilyDatabase::builder().pool_size(0).open(tmpfile.path()).unwrap())
    };

    // Create separate CF for each thread
    for i in 0..num_threads {
        db.create_column_family(&format!("cf_{}", i), Some(10 * 1024 * 1024)).unwrap();
    }

    let txns_per_thread = 20;
    let ops_per_txn = 50;
    let data = vec![0u8; 1024];

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
    let total_ops = num_threads * txns_per_thread * ops_per_txn;
    let ops_per_sec = total_ops as f64 / duration.as_secs_f64();

    println!("  {} - {} threads: {:.0} ops/sec ({:.2}s for {} ops)",
             name, num_threads, ops_per_sec, duration.as_secs_f64(), total_ops);

    ops_per_sec
}

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("WAL vs NO-WAL: Concurrent Writes to Different Column Families");
    println!("{}\n", "=".repeat(80));

    for num_threads in [1, 2, 4, 8] {
        println!("\nTesting with {} threads:", num_threads);
        let without = test_scenario("NO WAL ", false, num_threads);
        let with = test_scenario("WITH WAL", true, num_threads);
        let improvement = with / without;
        println!("  → Improvement: {:.2}x", improvement);
    }

    println!("\n{}", "=".repeat(80));
}
