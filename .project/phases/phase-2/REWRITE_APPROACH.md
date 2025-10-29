# Phase 2: Storage Layer Rewrite Approach

**Date**: 2025-01-XX  
**Status**: Active  
**Branch**: storage-rewrite  
**Parent Doc**: HYPERSPATIAL_IMPLEMENTATION_PHASES.md

---

## Strategic Decision: Rewrite, Not Refactor

**Rationale**: With 528+ redb API references across 20 files, a traditional refactor would require touching every reference while preserving architectural assumptions built around redb's limitations. Instead, we're building a clean storage layer using Manifold's column families and domain crates.

### What This Means

**NOT doing:**
- Line-by-line replacement of redb API calls
- Preserving old file structure and patterns
- Maintaining backwards compatibility
- Gradual migration with feature flags

**DOING:**
- Building new `src/storage/` module from scratch
- Using Manifold's architecture properly (column families, domain crates, WAL)
- Keeping high-level Router API stable
- Using old code as reference for business logic only

---

## Revised Phase 2 Timeline: 6 Weeks

**Original estimate**: 4-6 weeks for migration  
**Revised estimate**: 6 weeks for rewrite

### Week-by-Week Breakdown

#### Week 1: Foundation & Design
- Move `src/persistence/` → `_old_persistence/` (reference)
- Create new `src/storage/` module structure
- Define `ManifoldCollection` (wraps `ColumnFamilyDatabase`)
- Design table organization within column families
- Scaffold module files with empty implementations
- Bring back `router.rs` and `coordination/request_coordinator.rs`

**Deliverables**:
- Clean module structure in `src/storage/`
- Design document validation
- Empty implementations ready for Week 2

#### Week 2: Core Entity & Property Storage
- Implement entity CRUD using Manifold column families
- Column family per entity type (patients, admissions, etc.)
- Entity storage with UUID keys
- Property storage with tuple keys `(Uuid, &str)`
- Master routing index (entity_id → column family)
- Unit tests for entities and properties

**Deliverables**:
- `storage/entities.rs` - Full implementation
- `storage/properties.rs` - Full implementation
- `storage/index.rs` - Master index
- Unit tests passing

#### Week 3: Graph Storage
- Integrate `manifold_graph::GraphTable`
- Edge CRUD operations
- Bidirectional queries (outgoing/incoming)
- UUID vertices, `(bool, f32)` edge properties
- Graph operation tests

**Deliverables**:
- `storage/graph.rs` - Full implementation
- Bidirectional edge queries working
- Unit tests passing

#### Week 4: Vector Storage
- Integrate `manifold_vectors::VectorTable<DIM>`
- Vector registration system
- Zero-copy vector access
- Multi-vector support (multiple tables per entity type)
- Vector operation tests

**Deliverables**:
- `storage/vectors.rs` - Full implementation
- Vector registry metadata
- Unit tests passing

#### Week 5: Time Series Storage
- Integrate `manifold_timeseries::TimeSeriesTable`
- PositionSnapshot storage (3×17D trajectories)
- Range queries for trajectories
- Measure history (cascade temporal tracking)
- Time series tests

**Deliverables**:
- `storage/timeseries.rs` - Full implementation
- `storage/measures.rs` - Cascade value storage
- Unit tests passing

#### Week 6: Integration & Validation
- Update `Router` to use new `ManifoldCollection`
- Update collection creation/management
- Preserve Router API contract
- Run full test suite (620 tests)
- Performance benchmarking
- Documentation updates

**Deliverables**:
- Router fully integrated
- All 620 tests passing
- Performance benchmarks
- Migration complete

---

## Architecture Changes from Original Plan

### Original Plan (Migration)
- Column families for separate concerns (entities, properties, edges, etc.)
- One column family per data type across all entity types

### New Plan (Rewrite)
- **Column family per entity type** (patients, admissions, chart_events)
- **Tables within each column family** for different data types:
  - `core` table - entity metadata
  - `properties` table - key-value attributes
  - `vectors_semantic_768` table - embeddings (manifold-vectors)
  - `edges` table - graph relationships (manifold-graph)
  - `trajectories` table - position history (manifold-timeseries)
  - `measures` table - cascade values

### Why This is Better

**Logical grouping**: All data for "patients" entity type in one column family  
**Better isolation**: Write to patients doesn't block writes to admissions  
**Easier management**: Create/delete column family = create/delete entity type  
**Natural sharding**: Column families are natural shard boundaries

---

## What Moves vs What Stays

### Moved to `_old_persistence/` (Reference Only)
- Entire `src/persistence/` directory
  - `storage/core.rs` - Look at for entity operations
  - `storage/properties.rs` - Look at for property operations
  - `storage/edges.rs` - Look at for graph operations
  - `vectors/storage/` - Look at for vector operations
  - `storage/trajectories.rs` - Look at for time series operations
  - `collections/collection.rs` - Look at for collection logic
  - `batch_operations.rs` - Look at for batch patterns
  - `connection_pool.rs` - Manifold handles this differently

### Kept and Updated
- `collections/router.rs` → `src/router.rs` (updated for new storage)
- `coordination/request_coordinator.rs` → `src/coordination/` (minor updates)
- `schema/` → `src/schema/` (may need updates for column families)
- `expressions/` → `src/expressions/` (unchanged)

