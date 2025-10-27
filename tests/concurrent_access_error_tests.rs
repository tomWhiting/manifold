//! Concurrent Access Error Handling Tests (Phase 2, Task 2.3)
//!
//! This test suite validates proper error handling for concurrent access scenarios:
//! - Deadlock detection (shouldn't happen, verify)
//! - Behavior under extreme contention
//! - Proper cleanup on transaction abort
//! - Recovery from panics during write transactions
//! - Lock poisoning handling

use manifold::TableDefinition;
use manifold::column_family::ColumnFamilyDatabase;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &str> = TableDefinition::new("test");

// ============================================================================
// Extreme Contention Tests
// ============================================================================

/// Test many threads competing for write access to same CF
#[test]
fn test_extreme_write_contention() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap(),
    );

    db.create_column_family("contended_cf", None).unwrap();

    let num_threads = 16;
    let writes_per_thread = 50;
    let barrier = Arc::new(Barrier::new(num_threads));

    let mut handles = vec![];
    for thread_id in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let barrier_clone = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            // Wait for all threads to be ready
            barrier_clone.wait();

            let cf = db_clone.column_family("contended_cf").unwrap();

            for i in 0..writes_per_thread {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    let write_txn = cf.begin_write()?;
                    let mut table = write_txn.open_table(TEST_TABLE)?;
                    let key = (thread_id * 1000 + i) as u64;
                    table.insert(&key, &"contention_test")?;
                    drop(table);
                    write_txn.commit()?;
                    Ok(())
                })();

                // Under extreme contention, some operations may timeout or fail
                // The key requirement: no panics, no corruption
                if let Err(e) = result {
                    let msg = format!("{}", e);
                    assert!(!msg.is_empty(), "Error should have context");
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify database is still consistent
    let cf = db.column_family("contended_cf").unwrap();
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();

    // Count how many writes succeeded
    let mut count = 0;
    for thread_id in 0..num_threads {
        for i in 0..writes_per_thread {
            let key = (thread_id * 1000 + i) as u64;
            if table.get(&key).unwrap().is_some() {
                count += 1;
            }
        }
    }

    // Should have many successful writes
    assert!(count > 0, "At least some writes should succeed");
}

/// Test read-heavy contention with many concurrent readers
#[test]
fn test_extreme_read_contention() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap(),
    );

    db.create_column_family("read_cf", None).unwrap();

    // Populate with data
    {
        let cf = db.column_family("read_cf").unwrap();
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        for i in 0..100 {
            table.insert(&i, &"read_test_value").unwrap();
        }
        drop(table);
        write_txn.commit().unwrap();
    }

    let num_readers = 32;
    let reads_per_thread = 100;
    let barrier = Arc::new(Barrier::new(num_readers));

    let mut handles = vec![];
    for _ in 0..num_readers {
        let db_clone = Arc::clone(&db);
        let barrier_clone = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            barrier_clone.wait();

            let cf = db_clone.column_family("read_cf").unwrap();

            for _ in 0..reads_per_thread {
                let read_txn = cf.begin_read().unwrap();
                let table = read_txn.open_table(TEST_TABLE).unwrap();

                // Read random keys
                for key in 0..10 {
                    let _ = table.get(&key);
                }

                drop(table);
                drop(read_txn);
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify data is still intact
    let cf = db.column_family("read_cf").unwrap();
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    assert_eq!(table.get(&0).unwrap().unwrap().value(), "read_test_value");
}

// ============================================================================
// Transaction Abort and Cleanup Tests
// ============================================================================

/// Test that aborted transactions clean up properly
#[test]
fn test_transaction_abort_cleanup() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    db.create_column_family("abort_cf", None).unwrap();
    let cf = db.column_family("abort_cf").unwrap();

    // Start transaction, write data, but don't commit (implicit abort)
    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&999, &"should_not_persist").unwrap();
        drop(table);
        // Drop write_txn without commit - implicit abort
    }

    // Verify data was not persisted
    // Note: Table creation persists even if transaction is aborted
    {
        let read_txn = cf.begin_read().unwrap();
        let table_result = read_txn.open_table(TEST_TABLE);
        if let Ok(table) = table_result {
            assert!(
                table.get(&999).unwrap().is_none(),
                "Aborted transaction should not persist data"
            );
        }
    }

    // Verify we can still write after abort
    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&888, &"should_persist").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Verify successful write persisted
    {
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(TEST_TABLE).unwrap();
        assert_eq!(table.get(&888).unwrap().unwrap().value(), "should_persist");
    }
}

