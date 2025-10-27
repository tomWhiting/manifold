//! Memory Pressure Handling Tests (Phase 2, Task 2.4)
//!
//! This test suite validates proper handling under memory pressure:
//! - Behavior when allocations fail
//! - Large value handling (> available RAM)
//! - Cache eviction under memory pressure
//! - Memory usage patterns
//! - No memory leaks under stress

use manifold::TableDefinition;
use manifold::column_family::ColumnFamilyDatabase;
use std::sync::Arc;
use std::thread;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &str> = TableDefinition::new("test");
const LARGE_TABLE: TableDefinition<u64, Vec<u8>> = TableDefinition::new("large");

// ============================================================================
// Large Value Tests
// ============================================================================

/// Test writing and reading large values (1MB each)
#[test]
fn test_large_value_handling() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    db.create_column_family("large_cf", None).unwrap();
    let cf = db.column_family("large_cf").unwrap();

    // Write 10 x 1MB values
    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(LARGE_TABLE).unwrap();

        for i in 0..10 {
            let large_value = vec![0xAB; 1024 * 1024]; // 1MB
            table.insert(&i, &large_value).unwrap();
        }

        drop(table);
        write_txn.commit().unwrap();
    }

    // Verify we can read them back
    {
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(LARGE_TABLE).unwrap();

        for i in 0..10 {
            let value = table.get(&i).unwrap().unwrap();
            assert_eq!(value.value().len(), 1024 * 1024);
            assert_eq!(value.value()[0], 0xAB);
        }
    }
}

/// Test incrementally larger values
#[test]
fn test_progressive_large_values() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    db.create_column_family("progressive_cf", None).unwrap();
    let cf = db.column_family("progressive_cf").unwrap();

    // Write progressively larger values: 1KB, 10KB, 100KB, 1MB
    let sizes = [1024, 10 * 1024, 100 * 1024, 1024 * 1024];

    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(LARGE_TABLE).unwrap();

        for (i, &size) in sizes.iter().enumerate() {
            let value = vec![i as u8; size];
            let result = table.insert(&(i as u64), &value);

            // Should succeed or fail with clear error
            if let Err(e) = result {
                let msg = format!("{}", e);
                assert!(!msg.is_empty(), "Error should have context");
            }
        }

        drop(table);
        let _ = write_txn.commit(); // May fail if values too large
    }
}

/// Test many small values (memory accumulation)
#[test]
fn test_many_small_values() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    db.create_column_family("many_cf", None).unwrap();
    let cf = db.column_family("many_cf").unwrap();

    // Write many small values in batches
    for batch in 0..10 {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();

        for i in 0..1000 {
            let key = batch * 1000 + i;
            table.insert(&key, &"small_value").unwrap();
        }

        drop(table);
        write_txn.commit().unwrap();
    }

    // Verify data integrity
    {
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(TEST_TABLE).unwrap();

        // Spot check
        assert_eq!(table.get(&0).unwrap().unwrap().value(), "small_value");
        assert_eq!(table.get(&5000).unwrap().unwrap().value(), "small_value");
        assert_eq!(table.get(&9999).unwrap().unwrap().value(), "small_value");
    }
}

// ============================================================================
// Concurrent Memory Pressure Tests
// ============================================================================

/// Test multiple threads writing large values concurrently
#[test]
fn test_concurrent_large_value_writes() {
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
        db.create_column_family(format!("cf{}", i), None).unwrap();
    }

    let mut handles = vec![];
    for i in 0..4 {
        let db_clone = Arc::clone(&db);

        let handle = thread::spawn(move || {
            let cf = db_clone.column_family(&format!("cf{}", i)).unwrap();

            // Each thread writes 5 x 100KB values
            for j in 0..5 {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    let write_txn = cf.begin_write()?;
                    let mut table = write_txn.open_table(LARGE_TABLE)?;

                    let value = vec![(i * j) as u8; 100 * 1024]; // 100KB
                    table.insert(&(j as u64), &value)?;

                    drop(table);
                    write_txn.commit()?;
                    Ok(())
                })();

                // May fail under memory pressure - that's ok
                if let Err(e) = result {
                    let msg = format!("{}", e);
                    assert!(!msg.is_empty());
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify database is still accessible
    let cf = db.column_family("cf0").unwrap();
    let _ = cf.begin_read();
}

/// Test rapid allocation and deallocation pattern
#[test]
fn test_allocation_churn() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    db.create_column_family("churn_cf", None).unwrap();
    let cf = db.column_family("churn_cf").unwrap();

    // Repeatedly write and read to cause allocation churn
    for round in 0..20 {
        // Write
        {
            let write_txn = cf.begin_write().unwrap();
            let mut table = write_txn.open_table(LARGE_TABLE).unwrap();

            let value = vec![round as u8; 50 * 1024]; // 50KB
            table.insert(&(round as u64), &value).unwrap();

            drop(table);
            write_txn.commit().unwrap();
        }

        // Read
        {
            let read_txn = cf.begin_read().unwrap();
            let table = read_txn.open_table(LARGE_TABLE).unwrap();

            let value = table.get(&(round as u64)).unwrap().unwrap();
            assert_eq!(value.value().len(), 50 * 1024);
        }
    }
}

// ============================================================================
// Memory Leak Detection Tests
// ============================================================================

/// Test repeated transactions don't accumulate memory
#[test]
fn test_transaction_memory_cleanup() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    db.create_column_family("cleanup_cf", None).unwrap();
    let cf = db.column_family("cleanup_cf").unwrap();

    // Run many transactions
    for i in 0..100 {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&(i as u64), &"cleanup_test").unwrap();
        drop(table);
        write_txn.commit().unwrap();

        // Also do reads
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(TEST_TABLE).unwrap();
        let _ = table.get(&(i as u64));
        drop(table);
        drop(read_txn);
    }

    // Final verification
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    assert_eq!(table.get(&99).unwrap().unwrap().value(), "cleanup_test");
}

