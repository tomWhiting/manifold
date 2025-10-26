//! Test original redb with 8 separate database files

use manifold::{Database, TableDefinition};
use std::time::Instant;
use tempfile::TempDir;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n================================================================================");
    println!("Original redb with 8 separate database files (true parallelism)");
    println!("================================================================================\n");

    let data = vec![0u8; 1024];
    let tmpdir = TempDir::new().unwrap();

    let start = Instant::now();
    let mut handles = vec![];

    for thread_id in 0..8 {
        let tmpdir_path = tmpdir.path().to_path_buf();
        let data_clone = data.clone();

        let handle = std::thread::spawn(move || {
            let db_path = tmpdir_path.join(format!("db_{}.redb", thread_id));
            let db = Database::create(&db_path).unwrap();

            for batch in 0..100 {
                let txn = db.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    for i in 0..1000 {
                        table.insert(&((batch * 1000 + i) as u64), data_clone.as_slice()).unwrap();
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
    let total_ops = 8 * 100 * 1000;
    let throughput = total_ops as f64 / duration.as_secs_f64();

    println!("8 threads × 100 batches × 1000 ops = {} total", total_ops);
    println!("Time: {:.2}s", duration.as_secs_f64());
    println!("Throughput: {:.0} ops/sec", throughput);
    println!("\nThis is the MAXIMUM we can expect with perfect parallelism.");
}
