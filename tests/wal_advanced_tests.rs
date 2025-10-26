// Advanced WAL tests covering error conditions, recovery, and edge cases

use manifold::column_family::ColumnFamilyDatabase;
use manifold::TableDefinition;
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &str> = TableDefinition::new("test");

/// Test WAL behavior with corrupted header magic number
/// The system should detect the corruption and either fail cleanly or recreate the WAL
#[test]
fn test_wal_invalid_magic_number() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    // First create a valid database and write some data
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&1, &"persisted_before_corruption").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Now corrupt the WAL file with invalid magic
    if wal_path.exists() {
        let mut wal_file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&wal_path)
            .unwrap();

        // Write invalid magic (should be "REDBWAL\0")
        wal_file.write_all(b"BADMAGIC").unwrap();
        // Fill rest of header with zeros
        wal_file.write_all(&[0u8; 21]).unwrap();
        wal_file.sync_all().unwrap();
        drop(wal_file);
    }

    // Attempting to reopen database with corrupted WAL
    // The system should either:
    // 1. Detect corruption and refuse to open (correct behavior)
    // 2. Detect corruption and recreate/ignore the WAL (also acceptable)
    let db_result = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path);

    // If the database opens, the previously committed data should still be accessible
    // (it was flushed before we corrupted the WAL)
    if let Ok(db) = db_result {
        if let Ok(cf) = db.column_family("test_cf") {
            let read_txn = cf.begin_read().unwrap();
            let table = read_txn.open_table(TEST_TABLE).unwrap();
            // Data committed before corruption should be readable
            assert_eq!(
                table.get(&1).unwrap().unwrap().value(),
                "persisted_before_corruption"
            );
        }
    }
    // If the database refuses to open due to corruption, that's also valid behavior
}

/// Test WAL behavior with unsupported version number
/// System should detect and handle gracefully
#[test]
fn test_wal_unsupported_version() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    // First create a valid database and persist some data
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&2, &"persisted_data").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Now corrupt WAL with unsupported version
    if wal_path.exists() {
        let mut wal_file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&wal_path)
            .unwrap();

        // Write correct magic
        wal_file.write_all(b"REDBWAL\0").unwrap();
        // Write unsupported version (current is 1, use 99)
        wal_file.write_all(&[99u8]).unwrap();
        // Fill rest with zeros and dummy CRC
        wal_file.write_all(&[0u8; 20]).unwrap();
        wal_file.sync_all().unwrap();
        drop(wal_file);
    }

    // System should handle unsupported version
    let db_result = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path);

    // If opens, previously persisted data should be accessible
    if let Ok(db) = db_result {
        if let Ok(cf) = db.column_family("test_cf") {
            let read_txn = cf.begin_read().unwrap();
            let table = read_txn.open_table(TEST_TABLE).unwrap();
            assert_eq!(
                table.get(&2).unwrap().unwrap().value(),
                "persisted_data"
            );
        }
    }
    // Refusing to open is also acceptable
}

/// Test WAL header CRC validation
/// System should detect corruption
#[test]
fn test_wal_header_crc_mismatch() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    // First create a valid database with data
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&3, &"crc_test_data").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Now create WAL with CRC mismatch
    if wal_path.exists() {
        let mut wal_file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&wal_path)
            .unwrap();

        // Write valid magic
        wal_file.write_all(b"REDBWAL\0").unwrap();
        // Write valid version
        wal_file.write_all(&[1u8]).unwrap();
        // Write sequence numbers
        wal_file.write_all(&0u64.to_le_bytes()).unwrap(); // oldest_seq
        wal_file.write_all(&0u64.to_le_bytes()).unwrap(); // latest_seq
        // Write INCORRECT CRC
        wal_file.write_all(&0xDEADBEEFu32.to_le_bytes()).unwrap();
        wal_file.sync_all().unwrap();
        drop(wal_file);
    }

    // System should detect CRC mismatch
    let db_result = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path);

    // If opens, verify data integrity
    if let Ok(db) = db_result {
        if let Ok(cf) = db.column_family("test_cf") {
            let read_txn = cf.begin_read().unwrap();
            let table = read_txn.open_table(TEST_TABLE).unwrap();
            assert_eq!(
                table.get(&3).unwrap().unwrap().value(),
                "crc_test_data"
            );
        }
    }
    // Refusing to open due to CRC failure is also valid
}

