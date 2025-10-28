//! Integration tests for manifold-graph

use manifold::column_family::ColumnFamilyDatabase;
use manifold_graph::{Edge, GraphTable, GraphTableRead};
use uuid::Uuid;

#[test]
fn test_basic_edge_operations() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let user3 = Uuid::new_v4();

    // Add edges
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();

        graph
            .add_edge(&user1, "follows", &user2, true, 1.0)
            .unwrap();
        graph.add_edge(&user1, "knows", &user3, true, 0.5).unwrap();
        graph
            .add_edge(&user2, "follows", &user3, true, 1.0)
            .unwrap();

        drop(graph);
        write_txn.commit().unwrap();
    }

    // Read edges
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();

    // Test get_edge
    let edge = graph.get_edge(&user1, "follows", &user2).unwrap();
    assert!(edge.is_some());
    let edge = edge.unwrap();
    assert_eq!(edge.source, user1);
    assert_eq!(edge.edge_type, "follows");
    assert_eq!(edge.target, user2);
    assert!(edge.is_active);
    assert_eq!(edge.weight, 1.0);

    // Test outgoing edges
    let outgoing: Vec<Edge> = graph
        .outgoing_edges(&user1)
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(outgoing.len(), 2);

    // Test incoming edges
    let incoming: Vec<Edge> = graph
        .incoming_edges(&user3)
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(incoming.len(), 2);
}

#[test]
fn test_bidirectional_consistency() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();

    // Add edge
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();
        graph
            .add_edge(&user1, "follows", &user2, true, 1.0)
            .unwrap();
        drop(graph);
        write_txn.commit().unwrap();
    }

    // Verify bidirectional access
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();

    // Check outgoing from user1
    let outgoing: Vec<Edge> = graph
        .outgoing_edges(&user1)
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].source, user1);
    assert_eq!(outgoing[0].target, user2);

    // Check incoming to user2
    let incoming: Vec<Edge> = graph
        .incoming_edges(&user2)
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0].source, user1);
    assert_eq!(incoming[0].target, user2);
}

#[test]
fn test_remove_edge() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();

    // Add then remove edge
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();
        graph
            .add_edge(&user1, "follows", &user2, true, 1.0)
            .unwrap();
        graph.remove_edge(&user1, "follows", &user2).unwrap();
        drop(graph);
        write_txn.commit().unwrap();
    }

    // Verify edge is gone
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();

    let edge = graph.get_edge(&user1, "follows", &user2).unwrap();
    assert!(edge.is_none());

    let outgoing: Vec<Edge> = graph
        .outgoing_edges(&user1)
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(outgoing.len(), 0);
}

#[test]
fn test_update_edge() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();

    // Add edge with initial properties
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();
        graph
            .add_edge(&user1, "follows", &user2, true, 1.0)
            .unwrap();
        drop(graph);
        write_txn.commit().unwrap();
    }

    // Update properties
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();
        graph
            .update_edge(&user1, "follows", &user2, false, 0.5)
            .unwrap();
        drop(graph);
        write_txn.commit().unwrap();
    }

    // Verify updated properties
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();

    let edge = graph.get_edge(&user1, "follows", &user2).unwrap().unwrap();
    assert!(!edge.is_active);
    assert_eq!(edge.weight, 0.5);
}

#[test]
fn test_edge_type_filtering() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let user3 = Uuid::new_v4();

    // Add different edge types
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();
        graph
            .add_edge(&user1, "follows", &user2, true, 1.0)
            .unwrap();
        graph.add_edge(&user1, "blocks", &user3, true, 1.0).unwrap();
        drop(graph);
        write_txn.commit().unwrap();
    }

    // Read and filter by edge type
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();

    let outgoing: Vec<Edge> = graph
        .outgoing_edges(&user1)
        .unwrap()
        .map(|r| r.unwrap())
        .filter(|e| e.edge_type == "follows")
        .collect();
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].target, user2);
}

#[test]
fn test_empty_graph() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let user1 = Uuid::new_v4();

    // Create empty graph
    {
        let write_txn = cf.begin_write().unwrap();
        let _graph = GraphTable::open(&write_txn, "edges").unwrap();
        drop(_graph);
        write_txn.commit().unwrap();
    }

    // Query empty graph
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();

    let outgoing: Vec<Edge> = graph
        .outgoing_edges(&user1)
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(outgoing.len(), 0);

    assert!(graph.is_empty().unwrap());
}

