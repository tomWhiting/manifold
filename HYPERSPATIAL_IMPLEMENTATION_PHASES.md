# Hyperspatial Redesign - Implementation Phases

**Date:** 2024-10-29  
**Status:** Implementation Plan  
**Parent Document:** [HYPERSPATIAL_REDESIGN.md](./HYPERSPATIAL_REDESIGN.md)

This document provides detailed, actionable steps for implementing the Hyperspatial ecosystem redesign. Each phase includes specific tasks, deliverables, and success criteria.

---

## Overview

**Total Timeline:** 19-27 weeks (4.5-6 months)  
**Approach:** Bottom-up rebuild on new `redesign` branch  
**Constraint:** Greenfield (no backwards compatibility needed)

### Phase Summary

| Phase | Duration | Focus | Key Deliverable |
|-------|----------|-------|-----------------|
| 1 | 1-2 weeks | Preparation & Analysis | Interface definitions, test suite |
| 2 | 4-6 weeks | Manifold Storage Layer | Complete storage on Manifold |
| 3 | 3-4 weeks | HyperQL Refactoring | General-purpose HyperQL + compile crate |
| 4 | 6-8 weeks | Hyperspatial Rebuild | Clean Hyperspatial implementation |
| 5 | 2-3 weeks | Tessera Integration | Native embedding generation |
| 6 | 3-4 weeks | Testing & Optimization | Production-ready system |

---

## Coding Standards

Before beginning implementation, establish these conventions:

### Architectural Standards
- Follow Manifold's patterns (study manifold-vectors, manifold-timeseries, manifold-graph)
- Separation of concerns: one module, one responsibility
- Folder modules use `mod.rs` for organization only (no implementation code)
- Fixed-width UUID keys (16 bytes) for all entity IDs

### File Organization
- Target: <600 lines per file (max 800)
- Split large modules into logical sub-modules
- Clear, descriptive file names
- Group related code together

### Development Process
- Record dev notes in `.project/phases/phase-N/journal.md`
- Update task lists as work progresses
- Write tests alongside implementation
- Benchmark critical paths

### Code Quality
- Public APIs must have doc comments
- Use Result types with contextual errors
- Leverage type system (const generics, traits)
- Minimize unsafe code

---

## Phase 1: Preparation & Analysis

**Duration:** 1-2 weeks  
**Goal:** Understand what exists, define what's needed, establish testing baseline

### Tasks

#### 1.1: Code Audit
- [ ] Review all `src/persistence/storage/*.rs` files
  - Identify redb usage patterns
  - Document key-value schemas
  - Note transaction patterns
- [ ] Review `src/hyperbolic/*` modules
  - Identify core algorithms to preserve
  - Mark ML-based code for removal/replacement
  - Document dependencies
- [ ] Review `src/cascade/*` modules
  - Understand dependency graph structure
  - Document trigger mechanisms
  - Identify Manifold integration points
- [ ] Review `src/streams/*` modules
  - Document producer/consumer patterns
  - Identify persistence touchpoints
  - Note coordination mechanisms
- [ ] Review `src/compute/*` modules
  - Verify Lua runtime is self-contained
  - Verify WASM runtime is self-contained
  - Note any storage dependencies
- [ ] Review `src/server/*` modules
  - Identify what moves to hyperspatial-server crate
  - Document gRPC implementation
  - Note configuration patterns

**Deliverable:** `AUDIT_REPORT.md` documenting findings

#### 1.2: Interface Definitions
- [ ] Define storage traits
  - `EntityStorage` trait for entity CRUD
  - `PropertyStorage` trait for properties
  - `VectorStorage` trait for vector operations
  - `GraphStorage` trait for edge operations
  - `TimeSeriesStorage` trait for temporal data
- [ ] Define hyperbolic traits
  - `DistanceCalculator` trait
  - `PositionLearner` trait
  - `TrajectoryAnalyzer` trait
