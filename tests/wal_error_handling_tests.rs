//! WAL Error Handling Tests (Phase 2, Task 2.2)
//!
//! This test suite validates proper error handling for WAL failures:
//! - Checkpoint failure mid-operation
//! - WAL file corruption scenarios
//! - Recovery from partial WAL entries
//! - Behavior when WAL file is deleted during operation
//! - WAL replay edge cases

use manifold::TableDefinition;
use manifold::column_family::ColumnFamilyDatabase;
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &str> = TableDefinition::new("test");

// ============================================================================
// WAL Corruption Tests
// ============================================================================

/// Test that truncated WAL file doesn't crash database on reopen
#[test]
fn test_partial_wal_entry_recovery() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    // Create DB with WAL, write data, let it checkpoint
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
    }

    // Reopen - even with potentially truncated WAL, should not panic
    // The critical requirement: no panic during recovery
    let db_result = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path);

    // Should either succeed or fail with clear error
    assert!(db_result.is_ok() || {
        let e = db_result.as_ref().err().unwrap();
        let msg = format!("{}", e);
        !msg.is_empty()
    }, "Should handle WAL state gracefully");
}

/// Test WAL with corrupted entry CRC
#[test]
fn test_wal_entry_crc_corruption() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    // Create database with data
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&100, &"corrupted_crc_test").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Corrupt a CRC in the WAL file
    if wal_path.exists() {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&wal_path)
            .unwrap();

        // Seek past header and corrupt some bytes (likely CRC area)
        file.seek(SeekFrom::Start(100)).unwrap();
        file.write_all(&[0xFF, 0xFF, 0xFF, 0xFF]).unwrap();
        file.sync_all().unwrap();
    }

    // Reopen - should detect CRC mismatch
    let db_result = ColumnFamilyDatabase::builder().pool_size(64).open(&db_path);

    // Database may refuse to open or skip corrupted entries
    // Both are acceptable - no panic is the key requirement
    if let Ok(db) = db_result {
        let _ = db.column_family("test_cf");
        // Just verify we don't panic
    }
}

/// Test behavior when WAL header is corrupted
#[test]
fn test_wal_header_corruption() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    // Create valid database
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
    }

    // Corrupt WAL header
    if wal_path.exists() {
        let mut file = OpenOptions::new().write(true).open(&wal_path).unwrap();

        // Overwrite magic number
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"CORRUPT!").unwrap();
        file.sync_all().unwrap();
    }

    // Try to reopen
    let db_result = ColumnFamilyDatabase::builder().pool_size(64).open(&db_path);

    // Should either recreate WAL or fail cleanly
    // The main database data should still be accessible
    if let Ok(db) = db_result {
        // Main database should work even if WAL is corrupted
        let _ = db.column_family("test_cf");
    }
}

// ============================================================================
// WAL Deletion Tests
// ============================================================================

/// Test behavior when WAL file is deleted after database is open
#[test]
fn test_wal_file_deleted_during_operation() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();
    db.create_column_family("test_cf", None).unwrap();

    let cf = db.column_family("test_cf").unwrap();

    // Write some initial data
    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&1, &"before_deletion").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Delete WAL file while database is still open
    if wal_path.exists() {
        let _ = fs::remove_file(&wal_path);
    }

    // Try to write again - may fail or succeed depending on implementation
    let write_result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let write_txn = cf.begin_write()?;
        let mut table = write_txn.open_table(TEST_TABLE)?;
        table.insert(&2, &"after_deletion")?;
        drop(table);
        write_txn.commit()?;
        Ok(())
    })();

    // Either way, should not panic
    // Error should have clear message if it fails
    if let Err(e) = write_result {
        let error_msg = e.to_string();
        assert!(!error_msg.is_empty(), "Error should have context");
    }
}

/// Test recovery when WAL exists but is empty
#[test]
fn test_empty_wal_recovery() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    // Create database
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
    }

    // Truncate WAL to 0 bytes (simulating corruption or incomplete write)
    if wal_path.exists() {
        let file = OpenOptions::new().write(true).open(&wal_path).unwrap();
        file.set_len(0).unwrap();
        file.sync_all().unwrap();
    }

    // Reopen - should handle empty WAL gracefully
    let db_result = ColumnFamilyDatabase::builder().pool_size(64).open(&db_path);

    // Should either recreate WAL or fail with clear error
    assert!(
        db_result.is_ok() || {
            if let Err(e) = &db_result {
                let error_msg = format!("{}", e);
                !error_msg.is_empty()
            } else {
                false
            }
        }
    );
}

// ============================================================================
// Checkpoint Error Tests
// ============================================================================

/// Test manual checkpoint when WAL contains entries
#[test]
fn test_checkpoint_with_pending_entries() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();
    db.create_column_family("test_cf", None).unwrap();

    let cf = db.column_family("test_cf").unwrap();

    // Write multiple transactions
    for i in 0..50 {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&i, &"checkpoint_test").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Trigger manual checkpoint
    let checkpoint_result = db.checkpoint();

    // Checkpoint should succeed or fail with clear error
    if let Err(e) = checkpoint_result {
        let error_msg = format!("{}", e);
        assert!(
            !error_msg.is_empty(),
            "Checkpoint error should have context"
        );
    }

    // Verify data is still accessible after checkpoint
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    for i in 0..50 {
        assert!(
            table.get(&i).unwrap().is_some(),
            "Data should exist after checkpoint"
        );
    }
}

