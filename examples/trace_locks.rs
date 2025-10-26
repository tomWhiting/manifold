//! Trace lock contention by timing different phases

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n================================================================================");
    println!("If 8 threads take 8x longer than 1 thread (same total work),");
    println!("they're running serially, not in parallel.");
    println!("================================================================================\n");

    let data = vec![0u8; 1024];

    // Single thread baseline
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::builder()
        .pool_size(0)
        .open(tmpfile.path())
        .unwrap());
    db.create_column_family("cf_0", Some(10 * 1024 * 1024)).unwrap();
    
    let start = Instant::now();
    let cf = db.column_family("cf_0").unwrap();
    for batch in 0..100 {
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..1000 {
                table.insert(&((batch * 1000 + i) as u64), data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }
    let single_thread_time = start.elapsed();
    println!("1 thread (100 batches): {:.2}s\n", single_thread_time.as_secs_f64());

    // 8 threads, each doing 100/8 = 12.5 batches (same total work as 1 thread)
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::builder()
        .pool_size(0)
        .open(tmpfile.path())
        .unwrap());
    for i in 0..8 {
        db.create_column_family(&format!("cf_{}", i), Some(10 * 1024 * 1024)).unwrap();
    }
    
    let start = Instant::now();
    let mut handles = vec![];
    
    for thread_id in 0..8 {
        let db_clone = db.clone();
        let data_clone = data.clone();

        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family(&format!("cf_{}", thread_id)).unwrap();
            for batch in 0..13 {  // ~100/8
                let txn = cf.begin_write().unwrap();
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
    let eight_thread_time = start.elapsed();
    
    println!("8 threads (13 batches each ≈ same work): {:.2}s", eight_thread_time.as_secs_f64());
    println!("Ratio: {:.2}x", eight_thread_time.as_secs_f64() / single_thread_time.as_secs_f64());
    println!("\nIf ratio ≈ 1.0x: threads running in parallel ✅");
    println!("If ratio ≈ 8.0x: threads running serially ❌");
}
