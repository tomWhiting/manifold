//! WAL performance with concurrent threads doing BATCHED operations

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("WAL CONCURRENT BATCHED OPERATIONS");
    println!("{}\n", "=".repeat(80));

    for num_threads in [1, 2, 4, 8, 16] {
        let tmpfile = NamedTempFile::new().unwrap();
        let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

        // Create separate CF for each thread
        for thread_id in 0..num_threads {
            db.create_column_family(&format!("cf_{}", thread_id), Some(10 * 1024 * 1024))
                .unwrap();
        }

        let txns_per_thread = 10;
        let ops_per_txn = 1000;
        let data = vec![0u8; 1024];

        let start = Instant::now();
        let mut handles = vec![];

        for thread_id in 0..num_threads {
            let db_clone = db.clone();
            let data_clone = data.clone();

            let handle = std::thread::spawn(move || {
                let cf = db_clone.column_family(&format!("cf_{}", thread_id)).unwrap();

                for batch in 0..txns_per_thread {
                    let txn = cf.begin_write().unwrap();
                    {
                        let mut table = txn.open_table(TEST_TABLE).unwrap();
                        for i in 0..ops_per_txn {
                            let key = (batch * ops_per_txn + i) as u64;
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
        let total_txns = num_threads * txns_per_thread;
        let total_ops = total_txns * ops_per_txn;
        
        println!(
            "{:2} threads × {} txns × {} ops = {:6} total | {:.2}s | {:.0} ops/sec",
            num_threads,
            txns_per_thread,
            ops_per_txn,
            total_ops,
            duration.as_secs_f64(),
            total_ops as f64 / duration.as_secs_f64()
        );
    }

    println!("\n{}", "=".repeat(80));
    println!("With batched operations, WAL enables high-throughput durable writes!");
    println!("{}", "=".repeat(80));
}