### Kept Unchanged
- `src/cascade/` - Cascade system (just uses Router API)
- `src/streams/` - Stream coordination (just uses Router API)
- `src/compute/` - Lua/WASM runtimes (just uses Router API)
- `src/query/` - HyperQL integration (just uses Router API)
- `src/import/` - Import logic (updated for new storage backend)
- `src/hyperbolic/` - Core algorithms (unchanged)
- `src/indexing/` - HNSW, vector indexes (unchanged)
- `src/types/` - Core types (unchanged)

---

## Key Design Patterns

### Pattern 1: Column Family = Entity Type

```rust
// Old: Collection → Shards → DatabaseManager → Multiple .redb files
// New: Collection → Manifold DB → Column Families (one per entity type)

let medical_db = ColumnFamilyDatabase::open("medical.manifold")?;
let patients_cf = medical_db.column_family_or_create("patients")?;
let admissions_cf = medical_db.column_family_or_create("admissions")?;
```

### Pattern 2: Tables Within Column Family

```rust
// Within "patients" column family:
let write_txn = patients_cf.begin_write()?;

// Core entity metadata
let mut core_table = write_txn.open_table(CORE_TABLE)?;
core_table.insert(uuid, entity_core_bytes)?;

// Properties
let mut props_table = write_txn.open_table(PROPERTIES_TABLE)?;
props_table.insert((uuid, "age"), value_bytes)?;

// Vectors (manifold-vectors)
let mut vectors = VectorTable::<768>::open(&write_txn, "vectors_semantic_768")?;
vectors.insert(uuid, &embedding)?;

write_txn.commit()?;
```

### Pattern 3: Router API Preserved

```rust
// External API unchanged:
router.create_entity(collection, entity)?;
router.get_entity(collection, entity_id)?;
router.set_property(collection, entity_id, "age", 42)?;

// Internal implementation changed:
// - Router → ManifoldCollection
// - ManifoldCollection → ColumnFamilyDatabase
// - Column family lookup by entity type
// - Operations use new storage layer
```

---

## Success Criteria (Updated)

### Functional Requirements
- [ ] Router API works identically to before
- [ ] All 620 existing tests pass
- [ ] Entity CRUD operations work
- [ ] Property operations work
- [ ] Graph operations work (bidirectional)
- [ ] Vector operations work (zero-copy)
- [ ] Time series operations work
- [ ] Cascade system functions
- [ ] Import system works

### Performance Requirements
- [ ] Entity ops: ≥ baseline
- [ ] Property ops: ≥ baseline
- [ ] Graph queries: **Faster** (O(log N) vs O(N))
- [ ] Vector access: **Faster** (zero-copy)
- [ ] Cold start: **Faster** (no type index rebuild)
- [ ] File handles: **28x fewer** (140 → 5)

### Code Quality
- [ ] Zero `use redb::` imports (except in `_old_persistence/`)
- [ ] Clean Manifold domain crate usage
- [ ] Comprehensive unit tests (>80% coverage)
- [ ] Integration tests for all workflows
- [ ] Documentation complete

---

## Risk Mitigation

### Risk 1: Scope Creep
**Mitigation**: Strict boundary enforcement. Router API is the contract. Everything below Router is rewrite scope. Everything above (cascade, streams, compute) is out of scope.

### Risk 2: Subtle Business Logic Bugs
**Mitigation**: 620 tests must all pass. Use old code as reference for understanding required behavior. Add new tests when old ones are insufficient.

### Risk 3: Performance Regression
**Mitigation**: Benchmark each week. If new implementation is slower, investigate and optimize before moving on.

### Risk 4: Timeline Slip
**Mitigation**: Weekly checkpoints. If behind, reduce scope (e.g., skip optimization, add TODOs for Phase 6).

---

## Weekly Checkpoints

### End of Week 1
- [ ] Module structure complete
- [ ] Design validated
- [ ] Old code moved to reference location
- [ ] Router/coordinator preserved

### End of Week 2
- [ ] Entity operations work
- [ ] Property operations work
- [ ] Master index functional
- [ ] Unit tests passing

### End of Week 3
- [ ] Graph operations work
- [ ] Bidirectional queries working
- [ ] Edge tests passing

### End of Week 4
- [ ] Vector operations work
- [ ] Zero-copy access validated
- [ ] Vector tests passing

### End of Week 5
- [ ] Time series operations work
- [ ] Trajectory tracking working
- [ ] Measure storage functional

### End of Week 6
- [ ] Router integration complete
- [ ] All 620 tests passing
- [ ] Performance benchmarks done
- [ ] Ready for Phase 3

---

## References

- [NEW_STORAGE_ARCHITECTURE.md](../../../hyperspatial/docs/NEW_STORAGE_ARCHITECTURE.md) - Detailed design
- [AUDIT_REPORT.md](../phase-1/AUDIT_REPORT.md) - Analysis of old code
- [REDB_USAGE_CATALOG.md](../phase-1/REDB_USAGE_CATALOG.md) - 528+ reference catalog
- [HYPERSPATIAL_IMPLEMENTATION_PHASES.md](../../HYPERSPATIAL_IMPLEMENTATION_PHASES.md) - Original plan

---

**Next Action**: Begin Week 1 implementation (move old code, scaffold new structure)