#[test]
fn test_batch_insertion_unsorted() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let u1 = Uuid::new_v4();
    let u2 = Uuid::new_v4();
    let u3 = Uuid::new_v4();
    let u4 = Uuid::new_v4();

    // Add edges in batch (unsorted)
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();

        let edges = vec![
            (u3, "follows", u1, true, 0.8),
            (u1, "follows", u2, true, 1.0),
            (u4, "follows", u2, true, 0.6),
            (u2, "follows", u3, true, 0.9),
            (u1, "knows", u4, true, 0.5),
        ];

        let count = graph.add_edges_batch(edges, false).unwrap();
        assert_eq!(count, 5);
        assert_eq!(graph.len().unwrap(), 5);

        drop(graph);
        write_txn.commit().unwrap();
    }

    // Verify all edges were inserted correctly
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();

    assert_eq!(graph.len().unwrap(), 5);

    // Verify specific edges
    assert!(graph.get_edge(&u1, "follows", &u2).unwrap().is_some());
    assert!(graph.get_edge(&u2, "follows", &u3).unwrap().is_some());
    assert!(graph.get_edge(&u3, "follows", &u1).unwrap().is_some());
    assert!(graph.get_edge(&u4, "follows", &u2).unwrap().is_some());
    assert!(graph.get_edge(&u1, "knows", &u4).unwrap().is_some());

    // Verify outgoing edges
    let u1_outgoing: Vec<Edge> = graph
        .outgoing_edges(&u1)
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(u1_outgoing.len(), 2);

    // Verify incoming edges
    let u2_incoming: Vec<Edge> = graph
        .incoming_edges(&u2)
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(u2_incoming.len(), 2);
}

#[test]
fn test_batch_insertion_sorted() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let u1 = Uuid::nil();
    let u2 = Uuid::from_u128(1);
    let u3 = Uuid::from_u128(2);

    // Add edges in batch (pre-sorted)
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();

        // Edges sorted by (source, edge_type, target)
        let edges = vec![
            (u1, "follows", u2, true, 1.0),
            (u1, "follows", u3, true, 0.8),
            (u2, "follows", u3, true, 0.9),
        ];

        let count = graph.add_edges_batch(edges, true).unwrap();
        assert_eq!(count, 3);

        drop(graph);
        write_txn.commit().unwrap();
    }

    // Verify
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();
    assert_eq!(graph.len().unwrap(), 3);
}

#[test]
fn test_batch_insertion_empty() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    // Add empty batch
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();

        let edges: Vec<(Uuid, &str, Uuid, bool, f32)> = vec![];
        let count = graph.add_edges_batch(edges, false).unwrap();
        assert_eq!(count, 0);

        drop(graph);
        write_txn.commit().unwrap();
    }

    // Verify empty
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();
    assert!(graph.is_empty().unwrap());
}

#[test]
fn test_full_graph_iteration() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let u1 = Uuid::new_v4();
    let u2 = Uuid::new_v4();
    let u3 = Uuid::new_v4();

    // Add edges
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();

        let edges = vec![
            (u1, "follows", u2, true, 1.0),
            (u1, "follows", u3, true, 0.8),
            (u2, "follows", u3, true, 0.9),
            (u3, "knows", u1, false, 0.5),
        ];

        graph.add_edges_batch(edges, false).unwrap();
        drop(graph);
        write_txn.commit().unwrap();
    }

    // Iterate over all edges
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();

    let all_edges: Vec<Edge> = graph.iter().unwrap().map(|r| r.unwrap()).collect();

    assert_eq!(all_edges.len(), 4);

    // Verify we can find specific edges
    assert!(
        all_edges
            .iter()
            .any(|e| e.source == u1 && e.target == u2 && e.edge_type == "follows")
    );
    assert!(
        all_edges
            .iter()
            .any(|e| e.source == u1 && e.target == u3 && e.edge_type == "follows")
    );
    assert!(
        all_edges
            .iter()
            .any(|e| e.source == u2 && e.target == u3 && e.edge_type == "follows")
    );
    assert!(
        all_edges
            .iter()
            .any(|e| e.source == u3 && e.target == u1 && e.edge_type == "knows")
    );
}

#[test]
fn test_edge_source_trait() {
    use manifold_graph::EdgeSource;

    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let u1 = Uuid::new_v4();
    let u2 = Uuid::new_v4();

    // Add edges
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();
        graph.add_edge(&u1, "follows", &u2, true, 1.0).unwrap();
        drop(graph);
        write_txn.commit().unwrap();
    }

    // Use EdgeSource trait
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();

    assert_eq!(graph.edge_count().unwrap(), 1);
    assert!(!graph.is_empty().unwrap());

    let edges: Vec<Edge> = graph.iter_edges().unwrap().map(|r| r.unwrap()).collect();

    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].source, u1);
    assert_eq!(edges[0].target, u2);
}

#[test]
fn test_batch_with_duplicates() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("graph.db");
    let db = ColumnFamilyDatabase::open(&db_path).unwrap();
    let cf = db.column_family_or_create("test").unwrap();

    let u1 = Uuid::new_v4();
    let u2 = Uuid::new_v4();

    // Add batch with duplicate edges (should overwrite)
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();

        let edges = vec![
            (u1, "follows", u2, true, 1.0),
            (u1, "follows", u2, false, 0.5), // Duplicate, should overwrite
        ];

        let count = graph.add_edges_batch(edges, false).unwrap();
        assert_eq!(count, 2); // Both inserts attempted

        drop(graph);
        write_txn.commit().unwrap();
    }

    // Verify only one edge exists with updated properties
    let read_txn = cf.begin_read().unwrap();
    let graph = GraphTableRead::open(&read_txn, "edges").unwrap();

    assert_eq!(graph.len().unwrap(), 1);

    let edge = graph.get_edge(&u1, "follows", &u2).unwrap().unwrap();
    assert!(!edge.is_active); // Should have the second value
    assert_eq!(edge.weight, 0.5);
}
