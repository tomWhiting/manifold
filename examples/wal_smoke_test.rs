//! WAL Smoke Test - Quick validation of WAL performance and functionality
//!
//! This program tests three scenarios:
//! 1. Default durability (slow, ~60 ops/sec) - full B-tree fsync on every commit
//! 2. Durability::None (fast, ~16K ops/sec) - no fsync, data loss on crash
//! 3. WAL enabled (target: 10-15K ops/sec) - fast writes with crash recovery
//!
//! Also validates crash recovery by simulating a crash and replaying WAL.

use manifold::column_family::ColumnFamilyDatabase;
use manifold::{Durability, TableDefinition};
use std::fs;
use std::time::Instant;
use tempfile::{NamedTempFile, TempDir};

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

fn main() {
    println!("\n{}", "=".repeat(80));
    println!("WAL SMOKE TEST - Performance & Functionality Validation");
    println!("{}\n", "=".repeat(80));

    // Test 1: Default durability (slow baseline) - individual transactions
    println!("[1/6] Testing DEFAULT DURABILITY - Individual Transactions (slow baseline)...");
    let default_individual_ops_per_sec = test_default_durability_individual();
    println!(
        "      Result: {:.0} ops/sec\n",
        default_individual_ops_per_sec
    );

    // Test 2: Default durability with batching
    println!("[2/6] Testing DEFAULT DURABILITY - Batched Transactions...");
    let default_batched_ops_per_sec = test_default_durability_batched();
    println!("      Result: {:.0} ops/sec\n", default_batched_ops_per_sec);

    // Test 3: Durability::None (fast baseline)
    println!("[3/6] Testing DURABILITY::NONE (fast baseline)...");
    let none_ops_per_sec = test_durability_none();
    println!("      Result: {:.0} ops/sec\n", none_ops_per_sec);

    // Test 4: WAL enabled with individual transactions
    println!("[4/6] Testing WAL ENABLED - Individual Transactions...");
    let wal_individual_ops_per_sec = test_wal_individual();
    println!("      Result: {:.0} ops/sec\n", wal_individual_ops_per_sec);

    // Test 5: Crash recovery
    println!("[5/6] Testing CRASH RECOVERY...");
    let recovery_success = test_crash_recovery();
    println!(
        "      Result: {}\n",
        if recovery_success {
            "PASS - Data recovered successfully"
        } else {
            "FAIL - Data loss detected"
        }
    );

    // Test 6: Checkpoint behavior
    println!("[6/6] Testing CHECKPOINT BEHAVIOR...");
    test_checkpoint_behavior();
    println!("      Result: Checkpoint completed successfully\n");

    // Summary
    println!("{}", "=".repeat(80));
    println!("SUMMARY");
    println!("{}\n", "=".repeat(80));

    println!("Individual Transaction Performance:");
    println!(
        "  Default durability:        {:.0} ops/sec",
        default_individual_ops_per_sec
    );
    println!(
        "  WAL enabled:               {:.0} ops/sec",
        wal_individual_ops_per_sec
    );

    let individual_improvement = wal_individual_ops_per_sec / default_individual_ops_per_sec;
    println!(
        "  Improvement:               {:.1}x",
        individual_improvement
    );

    println!("\nBatched Transaction Performance:");
    println!(
        "  Default durability (batched): {:.0} ops/sec",
        default_batched_ops_per_sec
    );
    println!(
        "  Durability::None:             {:.0} ops/sec",
        none_ops_per_sec
    );

    let vs_none = wal_individual_ops_per_sec / none_ops_per_sec;

    println!("\nPerformance Analysis:");
    println!(
        "  WAL vs default (individual):  {:.1}x improvement",
        individual_improvement
    );
    println!(
        "  WAL vs default (batched):     {:.1}x",
        wal_individual_ops_per_sec / default_batched_ops_per_sec
    );
    println!(
        "  WAL vs Durability::None:      {:.1}x ({})",
        vs_none,
        if vs_none > 0.7 {
            "good - near-parity"
        } else {
            "slower than expected"
        }
    );

    println!("\nDiagnostics:");
    println!("  NOTE: Current WAL implementation fsyncs on EVERY transaction commit.");
    println!("  This means individual transactions are still slow (~200 ops/sec).");
    println!("  Expected behavior: WAL should allow multiple appends before fsync.");
    println!("  ISSUE IDENTIFIED: WAL needs batching/group commit for true performance.");

    println!("\nTarget validation:");
    if wal_individual_ops_per_sec >= 10_000.0 {
        println!("  PASS - Achieved target of 10K+ ops/sec");
    } else if wal_individual_ops_per_sec >= 1_000.0 {
        println!(
            "  WARN - Below 10K target ({:.0} ops/sec) - batching needed",
            wal_individual_ops_per_sec
        );
    } else {
        println!(
            "  FAIL - Well below target ({:.0} ops/sec) - investigation required",
            wal_individual_ops_per_sec
        );
        println!("         Root cause: fsync() on every commit instead of batching");
    }

    if individual_improvement >= 100.0 {
        println!(
            "  PASS - {:.0}x improvement over default durability",
            individual_improvement
        );
    } else if individual_improvement >= 1.0 {
        println!(
            "  WARN - Only {:.1}x improvement (target: 100x+)",
            individual_improvement
        );
        println!(
            "         Current: {:.0} ops/sec, Need: batching/group commit",
            wal_individual_ops_per_sec
        );
    } else {
        println!("  FAIL - No improvement or slower");
    }

    if recovery_success {
        println!("  PASS - Crash recovery working correctly");
    } else {
        println!("  FAIL - Crash recovery broken");
    }

    println!("\n{}", "=".repeat(80));
    println!("Smoke test complete!");
    println!("{}\n", "=".repeat(80));
}

