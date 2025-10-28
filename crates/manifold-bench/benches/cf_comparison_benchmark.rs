//! Column Family Concurrent Write Benchmark
//!
//! This benchmark showcases Manifold's strength: concurrent writes across
//! multiple column families with WAL-enabled group commit batching.
//!
//! Key differences from LMDB benchmark:
//! - Multiple concurrent writers to separate column families
//! - Small transactions (batches of 1000) that benefit from WAL group commit
//! - Tests the intended use case for column family architecture
//!
//! Databases without native column family support use a single database
//! with all writers competing for the same lock, demonstrating Manifold's
//! architectural advantage.

use std::env::current_dir;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use std::{fs, process};
use tempfile::{NamedTempFile, TempDir};

mod common;
use common::*;

const NUM_COLUMN_FAMILIES: usize = 8;
const WRITES_PER_CF: usize = 50_000;
const CONCURRENT_WRITERS: usize = 8;
const BATCH_SIZE: usize = 1000;

fn main() {
    let _ = env_logger::try_init();
    let tmpdir = current_dir().unwrap().join(".benchmark_cf");
    let _ = fs::remove_dir_all(&tmpdir);
    fs::create_dir(&tmpdir).unwrap();

    let tmpdir2 = tmpdir.clone();
    ctrlc::set_handler(move || {
        let _ = fs::remove_dir_all(&tmpdir2);
        process::exit(1);
    })
    .unwrap();

    println!("\n{}", "=".repeat(80));
    println!("Column Family Concurrent Write Benchmark");
    println!("{}", "=".repeat(80));
    println!("\nConfiguration:");
    println!("  Column Families: {}", NUM_COLUMN_FAMILIES);
    println!("  Concurrent writers: {} (1 per CF)", CONCURRENT_WRITERS);
    println!(
        "  Writes per CF: {} ({} batches of {})",
        WRITES_PER_CF,
        WRITES_PER_CF / BATCH_SIZE,
        BATCH_SIZE
    );
    println!(
        "  Total operations: {}",
        NUM_COLUMN_FAMILIES * WRITES_PER_CF
    );
    println!();

    let manifold_wal_results = benchmark_manifold_cf(&tmpdir, true);
    let manifold_nowal_results = benchmark_manifold_cf(&tmpdir, false);
    let rocksdb_results = benchmark_rocksdb_cf(&tmpdir);
    let fjall_results = benchmark_fjall_cf(&tmpdir);
    let lmdb_results = benchmark_lmdb_single(&tmpdir);

    let _ = fs::remove_dir_all(&tmpdir);

    println!("\n{}", "=".repeat(80));
    println!("Results");
    println!("{}", "=".repeat(80));
    println!();

    let total_ops = NUM_COLUMN_FAMILIES * WRITES_PER_CF;

    println!("| Database | Time (s) | Throughput (ops/sec) | vs Manifold+WAL |");
    println!("|----------|----------|----------------------|-----------------|");

    let manifold_wal_time = manifold_wal_results.as_secs_f64();

    for (name, duration) in [
        ("Manifold (WAL)", manifold_wal_results),
        ("Manifold (no WAL)", manifold_nowal_results),
        ("RocksDB (CF)", rocksdb_results),
        ("Fjall (Partitions)", fjall_results),
        ("LMDB (single DB)", lmdb_results),
    ] {
        let time_secs = duration.as_secs_f64();
        let throughput = (total_ops as f64 / time_secs) as u64;
        let speedup = time_secs / manifold_wal_time;

        println!(
            "| {:<20} | {:>8.2} | {:>20} | {:>15.2}x |",
            name,
            time_secs,
            format!("{:>6}K", throughput / 1000),
            speedup
        );
    }

    println!();
    println!("Note: Databases without native CF support use a single database");
    println!("      with all writers competing for locks, showing Manifold's");
    println!("      architectural advantage for concurrent multi-tenant workloads.");
    println!("{}", "=".repeat(80));
    println!();
}