- [ ] Define indexing traits
  - `SpatialIndex` trait (HNSW, etc.)
  - `VectorIndex` trait (for raw vectors)
  - `TemporalIndex` trait (for time series)
- [ ] Define HyperQL integration traits
  - Update `DataSource` trait as needed
  - Define executor extension traits

**Deliverable:** `src/traits.rs` and `src/traits/` module with all trait definitions

#### 1.3: Test Suite Creation
- [ ] Core entity operations
  - Create, read, update, delete entities
  - Entity listing by type
  - Bulk operations
- [ ] Property operations
  - Set, get, delete properties
  - Property queries
  - Bulk property operations
- [ ] Graph operations
  - Add, remove edges
  - Bidirectional queries
  - Edge property access
- [ ] Hyperbolic distance
  - Distance calculation correctness
  - Different coordinate systems
  - Edge cases (numerical stability)
- [ ] HNSW index
  - Index building
  - k-NN queries
  - Recall metrics
- [ ] Cascade operations
  - Dependency resolution
  - Propagation correctness
  - Aggregation functions
- [ ] Stream coordination
  - Producer/consumer patterns
  - Message routing
  - Loop-back workflows

**Deliverable:** `tests/baseline/` directory with comprehensive test suite

#### 1.4: Performance Baseline
- [ ] Benchmark current system
  - Entity CRUD throughput
  - k-NN query latency
  - SQL query latency
  - Import throughput
  - Commit latency
- [ ] Document current metrics
- [ ] Establish performance targets

**Deliverable:** `PERFORMANCE_BASELINE.md` with current metrics

#### 1.5: Module Structure Design
- [ ] Design new directory structure
- [ ] Map old files to new locations
- [ ] Identify files to delete vs. refactor
- [ ] Plan crate boundaries

**Deliverable:** `MODULE_MAP.md` showing old → new structure

### Phase 1 Success Criteria
- [ ] Complete understanding of existing codebase
- [ ] All interfaces defined and documented
- [ ] Comprehensive test suite passes against current implementation
- [ ] Performance baseline established
- [ ] Clear plan for code reorganization

---

## Phase 2: Manifold Storage Layer

**Duration:** 4-6 weeks  
**Goal:** Replace all redb usage with Manifold column families and domain crates

### Week 1-2: Core Entity Storage

#### 2.1: ColumnFamily Setup
- [ ] Create `src/storage/mod.rs` with storage module structure
- [ ] Create `src/storage/database.rs` for ColumnFamilyDatabase wrapper
- [ ] Implement column family initialization
  - `entities` CF for entity metadata
  - `properties` CF for entity properties
  - `edges` CF for graph relationships
  - `measures` CF for cascade values
  - `trajectories` CF for position history

**Dev Note:** Document column family naming scheme and key format

#### 2.2: Entity Storage with UUID Keys
- [ ] Create `src/storage/entities.rs`
- [ ] Implement `EntityStorage` trait using `entities` CF
- [ ] Use `uuid::Uuid` as fixed-width key (16 bytes)
- [ ] Implement entity CRUD operations
  - `create_entity(uuid, entity_data)`
  - `get_entity(uuid)`
  - `update_entity(uuid, entity_data)`
  - `delete_entity(uuid)`
- [ ] Implement entity listing
  - By type (using in-memory type index)
  - Bulk operations
- [ ] Write tests for entity operations
- [ ] Benchmark against old implementation

**Dev Note:** UUID keys enable zero-copy deserialization

#### 2.3: Type Index (In-Memory)
- [ ] Create `src/storage/type_index.rs`
- [ ] Implement `DashMap<String, Vec<Uuid>>` for type → entity mapping
- [ ] Build index on startup from entities CF
- [ ] Maintain index on entity creation/deletion
- [ ] Test index correctness and performance

### Week 3: Properties Storage

