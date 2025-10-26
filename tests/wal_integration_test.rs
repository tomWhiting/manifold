use manifold::TableDefinition;
use manifold::column_family::ColumnFamilyDatabase;
use std::fs;
use tempfile::NamedTempFile;

const TABLE: TableDefinition<&str, &str> = TableDefinition::new("my_data");

#[test]
fn test_wal_basic_commit() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    // Create database and column family
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    db.create_column_family("test_cf", None).unwrap();

    let cf = db.column_family("test_cf").unwrap();

    // Write some data
    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TABLE).unwrap();
        table.insert("key1", "value1").unwrap();
        table.insert("key2", "value2").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Verify WAL file exists
    let wal_path = db_path.with_extension("wal");
    assert!(wal_path.exists(), "WAL file should exist after commit");

    // Verify WAL file has content (should be > header size)
    let wal_size = fs::metadata(&wal_path).unwrap().len();
    assert!(wal_size > 512, "WAL file should contain header and entries");

    // Read data back
    {
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(TABLE).unwrap();

        assert_eq!(table.get("key1").unwrap().unwrap().value(), "value1");
        assert_eq!(table.get("key2").unwrap().unwrap().value(), "value2");
    }
}

#[test]
fn test_wal_multiple_commits() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    db.create_column_family("test_cf", None).unwrap();

    let cf = db.column_family("test_cf").unwrap();

    // Write multiple transactions
    for i in 0..10 {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TABLE).unwrap();
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        table.insert(key.as_str(), value.as_str()).unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    // Verify WAL grew
    let wal_path = db_path.with_extension("wal");
    let wal_size = fs::metadata(&wal_path).unwrap().len();
    assert!(
        wal_size > 1024,
        "WAL file should grow with multiple commits"
    );

    // Verify all data
    {
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(TABLE).unwrap();

        for i in 0..10 {
            let key = format!("key{}", i);
            let expected = format!("value{}", i);
            assert_eq!(table.get(key.as_str()).unwrap().unwrap().value(), expected);
        }
    }
}

#[test]
fn test_wal_concurrent_column_families() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    db.create_column_family("cf1", None).unwrap();
    db.create_column_family("cf2", None).unwrap();

    let cf1 = db.column_family("cf1").unwrap();
    let cf2 = db.column_family("cf2").unwrap();

    // Write to different CFs
    {
        let txn1 = cf1.begin_write().unwrap();
        let mut table1 = txn1.open_table(TABLE).unwrap();
        table1.insert("cf1_key", "cf1_value").unwrap();
        drop(table1);
        txn1.commit().unwrap();
    }

    {
        let txn2 = cf2.begin_write().unwrap();
        let mut table2 = txn2.open_table(TABLE).unwrap();
        table2.insert("cf2_key", "cf2_value").unwrap();
        drop(table2);
        txn2.commit().unwrap();
    }

    // Verify WAL contains entries from both CFs
    let wal_path = db_path.with_extension("wal");
    assert!(wal_path.exists());

    // Verify data from both CFs
    {
        let read_txn1 = cf1.begin_read().unwrap();
        let table1 = read_txn1.open_table(TABLE).unwrap();
        assert_eq!(table1.get("cf1_key").unwrap().unwrap().value(), "cf1_value");
    }

    {
        let read_txn2 = cf2.begin_read().unwrap();
        let table2 = read_txn2.open_table(TABLE).unwrap();
        assert_eq!(table2.get("cf2_key").unwrap().unwrap().value(), "cf2_value");
    }
}

#[test]
fn test_wal_data_visible_after_commit() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();

    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    db.create_column_family("test_cf", None).unwrap();

    let cf = db.column_family("test_cf").unwrap();

    // Write data
    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = write_txn.open_table(TABLE).unwrap();
        table.insert("test_key", "test_value").unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }
    // Transaction committed - data should be visible immediately

    // Verify data is immediately visible (non_durable_commit was called)
    {
        let read_txn = cf.begin_read().unwrap();
        let table = read_txn.open_table(TABLE).unwrap();
        assert_eq!(
            table.get("test_key").unwrap().unwrap().value(),
            "test_value",
            "Data should be visible immediately after WAL fsync"
        );
    }
}
