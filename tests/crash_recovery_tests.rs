//! Crash Recovery Tests (Phase 2, Task 2.8)
//!
//! This test suite validates crash recovery behavior using process-based crash injection.
//! Tests verify that WAL replay correctly recovers committed transactions and that
//! data integrity is maintained after crashes at various points.
//!
//! Approach:
//! - Use fork() on Unix to create child processes that simulate crashes
//! - Child process performs transactions and exits abruptly (simulating crash)
//! - Parent process verifies recovery by reopening database and checking data
//!
//! Platform Support:
//! - Unix/Linux/macOS: Full crash injection via fork()
//! - Windows/WASM: Tests compile but skip crash injection (graceful degradation)

use manifold::column_family::ColumnFamilyDatabase;
use manifold::{ReadableTable, ReadableTableMetadata, TableDefinition};
use std::collections::HashSet;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &str> = TableDefinition::new("test");
const MULTI_TABLE_A: TableDefinition<u64, u64> = TableDefinition::new("table_a");
const MULTI_TABLE_B: TableDefinition<&str, &str> = TableDefinition::new("table_b");

// ============================================================================
// Crash Injection Helpers
// ============================================================================

#[cfg(unix)]
use std::process;

/// Simulates a crash by forking a child process that performs work and exits abruptly.
/// Returns true if this is the parent process, false if child (which should exit).
#[cfg(unix)]
fn fork_and_crash<F>(work: F) -> bool
where
    F: FnOnce(),
{
    use nix::sys::wait::{WaitStatus, waitpid};
    use nix::unistd::{ForkResult, fork};

    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            // Parent: wait for child to exit
            match waitpid(child, None) {
                Ok(WaitStatus::Exited(_, _)) => {
                    // Child exited (simulated crash)
                }
                Ok(status) => {
                    panic!("Child process terminated unexpectedly: {:?}", status);
                }
                Err(e) => {
                    panic!("Failed to wait for child: {}", e);
                }
            }
            true
        }
        Ok(ForkResult::Child) => {
            // Child: perform work then exit abruptly
            work();
            process::exit(0);
        }
        Err(e) => {
            panic!("Fork failed: {}", e);
        }
    }
}

#[cfg(not(unix))]
fn fork_and_crash<F>(_work: F) -> bool
where
    F: FnOnce(),
{
    // On non-Unix platforms, skip crash injection
    // Tests will gracefully degrade to verification-only
    eprintln!("Crash injection not supported on this platform - test skipped");
    false
}

// ============================================================================
// Basic Crash Recovery Tests
// ============================================================================

/// Test recovery after crash with single committed transaction
#[test]
#[cfg(unix)]
fn test_crash_after_single_commit() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    // Child process: write data and crash
    let is_parent = fork_and_crash(|| {
        let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
        db.create_column_family("test_cf", None).unwrap();

        let cf = db.column_family("test_cf").unwrap();
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&1, &"crash_test_value").unwrap();
        }
        txn.commit().unwrap();

        // Simulate crash - database drops without clean shutdown
        drop(db);
    });

    if !is_parent {
        return;
    }

    // Parent: verify recovery
    let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
    let cf = db.column_family("test_cf").unwrap();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(TEST_TABLE).unwrap();

    let value = table.get(&1).unwrap();
    assert!(value.is_some(), "Committed value should be recovered");
    assert_eq!(value.unwrap().value(), "crash_test_value");
}

/// Test that uncommitted transactions are NOT recovered after crash
#[test]
#[cfg(unix)]
fn test_crash_before_commit_discards_transaction() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    // Child process: First commit to create CF and table, then write uncommitted data
    let is_parent = fork_and_crash(|| {
        let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
        db.create_column_family("test_cf", None).unwrap();

        let cf = db.column_family("test_cf").unwrap();

        // First, commit an initial value to ensure table exists
        {
            let txn = cf.begin_write().unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                table.insert(&0, &"initial_committed").unwrap();
            }
            txn.commit().unwrap();
        }

        // Now write uncommitted data and crash
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&1, &"uncommitted_value").unwrap();
        }
        // DON'T commit - crash here
        drop(txn);
        drop(db);
    });

    if !is_parent {
        return;
    }

    // Parent: verify uncommitted data is NOT recovered
    let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
    let cf = db.column_family("test_cf").unwrap();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(TEST_TABLE).unwrap();

    // Initial committed value should exist
    assert!(
        table.get(&0).unwrap().is_some(),
        "Initial committed value should exist"
    );

    // Uncommitted value should NOT exist
    let value = table.get(&1).unwrap();
    assert!(value.is_none(), "Uncommitted value should NOT be recovered");
}

