//! # manifold-graph
//!
//! Graph storage optimizations for the Manifold embedded database.
//!
//! This crate provides ergonomic, type-safe wrappers around Manifold's core primitives
//! for storing and querying graph edges with bidirectional indexes.
//!
//! ## Features
//!
//! - **Automatic bidirectional indexes**: Efficient queries for both outgoing and incoming edges
//! - **UUID-based vertices**: Fixed-width 16-byte vertex IDs with proper ordering
//! - **Type-safe edge properties**: Fixed-width `(bool, f32)` tuple for is_active and weight
//! - **Atomic updates**: Both forward and reverse indexes updated in same transaction
//! - **Efficient traversal**: Range scans leverage tuple key ordering for fast queries
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use manifold::column_family::ColumnFamilyDatabase;
//! use manifold_graph::{GraphTable, GraphTableRead, Edge};
//! use uuid::Uuid;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let db = ColumnFamilyDatabase::open("my.db")?;
//! let cf = db.column_family_or_create("social")?;
//!
//! let user1 = Uuid::new_v4();
//! let user2 = Uuid::new_v4();
//!
//! // Write edges
//! {
//!     let write_txn = cf.begin_write()?;
//!     let mut graph = GraphTable::open(&write_txn, "edges")?;
//!     graph.add_edge(&user1, "follows", &user2, true, 1.0)?;
//!     drop(graph);
//!     write_txn.commit()?;
//! }
//!
//! // Read with efficient traversal
//! let read_txn = cf.begin_read()?;
//! let graph = GraphTableRead::open(&read_txn, "edges")?;
//!
//! // Get all outgoing edges (range scan on forward table)
//! for edge_result in graph.outgoing_edges(&user1)? {
//!     let edge = edge_result?;
//!     println!("{:?} -[{}]-> {:?}", edge.source, edge.edge_type, edge.target);
//! }
//!
//! // Get all incoming edges (range scan on reverse table)
//! for edge_result in graph.incoming_edges(&user2)? {
//!     let edge = edge_result?;
//!     println!("{:?} -[{}]-> {:?}", edge.source, edge.edge_type, edge.target);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! This crate does **not** implement graph algorithms (BFS, DFS, PageRank, etc.).
//! Instead, it focuses on efficient storage and provides integration traits for external
//! graph libraries. The `EdgeSource` trait enables graph algorithm libraries to consume
//! edges efficiently.
//!
//! ## Edge Properties
//!
//! Edges store two fixed-width properties:
//! - `is_active: bool` - For active/passive edges, soft deletes, hidden edges
//! - `weight: f32` - General-purpose edge weight or score
//!
//! These properties are stored as a fixed-width tuple `(bool, f32)` for zero-overhead
//! serialization (5 bytes total).

#![deny(missing_docs)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

pub mod edge;
pub mod graph;
pub mod integration;

pub use edge::Edge;
pub use graph::{AllEdgesIter, GraphTable, GraphTableRead, IncomingEdgeIter, OutgoingEdgeIter};
pub use integration::EdgeSource;
