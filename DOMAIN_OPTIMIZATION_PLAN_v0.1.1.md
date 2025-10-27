# Manifold Domain Optimization Plan v0.1.1

## Executive Summary

Manifold has achieved its goal as a high-performance, general-purpose embedded column family database with concurrent writes (451K ops/sec), WAL-based durability, and full WASM support. This document outlines specialized optimizations for domain-specific workloads while maintaining Manifold's general-purpose nature.

**Core Principle:** Keep Manifold's core general-purpose and build domain-specific functionality as optional layers on top, implemented as separate crates that depend on Manifold.

**Target Use Case:** Hyperspatial - a multi-modal database requiring vectors (dense, sparse, multi-vector), graph relationships, structured data, time series, and hyperbolic spatial indexing within a single unified system.

**Architecture Strategy:** Use column families as logical collections (e.g., "news_articles", "user_profiles"), with multiple specialized tables within each collection for different data types and indexes. This enables atomic updates across related data while maintaining clean separation of concerns.

---

## Context: Tables Within Column Families

A critical architectural clarification: Manifold's power comes from organizing data into **column families** that contain multiple **tables**.

**Column Family = Logical Collection**
- Example: "news_articles" column family
- Represents a cohesive domain entity
- Shares write transaction isolation
- All tables within can be updated atomically

**Tables = Different Data Types/Indexes**
- Example tables within "news_articles":
  - `articles` - Document content (String → Article)
  - `vectors_dense` - Dense embeddings (String → [f32; 768])
  - `vectors_sparse` - Sparse features (String → SparseVector)
  - `metadata` - Timestamps, authors (String → Metadata)
  - `sentiment` - Sentiment scores (String → f32)

This organization enables:
- **Atomic updates** - Insert article + embedding + metadata in one transaction
- **Efficient queries** - Range scan metadata, look up embedding by article ID
- **Clean separation** - Different tables for different data shapes
- **Concurrent access** - Multiple column families allow parallel writes to different collections

**Only use separate column families for truly independent collections** (e.g., "news_articles", "user_profiles", "chat_messages").

---

## Context: Why Domain Layers?

Manifold's core provides primitives: key-value storage, transactions, iteration, WAL. While these are sufficient for any workload, specialized domains benefit from higher-level abstractions:

**Vector Workloads** need:
- Fixed-width storage for efficient memory access
- Zero-copy reads to avoid allocation overhead
- Batch insertion optimized for bulk embedding storage
- Integration points for external vector indexes (HNSW, IVF-PQ)

**Graph Workloads** need:
- Composite keys for edge representation
- Bidirectional edge queries (forward and reverse)
- Efficient "all edges from vertex X" scans
- Graph algorithm integration

**Time Series Workloads** need:
- Timestamp-based key ordering
- Downsampling across multiple granularities
- Retention policy enforcement
- Efficient range queries by time

**Hyperbolic Space Workloads** need:
- Specialized distance functions (hyperbolic, not Euclidean)
- Spatial indexing preserving hyperbolic geometry
- Support for Poincaré disk and hyperboloid models
- Integration with hyperbolic ML libraries

Rather than bloating Manifold's core with domain-specific code, we build these as **optional layers** that users can choose based on their needs.

---

## Architecture: Domain Layers as Separate Crates

Each domain optimization is implemented as a separate crate:

```
manifold-core (this repository)
    ↓ (dependency)
manifold-vectors     (crate: manifold-vectors)
manifold-graph       (crate: manifold-graph)
manifold-timeseries  (crate: manifold-timeseries)
manifold-hyperbolic  (crate: manifold-hyperbolic)
    ↓ (used by)
hyperspatial-manifold (orchestration layer)
```

**Benefits:**
1. **Maintainability** - Core stays focused, domain layers evolve independently
2. **Optional dependencies** - Users only include what they need
3. **Independent versioning** - Domain layers can release updates without core changes
4. **Clean testing** - Each layer tested in isolation
5. **Community contributions** - Domain experts can maintain specific layers

**Integration Pattern:**
- Domain layers provide trait-based abstractions (`VectorStore`, `GraphStore`)
- These traits wrap Manifold's `ColumnFamily` with specialized methods
- Users interact with domain-specific APIs, not raw key-value operations
- Under the hood, domain layers use Manifold's tables efficiently

---

## Phase 1: Vector Table Optimization

**Status:** Not Started

**Objective:** Provide efficient storage and access patterns for vector embeddings (dense, sparse, multi-vector formats) commonly used in ML/AI applications.

**Estimated Time:** 12-16 hours

### Context

