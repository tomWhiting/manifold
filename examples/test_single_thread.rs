use manifold::column_family::ColumnFamilyDatabase;
use manifold::{Durability, TableDefinition};
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("test");

fn main() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();

    println!("Creating 1 column family with 100MB...");
    db.create_column_family("cf_0", Some(100 * 1024 * 1024)).unwrap();
    
    println!("Starting single-threaded writes with Durability::None...");
    let cf = db.column_family("cf_0").unwrap();
    let data = vec![0u8; 1024];
    
    let start = Instant::now();
    for j in 0..4000 {
        let mut txn = cf.begin_write().unwrap();
        txn.set_durability(Durability::None).unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&(j as u64), data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }
    let duration = start.elapsed();
    
    println!("\n4000 writes in {:?}", duration);
    println!("Throughput: {:.0} ops/sec", 4000.0 / duration.as_secs_f64());
}