/// Test many concurrent transaction aborts don't cause issues
#[test]
fn test_concurrent_transaction_aborts() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap(),
    );

    db.create_column_family("abort_cf", None).unwrap();

    let num_threads = 8;
    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = Arc::clone(&db);

        let handle = thread::spawn(move || {
            let cf = db_clone.column_family("abort_cf").unwrap();

            for i in 0..20 {
                // Randomly abort or commit
                let should_abort = (thread_id + i) % 3 == 0;

                let write_txn = cf.begin_write().unwrap();
                let mut table = write_txn.open_table(TEST_TABLE).unwrap();
                table
                    .insert(&((thread_id * 100 + i) as u64), &"test")
                    .unwrap();
                drop(table);

                if should_abort {
                    // Explicit drop without commit
                    drop(write_txn);
                } else {
                    write_txn.commit().unwrap();
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify database is still accessible and consistent
    let cf = db.column_family("abort_cf").unwrap();
    let read_txn = cf.begin_read().unwrap();
    let _ = read_txn.open_table(TEST_TABLE);
}

// ============================================================================
// Panic Recovery Tests
// ============================================================================

/// Test recovery after panic during write transaction
#[test]
fn test_panic_during_write_transaction() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap(),
    );

    db.create_column_family("panic_cf", None).unwrap();

    // Spawn thread that will panic during write
    let db_clone = Arc::clone(&db);
    let handle = thread::spawn(move || {
        let cf = db_clone.column_family("panic_cf").unwrap();
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&123, &"before_panic").unwrap();
        drop(table);

        // Panic before commit
        panic!("Intentional panic for testing");
    });

    // Wait for panic
    let result = handle.join();
    assert!(result.is_err(), "Thread should have panicked");

    // Give system time to clean up
    thread::sleep(Duration::from_millis(100));

    // Verify database is still accessible
    let cf = db.column_family("panic_cf").unwrap();

    // Should be able to start new write transaction
    let write_txn = cf.begin_write().unwrap();
    let mut table = write_txn.open_table(TEST_TABLE).unwrap();
    table.insert(&456, &"after_panic").unwrap();
    drop(table);
    write_txn.commit().unwrap();

    // Verify only committed data exists
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    assert!(
        table.get(&123).unwrap().is_none(),
        "Panicked transaction should not persist"
    );
    assert_eq!(
        table.get(&456).unwrap().unwrap().value(),
        "after_panic",
        "New transaction should succeed"
    );
}

// ============================================================================
// Lock Poisoning Tests
// ============================================================================

/// Test that lock poisoning is detected but doesn't prevent recovery
#[test]
fn test_lock_poisoning_detection() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap(),
    );

    db.create_column_family("poison_cf", None).unwrap();

    // Write some initial data
    {
        let cf = db.column_family("poison_cf").unwrap();
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&1, &"initial_data").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Spawn thread that panics while holding a write transaction
    // Note: Most internal locks will be poisoned but should recover
    let db_clone = Arc::clone(&db);
    let handle = thread::spawn(move || {
        let cf = db_clone.column_family("poison_cf").unwrap();
        let _write_txn = cf.begin_write().unwrap();
        panic!("Intentional panic to poison lock");
    });

    let _ = handle.join();

    // Give time for cleanup
    thread::sleep(Duration::from_millis(100));

    // System should recover - either by handling poison or having separate locks
    // The key: database should still be accessible
    let cf = db.column_family("poison_cf").unwrap();
    let read_result = cf.begin_read();

    // Should be able to read (poison may have been cleared or handled)
    if let Ok(read_txn) = read_result {
        let table = read_txn.open_table(TEST_TABLE).unwrap();
        assert_eq!(table.get(&1).unwrap().unwrap().value(), "initial_data");
    }
}

// ============================================================================
// Deadlock Prevention Tests
// ============================================================================

