use manifold::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};

const STR_TABLE: TableDefinition<&str, &str> = TableDefinition::new("str_table");
const U64_TABLE: TableDefinition<u64, u64> = TableDefinition::new("u64_table");
const BYTES_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("bytes_table");

fn create_tempfile() -> tempfile::NamedTempFile {
    if cfg!(target_os = "wasi") {
        tempfile::NamedTempFile::new_in("/tmp").unwrap()
    } else {
        tempfile::NamedTempFile::new().unwrap()
    }
}

#[test]
fn test_table_insert_and_get() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(STR_TABLE).unwrap();
        table.insert("key1", "value1").unwrap();
        table.insert("key2", "value2").unwrap();
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(STR_TABLE).unwrap();
    assert_eq!(table.get("key1").unwrap().unwrap().value(), "value1");
    assert_eq!(table.get("key2").unwrap().unwrap().value(), "value2");
}

#[test]
fn test_table_remove() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    // Insert data
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(STR_TABLE).unwrap();
        table.insert("key1", "value1").unwrap();
        table.insert("key2", "value2").unwrap();
    }
    write_txn.commit().unwrap();

    // Remove one key
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(STR_TABLE).unwrap();
        let removed = table.remove("key1").unwrap();
        assert_eq!(removed.unwrap().value(), "value1");
    }
    write_txn.commit().unwrap();

    // Verify removal
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(STR_TABLE).unwrap();
    assert!(table.get("key1").unwrap().is_none());
    assert_eq!(table.get("key2").unwrap().unwrap().value(), "value2");
}

#[test]
fn test_table_len() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(U64_TABLE).unwrap();
        for i in 0..100 {
            table.insert(&i, &(i * 2)).unwrap();
        }
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(U64_TABLE).unwrap();
    assert_eq!(table.len().unwrap(), 100);
}

#[test]
fn test_table_is_empty() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    // Create table first
    let write_txn = db.begin_write().unwrap();
    {
        let _table = write_txn.open_table(STR_TABLE).unwrap();
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(STR_TABLE).unwrap();
    assert!(table.is_empty().unwrap());
    drop(table);
    drop(read_txn);

    // Insert data
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(STR_TABLE).unwrap();
        table.insert("key", "value").unwrap();
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(STR_TABLE).unwrap();
    assert!(!table.is_empty().unwrap());
}

#[test]
fn test_table_iter() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(U64_TABLE).unwrap();
        for i in 0..10 {
            table.insert(&i, &(i * 10)).unwrap();
        }
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(U64_TABLE).unwrap();
    let mut count = 0;
    for item in table.iter().unwrap() {
        let (key, value) = item.unwrap();
        assert_eq!(value.value(), key.value() * 10);
        count += 1;
    }
    assert_eq!(count, 10);
}

#[test]
fn test_table_range() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(U64_TABLE).unwrap();
        for i in 0..20 {
            table.insert(&i, &(i * 10)).unwrap();
        }
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(U64_TABLE).unwrap();

    // Range 5..15
    let mut count = 0;
    for item in table.range(5u64..15u64).unwrap() {
        let (key, _value) = item.unwrap();
        assert!(key.value() >= 5 && key.value() < 15);
        count += 1;
    }
    assert_eq!(count, 10);
}

#[test]
fn test_table_drain_filter() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(U64_TABLE).unwrap();
        for i in 0..20 {
            table.insert(&i, &(i * 10)).unwrap();
        }

        // Remove items with keys between 5 and 15
        for i in 5..15 {
            table.remove(&i).unwrap();
        }
    }
    write_txn.commit().unwrap();

    // Verify removed items are gone
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(U64_TABLE).unwrap();
    assert_eq!(table.len().unwrap(), 10);

    for i in 0..5 {
        assert!(table.get(&i).unwrap().is_some());
    }
    for i in 5..15 {
        assert!(table.get(&i).unwrap().is_none());
    }
    for i in 15..20 {
        assert!(table.get(&i).unwrap().is_some());
    }
}

#[test]
fn test_table_update_value() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(STR_TABLE).unwrap();
        table.insert("key", "value1").unwrap();
    }
    write_txn.commit().unwrap();

    // Update the value
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(STR_TABLE).unwrap();
        table.insert("key", "value2").unwrap();
    }
    write_txn.commit().unwrap();

    // Verify update
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(STR_TABLE).unwrap();
    assert_eq!(table.get("key").unwrap().unwrap().value(), "value2");
}

