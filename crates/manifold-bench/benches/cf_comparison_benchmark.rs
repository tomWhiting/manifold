use std::env::current_dir;
use std::sync::Arc;
use std::thread;
use std::{fs, process};
use tempfile::{NamedTempFile, TempDir};

mod common;
use common::*;

const NUM_COLUMN_FAMILIES: usize = 8;
const WRITES_PER_CF: usize = 50_000;
const CONCURRENT_WRITERS: usize = 8;

fn main() {
    let _ = env_logger::try_init();
    let tmpdir = current_dir().unwrap().join(".benchmark_cf");
    fs::create_dir(&tmpdir).unwrap();

    let tmpdir2 = tmpdir.clone();
    ctrlc::set_handler(move || {
        fs::remove_dir_all(&tmpdir2).unwrap();
        process::exit(1);
    })
    .unwrap();

    println!("\n=== Column Family Concurrent Write Benchmark ===");
    println!("Column Families: {}", NUM_COLUMN_FAMILIES);
    println!("Writes per CF: {}", WRITES_PER_CF);
    println!("Concurrent writers: {}", CONCURRENT_WRITERS);
    println!();

    let manifold_results = benchmark_manifold_cf(&tmpdir);
    let rocksdb_results = benchmark_rocksdb_cf(&tmpdir);
    let fjall_results = benchmark_fjall_cf(&tmpdir);

    fs::remove_dir_all(&tmpdir).unwrap();

    println!("\n=== Results ===\n");
    println!("| Database | Total Time | Throughput | Speedup vs RocksDB |");
    println!("|----------|------------|------------|-------------------|");

    let rocksdb_time_ms = rocksdb_results.as_millis();

    for (name, duration) in [
        ("Manifold (CF)", manifold_results),
        ("RocksDB (CF)", rocksdb_results),
        ("Fjall (Partitions)", fjall_results),
    ] {
        let time_ms = duration.as_millis();
        let total_ops = NUM_COLUMN_FAMILIES * WRITES_PER_CF;
        let throughput = (total_ops as f64 / duration.as_secs_f64()) as u64;
        let speedup = rocksdb_time_ms as f64 / time_ms as f64;

        println!(
            "| {} | {}ms | {:>6}K ops/sec | {:.2}x |",
            name,
            time_ms,
            throughput / 1000,
            speedup
        );
    }

    println!();
}

