//! Debug why we're not getting linear scaling with column families

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn test_threads(num_threads: usize, with_wal: bool) -> (f64, f64) {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = if with_wal {
        Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap())
    } else {
        Arc::new(ColumnFamilyDatabase::builder().pool_size(0).open(tmpfile.path()).unwrap())
    };

    for i in 0..num_threads {
        db.create_column_family(&format!("cf_{}", i), Some(10 * 1024 * 1024)).unwrap();
    }

    let batches_per_thread = 100;
    let ops_per_batch = 100;
    let data = vec![0u8; 1024];

    let start = Instant::now();
    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = db.clone();
        let data_clone = data.clone();

        let handle = std::thread::spawn(move || {
            let thread_start = Instant::now();
            let cf = db_clone.column_family(&format!("cf_{}", thread_id)).unwrap();

            for batch in 0..batches_per_thread {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    for i in 0..ops_per_batch {
                        let key = (batch * ops_per_batch + i) as u64;
                        table.insert(&key, data_clone.as_slice()).unwrap();
                    }
                }
                txn.commit().unwrap();
            }
            thread_start.elapsed()
        });
        handles.push(handle);
    }

    let mut max_thread_time = std::time::Duration::ZERO;
    for handle in handles {
        let thread_time = handle.join().unwrap();
        if thread_time > max_thread_time {
            max_thread_time = thread_time;
        }
    }

    let total_time = start.elapsed();
    let total_ops = num_threads * batches_per_thread * ops_per_batch;
    let throughput = total_ops as f64 / total_time.as_secs_f64();
    
    (throughput, max_thread_time.as_secs_f64())
}

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("PARALLELISM SCALING DEBUG");
    println!("{}\n", "=".repeat(80));

    println!("Testing WITHOUT WAL:");
    println!("{:>6} {:>12} {:>15} {:>12} {:>10}", "Threads", "Throughput", "Scaling", "Slowest", "Efficiency");
    println!("{}", "-".repeat(80));
    
    let baseline = test_threads(1, false).0;
    for num_threads in [1, 2, 4, 8, 16] {
        let (throughput, max_thread) = test_threads(num_threads, false);
        let scaling = throughput / baseline;
        let efficiency = scaling / num_threads as f64 * 100.0;
        println!("{:6} {:12.0} {:14.2}x {:11.2}s {:9.0}%", 
                 num_threads, throughput, scaling, max_thread, efficiency);
    }

    println!("\nTesting WITH WAL:");
    println!("{:>6} {:>12} {:>15} {:>12} {:>10}", "Threads", "Throughput", "Scaling", "Slowest", "Efficiency");
    println!("{}", "-".repeat(80));
    
    let baseline = test_threads(1, true).0;
    for num_threads in [1, 2, 4, 8, 16] {
        let (throughput, max_thread) = test_threads(num_threads, true);
        let scaling = throughput / baseline;
        let efficiency = scaling / num_threads as f64 * 100.0;
        println!("{:6} {:12.0} {:14.2}x {:11.2}s {:9.0}%", 
                 num_threads, throughput, scaling, max_thread, efficiency);
    }

    println!("\n{}", "=".repeat(80));
    println!("Analysis:");
    println!("  - Perfect scaling: Efficiency = 100% (throughput = baseline × threads)");
    println!("  - If efficiency drops, there's contention somewhere");
    println!("  - 'Slowest thread' shows if threads are running in parallel or serial");
    println!("{}", "=".repeat(80));
}