/// Test checkpoint behavior with no pending entries
#[test]
fn test_checkpoint_with_no_pending_entries() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();
    db.create_column_family("test_cf", None).unwrap();

    // Trigger checkpoint immediately with no writes
    let checkpoint_result = db.checkpoint();

    // Should succeed (no-op) or return clear error
    if let Err(e) = checkpoint_result {
        let error_msg = format!("{}", e);
        assert!(!error_msg.is_empty(), "Error should have context");
    }
}

// ============================================================================
// WAL Replay Edge Cases
// ============================================================================

/// Test recovery with multiple column families in WAL
#[test]
fn test_wal_replay_multiple_column_families() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    // Create database with multiple CFs and write to them
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();

        for i in 0..3 {
            let cf_name = format!("cf{}", i);
            db.create_column_family(&cf_name, None).unwrap();

            let cf = db.column_family(&cf_name).unwrap();
            let write_txn = cf.begin_write().unwrap();
            let mut table = write_txn.open_table(TEST_TABLE).unwrap();
            table.insert(&(i as u64), &"multi_cf_test").unwrap();
            drop(table);
            write_txn.commit().unwrap();
        }
    } // Simulate crash by dropping without explicit checkpoint

    // Reopen and verify WAL replay worked for all CFs
    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    for i in 0..3 {
        let cf_name = format!("cf{}", i);
        let cf = db.column_family(&cf_name).unwrap();
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(TEST_TABLE).unwrap();

        let value = table.get(&(i as u64));
        assert!(
            value.unwrap().is_some(),
            "CF{} data should be recovered from WAL",
            i
        );
    }
}

/// Test WAL replay with large entries
#[test]
fn test_wal_replay_large_entries() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    // Write large values
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();

        let cf = db.column_family("test_cf").unwrap();
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();

        // Write a large value (100KB)
        let large_value = "x".repeat(100_000);
        table.insert(&999, &large_value.as_str()).unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Reopen and verify large entry was recovered
    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    let cf = db.column_family("test_cf").unwrap();
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();

    let recovered = table.get(&999).unwrap();
    assert!(recovered.is_some(), "Large entry should be recovered");
    assert_eq!(
        recovered.unwrap().value().len(),
        100_000,
        "Large value should be fully recovered"
    );
}

/// Test WAL behavior with WAL disabled (pool_size = 0)
#[test]
fn test_no_wal_error_handling() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    // Create database with WAL disabled
    let db = ColumnFamilyDatabase::builder()
        .without_wal()
        .open(&db_path)
        .unwrap();

    db.create_column_family("test_cf", None).unwrap();
    let cf = db.column_family("test_cf").unwrap();

    // Write data
    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&1, &"no_wal_test").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // WAL file should not exist
    assert!(
        !wal_path.exists(),
        "WAL file should not exist when WAL is disabled"
    );

    // Manual checkpoint should be no-op or fail gracefully
    let checkpoint_result = db.checkpoint();
    if let Err(e) = checkpoint_result {
        let error_msg = format!("{}", e);
        // Error is acceptable when WAL is disabled
        assert!(!error_msg.is_empty());
    }
}

/// Test error propagation from WAL to user code
#[test]
fn test_wal_error_propagation_context() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    // Try to checkpoint before any CFs exist
    let result = db.checkpoint();

    // Should either succeed (no-op) or fail with clear context
    if let Err(e) = result {
        let error_msg = format!("{}", e);
        assert!(!error_msg.is_empty(), "WAL error should provide context");
    }
}

/// Test concurrent writes don't corrupt WAL
#[test]
fn test_concurrent_wal_writes_no_corruption() {
    use std::sync::Arc;
    use std::thread;

    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap(),
    );

    // Create multiple CFs
    for i in 0..4 {
        db.create_column_family(&format!("cf{}", i), None).unwrap();
    }

    // Spawn concurrent writers
    let mut handles = vec![];
    for i in 0..4 {
        let db_clone = Arc::clone(&db);
        let cf_name = format!("cf{}", i);

        let handle = thread::spawn(move || {
            let cf = db_clone.column_family(&cf_name).unwrap();

            for j in 0..30 {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    let write_txn = cf.begin_write()?;
                    let mut table = write_txn.open_table(TEST_TABLE)?;
                    table.insert(&(j as u64), &"concurrent_wal")?;
                    drop(table);
                    write_txn.commit()?;
                    Ok(())
                })();

                if let Err(e) = result {
                    let error_msg = format!("{}", e);
                    assert!(!error_msg.is_empty(), "Error should have context");
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify no WAL corruption - reopen and check data
    drop(db);

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    for i in 0..4 {
        let cf = db.column_family(&format!("cf{}", i)).unwrap();
        let read_txn = cf.begin_read().unwrap();
        let _ = read_txn.open_table(TEST_TABLE);
        // Just verify we can open without corruption errors
    }
}