fn benchmark_manifold_cf(tmpdir: &std::path::Path) -> std::time::Duration {
    use manifold::TableDefinition;
    use manifold::column_family::ColumnFamilyDatabase;

    const TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

    let tmpfile: NamedTempFile = NamedTempFile::new_in(tmpdir).unwrap();
    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(256)
            .open(tmpfile.path())
            .unwrap(),
    );

    // Create column families
    for i in 0..NUM_COLUMN_FAMILIES {
        db.create_column_family(&format!("cf_{}", i), None).unwrap();
    }

    let start = std::time::Instant::now();

    // Spawn concurrent writers, each writing to a different CF
    let handles: Vec<_> = (0..CONCURRENT_WRITERS)
        .map(|writer_id| {
            let db = Arc::clone(&db);
            thread::spawn(move || {
                let cf_id = writer_id % NUM_COLUMN_FAMILIES;
                let cf = db.column_family(&format!("cf_{}", cf_id)).unwrap();

                let writes_per_thread =
                    WRITES_PER_CF / (CONCURRENT_WRITERS / NUM_COLUMN_FAMILIES).max(1);

                for batch_start in (0..writes_per_thread).step_by(1000) {
                    let txn = cf.begin_write().unwrap();
                    {
                        let mut table = txn.open_table(TABLE).unwrap();
                        for i in
                            batch_start..batch_start.saturating_add(1000).min(writes_per_thread)
                        {
                            let key = (writer_id * writes_per_thread + i) as u64;
                            let value = vec![0u8; 150];
                            table.insert(&key, value.as_slice()).unwrap();
                        }
                    }
                    txn.commit().unwrap();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    start.elapsed()
}

fn benchmark_rocksdb_cf(tmpdir: &std::path::Path) -> std::time::Duration {
    use rocksdb::{ColumnFamilyDescriptor, DB, Options};

    let tmpfile: TempDir = tempfile::tempdir_in(tmpdir).unwrap();

    let cache = rocksdb::Cache::new_lru_cache(CACHE_SIZE);
    let write_buffer = rocksdb::WriteBufferManager::new_write_buffer_manager_with_cache(
        CACHE_SIZE / 2,
        false,
        cache.clone(),
    );

    let mut bb = rocksdb::BlockBasedOptions::default();
    bb.set_block_cache(&cache);
    bb.set_bloom_filter(10.0, false);

    let mut opts = Options::default();
    opts.set_block_based_table_factory(&bb);
    opts.set_write_buffer_manager(&write_buffer);
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);

    // Create column family descriptors
    let cf_names: Vec<String> = (0..NUM_COLUMN_FAMILIES)
        .map(|i| format!("cf_{}", i))
        .collect();

    let mut cfs = vec![ColumnFamilyDescriptor::new("default", Options::default())];
    for name in &cf_names {
        cfs.push(ColumnFamilyDescriptor::new(name, Options::default()));
    }

    let db = Arc::new(DB::open_cf_descriptors(&opts, tmpfile.path(), cfs).unwrap());

    let start = std::time::Instant::now();

    // Spawn concurrent writers
    let handles: Vec<_> = (0..CONCURRENT_WRITERS)
        .map(|writer_id| {
            let db = Arc::clone(&db);
            let cf_name = cf_names[writer_id % NUM_COLUMN_FAMILIES].clone();

            thread::spawn(move || {
                let cf = db.cf_handle(&cf_name).unwrap();
                let writes_per_thread =
                    WRITES_PER_CF / (CONCURRENT_WRITERS / NUM_COLUMN_FAMILIES).max(1);

                for batch_start in (0..writes_per_thread).step_by(1000) {
                    let mut batch = rocksdb::WriteBatch::default();
                    for i in batch_start..batch_start.saturating_add(1000).min(writes_per_thread) {
                        let key = (writer_id * writes_per_thread + i) as u64;
                        let value = vec![0u8; 150];
                        batch.put_cf(&cf, key.to_le_bytes(), value);
                    }
                    db.write(batch).unwrap();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    start.elapsed()
}

fn benchmark_fjall_cf(tmpdir: &std::path::Path) -> std::time::Duration {
    let tmpfile: TempDir = tempfile::tempdir_in(tmpdir).unwrap();

    let mut keyspace = fjall::Config::new(tmpfile.path())
        .cache_size(CACHE_SIZE.try_into().unwrap())
        .open_transactional()
        .unwrap();

    // Create partitions (Fjall's equivalent of column families)
    let partition_names: Vec<String> = (0..NUM_COLUMN_FAMILIES)
        .map(|i| format!("cf_{}", i))
        .collect();

    for name in &partition_names {
        keyspace.open_partition(name, Default::default()).unwrap();
    }

    let keyspace = Arc::new(keyspace);
    let start = std::time::Instant::now();

    // Spawn concurrent writers
    let handles: Vec<_> = (0..CONCURRENT_WRITERS)
        .map(|writer_id| {
            let keyspace = Arc::clone(&keyspace);
            let partition_name = partition_names[writer_id % NUM_COLUMN_FAMILIES].clone();

            thread::spawn(move || {
                let partition = keyspace
                    .open_partition(&partition_name, Default::default())
                    .unwrap();
                let writes_per_thread =
                    WRITES_PER_CF / (CONCURRENT_WRITERS / NUM_COLUMN_FAMILIES).max(1);

                for batch_start in (0..writes_per_thread).step_by(1000) {
                    let mut tx = keyspace.write_tx();
                    for i in batch_start..batch_start.saturating_add(1000).min(writes_per_thread) {
                        let key = (writer_id * writes_per_thread + i) as u64;
                        let value = vec![0u8; 150];
                        tx.insert(&partition, key.to_le_bytes(), value).unwrap();
                    }
                    tx.commit().unwrap();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    start.elapsed()
}