Vector embeddings are ubiquitous in modern AI applications:
- **Sentence embeddings** - Dense 768/1024-dimensional vectors from BERT, RoBERTa
- **Image embeddings** - Dense 512/2048-dimensional vectors from ResNet, CLIP
- **Sparse features** - TF-IDF, BM25 scores with thousands of dimensions but < 1% non-zero
- **Multi-vectors** - Colbert-style token embeddings (sequence of dense vectors)

Current approach: Store as serialized bytes. This works but is inefficient:
- Deserialization overhead on every read
- Allocations for temporary buffers
- No type safety at compile time
- Difficult to integrate with vector index libraries

### Design

Create `manifold-vectors` crate providing:

**1. VectorTable<const DIM: usize>** - Fixed-dimension dense vectors
```rust
// Example usage
let vectors: VectorTable<768> = cf.vector_table("embeddings")?;
vectors.insert("doc_123", &embedding)?;
let vec: &[f32; 768] = vectors.get_zero_copy("doc_123")?;
```

**2. SparseVectorTable** - Efficient sparse vector storage
```rust
// Store only non-zero entries
let sparse: SparseVectorTable = cf.sparse_vector_table("features")?;
sparse.insert("doc_123", &[(0, 0.5), (42, 0.8), (1000, 0.3)])?;
```

**3. MultiVectorTable<const DIM: usize>** - Sequence of vectors (Colbert)
```rust
// Store variable-length sequences
let multi: MultiVectorTable<128> = cf.multi_vector_table("tokens")?;
multi.insert("doc_123", &[vec1, vec2, vec3])?;
```

**4. Integration helpers** - Connect to external indexes
```rust
// Build HNSW index from VectorTable
let index = HnswIndex::from_table(&vectors, params)?;
```

### Implementation Tasks

- [ ] **1.1: VectorTable core implementation**
  - Implement `VectorTable<const DIM: usize>` struct
  - Use `fixed_width()` trait for zero-copy access
  - Validate dimension at compile time
  - Support f32 and f64 element types
  - **Dev Notes:**

- [ ] **1.2: SparseVectorTable implementation**
  - Implement compressed sparse format (COO or CSR)
  - Efficient serialization/deserialization
  - Support common operations (dot product, cosine similarity)
  - **Dev Notes:**

- [ ] **1.3: MultiVectorTable implementation**
  - Variable-length sequence storage
  - Efficient iteration over token vectors
  - Padding/truncation helpers
  - **Dev Notes:**

- [ ] **1.4: Batch operations**
  - Batch insert optimized for bulk embedding storage
  - Parallel processing for batch operations
  - Progress callbacks for large batches
  - **Dev Notes:**

- [ ] **1.5: Index integration**
  - Trait for external index libraries
  - HNSW index builder example
  - Incremental index updates
  - **Dev Notes:**

- [ ] **1.6: Examples and documentation**
  - RAG (Retrieval Augmented Generation) example
  - Image similarity search example
  - Benchmark comparison vs raw bytes
  - **Dev Notes:**

### Success Criteria

- ✅ Zero-copy vector access for fixed-width types
- ✅ 10x faster than serialized bytes for large vectors
- ✅ Type-safe API with compile-time dimension checking
- ✅ Examples demonstrate integration with ML libraries
- ✅ Comprehensive documentation

---

## Phase 2: Graph Table Optimization

**Status:** Not Started

**Objective:** Efficient edge storage and traversal for graph workloads with support for property graphs and labeled edges.

**Estimated Time:** 10-14 hours

### Context

Graph databases are essential for:
- **Social networks** - Friends, follows, likes
- **Knowledge graphs** - Entity relationships, ontologies
- **Recommendation systems** - User-item interactions
- **Network analysis** - Communication patterns, dependencies

Current approach: Store edges as key-value pairs. This works but requires careful key design:
- Forward edges need one key format
- Reverse edges need another (for bidirectional traversal)
- Finding "all edges from vertex X" requires prefix scans
- Edge properties need separate storage or embedded serialization

### Design

Create `manifold-graph` crate providing:

**1. Composite Key Format**
```rust
// Edge key: {source}|{edge_type}|{target}
// Enables efficient queries:
// - All edges from vertex: range scan "{source}|"
// - All edges of type: range scan "*|{edge_type}|"
// - Specific edge: exact lookup "{source}|{edge_type}|{target}"
```

**2. GraphTable API**
```rust
let graph: GraphTable = cf.graph_table("edges")?;

// Add edge with properties
graph.add_edge("user_123", "follows", "user_456", properties)?;

// Query patterns
for edge in graph.outgoing_edges("user_123")? {
    // Iterate outgoing edges
}

for edge in graph.incoming_edges("user_456")? {
    // Iterate incoming edges (uses reverse index)
}

for edge in graph.edges_of_type("follows")? {
    // All edges of specific type
}
```