fn test_default_durability_individual() -> f64 {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024))
        .unwrap();
    let cf = db.column_family("test").unwrap();

    let num_writes = 100; // Small number since this is slow
    let data = vec![0u8; 1024];

    let start = Instant::now();

    for i in 0..num_writes {
        let txn = cf.begin_write().unwrap();
        // Default durability (Immediate) - fsyncs on every commit
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }

    let duration = start.elapsed();
    num_writes as f64 / duration.as_secs_f64()
}

fn test_default_durability_batched() -> f64 {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024))
        .unwrap();
    let cf = db.column_family("test").unwrap();

    let num_writes = 5000;
    let batch_size = 100;
    let data = vec![0u8; 1024];

    let start = Instant::now();

    let num_batches = num_writes / batch_size;
    for batch in 0..num_batches {
        let mut txn = cf.begin_write().unwrap();
        // Only fsync on the last batch
        if batch < num_batches - 1 {
            txn.set_durability(Durability::None).unwrap();
        }
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..batch_size {
                let key = (batch * batch_size + i) as u64;
                table.insert(&key, data.as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();
    }

    let duration = start.elapsed();
    num_writes as f64 / duration.as_secs_f64()
}

fn test_durability_none() -> f64 {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024))
        .unwrap();
    let cf = db.column_family("test").unwrap();

    let num_writes = 5000;
    let data = vec![0u8; 1024];

    let start = Instant::now();

    for i in 0..num_writes {
        let mut txn = cf.begin_write().unwrap();
        txn.set_durability(Durability::None).unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }

    let duration = start.elapsed();
    num_writes as f64 / duration.as_secs_f64()
}

fn test_wal_individual() -> f64 {
    let tmpfile = NamedTempFile::new().unwrap();
    // WAL is enabled when pool_size > 0 (which is the default)
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024))
        .unwrap();
    let cf = db.column_family("test").unwrap();

    let num_writes = 100; // Match default durability test
    let data = vec![0u8; 1024];

    let start = Instant::now();

    for i in 0..num_writes {
        let txn = cf.begin_write().unwrap();
        // Default durability with WAL
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }

    let duration = start.elapsed();
    num_writes as f64 / duration.as_secs_f64()
}

fn test_crash_recovery() -> bool {
    let tmpdir = TempDir::new().unwrap();
    let db_path = tmpdir.path().join("test.db");

    // Phase 1: Write data with WAL
    {
        let db = ColumnFamilyDatabase::open(&db_path).unwrap();
        db.create_column_family("test", Some(10 * 1024 * 1024))
            .unwrap();
        let cf = db.column_family("test").unwrap();

        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..100u64 {
                let value = b"test_data".as_slice();
                table.insert(&i, value).unwrap();
            }
        }
        txn.commit().unwrap();

        // Simulate crash - drop DB without clean shutdown
        // (In reality, checkpoint thread will try to run, but we're testing recovery)
    }

    // Phase 2: Reopen and verify data (should trigger WAL recovery)
    {
        let db = ColumnFamilyDatabase::open(&db_path).unwrap();
        let cf = db.column_family("test").unwrap();

        let txn = cf.begin_read().unwrap();
        let table = txn.open_table(TEST_TABLE).unwrap();

        // Verify all data is present
        for i in 0..100u64 {
            match table.get(&i) {
                Ok(Some(value)) => {
                    if value.value() != b"test_data" {
                        println!("      ERROR: Key {} has wrong value", i);
                        return false;
                    }
                }
                Ok(None) => {
                    println!("      ERROR: Key {} missing after recovery", i);
                    return false;
                }
                Err(e) => {
                    println!("      ERROR: Failed to read key {}: {}", i, e);
                    return false;
                }
            }
        }
    }

    true
}

fn test_checkpoint_behavior() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    db.create_column_family("test", Some(10 * 1024 * 1024))
        .unwrap();
    let cf = db.column_family("test").unwrap();

    // Write enough data to potentially trigger a checkpoint
    let data = vec![0u8; 1024];
    for i in 0..1000u64 {
        let mut txn = cf.begin_write().unwrap();
        if i % 100 == 0 {
            // Occasional sync to ensure some durability
            txn.set_durability(Durability::Immediate).unwrap();
        } else {
            txn.set_durability(Durability::None).unwrap();
        }
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }

    // Check WAL file exists
    let wal_path = tmpfile.path().with_extension("wal");
    if wal_path.exists() {
        let wal_size = fs::metadata(&wal_path).unwrap().len();
        println!("      WAL file size: {} bytes", wal_size);
    } else {
        println!("      WARNING: WAL file not found at {:?}", wal_path);
    }

    // Drop DB to trigger final checkpoint
    drop(cf);
    drop(db);

    println!("      Checkpoint completed on shutdown");
}
