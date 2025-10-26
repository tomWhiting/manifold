//! Find the bottleneck preventing parallel scaling

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("ISOLATING THE BOTTLENECK");
    println!("{}\n", "=".repeat(80));

    let data = vec![0u8; 1024];

    for num_threads in [1, 2, 4, 8] {
        // Test WITHOUT WAL - should scale linearly if no shared locks
        let tmpfile = NamedTempFile::new().unwrap();
        let db = Arc::new(ColumnFamilyDatabase::builder()
            .pool_size(0)
            .open(tmpfile.path())
            .unwrap());

        for i in 0..num_threads {
            db.create_column_family(&format!("cf_{}", i), Some(10 * 1024 * 1024)).unwrap();
        }

        let batches = 100;
        let ops = 1000;

        let start = Instant::now();
        let mut handles = vec![];

        for thread_id in 0..num_threads {
            let db_clone = db.clone();
            let data_clone = data.clone();

            let handle = std::thread::spawn(move || {
                let cf = db_clone.column_family(&format!("cf_{}", thread_id)).unwrap();

                for batch in 0..batches {
                    let txn = cf.begin_write().unwrap();
                    {
                        let mut table = txn.open_table(TEST_TABLE).unwrap();
                        for i in 0..ops {
                            let key = (batch * ops + i) as u64;
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
        let total_ops = num_threads * batches * ops;
        let throughput = total_ops as f64 / duration.as_secs_f64();
        let expected = 82000.0 * num_threads as f64; // Based on single-thread perf
        let efficiency = (throughput / expected) * 100.0;

        println!("{} threads: {:.0} ops/sec (expected ~{:.0}, efficiency {:.0}%)",
                 num_threads, throughput, expected, efficiency);
    }

    println!("\n{}", "=".repeat(80));
    println!("If efficiency drops significantly, there's a shared lock/resource.");
    println!("{}", "=".repeat(80));
}