**3. Bidirectional Support**
- Automatic reverse index maintenance
- Choose forward-only or bidirectional at creation
- Atomic updates to both indexes

### Implementation Tasks

- [ ] **2.1: Composite key encoding**
  - Implement efficient key serialization
  - Support different vertex ID types (String, u64, UUID)
  - Null-safe encoding for range scans
  - **Dev Notes:**

- [ ] **2.2: GraphTable core implementation**
  - Add/remove edge operations
  - Forward and reverse index management
  - Atomic bidirectional updates
  - **Dev Notes:**

- [ ] **2.3: Traversal APIs**
  - Outgoing/incoming edge iterators
  - Edge type filtering
  - Property access
  - Batch operations
  - **Dev Notes:**

- [ ] **2.4: Graph algorithms integration**
  - Trait for graph algorithm libraries
  - BFS/DFS examples
  - PageRank example
  - Shortest path helpers
  - **Dev Notes:**

- [ ] **2.5: Property graph support**
  - Edge properties storage
  - Vertex properties storage
  - Typed property access
  - **Dev Notes:**

- [ ] **2.6: Examples and documentation**
  - Social network example
  - Knowledge graph example
  - Recommendation system example
  - Performance benchmarks
  - **Dev Notes:**

### Success Criteria

- ✅ Efficient edge traversal (< 1ms for 1000 edges)
- ✅ Atomic bidirectional edge updates
- ✅ Flexible property graph support
- ✅ Integration with graph algorithm libraries
- ✅ Real-world examples

---

## Phase 3: Time Series Table Optimization

**Status:** Not Started

**Objective:** Efficient storage and querying of time-series data with automatic downsampling and retention policies.

**Estimated Time:** 8-12 hours

### Context

Time series data is critical for:
- **Metrics and monitoring** - System performance, application metrics
- **IoT sensor data** - Temperature, pressure, location tracking
- **Financial data** - Stock prices, trading volumes
- **Event logs** - Application events, user activity

Challenges:
- **High write volume** - Thousands of data points per second
- **Range queries** - "Show me last hour/day/week"
- **Downsampling** - Store raw + aggregated data at multiple granularities
- **Retention** - Delete old data automatically

### Design

Create `manifold-timeseries` crate providing:

**1. Timestamp-Prefixed Keys**
```rust
// Key format: {timestamp}|{series_id}
// Enables:
// - Range queries by time
// - Efficient iteration in time order
// - Series-level partitioning
```

**2. TimeSeriesTable API**
```rust
let ts: TimeSeriesTable = cf.timeseries_table("metrics")?;

// Write data point
ts.write("cpu.usage", timestamp, value)?;

// Range query
for (timestamp, value) in ts.range("cpu.usage", start, end)? {
    // Process time range
}
```

**3. Multi-Granularity Storage**
```rust
// Within column family "metrics":
// - raw table: full resolution
// - minute table: 1-minute aggregates
// - hour table: hourly rollups
// - day table: daily summaries

let tables = TimeSeriesTables::new(&cf, "cpu.usage")?;
tables.write_raw(timestamp, value)?;
// Automatic downsampling to minute/hour/day tables
```

### Implementation Tasks

- [ ] **3.1: Key encoding**
  - Timestamp-prefixed key format
  - Support different timestamp resolutions (ms, us, ns)
  - Series ID encoding
  - **Dev Notes:**

- [ ] **3.2: TimeSeriesTable implementation**
  - Write/read operations
  - Range queries
  - Efficient iteration
  - **Dev Notes:**

- [ ] **3.3: Downsampling system**
  - Automatic aggregation (min, max, avg, sum, count)
  - Background downsampling task
  - Configurable granularities
  - **Dev Notes:**

- [ ] **3.4: Retention policies**
  - Automatic deletion of old data
  - Per-granularity retention
  - Background cleanup task
  - **Dev Notes:**

- [ ] **3.5: Aggregation queries**
  - Query across multiple granularities
  - Automatic granularity selection
  - Gap filling and interpolation
  - **Dev Notes:**

- [ ] **3.6: Examples and documentation**
  - Metrics collection example
  - IoT sensor data example
  - Performance benchmarks
  - **Dev Notes:**

### Success Criteria

- ✅ High write throughput (> 100K points/sec)
- ✅ Efficient range queries
- ✅ Automatic downsampling working
- ✅ Retention policies enforced
- ✅ Examples for common patterns

---