#### 2.4: Properties with Composite Keys
- [ ] Create `src/storage/properties.rs`
- [ ] Implement `PropertyStorage` trait using `properties` CF
- [ ] Use composite key: `(Uuid, property_name: String)`
- [ ] Implement property operations
  - `set_property(entity_uuid, name, value)`
  - `get_property(entity_uuid, name)`
  - `get_all_properties(entity_uuid)`
  - `delete_property(entity_uuid, name)`
- [ ] Implement bulk operations
- [ ] Write tests for property operations
- [ ] Benchmark against old implementation

**Dev Note:** Consider property value serialization format

### Week 4: Graph Storage

#### 2.5: Graph Storage with manifold-graph
- [ ] Create `src/storage/graphs.rs`
- [ ] Use `manifold_graph::GraphTable` for edge storage
- [ ] Implement `GraphStorage` trait
- [ ] Edge operations with UUID vertices
  - `add_edge(source_uuid, edge_type, target_uuid, active, weight)`
  - `remove_edge(source_uuid, edge_type, target_uuid)`
  - `get_outgoing_edges(vertex_uuid)`
  - `get_incoming_edges(vertex_uuid)`
- [ ] Simplify edge properties to `(bool, f32)` only
- [ ] Migrate any complex edge metadata to separate properties table
- [ ] Write tests for graph operations
- [ ] Benchmark against old implementation

**Dev Note:** Document edge property simplification and migration strategy for complex properties

### Week 5: Vector Storage

#### 2.6: Vector Registry with manifold-vectors
- [ ] Create `src/storage/vectors.rs`
- [ ] Implement vector table registry
  - Map: `vector_name` → `VectorTable<DIM>` instance
  - Support multiple named vectors per entity
- [ ] Create separate column families per vector type
  - `semantic_vectors` CF → `VectorTable<768>`
  - `code_vectors` CF → `VectorTable<512>`
  - `sparse_vectors` CF → `SparseVectorTable`
  - `multi_vectors` CF → `MultiVectorTable`
- [ ] Implement `VectorStorage` trait
  - `store_vector(entity_uuid, vector_name, vector_data)`
  - `get_vector(entity_uuid, vector_name)`
  - `list_vector_names(entity_uuid)`
  - `delete_vector(entity_uuid, vector_name)`
- [ ] Use UUID keys for all vector tables
- [ ] Write tests for vector operations
- [ ] Benchmark against old implementation

**Dev Note:** Same UUID used across all vector tables for single entity

### Week 6: Time Series Storage

#### 2.7: Time Series with manifold-timeseries
- [ ] Create `src/storage/timeseries.rs`
- [ ] Use `manifold_timeseries::TimeSeriesTable` for measures
- [ ] Implement custom `PositionSnapshot` type
  ```rust
  struct PositionSnapshot {
      graph_position: [f32; 17],
      embedding_position: [f32; 17],
      property_position: [f32; 17],
  }
  ```
- [ ] Implement trajectory storage
  - Use `TimeSeriesTable<AbsoluteEncoding, PositionSnapshot>`
  - Composite key: `(timestamp_ms, entity_uuid)`
- [ ] Implement `TimeSeriesStorage` trait
  - `store_measure(series_id, timestamp, value)`
  - `store_trajectory(entity_uuid, timestamp, positions)`
  - `query_range(series_id, start, end)`
  - `get_trajectory(entity_uuid, start, end)`
- [ ] Write tests for time series operations
- [ ] Benchmark against old implementation

**Dev Note:** Evaluate delta encoding vs. absolute for different use cases

### Week 6: Router Refactoring

#### 2.8: Router Update
- [ ] Update `src/storage/router.rs`
- [ ] Replace `DatabaseManager` with `ColumnFamilyDatabase`
- [ ] Update `Collection` to use new storage layer
- [ ] Maintain `MetaIndex` for sharding
- [ ] Keep cache layers
  - Entity cache (50K, LRU)
  - Property cache (10K, LRU)
