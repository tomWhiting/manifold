//! # manifold-vectors
//!
//! Vector storage optimizations for the Manifold embedded database.
//!
//! This crate provides ergonomic, type-safe wrappers around Manifold's core primitives
//! for storing and retrieving vector embeddings commonly used in ML/AI applications.
//!
//! ## Features
//!
//! - **Zero-copy access**: Fixed-dimension vectors leverage Manifold's `fixed_width()` trait
//!   for direct memory-mapped access without deserialization overhead
//! - **Type safety**: Compile-time dimension checking via const generics
//! - **High performance**: Bulk operations, efficient encoding, WAL group commit
//! - **Multiple formats**: Dense, sparse (COO), and multi-vector (ColBERT-style) support
//! - **Integration-ready**: Traits for external index libraries (HNSW, FAISS, etc.)
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use manifold::column_family::ColumnFamilyDatabase;
//! use manifold_vectors::{VectorTable, distance};
//! use manifold_vectors::dense::VectorTableRead;
//! use uuid::Uuid;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let db = ColumnFamilyDatabase::open("my.db")?;
//! let cf = db.column_family_or_create("embeddings")?;
//!
//! let doc_id = Uuid::new_v4();
//!
//! // Write vectors
//! {
//!     let write_txn = cf.begin_write()?;
//!     let mut vectors = VectorTable::<768>::open(&write_txn, "vectors")?;
//!     let embedding = [0.1f32; 768];
//!     vectors.insert(&doc_id, &embedding)?;
//!     drop(vectors);
//!     write_txn.commit()?;
//! }
//!
//! // Read with zero-copy access - no allocations!
//! let read_txn = cf.begin_read()?;
//! let vectors = VectorTableRead::<768>::open(&read_txn, "vectors")?;
//!
//! if let Some(guard) = vectors.get(&doc_id)? {
//!     // guard provides zero-copy access to mmap'd data
//!     let query = [0.1f32; 768];
//!     let similarity = distance::cosine(&query, guard.value());
//!     println!("Cosine similarity: {}", similarity);
//!     // guard dropped here - no malloc/free occurred!
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! This crate does **not** implement vector indexing algorithms (HNSW, IVF, PQ, etc.).
//! Instead, it focuses on efficient storage and provides integration traits for external
//! libraries. For approximate nearest neighbor search, use libraries like:
//!
//! - [`instant-distance`](https://crates.io/crates/instant-distance) - Pure Rust HNSW
//! - [`hnswlib`](https://crates.io/crates/hnswlib) - Bindings to C++ hnswlib
//!
//! See the examples directory for integration patterns.

#![deny(missing_docs)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

pub mod dense;
pub mod distance;
pub mod integration;
pub mod multi;
pub mod sparse;

pub use dense::{VectorGuard, VectorTable, VectorTableRead};
pub use multi::{MultiVectorTable, MultiVectorTableRead};
pub use sparse::{SparseVector, SparseVectorTable, SparseVectorTableRead};