## Phase 4: Hyperbolic Space Optimization

**Status:** Not Started

**Objective:** Specialized storage and indexing for hyperbolic embeddings used in hierarchical representation learning.

**Estimated Time:** 16-20 hours (most complex)

### Context

Hyperbolic embeddings are increasingly important in ML:
- **Hierarchical data** - Taxonomies, org charts, file systems
- **Knowledge graphs** - Ontologies with parent-child relationships
- **Social networks** - Community hierarchies
- **Natural language** - Semantic hierarchies, entailment

Why hyperbolic space?
- **Exponential volume growth** - Perfect for tree-like structures
- **Low distortion** - Embed trees with minimal distance distortion
- **Better than Euclidean** - 2D hyperbolic can embed arbitrary trees

Challenges:
- **Non-standard distance functions** - Hyperbolic distance, not Euclidean
- **Numerical stability** - Careful handling of Poincaré disk boundaries
- **Spatial indexing** - Traditional indexes (k-d trees, HNSW) don't work
- **Multiple models** - Poincaré disk, hyperboloid, half-space

For Hyperspatial specifically:
- 17-dimensional hyperbolic arrays (3 positions: graph, property, vector)
- Need efficient storage (17 × f64 = 136 bytes per point)
- Need hyperbolic distance-aware indexing
- Need integration with hyperbolic ML libraries

### Design

Create `manifold-hyperbolic` crate providing:

**1. HyperbolicTable<const DIM: usize>**
```rust
let hyper: HyperbolicTable<17> = cf.hyperbolic_table("positions")?;

// Store hyperbolic coordinates
hyper.insert("entity_123", &coords)?;

// Zero-copy access
let coords: &[f64; 17] = hyper.get_zero_copy("entity_123")?;

// Distance queries (hyperbolic distance)
let neighbors = hyper.nearest("entity_123", k=10)?;
```

**2. Multiple Model Support**
```rust
enum HyperbolicModel {
    PoincareDisc,
    Hyperboloid,
    HalfSpace,
}

// Convert between models
let hyperboloid_coords = poincare_to_hyperboloid(&coords);
```

**3. Spatial Index**
```rust
// Hyperbolic ball tree or custom index structure
let index: HyperbolicIndex<17> = HyperbolicIndex::build(&table)?;

// Range queries in hyperbolic space
let points = index.range_query(center, radius)?;

// k-NN queries
let neighbors = index.knn(query, k=10)?;
```

### Implementation Tasks

- [ ] **4.1: HyperbolicTable implementation**
  - Fixed-width storage for coordinates
  - Zero-copy access
  - Support f32 and f64
  - **Dev Notes:**

- [ ] **4.2: Distance functions**
  - Poincaré disk distance
  - Hyperboloid distance
  - Half-space distance
  - Numerical stability handling
  - **Dev Notes:**

- [ ] **4.3: Model conversions**
  - Poincaré ↔ Hyperboloid
  - Poincaré ↔ Half-space
  - Stereographic projections
  - **Dev Notes:**

- [ ] **4.4: Spatial indexing**
  - Research hyperbolic spatial index structures
  - Implement hyperbolic ball tree or equivalent
  - Range queries
  - k-NN queries
  - **Dev Notes:**

- [ ] **4.5: Integration with ML libraries**
  - Trait for hyperbolic embedding libraries
  - Geoopt integration (PyTorch)
  - Hyperbolic gradient descent support
  - **Dev Notes:**

- [ ] **4.6: Examples and documentation**
  - Hierarchical embedding example
  - Tree embedding example
  - Visualization helpers
  - Performance benchmarks
  - **Dev Notes:**

### Success Criteria

- ✅ Efficient storage for high-dimensional hyperbolic coordinates
- ✅ Correct hyperbolic distance functions
- ✅ Spatial index for range/k-NN queries
- ✅ Integration with hyperbolic ML libraries
- ✅ Comprehensive documentation and examples

---

## Integration: HyperspatialManifold

**Objective:** Orchestration layer combining all domain optimizations for the Hyperspatial use case.

### Design

`HyperspatialManifold` manages multiple column families with specialized tables:

```rust
pub struct HyperspatialManifold {
    db: ColumnFamilyDatabase,
    // Each entity type gets a column family
}

impl HyperspatialManifold {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let db = ColumnFamilyDatabase::open(path)?;
        Ok(Self { db })
    }
    
    pub fn create_collection(&self, name: &str) -> Result<Collection> {
        // Create column family with all specialized tables
        let cf = self.db.column_family_or_create(name)?;
        
        Collection::new(cf)
    }
}

pub struct Collection {
    // Specialized tables within one column family
    properties: Table,
    edges_forward: GraphTable,
    edges_reverse: GraphTable,
    vectors_dense: VectorTable<768>,
    vectors_sparse: SparseVectorTable,
    vectors_multi: MultiVectorTable<128>,
    hyperbolic_graph: HyperbolicTable<17>,
    hyperbolic_property: HyperbolicTable<17>,
    hyperbolic_vector: HyperbolicTable<17>,
    metadata: TimeSeriesTable,
}
```