#[test]
fn test_table_empty_string_key() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(STR_TABLE).unwrap();
        table.insert("", "empty_key_value").unwrap();
        table.insert("normal", "normal_value").unwrap();
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(STR_TABLE).unwrap();
    assert_eq!(table.get("").unwrap().unwrap().value(), "empty_key_value");
    assert_eq!(
        table.get("normal").unwrap().unwrap().value(),
        "normal_value"
    );
}

#[test]
fn test_table_empty_string_value() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(STR_TABLE).unwrap();
        table.insert("key", "").unwrap();
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(STR_TABLE).unwrap();
    assert_eq!(table.get("key").unwrap().unwrap().value(), "");
}

#[test]
fn test_table_bytes() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let key_bytes: &[u8] = b"binary_key";
    let value_bytes: &[u8] = b"binary_value\x00\x01\x02";

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(BYTES_TABLE).unwrap();
        table.insert(key_bytes, value_bytes).unwrap();
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(BYTES_TABLE).unwrap();
    assert_eq!(table.get(key_bytes).unwrap().unwrap().value(), value_bytes);
}

#[test]
fn test_table_large_value() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let large_value = "x".repeat(10000);

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(STR_TABLE).unwrap();
        table.insert("large", large_value.as_str()).unwrap();
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(STR_TABLE).unwrap();
    assert_eq!(table.get("large").unwrap().unwrap().value(), large_value);
}

#[test]
fn test_table_many_small_entries() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(U64_TABLE).unwrap();
        for i in 0..1000 {
            table.insert(&i, &(i * 3)).unwrap();
        }
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(U64_TABLE).unwrap();
    assert_eq!(table.len().unwrap(), 1000);

    for i in 0..1000 {
        assert_eq!(table.get(&i).unwrap().unwrap().value(), i * 3);
    }
}

#[test]
fn test_table_first_last() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(U64_TABLE).unwrap();
        for i in 10..20 {
            table.insert(&i, &(i * 10)).unwrap();
        }
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(U64_TABLE).unwrap();

    // Get first entry
    let mut iter = table.iter().unwrap();
    let first = iter.next().unwrap().unwrap();
    assert_eq!(first.0.value(), 10);
    assert_eq!(first.1.value(), 100);
}

#[test]
fn test_table_pop_first() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(U64_TABLE).unwrap();
        for i in 0..5 {
            table.insert(&i, &(i * 10)).unwrap();
        }

        let first = table.pop_first().unwrap().unwrap();
        assert_eq!(first.0.value(), 0);
        assert_eq!(first.1.value(), 0);
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(U64_TABLE).unwrap();
    assert_eq!(table.len().unwrap(), 4);
    assert!(table.get(&0u64).unwrap().is_none());
}

#[test]
fn test_table_pop_last() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(U64_TABLE).unwrap();
        for i in 0..5 {
            table.insert(&i, &(i * 10)).unwrap();
        }

        let last = table.pop_last().unwrap().unwrap();
        assert_eq!(last.0.value(), 4);
        assert_eq!(last.1.value(), 40);
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(U64_TABLE).unwrap();
    assert_eq!(table.len().unwrap(), 4);
    assert!(table.get(&4u64).unwrap().is_none());
}

#[test]
fn test_table_remove_range() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(U64_TABLE).unwrap();
        for i in 0..10 {
            table.insert(&i, &(i * 10)).unwrap();
        }

        // Remove even keys
        for i in 0..10 {
            if i % 2 == 0 {
                table.remove(&i).unwrap();
            }
        }
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(U64_TABLE).unwrap();
    assert_eq!(table.len().unwrap(), 5);

    // Only odd keys should remain
    for i in 0..10 {
        if i % 2 == 0 {
            assert!(table.get(&i).unwrap().is_none());
        } else {
            assert!(table.get(&i).unwrap().is_some());
        }
    }
}

#[test]
fn test_table_insert_reserve() {
    let tmpfile = create_tempfile();
    let db = Database::create(tmpfile.path()).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(BYTES_TABLE).unwrap();
        let key: &[u8] = b"reserved_key";
        let value_data = b"reserved_value";

        let mut reserved = table.insert_reserve(key, value_data.len()).unwrap();
        reserved.as_mut().copy_from_slice(value_data);
    }
    write_txn.commit().unwrap();

    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(BYTES_TABLE).unwrap();
    assert_eq!(
        table
            .get(b"reserved_key" as &[u8])
            .unwrap()
            .unwrap()
            .value(),
        b"reserved_value" as &[u8]
    );
}