- [ ] Update coordination with new storage
- [ ] Write integration tests
- [ ] Benchmark full storage stack

**Dev Note:** Ensure cache invalidation works correctly

### Phase 2 Success Criteria
- [ ] All storage operations use Manifold (no redb)
- [ ] UUID keys used throughout (16-byte fixed-width)
- [ ] All existing tests pass with new storage
- [ ] Performance equal to or better than baseline
- [ ] File handle usage significantly reduced
- [ ] WAL commits ~0.5ms (vs. previous ~5ms)

---

## Phase 3: HyperQL Refactoring

**Duration:** 3-4 weeks  
**Goal:** Make HyperQL general-purpose and create compilation infrastructure

### Week 1: Decouple from Hyperspatial

#### 3.1: Remove Hyperspatial Dependencies
- [ ] Audit `hyperQL/src` for Hyperspatial-specific code
- [ ] Move Hyperspatial-specific logic to Hyperspatial
- [ ] Ensure `DataSource` trait is backend-agnostic
- [ ] Update executor to work with any DataSource implementation
- [ ] Write tests with mock DataSource

#### 3.2: Type System Consolidation
- [ ] Review entity/property/value types in both projects
- [ ] Centralize types in `hyperQL/src/types/`
- [ ] Have Hyperspatial import from HyperQL
- [ ] Update conversion code
- [ ] Test type conversions

### Week 2: Execution Plan Extensions

#### 3.3: Add Operation Types
- [ ] Add geometric operation nodes to execution plans
  - `HyperbolicDistance`
  - `WithinRadius`
  - `NearPositions`
- [ ] Add vector operation nodes
  - `CosineSimilarity`
  - `EuclideanDistance`
  - `KNN`
- [ ] Plans describe operations, don't execute them
- [ ] Update compiler to generate these nodes
- [ ] Test plan generation

### Week 3-4: HyperQL-Compile Crate

#### 3.4: Create Compilation Infrastructure
- [ ] Create `hyperQL/crates/hyperql-compile/` directory
- [ ] Setup `Cargo.toml` with dependencies
  - `quote` for code generation
  - `syn` for AST manipulation
  - `proc-macro2` for token streams
- [ ] Create CLI binary structure
  - Argument parsing (query files, output directory)
  - File I/O
  - Error reporting

#### 3.5: Type Inference Engine
- [ ] Create `src/inference.rs`
- [ ] Implement type environment
  - Track table → column → type mappings
  - Build from query usage
  - Detect conflicts
- [ ] Implement inference rules
  - Infer from comparisons (`age > 25` → age is numeric)
  - Infer from operations (`name LIKE '%foo%'` → name is string)
  - Unify across queries
- [ ] Implement conflict reporting
- [ ] Test with sample queries

#### 3.6: Code Generator
- [ ] Create `src/codegen.rs`
- [ ] Implement result type generation
  - Struct per query with typed fields
  - Derive macros (Debug, Clone, etc.)
- [ ] Implement function generation
  - Function signature with parameters
  - Manifold API usage
  - Error handling
- [ ] Generate readable, formatted code
- [ ] Test with sample queries

#### 3.7: Schema Generator
- [ ] Create `src/schema.rs`
- [ ] Generate schema JSON from inferred types
- [ ] Schema includes:
  - Table definitions
  - Column types
  - Indexes
  - Constraints
- [ ] Test schema generation

### Week 4: Integration & Testing

#### 3.8: End-to-End Testing
- [ ] Write sample `.hql` query files
- [ ] Run compiler to generate Rust code
- [ ] Compile generated code
- [ ] Execute against Manifold database
- [ ] Verify results match runtime queries
- [ ] Benchmark compiled vs. runtime

### Phase 3 Success Criteria
- [ ] HyperQL has no Hyperspatial dependencies
- [ ] DataSource trait works with any backend
- [ ] Type system consolidated in HyperQL
- [ ] hyperql-compile generates valid Rust code
- [ ] Compiled queries produce correct results
- [ ] Documentation complete

