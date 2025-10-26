use manifold::column_family::{ColumnFamilyDatabase, ColumnFamilyError};
use manifold::{ReadableTableMetadata, TableDefinition};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use tempfile::NamedTempFile;

const TEST_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("test");
const META_TABLE: TableDefinition<u64, &str> = TableDefinition::new("metadata");
const DATA_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("data");

// Integration Tests

#[test]
fn test_create_and_list_column_families() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();

    assert_eq!(db.list_column_families().len(), 0);

    db.create_column_family("cf1", Some(10 * 1024 * 1024))
        .unwrap();
    db.create_column_family("cf2", Some(20 * 1024 * 1024))
        .unwrap();
    db.create_column_family("cf3", Some(30 * 1024 * 1024))
        .unwrap();

    let families = db.list_column_families();
    assert_eq!(families.len(), 3);
    assert!(families.contains(&"cf1".to_string()));
    assert!(families.contains(&"cf2".to_string()));
    assert!(families.contains(&"cf3".to_string()));
}

#[test]
fn test_duplicate_column_family_name_fails() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();

    db.create_column_family("duplicate", None).unwrap();
    let result = db.create_column_family("duplicate", None);

    assert!(matches!(result, Err(ColumnFamilyError::AlreadyExists(_))));
}

#[test]
fn test_get_nonexistent_column_family_fails() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();

    let result = db.column_family("nonexistent");
    assert!(matches!(result, Err(ColumnFamilyError::NotFound(_))));
}

#[test]
fn test_persistence_across_reopens() {
    let tmpfile = NamedTempFile::new().unwrap();
    let path = tmpfile.path().to_path_buf();

    {
        let db = ColumnFamilyDatabase::open(&path).unwrap();
        db.create_column_family("persistent1", Some(50 * 1024 * 1024))
            .unwrap();
        db.create_column_family("persistent2", Some(100 * 1024 * 1024))
            .unwrap();

        let cf = db.column_family("persistent1").unwrap();
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            let data = vec![1u8, 2, 3, 4, 5];
            table.insert(&42, data.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }

    let db = ColumnFamilyDatabase::open(&path).unwrap();
    let families = db.list_column_families();
    assert_eq!(families.len(), 2);
    assert!(families.contains(&"persistent1".to_string()));
    assert!(families.contains(&"persistent2".to_string()));

    let cf = db.column_family("persistent1").unwrap();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(TEST_TABLE).unwrap();
    let value = table.get(&42).unwrap().unwrap();
    assert_eq!(value.value(), &[1u8, 2, 3, 4, 5]);
}

#[test]
fn test_concurrent_writes_to_different_cfs_succeed() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    db.create_column_family("cf_a", Some(50 * 1024 * 1024))
        .unwrap();
    db.create_column_family("cf_b", Some(50 * 1024 * 1024))
        .unwrap();

    let db1 = db.clone();
    let handle1 = thread::spawn(move || {
        let cf = db1.column_family("cf_a").unwrap();
        let data = vec![0xAA; 512];

        for i in 0..100 {
            let txn = cf.begin_write().unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                table.insert(&i, data.as_slice()).unwrap();
            }
            txn.commit().unwrap();
        }
    });

    let db2 = db.clone();
    let handle2 = thread::spawn(move || {
        let cf = db2.column_family("cf_b").unwrap();
        let data = vec![0xBB; 512];

        for i in 0..100 {
            let txn = cf.begin_write().unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                table.insert(&i, data.as_slice()).unwrap();
            }
            txn.commit().unwrap();
        }
    });

    handle1.join().unwrap();
    handle2.join().unwrap();

    let cf_a = db.column_family("cf_a").unwrap();
    let txn_a = cf_a.begin_read().unwrap();
    let table_a = txn_a.open_table(TEST_TABLE).unwrap();
    assert_eq!(table_a.len().unwrap(), 100u64);
    let val_a = table_a.get(&0).unwrap().unwrap();
    assert_eq!(val_a.value()[0], 0xAA);

    let cf_b = db.column_family("cf_b").unwrap();
    let txn_b = cf_b.begin_read().unwrap();
    let table_b = txn_b.open_table(TEST_TABLE).unwrap();
    assert_eq!(table_b.len().unwrap(), 100u64);
    let val_b = table_b.get(&0).unwrap().unwrap();
    assert_eq!(val_b.value()[0], 0xBB);
}