fn benchmark_manifold_cf(tmpdir: &std::path::Path, use_wal: bool) -> Duration {
    use manifold::TableDefinition;
    use manifold::column_family::ColumnFamilyDatabase;

    const TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

    let tmpfile: NamedTempFile = NamedTempFile::new_in(tmpdir).unwrap();

    let db = if use_wal {
        ColumnFamilyDatabase::builder()
            .pool_size(64) // WAL enabled
            .open(tmpfile.path())
            .unwrap()
    } else {
        ColumnFamilyDatabase::builder()
            .without_wal() // WAL disabled
            .open(tmpfile.path())
            .unwrap()
    };

    let db = Arc::new(db);

    // Create column families
    for i in 0..NUM_COLUMN_FAMILIES {
        db.create_column_family(&format!("cf_{}", i), None).unwrap();
    }

    println!(
        "Running Manifold (WAL {})...",
        if use_wal { "enabled" } else { "disabled" }
    );
    let start = Instant::now();

    // Spawn concurrent writers, each writing to its own CF
    let handles: Vec<_> = (0..CONCURRENT_WRITERS)
        .map(|writer_id| {
            let db = Arc::clone(&db);
            thread::spawn(move || {
                let cf = db.column_family(&format!("cf_{}", writer_id)).unwrap();

                for batch_start in (0..WRITES_PER_CF).step_by(BATCH_SIZE) {
                    let txn = cf.begin_write().unwrap();
                    {
                        let mut table = txn.open_table(TABLE).unwrap();
                        for i in
                            batch_start..batch_start.saturating_add(BATCH_SIZE).min(WRITES_PER_CF)
                        {
                            let key = (writer_id as u64 * WRITES_PER_CF as u64) + i as u64;
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

    let elapsed = start.elapsed();
    println!("  Completed in {:.2}s", elapsed.as_secs_f64());
    elapsed
}

fn benchmark_rocksdb_cf(tmpdir: &std::path::Path) -> Duration {
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
    opts.increase_parallelism(std::thread::available_parallelism().map_or(1, |n| n.get()) as i32);

    // Create column family descriptors
    let cf_names: Vec<String> = (0..NUM_COLUMN_FAMILIES)
        .map(|i| format!("cf_{}", i))
        .collect();

    let mut cfs = vec![ColumnFamilyDescriptor::new("default", Options::default())];
    for name in &cf_names {
        cfs.push(ColumnFamilyDescriptor::new(name, Options::default()));
    }

    let db = Arc::new(DB::open_cf_descriptors(&opts, tmpfile.path(), cfs).unwrap());

    println!("Running RocksDB (column families)...");
    let start = Instant::now();

    // Spawn concurrent writers
    let handles: Vec<_> = (0..CONCURRENT_WRITERS)
        .map(|writer_id| {
            let db = Arc::clone(&db);
            let cf_name = cf_names[writer_id].clone();

            thread::spawn(move || {
                let cf = db.cf_handle(&cf_name).unwrap();

                for batch_start in (0..WRITES_PER_CF).step_by(BATCH_SIZE) {
                    let mut batch = rocksdb::WriteBatch::default();
                    for i in batch_start..batch_start.saturating_add(BATCH_SIZE).min(WRITES_PER_CF)
                    {
                        let key = (writer_id as u64 * WRITES_PER_CF as u64) + i as u64;
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

    let elapsed = start.elapsed();
    println!("  Completed in {:.2}s", elapsed.as_secs_f64());
    elapsed
}

fn benchmark_fjall_cf(tmpdir: &std::path::Path) -> Duration {
    let tmpfile: TempDir = tempfile::tempdir_in(tmpdir).unwrap();

    let keyspace = fjall::Config::new(tmpfile.path())
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

    println!("Running Fjall (partitions)...");
    let start = Instant::now();

    // Spawn concurrent writers
    let handles: Vec<_> = (0..CONCURRENT_WRITERS)
        .map(|writer_id| {
            let keyspace = Arc::clone(&keyspace);
            let partition_name = partition_names[writer_id].clone();

            thread::spawn(move || {
                let partition = keyspace
                    .open_partition(&partition_name, Default::default())
                    .unwrap();

                for batch_start in (0..WRITES_PER_CF).step_by(BATCH_SIZE) {
                    let mut tx = keyspace.write_tx();
                    for i in batch_start..batch_start.saturating_add(BATCH_SIZE).min(WRITES_PER_CF)
                    {
                        let key = (writer_id as u64 * WRITES_PER_CF as u64) + i as u64;
                        let value = vec![0u8; 150];
                        tx.insert(&partition, key.to_le_bytes(), value);
                    }
                    tx.commit().unwrap();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start.elapsed();
    println!("  Completed in {:.2}s", elapsed.as_secs_f64());
    elapsed
}

fn benchmark_lmdb_single(tmpdir: &std::path::Path) -> Duration {
    let tempdir: TempDir = tempfile::tempdir_in(tmpdir).unwrap();
    let env = unsafe {
        heed::EnvOpenOptions::new()
            .map_size(4096 * 1024 * 1024)
            .open(tempdir.path())
            .unwrap()
    };

    let db: heed::Database<heed::types::Bytes, heed::types::Bytes> = {
        let mut txn = env.write_txn().unwrap();
        let db = env.create_database(&mut txn, None).unwrap();
        txn.commit().unwrap();
        db
    };
    let env = Arc::new(env);

    println!("Running LMDB (single database, all writers contend)...");
    let start = Instant::now();

    // All writers share the same database - no column family isolation
    let handles: Vec<_> = (0..CONCURRENT_WRITERS)
        .map(|writer_id| {
            let env = Arc::clone(&env);

            thread::spawn(move || {
                for batch_start in (0..WRITES_PER_CF).step_by(BATCH_SIZE) {
                    let mut txn = env.write_txn().unwrap();
                    for i in batch_start..batch_start.saturating_add(BATCH_SIZE).min(WRITES_PER_CF)
                    {
                        let key = (writer_id as u64 * WRITES_PER_CF as u64) + i as u64;
                        let value = vec![0u8; 150];
                        db.put(&mut txn, &key.to_le_bytes(), &value).unwrap();
                    }
                    txn.commit().unwrap();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start.elapsed();
    println!("  Completed in {:.2}s", elapsed.as_secs_f64());
    elapsed
}