---

## Phase 4: Hyperspatial Rebuild

**Duration:** 6-8 weeks  
**Goal:** Rebuild Hyperspatial on Manifold + HyperQL foundation

### Week 1-2: Hyperbolic Core

#### 4.1: Preserve Core Algorithms
- [ ] Copy preserved modules to new structure
  - `src/hyperbolic/geometry/` (distance, coordinates)
  - `src/hyperbolic/hyperboloid/` (operations)
  - `src/hyperbolic/trajectory/` (drift, velocity)
  - `src/hyperbolic/properties/` (fingerprint, similarity)
- [ ] Update to use new storage layer
- [ ] Remove ML-based positioning code (if problematic)
- [ ] Implement degree-based positioning
- [ ] Write tests for all operations
- [ ] Verify numerical stability

#### 4.2: Position Storage
- [ ] Use manifold-vectors for position storage
- [ ] Store three positions per entity
  - Graph position: `VectorTable<17>`
  - Embedding position: `VectorTable<17>`
  - Property position: `VectorTable<17>`
- [ ] Implement multi-modal distance calculation
  - α × d_graph + β × d_embedding + γ × d_property
- [ ] Test position operations

### Week 3-4: Indexing Infrastructure

#### 4.3: Hyperbolic HNSW Index
- [ ] Create `src/index/hyperbolic_hnsw.rs`
- [ ] Adapt existing HNSW for Manifold storage
- [ ] Use manifold-vectors for node storage
- [ ] Implement index building
- [ ] Implement k-NN search
- [ ] Test recall metrics
- [ ] Benchmark query latency (target: <70μs)

#### 4.4: Vector Indexes (New)
- [ ] Create `src/index/vector_hnsw.rs`
- [ ] Implement HNSW for raw vectors (not hyperbolic positions)
- [ ] Support multiple vector types
- [ ] Test approximate NN search
- [ ] Benchmark performance

#### 4.5: Graph Indexes (New)
- [ ] Create `src/index/graph_index.rs`
- [ ] Implement vertex-centric indexes
- [ ] Label indexes for filtering
- [ ] Optimize traversal patterns
- [ ] Test and benchmark

#### 4.6: Temporal Indexes (New)
- [ ] Create `src/index/temporal_index.rs`
- [ ] Implement time-range indexes
- [ ] Support efficient range queries
- [ ] Test and benchmark

### Week 5: Cascade System

#### 4.7: Cascade Integration
- [ ] Keep existing cascade modules
  - `src/cascade/engine_core.rs`
  - `src/cascade/dependencies.rs`
  - `src/cascade/triggers.rs`
  - `src/cascade/aggregation.rs`
  - `src/cascade/decay.rs`
- [ ] Update to use new storage layer
- [ ] Test cascade propagation
- [ ] Verify dependency resolution
- [ ] Benchmark cascade performance

### Week 6: Stream System

#### 4.8: Stream Coordination
- [ ] Keep existing stream modules
  - `src/streams/core/`
  - `src/streams/producers/`
  - `src/streams/consumers/`
  - `src/streams/processors/`
  - `src/streams/triggers/`
- [ ] Update database trigger integration
- [ ] Implement loop-back pattern
- [ ] Test producer/consumer workflows
- [ ] Verify message routing

### Week 7: Server Infrastructure

#### 4.9: Move Server to Separate Crate
- [ ] Create `crates/hyperspatial-server/` directory
- [ ] Move server code from `src/server/`
  - `http.rs` → HTTP API
  - `websocket.rs` → WebSocket API
  - `grpc.rs` → gRPC API (keep existing)
  - `config.rs` → Configuration
- [ ] Create server binary entry point
- [ ] Update Cargo.toml dependencies
- [ ] Test all server endpoints
- [ ] Verify gRPC implementation