#[test]
fn test_concurrent_writes_to_same_cf_serialize() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    db.create_column_family("shared", Some(100 * 1024 * 1024))
        .unwrap();

    let success_count = Arc::new(AtomicU64::new(0));

    let mut handles = vec![];
    for thread_id in 0..4 {
        let db_clone = db.clone();
        let counter = success_count.clone();

        let handle = thread::spawn(move || {
            let cf = db_clone.column_family("shared").unwrap();
            let data = vec![thread_id as u8; 128];

            for i in 0..25 {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    let key = (thread_id * 100 + i) as u64;
                    table.insert(&key, data.as_slice()).unwrap();
                }
                txn.commit().unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(success_count.load(Ordering::SeqCst), 100);

    let cf = db.column_family("shared").unwrap();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(TEST_TABLE).unwrap();
    assert_eq!(table.len().unwrap(), 100u64);
}

#[test]
fn test_multi_table_atomic_transaction() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();

    db.create_column_family("multi_table", Some(100 * 1024 * 1024))
        .unwrap();

    let cf = db.column_family("multi_table").unwrap();

    let txn = cf.begin_write().unwrap();
    {
        let mut meta_table = txn.open_table(META_TABLE).unwrap();
        let mut data_table = txn.open_table(DATA_TABLE).unwrap();

        meta_table.insert(&1, "user_alice").unwrap();
        meta_table.insert(&2, "user_bob").unwrap();

        data_table.insert(&1, b"alice_data".as_slice()).unwrap();
        data_table.insert(&2, b"bob_data".as_slice()).unwrap();
    }
    txn.commit().unwrap();

    let read_txn = cf.begin_read().unwrap();
    let meta_table = read_txn.open_table(META_TABLE).unwrap();
    let data_table = read_txn.open_table(DATA_TABLE).unwrap();

    assert_eq!(meta_table.len().unwrap(), 2u64);
    assert_eq!(data_table.len().unwrap(), 2u64);

    assert_eq!(meta_table.get(&1).unwrap().unwrap().value(), "user_alice");
    assert_eq!(data_table.get(&1).unwrap().unwrap().value(), b"alice_data");
}

#[test]
fn test_delete_and_recreate_column_family() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();

    db.create_column_family("temp", Some(50 * 1024 * 1024))
        .unwrap();

    let cf = db.column_family("temp").unwrap();
    let txn = cf.begin_write().unwrap();
    {
        let mut table = txn.open_table(TEST_TABLE).unwrap();
        table.insert(&1, b"data".as_slice()).unwrap();
    }
    txn.commit().unwrap();

    db.delete_column_family("temp").unwrap();

    assert!(db.column_family("temp").is_err());

    db.create_column_family("temp", Some(50 * 1024 * 1024))
        .unwrap();

    let cf_new = db.column_family("temp").unwrap();
    let txn_write = cf_new.begin_write().unwrap();
    {
        let table_new = txn_write.open_table(TEST_TABLE).unwrap();
        assert_eq!(table_new.len().unwrap(), 0u64);
    }
    txn_write.commit().unwrap();
}

// Stress Tests

#[test]
fn stress_test_many_concurrent_writers() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    let num_cfs = 8;
    let writes_per_thread = 200;

    for i in 0..num_cfs {
        db.create_column_family(format!("stress_cf_{i}"), Some(50 * 1024 * 1024))
            .unwrap();
    }

    let mut handles = vec![];
    for i in 0..num_cfs {
        let db_clone = db.clone();
        let handle = thread::spawn(move || {
            let cf = db_clone.column_family(&format!("stress_cf_{i}")).unwrap();
            let data = vec![i as u8; 1024];

            for j in 0..writes_per_thread {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    table.insert(&(j as u64), data.as_slice()).unwrap();
                }
                txn.commit().unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    for i in 0..num_cfs {
        let cf = db.column_family(&format!("stress_cf_{i}")).unwrap();
        let txn = cf.begin_read().unwrap();
        let table = txn.open_table(TEST_TABLE).unwrap();
        assert_eq!(table.len().unwrap(), writes_per_thread as u64);
    }
}

#[test]
fn stress_test_readers_and_writers() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    db.create_column_family("rw_stress", Some(100 * 1024 * 1024))
        .unwrap();

    let cf = db.column_family("rw_stress").unwrap();
    let data = vec![0xCC; 512];

    let initial_txn = cf.begin_write().unwrap();
    {
        let mut table = initial_txn.open_table(TEST_TABLE).unwrap();
        for i in 0..1000 {
            table.insert(&i, data.as_slice()).unwrap();
        }
    }
    initial_txn.commit().unwrap();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let mut handles = vec![];

    for _ in 0..8 {
        let db_clone = db.clone();
        let flag = stop_flag.clone();
        let handle = thread::spawn(move || {
            let cf = db_clone.column_family("rw_stress").unwrap();
            let mut read_count = 0u64;

            while !flag.load(Ordering::Relaxed) {
                let txn = cf.begin_read().unwrap();
                let table = txn.open_table(TEST_TABLE).unwrap();

                for i in (0..1000).step_by(10) {
                    if let Some(val) = table.get(&i).unwrap() {
                        assert_eq!(val.value().len(), 512);
                        read_count += 1;
                    }
                }
            }

            read_count
        });
        handles.push(handle);
    }

    let db_writer = db.clone();
    let flag_writer = stop_flag.clone();
    let writer_handle = thread::spawn(move || {
        let cf = db_writer.column_family("rw_stress").unwrap();
        let mut write_count = 0u64;

        for _ in 0..50 {
            let txn = cf.begin_write().unwrap();
            {
                let mut table = txn.open_table(TEST_TABLE).unwrap();
                for i in 0..20 {
                    let key = 1000 + write_count * 20 + i;
                    table.insert(&key, data.as_slice()).unwrap();
                }
            }
            txn.commit().unwrap();
            write_count += 1;
            thread::sleep(Duration::from_millis(5));
        }

        flag_writer.store(true, Ordering::Relaxed);
        write_count
    });

    let writes = writer_handle.join().unwrap();
    assert_eq!(writes, 50);

    let mut total_reads = 0u64;
    for handle in handles {
        total_reads += handle.join().unwrap();
    }
    assert!(total_reads > 0);

    let cf_final = db.column_family("rw_stress").unwrap();
    let final_txn = cf_final.begin_read().unwrap();
    let final_table = final_txn.open_table(TEST_TABLE).unwrap();
    assert_eq!(final_table.len().unwrap(), 1000u64 + 50 * 20);
}

#[test]
fn stress_test_rapid_cf_creation_deletion() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    for iteration in 0..20 {
        let cf_name = format!("temp_{iteration}");

        db.create_column_family(&cf_name, Some(10 * 1024 * 1024))
            .unwrap();

        let cf = db.column_family(&cf_name).unwrap();
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            for i in 0..10 {
                table.insert(&i, b"data".as_slice()).unwrap();
            }
        }
        txn.commit().unwrap();

        db.delete_column_family(&cf_name).unwrap();

        assert!(db.column_family(&cf_name).is_err());
    }

    assert_eq!(db.list_column_families().len(), 0);
}