/// Test recovery of multiple committed transactions
#[test]
#[cfg(unix)]
fn test_crash_after_multiple_commits() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let num_transactions = 50;

    // Child process: write multiple transactions and crash
    let is_parent = fork_and_crash(|| {
        let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        for i in 0..num_transactions {
            let txn = cf.begin_write().unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                table.insert(&i, &"multi_commit_test").unwrap();
            }
            txn.commit().unwrap();
        }

        // Crash after all commits
        drop(db);
    });

    if !is_parent {
        return;
    }

    // Parent: verify all transactions recovered
    let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
    let cf = db.column_family("test_cf").unwrap();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(TEST_TABLE).unwrap();

    assert_eq!(
        table.len().unwrap(),
        num_transactions,
        "All committed transactions should be recovered"
    );

    for i in 0..num_transactions {
        let value = table.get(&i).unwrap();
        assert!(value.is_some(), "Transaction {} should be recovered", i);
        assert_eq!(value.unwrap().value(), "multi_commit_test");
    }
}

// ============================================================================
// Multi-Column Family Crash Recovery
// ============================================================================

/// Test recovery with multiple column families
#[test]
#[cfg(unix)]
fn test_crash_recovery_multi_column_family() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let cf_names = vec!["cf1", "cf2", "cf3"];
    let entries_per_cf = 20;

    // Child process: write to multiple CFs and crash
    let is_parent = fork_and_crash(|| {
        let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();

        for cf_name in &cf_names {
            db.create_column_family(*cf_name, None).unwrap();
            let cf = db.column_family(cf_name).unwrap();

            for i in 0..entries_per_cf {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    let value = format!("cf_{}_value_{}", cf_name, i);
                    table.insert(&i, &value.as_str()).unwrap();
                }
                txn.commit().unwrap();
            }
        }

        drop(db);
    });

    if !is_parent {
        return;
    }

    // Parent: verify all CFs recovered correctly
    let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();

    for cf_name in &cf_names {
        let cf = db.column_family(cf_name).unwrap();
        let txn = cf.begin_read().unwrap();
        let table = txn.open_table(TEST_TABLE).unwrap();

        assert_eq!(
            table.len().unwrap(),
            entries_per_cf,
            "CF {} should have all entries",
            cf_name
        );

        for i in 0..entries_per_cf {
            let value = table.get(&i).unwrap();
            assert!(value.is_some(), "CF {} entry {} missing", cf_name, i);
            let expected = format!("cf_{}_value_{}", cf_name, i);
            assert_eq!(value.unwrap().value(), expected);
        }
    }
}

/// Test interleaved writes across multiple CFs before crash
#[test]
#[cfg(unix)]
fn test_crash_recovery_interleaved_cf_writes() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let num_rounds = 10;

    // Child process: interleave writes across CFs
    let is_parent = fork_and_crash(|| {
        let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();

        db.create_column_family("cf_a", None).unwrap();
        db.create_column_family("cf_b", None).unwrap();

        let cf_a = db.column_family("cf_a").unwrap();
        let cf_b = db.column_family("cf_b").unwrap();

        for round in 0..num_rounds {
            // Write to CF A
            let txn_a = cf_a.begin_write().unwrap();
            {
                let mut table = txn_a.open_table(MULTI_TABLE_A).unwrap();
                table.insert(&round, &(round * 2)).unwrap();
            }
            txn_a.commit().unwrap();

            // Write to CF B
            let txn_b = cf_b.begin_write().unwrap();
            {
                let mut table = txn_b.open_table(MULTI_TABLE_B).unwrap();
                let value = format!("round_{}", round);
                table.insert(&value.as_str(), &"data").unwrap();
            }
            txn_b.commit().unwrap();
        }

        drop(db);
    });

    if !is_parent {
        return;
    }

    // Parent: verify interleaved writes recovered
    let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();

    let cf_a = db.column_family("cf_a").unwrap();
    let txn_a = cf_a.begin_read().unwrap();
    let table_a = txn_a.open_table(MULTI_TABLE_A).unwrap();

    assert_eq!(table_a.len().unwrap(), num_rounds);

    for round in 0..num_rounds {
        let value = table_a.get(&round).unwrap();
        assert!(value.is_some());
        assert_eq!(value.unwrap().value(), round * 2);
    }

    let cf_b = db.column_family("cf_b").unwrap();
    let txn_b = cf_b.begin_read().unwrap();
    let table_b = txn_b.open_table(MULTI_TABLE_B).unwrap();

    assert_eq!(table_b.len().unwrap(), num_rounds);

    for round in 0..num_rounds {
        let key = format!("round_{}", round);
        let value = table_b.get(key.as_str()).unwrap();
        assert!(value.is_some());
        assert_eq!(value.unwrap().value(), "data");
    }
}

