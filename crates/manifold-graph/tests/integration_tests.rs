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

        graph.add_edge(&user1, "follows", &user2, true, 1.0).unwrap();
        graph.add_edge(&user1, "knows", &user3, true, 0.5).unwrap();
        graph.add_edge(&user2, "follows", &user3, true, 1.0).unwrap();

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
        graph.add_edge(&user1, "follows", &user2, true, 1.0).unwrap();
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
        graph.add_edge(&user1, "follows", &user2, true, 1.0).unwrap();
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
        graph.add_edge(&user1, "follows", &user2, true, 1.0).unwrap();
        drop(graph);
        write_txn.commit().unwrap();
    }

    // Update properties
    {
        let write_txn = cf.begin_write().unwrap();
        let mut graph = GraphTable::open(&write_txn, "edges").unwrap();
        graph.update_edge(&user1, "follows", &user2, false, 0.5).unwrap();
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
        graph.add_edge(&user1, "follows", &user2, true, 1.0).unwrap();
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
