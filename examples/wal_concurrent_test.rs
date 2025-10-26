//! WAL Concurrent Performance Test
//!
//! This test demonstrates group commit benefits with concurrent writers
//! to DIFFERENT column families (CFs). Each thread writes to its own CF,
//! allowing truly parallel writes that benefit from shared WAL group commit.

use manifold::TableDefinition;
use manifold::column_family::ColumnFamilyDatabase;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("WAL GROUP COMMIT - Concurrent Writers to Different Column Families");
    println!("{}\n", "=".repeat(80));

    // Test with varying numbers of concurrent writers
    for num_threads in [1, 2, 4, 8, 16, 32, 64] {
        let tmpfile = NamedTempFile::new().unwrap();
        let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

        // Create a separate column family for each thread
        for thread_id in 0..num_threads {
            db.create_column_family(&format!("cf_{}", thread_id), Some(10 * 1024 * 1024))
                .unwrap();
        }

        let writes_per_thread = 1000;
        let data = vec![0u8; 1024];

        let start = Instant::now();
        let mut handles = vec![];

        for thread_id in 0..num_threads {
            let db_clone = db.clone();
            let data_clone = data.clone();

            let handle = std::thread::spawn(move || {
                // Each thread writes to its own column family
                let cf = db_clone
                    .column_family(&format!("cf_{}", thread_id))
                    .unwrap();

                for i in 0..writes_per_thread {
                    let txn = cf.begin_write().unwrap();
                    {
                        let mut table = txn.open_table(TEST_TABLE).unwrap();
                        table.insert(&i, data_clone.as_slice()).unwrap();
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
        let total_ops = num_threads * writes_per_thread;
        let ops_per_sec = total_ops as f64 / duration.as_secs_f64();

        println!(
            "{:2} threads × {} ops = {:5} total | {:.2}s | {:.0} ops/sec",
            num_threads,
            writes_per_thread,
            total_ops,
            duration.as_secs_f64(),
            ops_per_sec
        );
    }

    println!("\n{}", "=".repeat(80));
    println!("Expected behavior:");
    println!("  - 1 thread:  ~500 ops/sec (limited by 2ms group commit interval)");
    println!("  - 2+ threads: ~10K+ ops/sec (group commit batches across CFs)");
    println!("\nNote: Each thread writes to its own column family, allowing");
    println!("      parallel writes that share a single WAL with group commit.");
    println!("      More threads = more transactions batched per 2ms fsync cycle.");
    println!("{}\n", "=".repeat(80));
}
