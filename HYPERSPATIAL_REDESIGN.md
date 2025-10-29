# Hyperspatial Ecosystem Redesign

**Date:** 2024-10-29  
**Status:** Design Document  
**Goal:** Reorganize and rebuild the Hyperspatial ecosystem with clear component boundaries, simplified architecture, and Manifold as the storage foundation.

---

## Executive Summary

This document outlines a comprehensive redesign of the Hyperspatial ecosystem to create a modular, composable platform with clear component boundaries. The redesign eliminates complexity, consolidates duplicate functionality, and establishes Manifold as the storage foundation while keeping Hyperspatial focused on its core value: hyperbolic space operations.

### Key Principles

1. **Separation of Concerns**: Each component has a single, well-defined responsibility
2. **Plug-and-Play Architecture**: Components are independently useful and optionally composable
3. **Simplify, Don't Complicate**: Use Manifold's clean primitives instead of custom implementations
4. **Bottom-Up Rebuild**: Start fresh on a new branch, rewriting cleanly using the right abstractions
5. **Greenfield Approach**: No backwards compatibility constraints, design the optimal architecture

---

## Component Ownership Model

### Tessera (Embeddings Library)
**Status:** Stable, no changes needed  
**Responsibility:** Multi-paradigm embedding generation

- Dense single-vector embeddings (BGE, Nomic, GTE, Qwen, Jina)
- Multi-vector token embeddings (ColBERT)
- Sparse learned representations (SPLADE)
- Vision-language embeddings (ColPali)
- Time series forecasting (Chronos Bolt)
- GPU acceleration (Metal, CUDA)
- Standalone library with Rust + Python bindings

**Dependencies:** None  
**Used by:** Hyperspatial (optional), Haematite (optional), user applications

---

### Manifold (Storage Primitives)
**Status:** Stable, no changes needed  
**Responsibility:** Fast, reliable, ACID-compliant embedded storage

#### Core Features
- Column families for concurrent writes
- Write-ahead log (WAL) for fast commits (~0.5ms)
- MVCC transactions with snapshot isolation
- WASM support via OPFS

#### Domain-Specific Crates
- **manifold-vectors**: Dense, sparse, multi-vector storage with zero-copy access
- **manifold-timeseries**: Multi-granularity time series with delta encoding
- **manifold-graph**: Bidirectional graph storage with UUID vertices and typed edges

**Key Design Points:**
- Each column family is a separate file descriptor within a single database file
- This reduces file handles from (collections × shards × storage_types × separate_databases) to (collections × shards × 1 database with N column families)
- **Fixed-width UUID keys**: All entity IDs use 16-byte UUID keys for zero-copy deserialization
- Composite keys use UUID components where applicable (e.g., graph edges use UUID vertices)

**Dependencies:** None  
**Used by:** Hyperspatial, compiled queries, user applications

---

### HyperQL (Query Language)
**Status:** Needs refactoring for general-purpose use  
**Responsibility:** Multi-paradigm query language with compilation capabilities

#### Core Components
- **Parser**: Winnow-based parser producing AST
- **AST**: Abstract syntax tree for all query constructs
- **Compiler**: Transforms AST to execution plans
- **Optimizer**: Query optimization (predicate pushdown, constant folding, etc.)
- **Executor**: Executes plans against DataSource implementations
- **Type Checker**: Static type validation
- **Code Generator**: NEW - Compiles queries to Rust code

#### What HyperQL Should Own
- Query syntax and semantics
- Execution plan definitions
- Optimization rules
- DataSource trait (abstraction for backends)
- Geometric/vector operation types in execution plans
- Schema definitions (table structure, column types)
- Type system for entities, properties, values