#[test]
fn stress_test_large_values() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();

    db.create_column_family("large", Some(512 * 1024 * 1024))
        .unwrap();

    let cf = db.column_family("large").unwrap();
    let large_value = vec![0x42; 1024 * 1024];

    for i in 0..10 {
        let txn = cf.begin_write().unwrap();
        {
            let mut table = txn.open_table(TEST_TABLE).unwrap();
            table.insert(&i, large_value.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }

    let read_txn = cf.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();

    assert_eq!(table.len().unwrap(), 10u64);

    for i in 0..10 {
        let value = table.get(&i).unwrap().unwrap();
        assert_eq!(value.value().len(), 1024 * 1024);
        assert_eq!(value.value()[0], 0x42);
        assert_eq!(value.value()[1024 * 1024 - 1], 0x42);
    }
}

#[test]
fn stress_test_auto_expansion_under_load() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    db.create_column_family("expanding", Some(32 * 1024))
        .unwrap();

    let num_threads = 4;
    let writes_per_thread = 100;
    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = db.clone();
        let handle = thread::spawn(move || {
            let cf = db_clone.column_family("expanding").unwrap();
            let data = vec![thread_id as u8; 2048];

            for i in 0..writes_per_thread {
                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    let key = (thread_id * 1000 + i) as u64;
                    table.insert(&key, data.as_slice()).unwrap();
                }
                txn.commit().unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let cf = db.column_family("expanding").unwrap();
    let txn = cf.begin_read().unwrap();
    let table = txn.open_table(TEST_TABLE).unwrap();

    assert_eq!(
        table.len().unwrap(),
        (num_threads * writes_per_thread) as u64
    );
}

#[test]
fn stress_test_data_integrity_verification() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = Arc::new(ColumnFamilyDatabase::open(tmpfile.path()).unwrap());

    let num_cfs = 4;
    let entries_per_cf = 500;

    for i in 0..num_cfs {
        db.create_column_family(format!("integrity_{i}"), Some(100 * 1024 * 1024))
            .unwrap();
    }

    let mut handles = vec![];
    for cf_id in 0..num_cfs {
        let db_clone = db.clone();
        let handle = thread::spawn(move || {
            let cf = db_clone
                .column_family(&format!("integrity_{cf_id}"))
                .unwrap();

            for entry_id in 0..entries_per_cf {
                let key = entry_id as u64;
                let mut value = vec![cf_id as u8; 64];
                let entry_id_u64 = entry_id as u64;
                value.extend_from_slice(&entry_id_u64.to_le_bytes());

                let txn = cf.begin_write().unwrap();
                {
                    let mut table = txn.open_table(TEST_TABLE).unwrap();
                    table.insert(&key, value.as_slice()).unwrap();
                }
                txn.commit().unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    for cf_id in 0..num_cfs {
        let cf = db.column_family(&format!("integrity_{cf_id}")).unwrap();
        let txn = cf.begin_read().unwrap();
        let table = txn.open_table(TEST_TABLE).unwrap();

        assert_eq!(table.len().unwrap(), entries_per_cf as u64);

        for entry_id in 0..entries_per_cf {
            let key = entry_id as u64;
            let value = table.get(&key).unwrap().unwrap();
            let value_bytes = value.value();

            assert_eq!(value_bytes.len(), 72);

            for (i, &byte) in value_bytes.iter().enumerate().take(64) {
                assert_eq!(
                    byte, cf_id as u8,
                    "CF {cf_id} entry {entry_id} byte {i} mismatch"
                );
            }

            let stored_entry_id = u64::from_le_bytes(value_bytes[64..72].try_into().unwrap());
            assert_eq!(
                stored_entry_id, entry_id as u64,
                "CF {cf_id} entry_id mismatch"
            );
        }
    }
}
