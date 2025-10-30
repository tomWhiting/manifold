use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::multi::{MultiVectorTable, MultiVectorTableRead};
use manifold_vectors::sparse::{SparseVector, SparseVectorTable, SparseVectorTableRead};
use manifold_vectors::{VectorTable, VectorTableRead, distance};
use tempfile::NamedTempFile;
use uuid::Uuid;

#[test]
fn test_dense_vector_zero_copy() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    let id = Uuid::new_v4();

    // Write
    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = VectorTable::<128>::open(&write_txn, "dense").unwrap();
        let vec1 = [1.0f32; 128];
        table.insert(&id, &vec1).unwrap();
        assert_eq!(table.len().unwrap(), 1);
        drop(table);
        write_txn.commit().unwrap();
    }

    // Read with zero-copy
    let read_txn = cf.begin_read().unwrap();
    let table = VectorTableRead::<128>::open(&read_txn, "dense").unwrap();
    let guard = table.get(&id).unwrap().unwrap();

    // Access via guard - zero copy!
    assert_eq!(guard.value().len(), 128);
    assert!((guard.value()[0] - 1.0).abs() < 1e-6);

    // Also works through Deref
    assert!((guard[0] - 1.0).abs() < 1e-6);

    // And as_slice()
    assert_eq!(guard.as_slice().len(), 128);
}

#[test]
fn test_distance_with_guards() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();

    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = VectorTable::<3>::open(&write_txn, "vecs").unwrap();
        table.insert(&id_a, &[1.0, 0.0, 0.0]).unwrap();
        table.insert(&id_b, &[0.0, 1.0, 0.0]).unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    let read_txn = cf.begin_read().unwrap();
    let table = VectorTableRead::<3>::open(&read_txn, "vecs").unwrap();

    let guard_a = table.get(&id_a).unwrap().unwrap();
    let guard_b = table.get(&id_b).unwrap().unwrap();

    // Distance functions work with guards through deref coercion
    let sim = distance::cosine(guard_a.value(), guard_b.value());
    assert!(sim.abs() < 1e-6); // Orthogonal vectors
}

#[test]
fn test_iterator_zero_copy() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = VectorTable::<32>::open(&write_txn, "iter_test").unwrap();
        table.insert(&Uuid::new_v4(), &[1.0; 32]).unwrap();
        table.insert(&Uuid::new_v4(), &[2.0; 32]).unwrap();
        table.insert(&Uuid::new_v4(), &[3.0; 32]).unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    let read_txn = cf.begin_read().unwrap();
    let table = VectorTableRead::<32>::open(&read_txn, "iter_test").unwrap();

    let mut count = 0;
    for result in table.all_vectors().unwrap() {
        let (_key, guard) = result.unwrap();
        // Zero-copy access through guard
        assert!(guard.value()[0] >= 1.0 && guard.value()[0] <= 3.0);
        count += 1;
    }
    assert_eq!(count, 3);
}

#[test]
fn test_sparse_vector() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    let id = Uuid::new_v4();

    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = SparseVectorTable::open(&write_txn, "sparse").unwrap();
        let vec = SparseVector::new(vec![(0, 1.0), (5, 2.0), (10, 3.0)]);
        table.insert(&id, &vec).unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    let read_txn = cf.begin_read().unwrap();
    let table = SparseVectorTableRead::open(&read_txn, "sparse").unwrap();
    let result = table.get(&id).unwrap().unwrap();
    assert_eq!(result.entries.len(), 3);
    assert_eq!(result.entries[0], (0, 1.0));
}

#[test]
fn test_sparse_vector_dot() {
    let a = SparseVector::new(vec![(0, 1.0), (2, 3.0), (5, 2.0)]);
    let b = SparseVector::new(vec![(0, 2.0), (3, 1.0), (5, 4.0)]);
    let dot = a.dot(&b);
    assert!((dot - 10.0).abs() < 1e-6);
}

#[test]
fn test_multi_vector() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    let id = Uuid::new_v4();

    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = MultiVectorTable::<64>::open(&write_txn, "multi").unwrap();
        let vecs = vec![[1.0f32; 64], [2.0f32; 64], [3.0f32; 64]];
        table.insert(&id, &vecs).unwrap();
        drop(table);
        write_txn.commit().unwrap();
    }

    let read_txn = cf.begin_read().unwrap();
    let table = MultiVectorTableRead::<64>::open(&read_txn, "multi").unwrap();
    let result = table.get(&id).unwrap().unwrap();
    assert_eq!(result.len(), 3);
    assert!((result[0][0] - 1.0).abs() < 1e-6);
}

#[test]
fn test_batch_insert() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = VectorTable::<32>::open(&write_txn, "dense").unwrap();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let items = vec![
            (id1, [1.0f32; 32]),
            (id2, [2.0f32; 32]),
            (id3, [3.0f32; 32]),
        ];
        table.insert_batch(&items, false).unwrap();
        assert_eq!(table.len().unwrap(), 3);
        drop(table);
        write_txn.commit().unwrap();
    }
}

#[test]
fn test_remove_vector() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();

    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = VectorTable::<3>::open(&write_txn, "dense").unwrap();

        table.insert(&id1, &[1.0, 2.0, 3.0]).unwrap();
        table.insert(&id2, &[4.0, 5.0, 6.0]).unwrap();
        assert_eq!(table.len().unwrap(), 2);

        let removed = table.remove(&id1).unwrap();
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().value(), &[1.0, 2.0, 3.0]);
        assert_eq!(table.len().unwrap(), 1);

        let removed_none = table.remove(&Uuid::new_v4()).unwrap();
        assert!(removed_none.is_none());
        drop(removed_none);

        drop(table);
        write_txn.commit().unwrap();
    }

    let read_txn = cf.begin_read().unwrap();
    let table = VectorTableRead::<3>::open(&read_txn, "dense").unwrap();
    assert!(table.get(&id1).unwrap().is_none());
    assert!(table.get(&id2).unwrap().is_some());
}

#[test]
fn test_bulk_remove() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = ColumnFamilyDatabase::open(tmpfile.path()).unwrap();
    let cf = db.column_family_or_create("vectors").unwrap();

    let ids: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();

    {
        let write_txn = cf.begin_write().unwrap();
        let mut table = VectorTable::<3>::open(&write_txn, "bulk").unwrap();

        for id in &ids {
            table.insert(id, &[1.0, 2.0, 3.0]).unwrap();
        }
        assert_eq!(table.len().unwrap(), 10);

        let removed_count = table.remove_bulk(&ids[0..5]).unwrap();
        assert_eq!(removed_count, 5);
        assert_eq!(table.len().unwrap(), 5);

        drop(table);
        write_txn.commit().unwrap();
    }

    let read_txn = cf.begin_read().unwrap();
    let table = VectorTableRead::<3>::open(&read_txn, "bulk").unwrap();

    for id in &ids[0..5] {
        assert!(table.get(id).unwrap().is_none());
    }

    for id in &ids[5..10] {
        assert!(table.get(id).unwrap().is_some());
    }
}