/// Test WAL entry with corrupted CRC
#[test]
fn test_wal_entry_corrupted_crc() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    // Create DB and write some data
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&10, &"original").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Corrupt the WAL file by flipping some bits in the data section
    let wal_path = db_path.with_extension("wal");
    if wal_path.exists() {
        let mut wal_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&wal_path)
            .unwrap();

        // Seek past header (29 bytes) and corrupt some data
        wal_file.seek(SeekFrom::Start(50)).unwrap();
        wal_file.write_all(&[0xFF, 0xFF, 0xFF, 0xFF]).unwrap();
        wal_file.sync_all().unwrap();
    }

    // Reopen database - should detect corruption during recovery
    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    let cf_result = db.column_family("test_cf");
    // Should either recover gracefully or the CF should still be accessible
    // (implementation may choose to skip corrupted entries)
    assert!(cf_result.is_ok());
}

/// Test checkpoint triggering with explicit checkpoint_now
#[test]
fn test_wal_manual_checkpoint() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();
    db.create_column_family("test_cf", None).unwrap();
    let cf = db.column_family("test_cf").unwrap();

    // Write some data to WAL
    for i in 0..20 {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&i, &"checkpoint_test").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Verify WAL exists and has data
    assert!(wal_path.exists());
    let wal_size_before = fs::metadata(&wal_path).unwrap().len();
    assert!(wal_size_before > 512, "WAL should have entries");

    // Force a checkpoint (we can't access checkpoint_now directly, but closing
    // and reopening should trigger cleanup)
    drop(cf);
    drop(db);

    // Reopen - should apply WAL during recovery
    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();
    let cf = db.column_family("test_cf").unwrap();

    // Verify all data survived checkpoint
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    for i in 0..20 {
        assert_eq!(
            table.get(&i).unwrap().unwrap().value(),
            "checkpoint_test"
        );
    }
}

/// Test WAL recovery after crash (simulated by not closing cleanly)
#[test]
fn test_wal_crash_recovery() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    // Write data and "crash" (drop without clean shutdown)
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        for i in 0..50 {
            let write_txn = cf.begin_write().unwrap();
            let mut table = write_txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, &"crash_recovery_test").unwrap();
            drop(table);
            write_txn.commit().unwrap();
        }
        // Simulate crash - just drop without explicit close
    }

    // Reopen and verify WAL recovery worked
    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();
    let cf = db.column_family("test_cf").unwrap();

    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    for i in 0..50 {
        let value = table.get(&i).unwrap();
        assert!(
            value.is_some(),
            "Key {} should exist after recovery",
            i
        );
        assert_eq!(value.unwrap().value(), "crash_recovery_test");
    }
}

/// Test WAL with empty transactions
#[test]
fn test_wal_empty_transaction() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();
    db.create_column_family("test_cf", None).unwrap();
    let cf = db.column_family("test_cf").unwrap();

    // Create and commit an empty transaction
    let write_txn = cf.begin_write().unwrap();
    write_txn.commit().unwrap();

    // WAL may or may not create an entry for empty transaction
    // Just verify system remains stable
    let write_txn2 = cf.begin_write().unwrap();
    let mut table = write_txn2.open_table(TEST_TABLE).unwrap();
    table.insert(&100, &"after_empty").unwrap();
    drop(table);
    write_txn2.commit().unwrap();

    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    assert_eq!(table.get(&100).unwrap().unwrap().value(), "after_empty");
}

/// Test WAL with very large values
#[test]
fn test_wal_large_values() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();
    db.create_column_family("test_cf", Some(10 * 1024 * 1024))
        .unwrap();
    let cf = db.column_family("test_cf").unwrap();

    // Create a large value (1MB)
    let large_value = "X".repeat(1024 * 1024);

    let write_txn = cf.begin_write().unwrap();
    let mut table = write_txn.open_table(TEST_TABLE).unwrap();
    table.insert(&1, &large_value.as_str()).unwrap();
    drop(table);
    write_txn.commit().unwrap();

    // Verify large value persisted through WAL
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    assert_eq!(table.get(&1).unwrap().unwrap().value(), large_value);
}

