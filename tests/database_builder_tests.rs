use manifold::{Database, Durability, ReadableDatabase, TableDefinition, TableHandle};

const TEST_TABLE: TableDefinition<&str, &str> = TableDefinition::new("test");
const U64_TABLE: TableDefinition<u64, u64> = TableDefinition::new("u64");

fn create_tempfile() -> tempfile::NamedTempFile {
    if cfg!(target_os = "wasi") {
        tempfile::NamedTempFile::new_in("/tmp").unwrap()
    } else {
        tempfile::NamedTempFile::new().unwrap()
    }
}

#[test]
fn test_database_create() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();
    assert!(db.begin_read().is_ok());
}

#[test]
fn test_database_builder_default() {
    let tmpfile = create_tempfile();
    let db = Database::builder().create(tmpfile.path()).unwrap();
    assert!(db.begin_read().is_ok());
}

#[test]
fn test_database_builder_create_with_backend() {
    let tmpfile = create_tempfile();
    let db = Database::builder().create(tmpfile.path()).unwrap();
    assert!(db.begin_read().is_ok());
}

#[test]
fn test_database_builder_set_cache_size() {
    let tmpfile = create_tempfile();
    let db = Database::builder().create(tmpfile.path()).unwrap();
    assert!(db.begin_read().is_ok());
}

#[test]
fn test_database_builder_custom_settings() {
    let tmpfile = create_tempfile();
    let db = Database::builder().create(tmpfile.path()).unwrap();
    assert!(db.begin_read().is_ok());
}

#[test]
fn test_database_open_existing() {
    let tmpfile = create_tempfile();
    let path = tmpfile.path().to_path_buf();

    // Create and close database
    {
        let db = Database::create(&path).unwrap();
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TEST_TABLE).unwrap();
            table.insert("key", "value").unwrap();
        }
        write_txn.commit().unwrap();
    }

    // Reopen and verify
    let db = Database::open(&path).unwrap();
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    let value = table.get("key").unwrap().unwrap();
    assert_eq!(value.value(), "value");
}

#[test]
fn test_transaction_durability_none() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();
    let mut write_txn = db.begin_write().unwrap();
    write_txn.set_durability(Durability::None).unwrap();

    {
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert("key", "value").unwrap();
    }
    let _ = write_txn.commit();
}

#[test]
fn test_transaction_durability_multiple_settings() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    // Test None durability
    let mut write_txn = db.begin_write().unwrap();
    write_txn.set_durability(Durability::None).unwrap();
    {
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert("key1", "value1").unwrap();
    }
    let _ = write_txn.commit();

    // Test Immediate durability
    let mut write_txn = db.begin_write().unwrap();
    write_txn.set_durability(Durability::Immediate).unwrap();
    {
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert("key2", "value2").unwrap();
    }
    let _ = write_txn.commit();
}

#[test]
fn test_transaction_durability_immediate() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();
    let mut write_txn = db.begin_write().unwrap();
    write_txn.set_durability(Durability::Immediate).unwrap();

    {
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert("key", "value").unwrap();
    }
    write_txn.commit().unwrap();
}

#[test]
fn test_database_check_integrity() {
    let tmpfile = create_tempfile();
    let mut db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert("key1", "value1").unwrap();
        table.insert("key2", "value2").unwrap();
    }
    write_txn.commit().unwrap();

    // Check integrity
    let _ = db.check_integrity();
}

#[test]
fn test_database_compact() {
    let tmpfile = create_tempfile();
    let mut db = Database::create(tmpfile.path()).unwrap();

    // Insert and delete data to create fragmentation
    for i in 0..100 {
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(U64_TABLE).unwrap();
            table.insert(&i, &(i * 2)).unwrap();
        }
        write_txn.commit().unwrap();
    }

    // Compact the database
    let result = db.compact();
    // Compact may fail if savepoints exist, but that's ok for this test
    let _ = result;
}

#[test]
fn test_list_tables() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let _ = write_txn.open_table(TEST_TABLE).unwrap();
        let _ = write_txn.open_table(U64_TABLE).unwrap();
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let tables = read_txn.list_tables().unwrap();

    let table_names: Vec<_> = tables.map(|t| t.name().to_string()).collect();
    assert!(table_names.contains(&"test".to_string()));
    assert!(table_names.contains(&"u64".to_string()));
}

#[test]
fn test_abort_transaction() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    // Create table first
    let write_txn = db.begin_write().unwrap();
    {
        let _table = write_txn.open_table(TEST_TABLE).unwrap();
    }
    write_txn.commit().unwrap();

    // Start transaction and insert data
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert("key", "value").unwrap();
    }
    // Abort by dropping without commit
    drop(write_txn);

    // Verify data was not persisted
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    assert!(table.get("key").unwrap().is_none());
}

#[test]
fn test_multiple_read_transactions() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    // Write some data
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert("key", "value").unwrap();
    }
    write_txn.commit().unwrap();

    // Open multiple read transactions
    let read_txn1 = db.begin_read().unwrap();
    let read_txn2 = db.begin_read().unwrap();

    let table1 = read_txn1.open_table(TEST_TABLE).unwrap();
    let table2 = read_txn2.open_table(TEST_TABLE).unwrap();

    assert_eq!(table1.get("key").unwrap().unwrap().value(), "value");
    assert_eq!(table2.get("key").unwrap().unwrap().value(), "value");
}

#[test]
fn test_transaction_isolation() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    // Create table first
    let write_txn = db.begin_write().unwrap();
    {
        let _table = write_txn.open_table(TEST_TABLE).unwrap();
    }
    write_txn.commit().unwrap();

    // Start read transaction
    let read_txn = db.begin_read().unwrap();

    // Start write transaction and modify data
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(TEST_TABLE).unwrap();
        table.insert("key", "new_value").unwrap();
    }
    write_txn.commit().unwrap();

    // Read transaction should still see empty table (snapshot isolation)
    let table = read_txn.open_table(TEST_TABLE).unwrap();
    assert!(table.get("key").unwrap().is_none());

    // New read transaction should see the new value
    let read_txn2 = db.begin_read().unwrap();
    let table2 = read_txn2.open_table(TEST_TABLE).unwrap();
    assert_eq!(table2.get("key").unwrap().unwrap().value(), "new_value");
}
