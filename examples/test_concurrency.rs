use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("test");

fn main() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    // Create CFs with HUGE initial allocations to avoid any expansion
    println!("Creating 4 column families with 100MB each...");
    for i in 0..4 {
        db.create_column_family(&format!("cf_{i}"), Some(100 * 1024 * 1024)).unwrap();
    }
    
    println!("Starting concurrent writes (no expansion should occur)...");
    let start = Instant::now();
    let mut handles = vec![];
    
    for i in 0..4 {
        let db_clone = db.clone();
        let handle = std::thread::spawn(move || {
            let cf = db_clone.column_family(&format!("cf_{i}")).unwrap();
            let data = vec![i as u8; 1024];
            
            for j in 0..1000 {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    table.insert(&((i * 1000 + j) as u64), data.as_slice()).unwrap();
                }
                txn.commit().unwrap();
            }
        });
        handles.push(handle);
    }
    
    for h in handles {
        h.join().unwrap();
    }
    
    let duration = start.elapsed();
    println!("\n4 CFs × 1000 writes = 4000 total writes in {:?}", duration);
    println!("Throughput: {:.0} ops/sec", 4000.0 / duration.as_secs_f64());
    println!("Per-thread throughput: {:.0} ops/sec", 1000.0 / duration.as_secs_f64());
}