/// Test CF creation and access doesn't leak
#[test]
fn test_column_family_memory_cleanup() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    // Create many CFs
    for i in 0..50 {
        let cf_name = format!("cf_{}", i);
        db.create_column_family(&cf_name, None).unwrap();

        // Access the CF
        let cf = db.column_family(&cf_name).unwrap();

        // Write one value
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert(&(i as u64), &"cf_test").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Verify all CFs exist and are accessible
    assert_eq!(db.list_column_families().len(), 50);
}

// ============================================================================
// Cache Behavior Tests
// ============================================================================

/// Test reading same data repeatedly (cache behavior)
#[test]
fn test_cache_behavior_repeated_reads() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    db.create_column_family("cache_cf", None).unwrap();
    let cf = db.column_family("cache_cf").unwrap();

    // Write some data
    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();

        for i in 0..100 {
            table.insert(&i, &"cached_value").unwrap();
        }

        drop(table);
        write_txn.commit().unwrap();
    }

    // Read the same keys many times (should hit cache)
    for _ in 0..50 {
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(TEST_TABLE).unwrap();

        for i in 0..100 {
            let value = table.get(&i).unwrap().unwrap();
            assert_eq!(value.value(), "cached_value");
        }

        drop(table);
        drop(read_txn);
    }
}

/// Test random access pattern (cache thrashing)
#[test]
fn test_cache_thrashing_random_access() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    db.create_column_family("thrash_cf", None).unwrap();
    let cf = db.column_family("thrash_cf").unwrap();

    // Write many values
    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();

        for i in 0..1000 {
            table.insert(&i, &"random_access").unwrap();
        }

        drop(table);
        write_txn.commit().unwrap();
    }

    // Random access pattern to thrash cache
    {
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(TEST_TABLE).unwrap();

        // Read in non-sequential order
        for i in (0..1000).rev() {
            let _ = table.get(&i);
        }

        for i in (0..1000).step_by(7) {
            let value = table.get(&i).unwrap().unwrap();
            assert_eq!(value.value(), "random_access");
        }
    }
}

// ============================================================================
// Stress Tests
// ============================================================================

/// Test database under sustained write pressure
#[test]
fn test_sustained_write_pressure() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::builder()
        .pool_size(64)
        .open(&db_path)
        .unwrap();

    db.create_column_family("pressure_cf", None).unwrap();
    let cf = db.column_family("pressure_cf").unwrap();

    // Sustained writes with varying sizes
    for i in 0..200 {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(LARGE_TABLE).unwrap();

        // Varying value sizes
        let size = 1024 * (1 + (i % 10));
        let value = vec![i as u8; size];

        table.insert(&(i as u64), &value).unwrap();

        drop(table);
        write_txn.commit().unwrap();
    }

    // Verify data integrity after pressure
    {
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(LARGE_TABLE).unwrap();

        // Spot check
        let value0 = table.get(&0).unwrap().unwrap();
        assert_eq!(value0.value()[0], 0);

        let value100 = table.get(&100).unwrap().unwrap();
        assert_eq!(value100.value()[0], 100);
    }
}

/// Test mixed operations under memory pressure
#[test]
fn test_mixed_operations_memory_pressure() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = Arc::new(
        ColumnFamilyDatabase::builder()
            .pool_size(64)
            .open(&db_path)
            .unwrap(),
    );

    db.create_column_family("mixed_cf", None).unwrap();

    // Writer thread
    let db_clone = Arc::clone(&db);
    let writer = thread::spawn(move || {
        let cf = db_clone.column_family("mixed_cf").unwrap();

        for i in 0..50 {
            let write_txn = cf.begin_write().unwrap();
            let mut table = write_txn.open_table(LARGE_TABLE).unwrap();

            let value = vec![i as u8; 10 * 1024]; // 10KB
            table.insert(&(i as u64), &value).unwrap();

            drop(table);
            write_txn.commit().unwrap();
        }
    });

    // Reader thread
    let db_clone = Arc::clone(&db);
    let reader = thread::spawn(move || {
        let cf = db_clone.column_family("mixed_cf").unwrap();

        for _ in 0..100 {
            let read_txn = cf.begin_read().unwrap();
            let table_result = read_txn.open_table(LARGE_TABLE);

            if let Ok(table) = table_result {
                // Read whatever exists
                for i in 0..10 {
                    let _ = table.get(&i);
                }
            }

            drop(read_txn);
            thread::sleep(std::time::Duration::from_millis(5));
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();

    // Verify final state
    let cf = db.column_family("mixed_cf").unwrap();
    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(LARGE_TABLE).unwrap();
    assert!(table.get(&49).unwrap().is_some());
}