#### What HyperQL Should NOT Own
- Actual execution of geometric operations (Hyperspatial's responsibility)
- Storage implementation details (Manifold's responsibility)
- Hyperbolic-specific logic (Hyperspatial's responsibility)
- Embedding generation (Tessera's responsibility)

**Dependencies:** None (general-purpose)  
**Used by:** Hyperspatial (implements DataSource), compiled queries, user applications

#### Workspace Structure
```
hyperQL/
├── src/                    # Core query language library
├── crates/
│   ├── hyperql-compile/    # NEW: Query compilation to Rust code
│   └── hyperql-treesitter/ # NEW: Tree-sitter grammar for editor integration
├── examples/
├── tests/
└── Cargo.toml
```

**New Crates:**
- **hyperql-compile**: Compiles HyperQL queries to type-safe Rust code
- **hyperql-treesitter**: Tree-sitter grammar for incremental parsing, syntax highlighting, and LSP integration

---

### Hyperspatial (Hyperbolic Space Database)
**Status:** Major redesign needed (70-75% of work here)  
**Responsibility:** Multi-paradigm database unified through hyperbolic space

#### Core Value Propositions
1. **Hyperbolic Positioning**: Self-supervised learning of geometric positions
2. **Sub-millisecond Similarity**: HNSW index for fast k-NN queries
3. **Multi-Modal Unification**: Graph + vectors + properties in unified space
4. **Cascade System**: Schema-driven hierarchical aggregations
5. **Stream Coordination**: Ephemeral streams for data workflows
6. **Server Infrastructure**: HTTP/WebSocket API for clients

#### What Hyperspatial Should Own

**Hyperbolic Space Operations**
- Hyperbolic distance calculations (hyperboloid model)
- Multi-modal positioning (graph, embedding, property positions)
- Position learning algorithms (degree-based, self-supervised)
- Trajectory tracking and drift analysis
- HNSW index adapted for hyperbolic space

**Cascade System**
- Cascade dependency graphs
- Cascade trigger index
- Cascade engine for propagation
- Aggregation functions (SUM, AVG, LATEST, etc.)
- Decay and time-window logic

**Stream System** (Ephemeral Coordination)
- Stream producers (database triggers, external sources)
- Stream consumers (processors, database writers)
- Stream routing and message passing
- **NOT persistent storage** - streams are coordination, not persistence
- Loop-back pattern (stream → external processor → database update)
- Example use cases:
  - Insert → stream → Tessera embedding → update entity
  - Scheduled check → stream → web scraper → update if changed
  - Database trigger → stream → external ML model → enrich data

**Server & Runtime**
- HTTP/WebSocket server (axum)
- Request coordination (async operations)
- Compute runtime (Lua and WASM)
- Custom function registry
- Connection management

**Import/Export**
- Data import from CSV, JSON, JSONL
- Batch operations
- Dependency resolution during import
- Position initialization strategies

**Indexing**
- **Hyperbolic HNSW**: HNSW adapted for hyperbolic distance (geometric similarity)
- **Vector Indexes**: HNSW/IVF-PQ for raw vector similarity (independent of hyperbolic positioning)
- **Graph Indexes**: Vertex-centric and label indexes for traversal optimization
- **Temporal Indexes**: Time-range indexes for time series queries
- **Global Position Index**: All entity positions for exact geometric queries
- **Type Indexes**: Entity type → entity list (in-memory DashMap)
- **Cache Layers**: Entity cache (50K), property cache (10K) with LRU eviction

**Integration Layer**
- DataSource implementation for HyperQL
- Geometric executor (hyperbolic distance operations)
- Vector executor (similarity operations with Tessera integration)
- Router for multi-collection access
- Schema engine for validation

#### What Hyperspatial Should NOT Own (Delegate to Manifold)

**Storage Primitives** - Use Manifold instead of custom implementations:
- ~~Custom vector storage with 5 redb databases~~ → manifold-vectors
- ~~Custom time series storage with columnar encoding~~ → manifold-timeseries
- ~~Custom graph storage with forward/reverse indexes~~ → manifold-graph
- ~~Complex edge properties system~~ → Simple bool + f32 from manifold-graph
- ~~Separate database files for core, properties, edges, etc.~~ → Column families

**Data to Remove**
- Problematic ML-based hyperbolic embedding code
- Redundant storage abstractions
- Overcomplicated edge property system
- Multiple database manager instances

#### Workspace Structure
```
hyperspatial/
├── src/              # Core hyperbolic database library
│   ├── hyperbolic/   # Positioning, distance, HNSW
│   ├── cascade/      # Cascade system
│   ├── streams/      # Stream coordination
│   ├── compute/      # Lua/WASM runtime (keep existing structure)
│   ├── coordination/ # Request coordination
│   ├── import/       # Import system
│   ├── query/        # HyperQL integration
│   └── lib.rs
├── crates/
│   ├── hyperspatial-cli/    # CLI tool with REPL, client, formatters
│   └── hyperspatial-server/ # Server binary (HTTP/WebSocket/gRPC)
├── examples/
├── tests/
└── Cargo.toml
```

**Crate Organization:**
- **hyperspatial-server**: Moved from `src/server/` to separate crate
  - HTTP server (axum)
  - WebSocket server
  - gRPC server (keep existing implementation)
  - Server configuration
  - Request handlers
- **hyperspatial-cli**: Exists separately, already has comprehensive structure
  - Commands: query, import, export, schema, describe, server, REPL
  - Client: HTTP, WebSocket, protocol
  - Formatters: JSON, CSV, table
  - REPL: completion, highlighting, hints

**Dependencies:**
- Manifold (storage)
- HyperQL (queries)
- Tessera (optional, for native embedding generation)

---

### Haematite (Reactive Notebooks)
**Status:** Future work, not part of initial redesign  
**Responsibility:** Mathematical thinking at Rust speed

Will integrate with the ecosystem once Hyperspatial redesign is complete.

---

## Architecture Diagrams

### Component Dependency Graph
```
┌─────────────┐
│  Haematite  │  (Future - reactive notebooks)
└──────┬──────┘
       │
       ├─────────────┬──────────────┬──────────────┐
       │             │              │              │
       v             v              v              v
┌──────────┐  ┌─────────────┐  ┌─────────┐  ┌──────────┐
│ HyperQL  │  │ Hyperspatial│  │ Tessera │  │ Manifold │
└──────────┘  └──────┬──────┘  └─────────┘  └──────────┘
                     │
                     ├──────────┬──────────┐
                     │          │          │
                     v          v          v
              ┌──────────┐ ┌─────────┐ ┌──────────┐
              │ HyperQL  │ │ Tessera │ │ Manifold │
              └──────────┘ └─────────┘ └──────────┘

Legend:
- Top layer depends on all below
- Hyperspatial uses all three primitives
- HyperQL, Tessera, Manifold are independent
```

### Storage Layer Consolidation

**Before (Current - Problematic):**
```
Collection "documents"
├── Shard 0000/
│   ├── core.redb              (1 DB)
│   ├── properties.redb        (1 DB)
│   ├── edges.redb             (1 DB)
│   ├── edge_properties.redb   (1 DB)
│   ├── measures.redb          (1 DB)
│   ├── trajectories.redb      (1 DB)
│   └── vectors/
│       ├── dense_vectors.redb     (1 DB)
│       ├── sparse_vectors.redb    (1 DB)
│       ├── multi_vectors.redb     (1 DB)
│       ├── binary_vectors.redb    (1 DB)
│       └── timeseries_vectors.redb (1 DB)
└── Total: 11 separate database files per shard

Issues:
- 11 file descriptors per shard
- Separate transaction logs
- No concurrent writes across types
- Complex coordination
```

**After (Redesign - Using Manifold):**
```
Collection "documents"
├── Shard 0000/
│   └── shard.manifold (1 ColumnFamilyDatabase)
│       ├── CF: entities          (entity metadata)
│       ├── CF: properties        (key-value properties)
│       ├── CF: edges             (graph relationships)
│       ├── CF: measures          (cascade values)
│       ├── CF: trajectories      (position history)
│       ├── CF: semantic_vectors  (VectorTable<768>)
│       ├── CF: code_vectors      (VectorTable<512>)
│       ├── CF: sparse_vectors    (SparseVectorTable)
│       └── CF: multi_vectors     (MultiVectorTable)
└── Total: 1 database file with N column families

Benefits:
- 1 file descriptor per shard (+ N CF file descriptors)
- Shared WAL and transaction infrastructure
- Concurrent writes across column families
- Simplified coordination
- ~0.5ms commit latency via WAL
```

---

## Key Design Decisions

### 1. Entity IDs Across Tables

**Question:** Can the same entity ID be used across multiple vector tables?

**Answer:** Yes. Each VectorTable is an independent Manifold table with its own key space. Entity "doc_123" can exist in:
- `semantic_vectors` (VectorTable<768>)
- `code_vectors` (VectorTable<512>)
- `sparse_vectors` (SparseVectorTable)

When retrieving embeddings:
```rust
// Same entity ID, different tables
let semantic = semantic_vectors.get("doc_123")?;
let code = code_vectors.get("doc_123")?;
let sparse = sparse_vectors.get("doc_123")?;
```

### 2. Combining Time Series and Vectors (Trajectories)

**Question:** How do we store position history (17D vectors over time)?

**Answer:** Use manifold-timeseries with a custom value type:

```rust
#[derive(Serialize, Deserialize)]
struct PositionSnapshot {
    graph_position: [f32; 17],
    embedding_position: [f32; 17],
    property_position: [f32; 17],
}

// TimeSeriesTable with composite key and custom value
let trajectories: TimeSeriesTable<(u64, &str), PositionSnapshot> = 
    TimeSeriesTable::new("trajectories")?;

// Store snapshot
trajectories.write(
    (timestamp_nanos, entity_id),
    PositionSnapshot { graph_position, embedding_position, property_position }
)?;

// Query range
for snapshot in trajectories.range(entity_id, start_time, end_time)? {
    let (timestamp, positions) = snapshot?;
    // All three positions available
}
```

### 3. Edge Properties Simplification

**Decision:** Adopt manifold-graph's simple model: `bool` (active/passive) + `f32` (weight/confidence)

**Rationale:**
- Covers 95% of use cases
- Simple and fast
- Aligns with Manifold's design
- For rare complex cases, store additional metadata in separate properties table

**Migration:**
- Remove complex edge property system from Hyperspatial
- Use GraphTable from manifold-graph
- Store edge metadata separately if needed

### 4. Compiled Queries vs. Hand-Written Functions

**Decision:** Compiled queries are for user-defined databases, not Hyperspatial internals

**Rationale:**
- Internal operations (HNSW updates, cascade propagation) are already in Rust
- No benefit to adding a compilation step for code we control
- Compiled queries valuable for users creating custom Manifold-based databases
- Example: Financial tick database with nanosecond-precision ingestion

**Use Cases for Compiled Queries:**
- Domain-specific databases (finance, IoT, analytics)
- Custom access patterns without writing Rust
- Type-safe query interfaces for applications
- Zero parsing overhead for known queries

### 5. Schema System

**Two-Mode Approach:**

**Mode 1: Inferred Schemas (Compiled Queries)**
- Queries imply schema through usage
- Type conflicts cause Rust compile errors
- Compiler optionally generates schema JSON
- No separate schema definition needed

**Mode 2: Explicit Schemas (Runtime Queries)**
- Define schema for validation
- HyperQL runtime enforces at query time
- Compiled queries validated at compile time
- Schema can be generated from compiled queries or defined explicitly

**Bridge:** Compiled queries can generate schema files that runtime queries consume

---

## Migration Strategy

### Phase 1: Preparation & Analysis

**Goals:**
1. Audit existing Hyperspatial code to identify what to keep vs. remove
2. Create component interface definitions
3. Design new module structure
4. Write comprehensive tests for core functionality to preserve

**Deliverables:**
- Interface definitions (traits, types)
- Test suite for hyperbolic operations
- Module map (old → new structure)

**Estimated Effort:** 1-2 weeks

---

### Phase 2: Manifold Storage Layer

**Goals:**
1. Replace all redb usage with Manifold column families
2. Implement storage adapters using manifold domain crates
3. Maintain API compatibility for upper layers

**Substeps:**

**2.1: Core Entity Storage**
- Replace CoreStorage with ColumnFamily for entities
- Implement in-memory type index (already exists, keep)
- Use EntityId as string key, serialize Entity as value
- Test: entity CRUD operations

**2.2: Properties Storage**
- Use ColumnFamily with composite key: `(entity_id, property_name)`
- Test: property operations, bulk updates

**2.3: Graph Storage**
- Replace edges.redb with manifold-graph GraphTable
- Simplify edge properties to bool + f32
- Test: edge CRUD, bidirectional queries

**2.4: Vector Storage**
- Create separate VectorTable instances per vector type/dimension
- Tables: semantic_vectors (VectorTable<768>), code_vectors (VectorTable<512>), etc.
- Use manifold-vectors instead of custom implementation
- Test: vector CRUD, bulk operations

**2.5: Time Series Storage**
- Use manifold-timeseries for measures
- Custom PositionSnapshot type for trajectories
- Test: time series queries, range scans

**2.6: Router Refactoring**
- Update Router to use ColumnFamilyDatabase
- Maintain Collection abstraction
- Update MetaIndex for routing
- Keep cache layers (entity cache, property cache)
- Test: multi-collection access, sharding

**Deliverables:**
- Complete storage layer using Manifold
- All tests passing
- Performance benchmarks showing equal or better performance

**Estimated Effort:** 4-6 weeks

---

### Phase 3: HyperQL Refactoring

**Goals:**
1. Make HyperQL general-purpose (remove Hyperspatial dependencies)
2. Move Hyperspatial-specific logic to Hyperspatial
3. Create hyperql-compile crate

**Substeps:**

**3.1: Core Query Language**
- Ensure parser, compiler, optimizer are backend-agnostic
- DataSource trait should work with any backend
- Remove Hyperspatial-specific assumptions
- Test: query parsing, compilation, optimization

**3.2: Type System Consolidation**
- Centralize entity, property, value types in HyperQL
- Hyperspatial imports and extends these types
- Test: type conversions, validation

**3.3: Execution Plan Extensions**
- Add geometric operation nodes to execution plans
- Add vector operation nodes
- Plans describe operations, don't execute them
- Test: plan generation, serialization

**3.4: HyperQL-Compile Crate**
- Create new crate in hyperQL/crates/hyperql-compile
- Implement type inference engine
- Implement Rust code generator using `quote` crate
- Generate functions using Manifold APIs
- Test: simple SELECT queries → Rust code

**Deliverables:**
- General-purpose HyperQL library
- hyperql-compile crate with basic code generation
- Clear separation from Hyperspatial

**Estimated Effort:** 3-4 weeks

---

### Phase 4: Hyperspatial Rebuild

**Goals:**
1. Rebuild Hyperspatial on Manifold + HyperQL foundation
2. Focus on core value: hyperbolic operations
3. Remove unnecessary complexity
4. Implement DataSource for HyperQL integration

**Substeps:**

**4.1: Hyperbolic Core**
- Keep: Distance calculations, position learning, trajectory analysis
- Remove: Problematic ML-based embedding code
- Update: Use manifold-vectors for position storage
- Test: Distance calculations, positioning algorithms

**4.2: HNSW Index**
- Adapt existing HNSW for Manifold storage
- Use manifold-vectors for position vectors
- Optimize for hyperbolic distance
- Test: k-NN queries, index building

**4.3: Cascade System**
- Keep: Dependency graphs, trigger index, engine
- Update: Use new storage layer
- Simplify: Remove unnecessary abstractions
- Test: Cascade propagation, aggregations

**4.4: Stream System**
- Implement ephemeral stream coordination
- Database triggers → streams
- External processors ↔ streams
- Loop-back to database
- Test: Stream workflows, producer/consumer patterns

**4.5: Server & Coordination**
- HTTP/WebSocket API using axum
- Request coordination for async ops
- Compute runtime (Lua/WASM)
- Test: API endpoints, concurrent requests

**4.6: HyperQL Integration**
- Implement DataSource trait using new storage
- Geometric executor for hyperbolic operations
- Vector executor with optional Tessera integration
- Test: HyperQL queries against Hyperspatial

**4.7: Import System**
- CSV, JSON, JSONL readers
- Batch operations using Manifold's bulk APIs
- Position initialization strategies
- Test: Large dataset imports

**Deliverables:**
- Clean, focused Hyperspatial implementation
- All core features working with new architecture
- Performance meeting or exceeding current system

**Estimated Effort:** 6-8 weeks

---

### Phase 5: Tessera Integration

**Goals:**
1. Add native embedding generation to HyperQL
2. Enable auto-embedding in Hyperspatial
3. Maintain Tessera as standalone (no changes to Tessera itself)

**Substeps:**

**5.1: HyperQL Embedding Functions**
- Add embedding functions to HyperQL: `embed_dense()`, `embed_multi()`, etc.
- Functions call Tessera when available
- Graceful degradation if Tessera not present
- Test: Embedding function parsing, compilation

**5.2: Hyperspatial Auto-Embedding**
- Configure Tessera models for collections
- Auto-embed on INSERT if configured
- Store embeddings using manifold-vectors
- Test: Auto-embedding workflows

**5.3: Compiled Query Integration**
- Generate code that calls Tessera APIs
- Type-safe embedding generation in compiled queries
- Test: Compiled queries with embeddings

**Deliverables:**
- Seamless Tessera integration
- Native embedding generation in queries
- Optional feature flag for Tessera dependency

**Estimated Effort:** 2-3 weeks

---

### Phase 6: Testing & Optimization

**Goals:**
1. Comprehensive test coverage
2. Performance optimization
3. Documentation
4. Examples

**Substeps:**

**6.1: Testing**
- Unit tests for all modules
- Integration tests for workflows
- Performance benchmarks
- Stress tests
- Test: All functionality under load

**6.2: Performance**
- Profile hot paths
- Optimize HNSW operations
- Tune cache sizes
- Optimize stream throughput
- Benchmark: Compare to current system

**6.3: Documentation**
- Architecture documentation
- API documentation
- Query language guide
- Migration guide for users
- Examples for common patterns

**Deliverables:**
- Production-ready system
- Complete documentation
- Performance validated

**Estimated Effort:** 3-4 weeks

---

## Implementation Phases Timeline

| Phase | Duration | Dependencies | Deliverable |
|-------|----------|--------------|-------------|
| Phase 1: Preparation | 1-2 weeks | None | Interface definitions, test suite, module map |
| Phase 2: Manifold Storage | 4-6 weeks | Phase 1 | Complete storage layer on Manifold |
| Phase 3: HyperQL Refactor | 3-4 weeks | Phase 1 | General-purpose HyperQL + compile crate |
| Phase 4: Hyperspatial Rebuild | 6-8 weeks | Phases 2, 3 | Clean Hyperspatial implementation |
| Phase 5: Tessera Integration | 2-3 weeks | Phase 4 | Native embedding generation |
| Phase 6: Testing & Optimization | 3-4 weeks | Phases 2-5 | Production-ready system |

**Total Estimated Duration:** 19-27 weeks (4.5-6 months)

---

### What Gets Deleted or Reorganized

### From Hyperspatial

**Storage Implementations (Replace with Manifold):**
- `src/persistence/storage/core.rs` → Use ColumnFamily with UUID keys
- `src/persistence/storage/properties.rs` → Use ColumnFamily with composite (UUID, property_name) keys
- `src/persistence/storage/edges.rs` → Use manifold-graph with UUID vertices
- `src/persistence/storage/edge_properties.rs` → Simplify to bool + f32 in manifold-graph
- `src/persistence/storage/measures.rs` → Use manifold-timeseries
- `src/persistence/storage/trajectories.rs` → Use manifold-timeseries with custom PositionSnapshot type
- `src/persistence/vectors/storage/*` → Use manifold-vectors with separate tables per vector type
- `src/persistence/timeseries/storage/*` → Use manifold-timeseries

**Server Code (Move to hyperspatial-server crate):**
- `src/server/mod.rs`
- `src/server/http.rs`
- `src/server/websocket.rs`
- `src/server/grpc.rs` (keep gRPC implementation)
- `src/server/config.rs`
- `src/server/generated/*` (gRPC generated code)

**Problematic ML Code:**
- `src/hyperbolic/learning/embeddings.rs` (if problematic)
- `src/hyperbolic/forces/*` (if using ML-based positioning)
- Any gradient descent or ML training code that didn't work

**Redundant Abstractions:**
- Multiple DatabaseManager instances
- Complex edge property serialization
- Custom vector compression (use Manifold's)
- Duplicate type definitions between Hyperspatial and HyperQL

### From HyperQL

**Hyperspatial-Specific Code:**
- Hard-coded references to Hyperspatial types
- Execution logic that belongs in Hyperspatial
- Any storage-specific assumptions

---

## What Gets Kept

### From Hyperspatial

**Core Value (Hyperbolic Space):**
- `src/hyperbolic/geometry/*` - Distance calculations, coordinate systems
- `src/hyperbolic/hyperboloid/*` - Hyperboloid model operations
- `src/hyperbolic/trajectory/*` - Drift, velocity, acceleration analysis
- `src/hyperbolic/properties/*` - Property fingerprinting and similarity
- `src/persistence/indices/hnsw.rs` - HNSW index for hyperbolic space
- `src/persistence/indices/global.rs` - Global position index

**Cascade System:**
- `src/cascade/*` - Complete cascade system

**Stream System:**
- `src/streams/*` - Complete stream coordination system

**Compute Runtime (Keep existing structure, audit for cleanup):**
- `src/compute/mod.rs` - Compute layer coordination
- `src/compute/lua/` - Lua runtime (6 files, well-organized)
  - `mod.rs` - Module definition
  - `runtime.rs` - Lua VM initialization
  - `sandbox.rs` - Sandboxing and security
  - `api_database.rs` - Database API bindings
  - `api_hyperbolic.rs` - Hyperbolic operations API
  - `functions.rs` - Custom function registry
- `src/compute/wasm/` - WASM runtime (5 files, well-organized)
  - `mod.rs` - Module definition
  - `runtime.rs` - WASM VM initialization
  - `memory.rs` - Memory management
  - `modules.rs` - Module loading
  - `host_functions.rs` - Host function bindings

**Coordination:**
- `src/coordination/*` - Request coordination (keep as-is)

**Import:**
- `src/import/*` - Import system (update to use Manifold APIs)

**Caching:**
- Entity cache, property cache (in Router)
- Type index (in-memory DashMap)

### From HyperQL

**All Core Components:**
- Parser, AST, compiler, optimizer
- Type checker, validator
- Execution plan definitions
- DataSource trait

---

## Success Criteria

### Functional Requirements
- [ ] All current Hyperspatial features working with new architecture
- [ ] HyperQL queries execute correctly
- [ ] Compiled queries generate valid Rust code
- [ ] Cascade system propagates correctly
- [ ] Stream system coordinates workflows
- [ ] Import system handles large datasets
- [ ] HNSW index provides sub-millisecond queries

### Performance Requirements
- [ ] Query latency ≤ current system (22-70μs for k-NN)
- [ ] WAL commits ~0.5ms (10x improvement over current)
- [ ] Cache hit rates ≥ 90% after warmup
- [ ] Import throughput ≥ current system
- [ ] Memory usage ≤ 2x current system

### Code Quality
- [ ] Module boundaries clear and enforced
- [ ] No circular dependencies
- [ ] Test coverage > 80%
- [ ] All public APIs documented
- [ ] Examples for common patterns

### Architecture
- [ ] Components independently usable
- [ ] Clean dependency graph (no cycles)
- [ ] Manifold used throughout (no redb)
- [ ] HyperQL general-purpose (no Hyperspatial deps)
- [ ] Hyperspatial focused on hyperbolic operations

---

## Risk Analysis

### Technical Risks

**Risk 1: Performance Regression**
- **Mitigation:** Comprehensive benchmarking at each phase
- **Fallback:** Optimize hot paths, tune Manifold configuration

**Risk 2: Unforeseen Manifold Limitations**
- **Mitigation:** Early proof-of-concept for critical operations
- **Fallback:** Extend Manifold if needed, or workaround

**Risk 3: Complex Migration Logic**
- **Mitigation:** Greenfield approach (no migration needed)
- **Fallback:** N/A (no production data)

**Risk 4: Timeline Underestimation**
- **Mitigation:** Phased approach with clear milestones
- **Fallback:** Adjust scope or timeline based on progress

### Resource Risks

**Risk 1: Single Developer**
- **Mitigation:** Clear documentation, frequent commits
- **Fallback:** Bring in help if needed

**Risk 2: Scope Creep**
- **Mitigation:** Strict adherence to design document
- **Fallback:** Defer non-essential features to future versions

---

## Open Questions

1. **Vector Table Naming:** How to dynamically create/manage named vector tables?
   - **Proposal:** Router maintains registry of vector table names → VectorTable instances
   
2. **Schema Storage:** Where to persist schema definitions?
   - **Proposal:** Use Manifold column family for schema metadata

3. **HNSW Serialization:** How to persist HNSW index efficiently?
   - **Current:** Custom format, works well
   - **Proposal:** Keep current approach, store in Manifold column family

4. **Compiled Query Distribution:** How do users distribute compiled queries?
   - **Proposal:** Generate Rust crate that users add as dependency

5. **Haematite Integration:** When and how to integrate?
   - **Proposal:** Phase 7 (future work), after Hyperspatial stable

---

## Appendix A: Module Structure

### Hyperspatial Module Map (New Structure)

```
hyperspatial/
├── src/
│   ├── lib.rs                    # Public API
│   ├── error.rs                  # Error types
│   │
│   ├── hyperbolic/              # Core hyperbolic operations
│   │   ├── mod.rs
│   │   ├── geometry/            # Geometric operations
│   │   │   ├── mod.rs
│   │   │   ├── distance.rs      # Distance calculations
│   │   │   ├── coordinates.rs   # Coordinate systems
│   │   │   └── manifold.rs      # Manifold operations
│   │   ├── hyperboloid/         # Hyperboloid model
│   │   │   ├── mod.rs
│   │   │   ├── operations.rs    # Core operations
│   │   │   └── visualization.rs # Visualization helpers
│   │   ├── trajectory/          # Trajectory analysis
│   │   │   ├── mod.rs
│   │   │   ├── drift.rs         # Drift detection
│   │   │   ├── velocity.rs      # Velocity calculations
│   │   │   └── acceleration.rs  # Acceleration analysis
│   │   └── properties/          # Property-based operations
│   │       ├── mod.rs
│   │       ├── fingerprint.rs   # Property fingerprinting
│   │       └── similarity.rs    # Property similarity
│   │
│   ├── index/                   # Indexing systems
│   │   ├── mod.rs
│   │   ├── hyperbolic_hnsw.rs   # HNSW for hyperbolic positions
│   │   ├── vector_hnsw.rs       # HNSW for raw vectors (new)
│   │   ├── graph_index.rs       # Graph traversal indexes (new)
│   │   ├── temporal_index.rs    # Time series indexes (new)
│   │   ├── global.rs            # Global position index
│   │   └── type_index.rs        # Entity type index (DashMap)
│   │
│   ├── cascade/                 # Cascade system (keep existing)
│   │   ├── mod.rs
│   │   ├── engine_core.rs       # Cascade engine
│   │   ├── dependencies.rs      # Dependency graph
│   │   ├── triggers.rs          # Trigger index
│   │   ├── aggregation.rs       # Aggregation functions
│   │   └── decay.rs             # Decay functions
│   │
│   ├── streams/                 # Stream coordination (keep existing structure)
│   │   ├── mod.rs
│   │   ├── core/                # Core stream logic
│   │   ├── producers/           # Producer implementations
│   │   ├── consumers/           # Consumer implementations
│   │   ├── processors/          # Stream processors
│   │   └── triggers/            # Trigger system
│   │
│   ├── storage/                 # Manifold integration (NEW)
│   │   ├── mod.rs
│   │   ├── router.rs            # Multi-collection routing
│   │   ├── collection.rs        # Collection management
│   │   ├── entities.rs          # Entity storage (ColumnFamily with UUID keys)
│   │   ├── properties.rs        # Property storage (ColumnFamily, composite keys)
│   │   ├── vectors.rs           # Vector registry (manifold-vectors)
│   │   ├── timeseries.rs        # Time series (manifold-timeseries)
│   │   ├── graphs.rs            # Graph storage (manifold-graph)
│   │   └── cache.rs             # Cache layers (LRU)
│   │
│   ├── query/                   # HyperQL integration (keep existing)
│   │   ├── mod.rs
│   │   ├── hyperql/             # HyperQL-specific
│   │   │   ├── datasource.rs    # DataSource implementation
│   │   │   ├── geometric.rs     # Geometric executor
│   │   │   ├── vector.rs        # Vector executor
│   │   │   └── conversion.rs    # Type conversions
│   │   ├── api/                 # Query API
│   │   ├── compiled/            # Compiled query support
│   │   └── runtime/             # Runtime query support
│   │
│   ├── compute/                 # Custom functions (keep existing structure)
│   │   ├── mod.rs
│   │   ├── lua/                 # Lua runtime (6 files)
│   │   │   ├── mod.rs
│   │   │   ├── runtime.rs
│   │   │   ├── sandbox.rs
│   │   │   ├── api_database.rs
│   │   │   ├── api_hyperbolic.rs
│   │   │   └── functions.rs
│   │   └── wasm/                # WASM runtime (5 files)
│   │       ├── mod.rs
│   │       ├── runtime.rs
│   │       ├── memory.rs
│   │       ├── modules.rs
│   │       └── host_functions.rs
│   │
│   ├── coordination/            # Async coordination (keep as-is)
│   │   ├── mod.rs
│   │   └── request_coordinator.rs
│   │
│   └── import/                  # Import system (keep existing)
│       ├── mod.rs
│       ├── config_parser.rs
│       ├── executor.rs
│       ├── readers/
│       │   ├── csv_reader.rs
│       │   ├── json_reader.rs
│       │   └── jsonl_reader.rs
│       └── position_init.rs
│
└── crates/
    ├── hyperspatial-server/     # Server binary (NEW - moved from src/server)
    │   ├── src/
    │   │   ├── main.rs          # Server entrypoint
    │   │   ├── http.rs          # HTTP API
    │   │   ├── websocket.rs     # WebSocket API
    │   │   ├── grpc.rs          # gRPC API
    │   │   ├── config.rs        # Server configuration
    │   │   └── handlers.rs      # Request handlers
    │   └── Cargo.toml
    │
    └── hyperspatial-cli/        # CLI tool (EXISTS - comprehensive)
        ├── src/
        │   ├── main.rs
        │   ├── client/          # HTTP/WebSocket/Protocol
        │   ├── commands/        # All CLI commands
        │   │   ├── query.rs
        │   │   ├── import/
        │   │   ├── export.rs
        │   │   ├── schema/
        │   │   ├── describe.rs
        │   │   ├── server.rs
        │   │   └── repl.rs
        │   ├── config/          # Configuration
        │   ├── formatter/       # Output formatters
        │   └── repl/            # REPL implementation
        └── Cargo.toml
```

### HyperQL Module Map (Refactored)

```
hyperQL/
├── src/
│   ├── lib.rs                   # Public API
│   ├── error.rs                 # Error types
│   │
│   ├── ast/                     # Abstract syntax tree
│   │   ├── mod.rs
│   │   ├── query.rs             # Query structures
│   │   ├── expression.rs        # Expressions
│   │   ├── literal.rs           # Literals
│   │   ├── function.rs          # Functions
│   │   ├── predicate.rs         # Predicates
│   │   ├── geometric.rs         # Geometric operations
│   │   ├── vector.rs            # Vector operations
│   │   ├── graph.rs             # Graph patterns
│   │   └── schema.rs            # Schema definitions
│   │
│   ├── parser/                  # Parser
│   │   ├── mod.rs
│   │   ├── lexer.rs             # Tokenization
│   │   ├── grammar.rs           # Grammar rules
│   │   └── combinators.rs       # Parser combinators
│   │
│   ├── compiler/                # Compiler
│   │   ├── mod.rs
│   │   ├── planner.rs           # Query planning
│   │   ├── expression.rs        # Expression compilation
│   │   └── select.rs            # SELECT compilation
│   │
│   ├── optimizer/               # Optimizer
│   │   ├── mod.rs
│   │   ├── rules.rs             # Optimization rules
│   │   ├── predicate_pushdown.rs
│   │   ├── constant_folding.rs
│   │   └── projection_pushdown.rs
│   │
│   ├── executor/                # Executor
│   │   ├── mod.rs
│   │   ├── plan_executor.rs     # Plan execution
│   │   ├── datasource.rs        # DataSource trait
│   │   └── expression_eval.rs   # Expression evaluation
│   │
│   ├── types/                   # Type system
│   │   ├── mod.rs
│   │   ├── entity.rs            # Entity types
│   │   ├── property.rs          # Property types
│   │   └── value.rs             # Value types
│   │
│   └── validator/               # Validation
│       ├── mod.rs
│       ├── type_checker.rs      # Type checking
│       └── semantic.rs          # Semantic validation
│
└── crates/
    └── hyperql-compile/         # Query compiler
        ├── src/
        │   ├── lib.rs           # Library API
        │   ├── cli.rs           # CLI tool
        │   ├── inference.rs     # Type inference
        │   ├── codegen.rs       # Code generation
        │   └── schema.rs        # Schema generation
        └── Cargo.toml
```

---

## Appendix B: Example Code

**Note:** Detailed code examples have been moved to a separate document `HYPERSPATIAL_EXAMPLES.md` to keep this design document focused. See that document for:
- Vector storage with multiple named vectors and UUID keys
- Trajectory storage with custom PositionSnapshot type
- Compiled query examples with generated Rust code
- Fixed-width key usage patterns

---

## Appendix C: Performance Targets

### Current Performance Baseline

| Metric | Current | Target (Post-Redesign) |
|--------|---------|------------------------|
| k-NN query (HNSW) | 22-70μs | ≤ 70μs (maintain) |
| SQL query (entity retrieval) | 145-300μs | ≤ 300μs (maintain) |
| Commit latency | ~5ms (redb) | ~0.5ms (Manifold WAL) |
| Cache hit rate | 90-95% | ≥ 90% (maintain) |
| Entity cache size | 50K capacity, 9.5MB | Maintain |
| Property cache size | 10K capacity, 19.1MB | Maintain |
| Import throughput | ~10K entities/sec | ≥ 10K entities/sec |
| File handles per shard | 11 | ~1 + N CFs |

### New Performance Opportunities

- **WAL Commits:** 10x faster (0.5ms vs 5ms)
- **Concurrent Writes:** Column families enable parallel writes to different data types
- **Vector Operations:** manifold-vectors optimizations (zero-copy, const generic dimensions)
- **Time Series:** manifold-timeseries delta encoding and compression
- **Graph Traversal:** manifold-graph bidirectional indexes
- **Compiled Queries:** Zero parsing overhead for known queries

---

## Coding Conventions

To maintain consistency with Manifold's architecture and ensure clean, maintainable code:

### Architectural Standards
- **Follow Manifold's patterns**: Study manifold-vectors, manifold-timeseries, manifold-graph for structure
- **Separation of concerns**: One module, one responsibility
- **Folder modules**: Use `mod.rs` files for organization and visibility control only
  - `mod.rs` contains documentation and `pub mod` declarations
  - `mod.rs` should NOT contain implementation code
  - Implementation goes in separate files (e.g., `distance.rs`, `trajectory.rs`)
- **Fixed-width keys**: Use UUID (16 bytes) for all entity IDs, enables zero-copy deserialization

### File Organization
- **Keep files under 600 lines**: Ideal target, 800 lines maximum
- **Split large modules**: Break into logical sub-modules when exceeding limits
- **Clear naming**: File names should describe their single responsibility
- **Related code together**: Group related functions/types in same file

### Development Process
- **Dev notes in plan**: Record decisions, challenges, solutions in phase plan documents
- **Mark progress**: Update task lists and journals in `.project/` directory
- **Test as you go**: Write tests alongside implementation
- **Benchmark critical paths**: Measure performance for storage and indexing operations

### Code Quality
- **Documentation**: Public APIs must have doc comments
- **Error handling**: Use Result types, provide context in errors
- **Type safety**: Leverage Rust's type system (const generics, traits, etc.)
- **No unsafe**: Avoid unsafe code unless absolutely necessary and well-justified

### Module Structure Example
```
// Good - mod.rs for organization
pub mod distance;
pub mod positioning;
pub mod trajectory;

use distance::HyperbolicDistance;
pub use positioning::PositionLearner;

// Bad - mod.rs with implementation
pub mod distance;

pub fn calculate_hyperbolic_distance(...) { // Don't do this!
    // Implementation here
}
```

---

## Next Steps

1. **Review & Approval:** Review this design document with stakeholders
2. **Branch Creation:** Create `redesign` branch from `main`
3. **Create Phase Plans:** Break each phase into detailed task documents
4. **Phase 1 Start:** Begin preparation phase
   - Audit existing code (detailed review of what to keep/modify/delete)
   - Write interface definitions
   - Create test suite
5. **Weekly Progress:** Track progress against timeline
6. **Milestone Reviews:** Review at end of each phase
7. **Dev Journals**: Maintain running notes in `.project/phases/` directory

---

## Conclusion

This redesign transforms Hyperspatial from a monolithic system with complex, duplicated storage code into a modular ecosystem with clear component boundaries. By consolidating on Manifold for storage, making HyperQL general-purpose, and focusing Hyperspatial on its core value (hyperbolic space operations), we create a platform that is:

- **Simpler:** Less code to maintain, cleaner abstractions
- **Faster:** Manifold's WAL, column families, and domain optimizations
- **More Composable:** Each component independently useful
- **More Maintainable:** Clear boundaries, no circular dependencies
- **More Powerful:** Compiled queries, native embeddings, reactive notebooks

The estimated 4.5-6 month timeline is aggressive but achievable with focused effort and the greenfield advantage of no migration constraints.

**Status:** Ready for implementation  
**Next Action:** Begin Phase 1 (Preparation & Analysis)