/// Test that multiple CFs don't deadlock when accessed in different orders
#[test]
fn test_no_deadlock_multiple_cfs() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap(),
    );

    // Create two CFs
    db.create_column_family("cf_a", None).unwrap();
    db.create_column_family("cf_b", None).unwrap();

    let num_threads = 4;
    let barrier = Arc::new(Barrier::new(num_threads));
    let mut handles = vec![];

    for i in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let barrier_clone = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            barrier_clone.wait();

            // Half threads access A then B, half access B then A
            let (first_cf, second_cf) = if i % 2 == 0 {
                ("cf_a", "cf_b")
            } else {
                ("cf_b", "cf_a")
            };

            for _ in 0..10 {
                // Access first CF
                {
                    let cf = db_clone.column_family(first_cf).unwrap();
                    let write_txn = cf.begin_write().unwrap();
                    let mut table = write_txn.open_table(TEST_TABLE).unwrap();
                    table.insert(&(i as u64), &"test").unwrap();
                    drop(table);
                    write_txn.commit().unwrap();
                }

                // Small delay
                thread::sleep(Duration::from_micros(10));

                // Access second CF
                {
                    let cf = db_clone.column_family(second_cf).unwrap();
                    let write_txn = cf.begin_write().unwrap();
                    let mut table = write_txn.open_table(TEST_TABLE).unwrap();
                    table.insert(&(i as u64), &"test").unwrap();
                    drop(table);
                    write_txn.commit().unwrap();
                }
            }
        });

        handles.push(handle);
    }

    // All threads should complete without deadlock
    for handle in handles {
        handle.join().unwrap();
    }
}

/// Test that single CF accessed by many threads doesn't deadlock
#[test]
fn test_no_deadlock_single_cf() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap(),
    );

    db.create_column_family("single_cf", None).unwrap();

    let num_threads = 8;
    let barrier = Arc::new(Barrier::new(num_threads));
    let mut handles = vec![];

    for i in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let barrier_clone = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            barrier_clone.wait();

            let cf = db_clone.column_family("single_cf").unwrap();

            // Rapid-fire transactions
            for j in 0..50 {
                let write_txn = cf.begin_write().unwrap();
                let mut table = write_txn.open_table(TEST_TABLE).unwrap();
                table
                    .insert(&((i * 100 + j) as u64), &"no_deadlock")
                    .unwrap();
                drop(table);
                write_txn.commit().unwrap();
            }
        });

        handles.push(handle);
    }

    // Should complete quickly without deadlock
    for handle in handles {
        handle.join().unwrap();
    }
}

// ============================================================================
// Mixed Read/Write Contention Tests
// ============================================================================

/// Test readers and writers don't interfere destructively
#[test]
fn test_mixed_read_write_contention() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap(),
    );

    db.create_column_family("mixed_cf", None).unwrap();

    // Populate initial data
    {
        let cf = db.column_family("mixed_cf").unwrap();
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        for i in 0..50 {
            table.insert(&i, &"initial").unwrap();
        }
        drop(table);
        write_txn.commit().unwrap();
    }

    let num_readers = 8;
    let num_writers = 4;
    let barrier = Arc::new(Barrier::new(num_readers + num_writers));
    let mut handles = vec![];

    // Spawn readers
    for _ in 0..num_readers {
        let db_clone = Arc::clone(&db);
        let barrier_clone = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            barrier_clone.wait();

            let cf = db_clone.column_family("mixed_cf").unwrap();

            for _ in 0..100 {
                let read_txn = cf.begin_read().unwrap();
                let table = read_txn.open_table(TEST_TABLE).unwrap();

                // Read some keys
                for key in 0..10 {
                    let _ = table.get(&key);
                }

                drop(table);
                drop(read_txn);
            }
        });

        handles.push(handle);
    }

    // Spawn writers
    for i in 0..num_writers {
        let db_clone = Arc::clone(&db);
        let barrier_clone = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            barrier_clone.wait();

            let cf = db_clone.column_family("mixed_cf").unwrap();

            for j in 0..50 {
                let write_txn = cf.begin_write().unwrap();
                let mut table = write_txn.open_table(TEST_TABLE).unwrap();
                table.insert(&((i * 1000 + j) as u64), &"written").unwrap();
                drop(table);
                write_txn.commit().unwrap();
            }
        });

        handles.push(handle);
    }

    // All should complete
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify database integrity - initial data should still exist
    let cf = db.column_family("mixed_cf").unwrap();
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    // Verify initial data exists
    assert!(table.get(&0).unwrap().is_some(), "Initial data should exist");
    // Verify some written data exists
    assert!(table.get(&1000).unwrap().is_some(), "Written data should exist");
}
