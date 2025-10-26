//! Profile where time is being spent in concurrent writes

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

static TOTAL_COMMIT_TIME: AtomicU64 = AtomicU64::new(0);
static TOTAL_COMMITS: AtomicU64 = AtomicU64::new(0);

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("PROFILING CONCURRENT WRITE BOTTLENECK");
    println!("{}\n", "=".repeat(80));

    let data = vec![0u8; 1024];
    
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::builder()
        .pool_size(0)
        .open(tmpfile.path())
        .unwrap());

    for i in 0..8 {
        db.create_column_family(&format!("cf_{}", i), Some(10 * 1024 * 1024)).unwrap();
    }

    let overall_start = Instant::now();
    let mut handles = vec![];

    for thread_id in 0..8 {
        let db_clone = db.clone();
        let data_clone = data.clone();

        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family(&format!("cf_{}", thread_id)).unwrap();

            for batch in 0..50 {
                let txn_start = Instant::now();
                
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    for i in 0..1000 {
                        let key = (batch * 1000 + i) as u64;
                        table.insert(&key, data_clone.as_slice()).unwrap();
                    }
                }
                
                let commit_start = Instant::now();
                txn.commit().unwrap();
                let commit_time = commit_start.elapsed();
                
                TOTAL_COMMIT_TIME.fetch_add(commit_time.as_micros() as u64, Ordering::Relaxed);
                TOTAL_COMMITS.fetch_add(1, Ordering::Relaxed);
                
                let total_time = txn_start.elapsed();
                
                // Print slow transactions
                if total_time.as_millis() > 100 {
                    println!("Thread {} batch {}: total {}ms (commit {}ms)", 
                             thread_id, batch, total_time.as_millis(), commit_time.as_millis());
                }
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let overall_time = overall_start.elapsed();
    let total_commits = TOTAL_COMMITS.load(Ordering::Relaxed);
    let total_commit_micros = TOTAL_COMMIT_TIME.load(Ordering::Relaxed);
    let avg_commit_ms = (total_commit_micros as f64 / total_commits as f64) / 1000.0;

    println!("\n{}", "=".repeat(80));
    println!("RESULTS:");
    println!("  Total time: {:.2}s", overall_time.as_secs_f64());
    println!("  Total commits: {}", total_commits);
    println!("  Average commit time: {:.2}ms", avg_commit_ms);
    println!("  Time in commits: {:.2}s ({:.0}% of total)",
             total_commit_micros as f64 / 1_000_000.0,
             (total_commit_micros as f64 / 1_000_000.0) / overall_time.as_secs_f64() * 100.0);
    println!("{}", "=".repeat(80));
}
