//! Measure how often segment expansion happens

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("MEASURING SEGMENT EXPANSION FREQUENCY");
    println!("{}\n", "=".repeat(80));

    let data = vec![0u8; 1024];

    // Test with different initial segment sizes
    for initial_size_mb in [1, 10, 100] {
        println!("Testing with {}MB initial segment size:", initial_size_mb);
        
        let tmpfile = NamedTempFile::new().unwrap();
        let db = Arc::new(ColumnFamilyDatabase::builder()
            .pool_size(0)
            .open(tmpfile.path())
            .unwrap());

        // Create 4 CFs
        for i in 0..4 {
            db.create_column_family(
                &format!("cf_{}", i), 
                Some(initial_size_mb * 1024 * 1024)
            ).unwrap();
        }

        let start = Instant::now();
        let mut handles = vec![];

        for thread_id in 0..4 {
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
        let total_ops = 4 * 100 * 1000;
        let throughput = total_ops as f64 / duration.as_secs_f64();

        println!("  Time: {:.2}s", duration.as_secs_f64());
        println!("  Throughput: {:.0} ops/sec", throughput);
        println!("  Total data written: ~{}MB per CF\n", (100 * 1000 * 1024) / (1024 * 1024));
    }

    println!("{}", "=".repeat(80));
    println!("If larger initial segments give much better performance,");
    println!("it means expansion is the bottleneck.");
    println!("{}", "=".repeat(80));
}
