# manifold-graph

Graph storage optimizations for the [Manifold](https://github.com/cberner/redb) embedded database.

[![Crates.io](https://img.shields.io/crates/v/manifold-graph.svg)](https://crates.io/crates/manifold-graph)
[![Documentation](https://docs.rs/manifold-graph/badge.svg)](https://docs.rs/manifold-graph)

## Overview

`manifold-graph` provides ergonomic, type-safe wrappers around Manifold's core primitives for storing and querying graph edges with automatic bidirectional indexes. It does **not** implement graph algorithms - instead, it focuses on efficient persistent storage and provides integration traits for external graph libraries like [petgraph](https://crates.io/crates/petgraph).

## Features

- **Automatic bidirectional indexes** - Efficient queries for both outgoing and incoming edges
- **UUID-based vertices** - Fixed-width 16-byte vertex IDs with proper ordering
- **Type-safe edge properties** - Fixed-width `(bool, f32)` tuple for `is_active` and `weight`
- **Atomic updates** - Both forward and reverse indexes updated in same transaction
- **Efficient traversal** - Range scans leverage tuple key ordering for O(k) queries
- **Batch operations** - High-throughput bulk loading with `add_edges_batch()`
- **Integration ready** - `EdgeSource` trait for external graph algorithm libraries

## Quick Start

```rust
use manifold::column_family::ColumnFamilyDatabase;
use manifold_graph::{GraphTable, GraphTableRead};
use uuid::Uuid;

// Open database and column family
let db = ColumnFamilyDatabase::open("my.db")?;
let cf = db.column_family_or_create("social")?;

let user1 = Uuid::new_v4();
let user2 = Uuid::new_v4();

// Write edges
{
    let write_txn = cf.begin_write()?;
    let mut graph = GraphTable::open(&write_txn, "follows")?;
    
    graph.add_edge(&user1, "follows", &user2, true, 1.0)?;
    
    drop(graph);
    write_txn.commit()?;
}

// Read with efficient traversal
let read_txn = cf.begin_read()?;
let graph = GraphTableRead::open(&read_txn, "follows")?;

// Query outgoing edges
for edge_result in graph.outgoing_edges(&user1)? {
    let edge = edge_result?;
    println!("{:?} -[{}]-> {:?}", edge.source, edge.edge_type, edge.target);
}

// Query incoming edges
for edge_result in graph.incoming_edges(&user2)? {
    let edge = edge_result?;
    println!("{:?} <-[{}]- {:?}", edge.target, edge.edge_type, edge.source);
}
```

## Batch Operations

For high-throughput graph loading, use batch operations which leverage Manifold's WAL group commit:

```rust
let edges = vec![
    (user1, "follows", user2, true, 1.0),
    (user1, "follows", user3, true, 0.8),
    (user2, "follows", user3, true, 0.9),
];

let write_txn = cf.begin_write()?;
let mut graph = GraphTable::open(&write_txn, "follows")?;

// Insert all edges in one batch
let count = graph.add_edges_batch(edges, false)?;
println!("Inserted {} edges", count);

drop(graph);
write_txn.commit()?;
```

## Edge Properties

Edges store two fixed-width properties:

- `is_active: bool` - For active/passive edges, soft deletes, hidden edges
- `weight: f32` - General-purpose edge weight or score

These are stored as a fixed-width tuple `(bool, f32)` for zero-overhead serialization (5 bytes total).

## Architecture

### Bidirectional Storage

Each `GraphTable` maintains two internal tables:
- **Forward table**: `(source, edge_type, target) -> (is_active, weight)`
- **Reverse table**: `(target, edge_type, source) -> (is_active, weight)`

Both tables are updated atomically, enabling efficient queries in both directions.

### Performance Characteristics

- **Write**: O(log n) × 2 for forward + reverse B-tree inserts, benefits from WAL group commit
- **Read outgoing**: O(log n) lookup + O(k) scan where k = outgoing edge count
- **Read incoming**: O(log n) lookup + O(k) scan where k = incoming edge count
- **Key size**: ~37-40 bytes (32 bytes UUIDs + 5-8 bytes edge type)
- **Value size**: 5 bytes fixed-width (1 byte bool + 4 bytes f32)

## Integration with Graph Libraries

The `EdgeSource` trait enables integration with external graph algorithm libraries:

```rust
use manifold_graph::EdgeSource;
use petgraph::graph::DiGraph;

let read_txn = cf.begin_read()?;
let graph = GraphTableRead::open(&read_txn, "links")?;

// Build petgraph DiGraph
let mut pg_graph: DiGraph<Uuid, f32> = DiGraph::new();
let mut node_map = HashMap::new();

// Add nodes...
for page in &pages {
    let idx = pg_graph.add_node(page.id);
    node_map.insert(page.id, idx);
}

// Add edges using EdgeSource
for edge_result in graph.iter_edges()? {
    let edge = edge_result?;
    if edge.is_active {
        pg_graph.add_edge(
            node_map[&edge.source],
            node_map[&edge.target],
            edge.weight
        );
    }
}

// Run petgraph algorithms
let pagerank = /* compute PageRank */;
let sccs = kosaraju_scc(&pg_graph);
```

## Examples

The crate includes comprehensive examples demonstrating real-world usage:

### 1. Social Network (`examples/social_network.rs`)
Twitter-like social network with:
- Multiple edge types (follows, blocks, mutes)
- Bidirectional queries (followers/following)
- Edge property updates
- Mutual connection detection

```bash
cargo run --example social_network -p manifold-graph
```

### 2. Petgraph Integration (`examples/petgraph_integration.rs`)
Integration with petgraph for:
- PageRank algorithm
- Strongly connected components
- Shortest paths (Dijkstra)
- Centrality measures

```bash
cargo run --example petgraph_integration -p manifold-graph
```

### 3. Knowledge Graph (`examples/knowledge_graph.rs`)
Movie/entertainment knowledge graph with:
- Multiple entity types (Person, Film, Studio, Genre)
- Rich relationship types (directed, acted_in, produced_by, genre_of)
- Multi-hop queries
- Recommendation engine

```bash
cargo run --example knowledge_graph -p manifold-graph
```

### 4. Dependency Graph (`examples/dependency_graph.rs`)
Package management graph with:
- Cycle detection (DFS)
- Topological sorting (Kahn's algorithm)
- Impact analysis (what breaks if X is removed)
- Dependency depth analysis

```bash
cargo run --example dependency_graph -p manifold-graph
```

## Use Cases

- **Social networks** - Followers, friends, blocks, mutes
- **Knowledge graphs** - Entity relationships, semantic networks
- **Dependency tracking** - Package management, build systems
- **Recommendation systems** - Product relationships, collaborative filtering
- **Network analysis** - Web graphs, citation networks
- **Access control** - Permission graphs, role hierarchies

## Combining with Other Domain Layers

`manifold-graph` works seamlessly with other manifold domain layers in the same database:

```rust
use manifold::column_family::ColumnFamilyDatabase;
use manifold_graph::GraphTable;
use manifold_vectors::VectorTable;

let db = ColumnFamilyDatabase::open("my_app.db")?;

// Use graphs and vectors in the same database
let graph_cf = db.column_family_or_create("social")?;
let vectors_cf = db.column_family_or_create("embeddings")?;

// Store social graph
let txn = graph_cf.begin_write()?;
let mut graph = GraphTable::open(&txn, "follows")?;
graph.add_edge(&user1, "follows", &user2, true, 1.0)?;

// Store user embeddings
let txn = vectors_cf.begin_write()?;
let mut vectors = VectorTable::<768>::open(&txn, "user_vectors")?;
vectors.insert("user1", &embedding)?;
```

## Requirements

- Rust 1.70+ (for const generics)
- `manifold` with `uuid` feature enabled

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Contributing

Contributions are welcome! This crate follows the patterns established in `manifold-vectors`.

## Related Crates

- [manifold](https://crates.io/crates/manifold) - Core embedded database
- [manifold-vectors](https://crates.io/crates/manifold-vectors) - Vector storage for embeddings
- [petgraph](https://crates.io/crates/petgraph) - Graph algorithms library