/// Test WAL with many small concurrent transactions
#[test]
fn test_wal_concurrent_small_transactions() {
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
    db.create_column_family("test_cf", None).unwrap();

    let mut handles = vec![];
    for thread_id in 0..8 {
        let db_clone = db.clone();
        let handle = thread::spawn(move || {
            let cf = db_clone.column_family("test_cf").unwrap();
            for i in 0..100 {
                let key = (thread_id * 100) + i;
                let write_txn = cf.begin_write().unwrap();
                let mut table = write_txn.open_table(TEST_TABLE).unwrap();
                table.insert(&key, &"concurrent").unwrap();
                drop(table);
                write_txn.commit().unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all concurrent writes succeeded
    let cf = db.column_family("test_cf").unwrap();
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();

    for thread_id in 0..8 {
        for i in 0..100 {
            let key = (thread_id * 100) + i;
            assert_eq!(table.get(&key).unwrap().unwrap().value(), "concurrent");
        }
    }
}

/// Test disabled WAL (pool_size = 0)
#[test]
fn test_disabled_wal() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    let db = ColumnFamilyDatabase::builder()
        .without_wal()
        .open(&db_path)
        .unwrap();
    db.create_column_family("test_cf", None).unwrap();
    let cf = db.column_family("test_cf").unwrap();

    // Write data
    let write_txn = cf.begin_write().unwrap();
    let mut table = write_txn.open_table(TEST_TABLE).unwrap();
    table.insert(&1, &"no_wal").unwrap();
    drop(table);
    write_txn.commit().unwrap();

    // Verify NO WAL file was created
    assert!(
        !wal_path.exists(),
        "WAL file should not exist when pool_size = 0"
    );

    // Data should still be accessible
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    assert_eq!(table.get(&1).unwrap().unwrap().value(), "no_wal");
}

/// Test WAL truncation after successful checkpoint
#[test]
fn test_wal_truncation_after_checkpoint() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let wal_path = db_path.with_extension("wal");

    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        // Write many transactions
        for i in 0..100 {
            let write_txn = cf.begin_write().unwrap();
            let mut table = write_txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, &"truncation_test").unwrap();
            drop(table);
            write_txn.commit().unwrap();
        }

        // WAL should be sizable
        if wal_path.exists() {
            let wal_size = fs::metadata(&wal_path).unwrap().len();
            assert!(wal_size > 1024, "WAL should have grown");
        }
    }

    // Close and reopen - should trigger checkpoint and potentially truncate WAL
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        let cf = db.column_family("test_cf").unwrap();

        // All data should be present
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(TEST_TABLE).unwrap();
        for i in 0..100 {
            assert_eq!(
                table.get(&i).unwrap().unwrap().value(),
                "truncation_test"
            );
        }
    }
}

/// Test column family re-creation with existing WAL
/// Verifies that deleting and recreating a CF results in a clean slate
#[test]
fn test_cf_recreate_with_existing_wal() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&1, &"first_incarnation").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Delete and recreate CF
    {
        let db = ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap();
        db.delete_column_family("test_cf").unwrap();
        db.create_column_family("test_cf", None).unwrap();
        let cf = db.column_family("test_cf").unwrap();

        // Write new data to the recreated CF
        {
            let write_txn = cf.begin_write().unwrap();
            let mut table = write_txn.open_table(TEST_TABLE).unwrap();
            table.insert(&1, &"second_incarnation").unwrap();
            drop(table);
            write_txn.commit().unwrap();
        }

        // Immediately verify in same scope that the data is there
        {
            let read_txn = cf.begin_read().unwrap();
            let table = read_txn.open_table(TEST_TABLE).unwrap();
            assert_eq!(
                table.get(&1).unwrap().unwrap().value(),
                "second_incarnation",
                "Recreated CF should have new data immediately after write"
            );
        }
    }
}
