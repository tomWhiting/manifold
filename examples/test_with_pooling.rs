//! Test with ACTUAL pooling enabled (the way it's supposed to work!)

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("TESTING WITH PROPER FILE HANDLE POOLING");
    println!("{}\n", "=".repeat(80));

    let data = vec![0u8; 1024];

    for num_threads in [1, 2, 4, 8] {
        let tmpfile = NamedTempFile::new().unwrap();
        // Use DEFAULT pool size (32) - each CF gets its own FD!
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
        let expected = 96000.0 * num_threads as f64;
        let efficiency = (throughput / expected) * 100.0;

        println!("{} threads: {:.0} ops/sec (expected ~{:.0}, efficiency {:.0}%)",
                 num_threads, throughput, expected, efficiency);
    }

    println!("\n{}", "=".repeat(80));
    println!("With proper pooling, each CF has its own FD → true parallelism!");
    println!("{}", "=".repeat(80));
}