#### 4.10: Compute Runtime
- [ ] Keep existing compute modules (no changes needed)
  - `src/compute/lua/` (6 files, well-organized)
  - `src/compute/wasm/` (5 files, well-organized)
- [ ] Verify storage integration points
- [ ] Test Lua API bindings
- [ ] Test WASM host functions

### Week 8: Integration & Import

#### 4.11: HyperQL Integration
- [ ] Create `src/query/datasource.rs`
- [ ] Implement `DataSource` trait using new storage
- [ ] Create geometric executor
  - Hyperbolic distance operations
  - Uses hyperbolic HNSW index
- [ ] Create vector executor
  - Vector similarity operations
  - Uses vector indexes
- [ ] Test query execution
- [ ] Benchmark query performance

#### 4.12: Import System
- [ ] Update import modules to use new storage
  - `src/import/readers/` (CSV, JSON, JSONL)
  - `src/import/executor.rs`
  - `src/import/position_init.rs`
- [ ] Use Manifold bulk APIs
- [ ] Test large dataset imports
- [ ] Benchmark import throughput

### Phase 4 Success Criteria
- [ ] Hyperspatial runs on Manifold storage
- [ ] All core features working
- [ ] HNSW query latency <70μs
- [ ] All indexes functional
- [ ] Cascade system operational
- [ ] Stream coordination working
- [ ] Server endpoints functional (HTTP/WS/gRPC)
- [ ] Import system handles large datasets

---

## Phase 5: Tessera Integration

**Duration:** 2-3 weeks  
**Goal:** Native embedding generation in queries

### Week 1: HyperQL Extensions

#### 5.1: Embedding Functions in HyperQL
- [ ] Add embedding function definitions to AST
  - `embed_dense(text, model)`
  - `embed_multi(text, model)`
  - `embed_sparse(text, model)`
  - `embed_vision(image, model)`
- [ ] Update parser to recognize embedding functions
- [ ] Add embedding nodes to execution plans
- [ ] Implement function compilation
- [ ] Test parsing and compilation

### Week 2: Hyperspatial Integration

#### 5.2: Tessera Executor
- [ ] Create `src/query/tessera_executor.rs`
- [ ] Load Tessera models on startup
- [ ] Implement embedding function execution
  - Call Tessera APIs
  - Cache model instances
  - Handle GPU acceleration
- [ ] Integrate with query executor
- [ ] Test embedding generation

#### 5.3: Auto-Embedding Configuration
- [ ] Add collection-level embedding config
- [ ] Configure models per collection
- [ ] Auto-embed on INSERT if configured
- [ ] Store embeddings using manifold-vectors
- [ ] Test auto-embedding workflows

### Week 3: Compiled Query Integration

#### 5.4: Code Generation for Embeddings
- [ ] Generate Tessera API calls in compiled queries
- [ ] Type-safe embedding generation
- [ ] Test compiled queries with embeddings
- [ ] Benchmark performance

### Phase 5 Success Criteria
- [ ] Embedding functions work in HyperQL
- [ ] Auto-embedding on INSERT functional
- [ ] Compiled queries support embeddings
- [ ] Performance acceptable
- [ ] Optional Tessera dependency (feature flag)

---

## Phase 6: Testing & Optimization

**Duration:** 3-4 weeks  
**Goal:** Production-ready system with comprehensive validation

### Week 1: Testing

#### 6.1: Unit Tests
- [ ] Every module has unit tests
- [ ] Edge cases covered
- [ ] Error paths tested
- [ ] Target: >80% coverage

#### 6.2: Integration Tests
- [ ] Full workflows tested
  - Import → position → index → query
  - Cascade propagation
  - Stream coordination
  - Server API endpoints
- [ ] Multi-collection scenarios
- [ ] Concurrent operations
- [ ] Error recovery