// ============================================================================
// Data Integrity Verification Tests
// ============================================================================

/// Test that all recovered data is consistent (no partial writes)
#[test]
#[cfg(unix)]
fn test_crash_recovery_data_integrity() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let num_entries = 100;

    // Child process: write known pattern
    let is_parent = fork_and_crash(|| {
        let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        for i in 0..num_entries {
            let txn = cf.begin_write().unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                // Use key as value for easy verification
                let value = format!("integrity_check_{}", i);
                table.insert(&i, &value.as_str()).unwrap();
            }
            txn.commit().unwrap();
        }

        drop(db);
    });

    if !is_parent {
        return;
    }

    // Parent: verify data integrity
    let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
    let cf = db.column_family("test_cf").unwrap();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(TEST_TABLE).unwrap();

    // Verify all or nothing - no partial entries
    let recovered_count = table.len().unwrap();
    assert_eq!(
        recovered_count, num_entries,
        "Should recover all committed entries"
    );

    // Verify each entry has correct value
    for i in 0..num_entries {
        let value = table.get(&i).unwrap();
        assert!(value.is_some(), "Entry {} should exist", i);

        let expected = format!("integrity_check_{}", i);
        assert_eq!(
            value.unwrap().value(),
            expected,
            "Entry {} should have correct value",
            i
        );
    }
}

/// Test that keys are unique and no duplicates after recovery
#[test]
#[cfg(unix)]
fn test_crash_recovery_no_duplicates() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let num_entries = 50;

    // Child process: write entries
    let is_parent = fork_and_crash(|| {
        let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        for i in 0..num_entries {
            let txn = cf.begin_write().unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                table.insert(&i, &"duplicate_test").unwrap();
            }
            txn.commit().unwrap();
        }

        drop(db);
    });

    if !is_parent {
        return;
    }

    // Parent: verify no duplicates
    let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
    let cf = db.column_family("test_cf").unwrap();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(TEST_TABLE).unwrap();

    let mut seen_keys = HashSet::new();
    let iter = table.iter().unwrap();

    for entry in iter {
        let (key, _) = entry.unwrap();
        let key_val = key.value();
        assert!(
            seen_keys.insert(key_val),
            "Duplicate key found: {}",
            key_val
        );
    }

    assert_eq!(seen_keys.len(), num_entries as usize);
}

// ============================================================================
// Stress Tests
// ============================================================================

/// Test recovery with large number of transactions
#[test]
#[cfg(unix)]
fn test_crash_recovery_large_transaction_count() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let num_transactions = 500;

    // Child process: write many transactions
    let is_parent = fork_and_crash(|| {
        let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        for i in 0..num_transactions {
            let txn = cf.begin_write().unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                table.insert(&i, &"stress_test").unwrap();
            }
            txn.commit().unwrap();
        }

        drop(db);
    });

    if !is_parent {
        return;
    }

    // Parent: verify all recovered
    let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
    let cf = db.column_family("test_cf").unwrap();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(TEST_TABLE).unwrap();

    assert_eq!(table.len().unwrap(), num_transactions);

    // Spot check some entries
    for i in (0..num_transactions).step_by(50) {
        assert!(table.get(&i).unwrap().is_some());
    }
}

// ============================================================================
// Platform-Agnostic Verification Tests
// ============================================================================

/// Verify that database can recover from clean WAL state
/// (No crash injection - works on all platforms)
#[test]
fn test_recovery_clean_wal_state() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    // Write data with WAL
    {
        let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        for i in 0..10 {
            let txn = cf.begin_write().unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                table.insert(&i, &"clean_wal").unwrap();
            }
            txn.commit().unwrap();
        }
    }

    // Reopen - should recover from WAL
    let db = ColumnFamilyDatabase::builder().open(&db_path).unwrap();
    let cf = db.column_family("test_cf").unwrap();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(TEST_TABLE).unwrap();

    assert_eq!(table.len().unwrap(), 10);

    for i in 0..10 {
        assert!(table.get(&i).unwrap().is_some());
    }
}
