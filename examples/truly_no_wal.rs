//! Force WAL to be COMPLETELY disabled

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("WAL TRULY DISABLED (pool_size = 0)");
    println!("{}\n", "=".repeat(80));

    let data = vec![0u8; 1024];

    for num_threads in [1, 2, 4, 8] {
        let tmpfile = NamedTempFile::new().unwrap();
        let db = Arc::new(ColumnFamilyDatabase::builder()
            .pool_size(0)  // This disables WAL
            .open(tmpfile.path())
            .unwrap());

        for i in 0..num_threads {
            db.create_column_family(&format!("cf_{}", i), Some(100 * 1024 * 1024)).unwrap();
        }

        let start = Instant::now();
        let mut handles = vec![];

        for thread_id in 0..num_threads {
            let db_clone = db.clone();
            let data_clone = data.clone();

            let handle = std::thread::spawn(move || {
                let cf = db_clone.column_family(&format!("cf_{}", thread_id)).unwrap();

                for batch in 0..100 {
                    let txn = cf.begin_write().unwrap();
                    {
                        let mut table = txn.open_table(TEST_TABLE).unwrap();
                        for i in 0..1000 {
                            let key = (batch * 1000 + i) as u64;
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
        let total_ops = num_threads * 100 * 1000;
        let throughput = total_ops as f64 / duration.as_secs_f64();

        println!("{} threads: {:.0} ops/sec ({:.2}s)", 
                 num_threads, throughput, duration.as_secs_f64());
    }

    println!("\n{}", "=".repeat(80));
    println!("pool_size=0 means NO pool, NO WAL - just column families");
    println!("{}", "=".repeat(80));
}