#### 6.3: Stress Tests
- [ ] Large dataset tests (100K+ entities)
- [ ] Concurrent client tests
- [ ] Memory leak detection
- [ ] Long-running stability

### Week 2-3: Performance Optimization

#### 6.4: Profiling
- [ ] Profile critical paths
  - HNSW queries
  - Entity retrieval
  - Property queries
  - Cascade propagation
  - Import operations
- [ ] Identify bottlenecks
- [ ] Document findings

#### 6.5: Optimization
- [ ] Optimize hot paths
- [ ] Tune HNSW parameters (ef, M)
- [ ] Tune cache sizes
- [ ] Optimize batch operations
- [ ] Implement parallel processing where applicable

#### 6.6: Benchmarking
- [ ] Compare to baseline metrics
- [ ] Verify performance targets met
  - k-NN latency <70μs
  - Commit latency ~0.5ms
  - Cache hit rate >90%
  - Import throughput maintained
- [ ] Document improvements

### Week 4: Documentation & Examples

#### 6.7: Documentation
- [ ] Architecture documentation
- [ ] API documentation (rustdoc)
- [ ] Query language guide
- [ ] Configuration guide
- [ ] Deployment guide

#### 6.8: Examples
- [ ] Basic usage examples
- [ ] Multi-modal positioning example
- [ ] Cascade system example
- [ ] Stream coordination example
- [ ] Compiled query example
- [ ] Tessera integration example

### Phase 6 Success Criteria
- [ ] >80% test coverage
- [ ] All integration tests pass
- [ ] Stress tests pass
- [ ] Performance targets met
- [ ] Complete documentation
- [ ] Examples functional

---

## Development Workflow

### Branch Strategy
```bash
# Create redesign branch
git checkout -b redesign

# Create phase branches
git checkout -b phase-1-preparation
git checkout -b phase-2-storage
# etc.

# Merge to redesign after each phase review
```

### Dev Journal
Maintain running notes in `.project/phases/`:
```
.project/
└── phases/
    ├── phase-1/
    │   ├── journal.md     # Running dev notes
    │   ├── tasks.md       # Task checklist
    │   └── decisions.md   # Design decisions
    ├── phase-2/
    │   └── ...
    └── phase-N/
        └── ...
```

### Progress Tracking
- Update task checklists daily
- Record blockers and solutions in journal
- Weekly progress reviews
- Phase completion reviews

---

## Risk Mitigation

### Technical Risks

**Manifold Performance Issues**
- Early POC in Phase 2 validates approach
- Benchmark continuously
- Profile and optimize as needed

**Type Inference Complexity**
- Start simple (basic SELECT queries)
- Iterate incrementally
- Defer complex cases to later

**Integration Failures**
- Comprehensive interface definitions in Phase 1
- Test early and often
- Maintain compatibility layer during transition

### Process Risks

**Timeline Slippage**
- Weekly progress reviews
- Adjust scope if needed
- Defer non-critical features

**Scope Creep**
- Strict adherence to design document
- Document and defer new ideas
- Focus on core functionality first

---

## Success Metrics

### Functional
- [ ] All baseline tests pass
- [ ] All features working
- [ ] No critical bugs

### Performance
- [ ] k-NN latency ≤70μs
- [ ] Commit latency ~0.5ms
- [ ] Cache hit rate ≥90%
- [ ] Import throughput maintained

### Quality
- [ ] Test coverage >80%
- [ ] All public APIs documented
- [ ] Code follows conventions
- [ ] No circular dependencies

### Architecture
- [ ] Clean module boundaries
- [ ] No Hyperspatial deps in HyperQL
- [ ] Manifold used throughout
- [ ] Proper crate organization

---

## Completion Criteria

The redesign is complete when:

1. All six phases are finished
2. All success criteria met
3. Documentation complete
4. Examples working
5. Performance validated
6. Code review passed
7. Stakeholder approval obtained

**Next Action:** Begin Phase 1 - Preparation & Analysis