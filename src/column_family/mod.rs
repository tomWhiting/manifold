//! Column family support for redb databases.
//!
//! This module provides a column family architecture that allows multiple independent
//! redb database instances to coexist within a single physical file. Each column family
//! operates as a complete redb database with its own transaction isolation, enabling
//! concurrent writes to different column families while maintaining ACID guarantees.
//!
//! # Architecture
//!
//! A column family database consists of:
//! - A master header describing the layout of all column families
//! - Multiple column families, each occupying a contiguous byte range in the file
//! - A partitioned storage backend that provides offset translation for each column family
//!
//! # Concurrency Model
//!
//! Each column family maintains independent write locks and transaction tracking:
//! - Thread A can write to column family "users" while thread B writes to "products"
//! - Within a column family, standard redb MVCC applies (single writer, multiple readers)
//! - Readers never block writers or other readers thanks to snapshot isolation
//!
//! # Example Usage
//!
//! ```ignore
//! use manifold::column_family::ColumnFamilyDatabase;
//!
//! let db = ColumnFamilyDatabase::open("my_collection.manifold")?;
//! db.create_column_family("users", Some(1024 * 1024 * 1024))?; // 1GB partition
//! db.create_column_family("products", Some(1024 * 1024 * 1024))?;
//!
//! let users_cf = db.column_family("users")?;
//! let products_cf = db.column_family("products")?;
//!
//! // Concurrent writes to different column families
//! std::thread::scope(|s| {
//!     s.spawn(|| {
//!         let txn = users_cf.begin_write()?;
//!         // ... write user data
//!         txn.commit()
//!     });
//!
//!     s.spawn(|| {
//!         let txn = products_cf.begin_write()?;
//!         // ... write product data
//!         txn.commit()
//!     });
//! });
//! ```

pub(crate) mod builder;
pub(crate) mod database;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod file_handle_pool;
pub(crate) mod header;
pub(crate) mod partitioned_backend;
pub(crate) mod state;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod unlocked_backend;
pub(crate) mod wal;

pub use builder::ColumnFamilyDatabaseBuilder;
pub use database::{ColumnFamily, ColumnFamilyDatabase, ColumnFamilyError};
#[cfg(not(target_arch = "wasm32"))]
pub use file_handle_pool::FileHandlePool;
pub use header::{ColumnFamilyMetadata, FORMAT_VERSION, MAGIC_NUMBER, MasterHeader};
pub use partitioned_backend::PartitionedStorageBackend;
pub use wal::WALConfig;
