# manifold-vectors

Vector storage optimizations for the [Manifold](https://github.com/cberner/redb) embedded database.

[![Crates.io](https://img.shields.io/crates/v/manifold-vectors.svg)](https://crates.io/crates/manifold-vectors)
[![Documentation](https://docs.rs/manifold-vectors/badge.svg)](https://docs.rs/manifold-vectors)

## Overview

`manifold-vectors` provides ergonomic, type-safe wrappers around Manifold's core primitives for storing and retrieving vector embeddings commonly used in ML/AI applications. It does **not** implement vector indexing algorithms - instead, it focuses on efficient persistent storage and provides integration traits for external libraries like [instant-distance](https://crates.io/crates/instant-distance).

## Features

- **Zero-copy access** - Fixed-dimension vectors leverage Manifold's `fixed_width()` trait for direct memory-mapped access without deserialization overhead
- **Type safety** - Compile-time dimension checking via const generics
- **Multiple formats** - Dense, sparse (COO), and multi-vector (ColBERT-style) support
- **High performance** - Batch operations, efficient encoding, WAL group commit
- **Integration ready** - `VectorSource` trait for external index libraries (HNSW, FAISS, etc.)

## Quick Start

### Dense Vectors

```rust
use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::{VectorTable, VectorTableRead, distance};

// Open database and column family
let db = ColumnFamilyDatabase::open("my.db")?;
let cf = db.column_family_or_create("embeddings")?;

// Write vectors
{
    let write_txn = cf.begin_write()?;
    let mut vectors = VectorTable::<768>::open(&write_txn, "docs")?;
    
    let embedding = [0.1f32; 768];
    vectors.insert("doc_1", &embedding)?;
    
    drop(vectors);
    write_txn.commit()?;
}

// Read with zero-copy access - no allocations!
let read_txn = cf.begin_read()?;
let vectors = VectorTableRead::<768>::open(&read_txn, "docs")?;

if let Some(guard) = vectors.get("doc_1")? {
    // guard provides zero-copy access to mmap'd data
    let query = [0.1f32; 768];
    let similarity = distance::cosine(guard.value(), &query);
    println!("Cosine similarity: {}", similarity);
    // guard dropped here - no malloc/free occurred!
}
```

### Batch Operations

For high-throughput vector loading, use batch operations which leverage Manifold's WAL group commit:

```rust
let items = vec![
    ("doc_1", [0.1f32; 768]),
    ("doc_2", [0.2f32; 768]),
    ("doc_3", [0.3f32; 768]),
];

let write_txn = cf.begin_write()?;
let mut vectors = VectorTable::<768>::open(&write_txn, "docs")?;

// Insert all vectors in one batch
vectors.insert_batch(&items, false)?;

drop(vectors);
write_txn.commit()?;
```

### Sparse Vectors

For high-dimensional sparse vectors (e.g., TF-IDF, one-hot encodings):

```rust
use manifold_vectors::sparse::{SparseVector, SparseVectorTable, SparseVectorTableRead};

// Create sparse vector (COO format: coordinate list)
let sparse = SparseVector::new(vec![
    (0, 0.5),     // index 0, value 0.5
    (100, 0.8),   // index 100, value 0.8
    (1000, 0.3),  // index 1000, value 0.3
]);

// Write
{
    let write_txn = cf.begin_write()?;
    let mut sparse_table = SparseVectorTable::open(&write_txn, "sparse")?;
    sparse_table.insert("doc_1", &sparse)?;
    drop(sparse_table);
    write_txn.commit()?;
}

// Read
let read_txn = cf.begin_read()?;
let sparse_table = SparseVectorTableRead::open(&read_txn, "sparse")?;
let retrieved = sparse_table.get("doc_1")?.unwrap();

// Compute sparse dot product
let other = SparseVector::new(vec![(0, 1.0), (100, 0.5)]);
let dot = retrieved.dot(&other);
println!("Sparse dot product: {}", dot);
```

### Multi-Vectors (ColBERT-style)

For storing multiple vectors per document (e.g., token embeddings):

```rust
use manifold_vectors::multi::{MultiVectorTable, MultiVectorTableRead};

// Each document has multiple token embeddings
let token_embeddings = vec![
    [0.1f32; 128],  // token 1
    [0.2f32; 128],  // token 2
    [0.3f32; 128],  // token 3
];

{
    let write_txn = cf.begin_write()?;
    let mut multi = MultiVectorTable::<128>::open(&write_txn, "tokens")?;
    multi.insert("doc_1", &token_embeddings)?;
    drop(multi);
    write_txn.commit()?;
}

let read_txn = cf.begin_read()?;
let multi = MultiVectorTableRead::<128>::open(&read_txn, "tokens")?;
let tokens = multi.get("doc_1")?.unwrap();
println!("Document has {} token embeddings", tokens.len());
```

## Distance Functions

The crate includes common distance and similarity metrics that work directly with zero-copy `VectorGuard` types:

```rust
use manifold_vectors::distance;

let vec_a = [1.0, 0.0, 0.0];
let vec_b = [0.0, 1.0, 0.0];

let cosine_sim = distance::cosine(&vec_a, &vec_b);        // 0.0 (orthogonal)
let euclidean = distance::euclidean(&vec_a, &vec_b);      // sqrt(2)
let euclidean_sq = distance::euclidean_squared(&vec_a, &vec_b); // 2.0 (faster)
let manhattan = distance::manhattan(&vec_a, &vec_b);       // 2.0
let dot = distance::dot_product(&vec_a, &vec_b);          // 0.0
```

## Architecture

### Zero-Copy Design

Dense vectors use Manifold's `Value` trait with fixed-width serialization:

```rust
impl Value for [f32; DIM] {
    type SelfType<'a> = [f32; DIM];
    type AsBytes<'a> = &'a [u8];
    
    fn fixed_width() -> Option<usize> {
        Some(DIM * 4)  // 4 bytes per f32
    }
    // ...
}
```

This enables **true zero-copy reads** - vectors are read directly from memory-mapped pages without deserialization.

### Performance Characteristics

- **Write (dense)**: O(log n) B-tree insert, benefits from WAL group commit
- **Read (dense)**: O(log n) lookup + zero-copy mmap access (no allocation)
- **Write (sparse)**: O(log n) + O(k log k) sorting where k = non-zero count
- **Read (sparse)**: O(log n) + allocation for coordinate list
- **Storage (dense)**: DIM × 4 bytes per vector (fixed width)
- **Storage (sparse)**: k × 8 bytes per vector (4 bytes index + 4 bytes value)

## Integration with Vector Index Libraries

The `VectorSource` trait enables integration with external vector index libraries:

```rust
use manifold_vectors::VectorSource;
use instant_distance::{Builder, Search};

let read_txn = cf.begin_read()?;
let vectors = VectorTableRead::<768>::open(&read_txn, "docs")?;

// Build HNSW index from stored vectors
let mut points = Vec::new();
let mut ids = Vec::new();

for result in vectors.all_vectors()? {
    let (id, guard) = result?;
    points.push(instant_distance::Point::new(guard.value().to_vec()));
    ids.push(id);
}

let hnsw = Builder::default().build(&points, &mut rand::rng());

// Search for nearest neighbors
let query = instant_distance::Point::new(vec![0.5f32; 768]);
let search = Search::default();
let results = hnsw.search(&query, &search);

for item in results {
    println!("Similar doc: {} (distance: {})", ids[item.pid], item.distance);
}
```

## Examples

The crate includes comprehensive examples demonstrating real-world usage:

### 1. Dense Semantic Search (`examples/dense_semantic_search.rs`)
Full RAG pipeline with:
- Document embeddings using tessera-embeddings
- Cosine similarity search
- Result ranking
- Zero-copy performance

```bash
cargo run --example dense_semantic_search -p manifold-vectors
```

### 2. Sparse Hybrid Search (`examples/sparse_hybrid_search.rs`)
Combines dense and sparse vectors for hybrid search:
- Dense semantic embeddings
- Sparse TF-IDF vectors
- Weighted fusion of results
- BM25-style ranking

```bash
cargo run --example sparse_hybrid_search -p manifold-vectors
```

### 3. Multi-Vector ColBERT (`examples/multi_vector_colbert.rs`)
Token-level embeddings for fine-grained matching:
- Multi-vector storage per document
- MaxSim scoring (ColBERT-style)
- Late interaction models

```bash
cargo run --example multi_vector_colbert -p manifold-vectors
```

### 4. RAG Complete (`examples/rag_complete.rs`)
Production RAG implementation:
- Document chunking and embedding
- Similarity search
- Context retrieval
- Integration patterns

```bash
cargo run --example rag_complete -p manifold-vectors
```

## Use Cases

- **Semantic search** - Document and query embeddings for retrieval
- **Recommendation systems** - User and item embeddings
- **RAG (Retrieval Augmented Generation)** - Document chunk embeddings
- **Image similarity** - Vision model embeddings
- **Anomaly detection** - Embedding-based outlier detection
- **Clustering** - High-dimensional data points

## Combining with Other Domain Layers

`manifold-vectors` works seamlessly with other manifold domain layers in the same database:

```rust
use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::VectorTable;
use manifold_graph::GraphTable;
use manifold_timeseries::TimeSeriesTable;

let db = ColumnFamilyDatabase::open("my_app.db")?;

// Different column families for different access patterns
let vectors_cf = db.column_family_or_create("embeddings")?;
let graph_cf = db.column_family_or_create("relationships")?;
let metrics_cf = db.column_family_or_create("usage")?;

// Store user embeddings
let txn = vectors_cf.begin_write()?;
let mut vectors = VectorTable::<512>::open(&txn, "users")?;
vectors.insert("user_1", &embedding)?;

// Store user relationships
let txn = graph_cf.begin_write()?;
let mut graph = GraphTable::open(&txn, "follows")?;
graph.add_edge(&user_1, "follows", &user_2, true, 1.0)?;

// Store user activity metrics
let txn = metrics_cf.begin_write()?;
let mut ts = TimeSeriesTable::open(&txn, "activity")?;
ts.write("user_1.logins", timestamp, 1.0)?;
```

## Requirements

- Rust 1.70+ (for const generics)
- `manifold` version 3.1+

## Performance Tips

1. **Use batch operations** for bulk loading - 2-3x faster than individual inserts
2. **Pre-sort data** when possible and set `sorted: true` - saves sorting overhead
3. **Use zero-copy guards** for read operations - avoid calling `.value().to_vec()` unless needed
4. **Choose the right format**:
   - Dense: Most data is non-zero, < 10,000 dimensions
   - Sparse: > 90% zeros, or very high dimensional
   - Multi-vector: Token-level or chunk-level embeddings

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Contributing

Contributions are welcome! This crate follows the patterns established in the manifold domain layer architecture.

## Related Crates

- [manifold](https://crates.io/crates/manifold) - Core embedded database
- [manifold-graph](https://crates.io/crates/manifold-graph) - Graph storage for relationships
- [manifold-timeseries](https://crates.io/crates/manifold-timeseries) - Time series storage for metrics
- [instant-distance](https://crates.io/crates/instant-distance) - HNSW vector index