### Example Usage

```rust
// Open database
let manifold = HyperspatialManifold::new("my_data.db")?;

// Create collection for news articles
let news = manifold.create_collection("news_articles")?;

// Atomic write across multiple tables
let txn = news.begin_write()?;
txn.properties.insert("article_123", &article)?;
txn.vectors_dense.insert("article_123", &embedding)?;
txn.edges_forward.add_edge("article_123", "cites", "article_456", None)?;
txn.hyperbolic_graph.insert("article_123", &graph_position)?;
txn.commit()?;

// Query
let similar = news.vectors_dense.nearest("article_123", k=10)?;
let related = news.edges_forward.outgoing_edges("article_123")?;
```

---

## Implementation Strategy

### Sequencing

1. **Start with VectorTable** (Phase 1)
   - Most common use case
   - Establishes patterns for other domains
   - Immediate value for ML workloads

2. **Add GraphTable** (Phase 2)
   - Complements vectors (RAG + knowledge graphs)
   - Patterns apply to other composite key scenarios

3. **Add TimeSeriesTable** (Phase 3)
   - Simpler than hyperbolic space
   - Useful for metrics and monitoring

4. **Add HyperbolicTable** (Phase 4)
   - Most complex
   - Benefits from lessons learned in earlier phases
   - Specific to Hyperspatial but demonstrates extensibility

### Development Process

For each phase:
1. **Design** - Write detailed design doc with examples
2. **Prototype** - Build core functionality in separate crate
3. **Test** - Comprehensive tests including performance benchmarks
4. **Document** - API docs, examples, integration guide
5. **Integrate** - Add to HyperspatialManifold orchestration layer

### Dependencies

- **Phase 1 (Vectors)** - No dependencies, can start immediately
- **Phase 2 (Graph)** - Independent, can run in parallel with Phase 1
- **Phase 3 (Time Series)** - Independent, can run in parallel
- **Phase 4 (Hyperbolic)** - Builds on VectorTable patterns from Phase 1
- **Integration** - Requires Phases 1-4 complete

### Timeline

Assuming sequential development:
- **Phase 1:** 2-3 weeks
- **Phase 2:** 2 weeks
- **Phase 3:** 1-2 weeks
- **Phase 4:** 3-4 weeks
- **Integration:** 1 week

**Total: 9-12 weeks for complete domain optimization suite**

Parallel development could reduce this to 6-8 weeks if multiple developers work on different phases simultaneously.

---

## Success Criteria

### Per-Phase Success

Each phase is considered complete when:
- ✅ Core functionality implemented and tested
- ✅ Performance benchmarks meet targets
- ✅ Comprehensive documentation written
- ✅ Examples demonstrate real-world usage
- ✅ Published as separate crate with clear versioning

### Overall Success

Domain optimization suite is complete when:
- ✅ All four phases complete and published
- ✅ HyperspatialManifold integration working
- ✅ Performance validated in production-like workloads
- ✅ Documentation enables users to adopt domain layers easily
- ✅ Community can contribute additional domain layers following established patterns

---

## Maintenance and Evolution

### Versioning Strategy

- **manifold-core:** Semantic versioning (currently tracking completion plan)
- **Domain crates:** Independent semantic versioning
- **Breaking changes:** Domain layers can evolve without affecting core
- **Compatibility:** Domain layers specify minimum manifold-core version

### Future Domain Layers

The pattern established here enables future domain-specific optimizations:
- **manifold-geospatial** - Geographic data and spatial indexes
- **manifold-document** - Full-text search integration
- **manifold-ml** - Direct integration with ML frameworks
- **manifold-crypto** - Blockchain and cryptographic data
- **Community contributions** - Others can build domain layers

### Performance Monitoring

Each domain layer should:
- Include benchmark suite
- Track performance across versions
- Document performance characteristics clearly
- Provide tuning guidance for different workloads

---

## Notes

- This plan is versioned (v0.1.1) to track evolution as we learn from implementation
- Update version when making substantial changes to approach or scope
- Each phase should update this document with learnings and design decisions
- Domain layers remain optional - users can use Manifold core directly if preferred
- Focus on enabling patterns, not prescribing solutions