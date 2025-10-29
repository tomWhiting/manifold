# Phase 1: Preparation & Analysis - Development Journal

**Branch**: redesign  
**Started**: 2025-01-XX  
**Status**: In Progress

---

## Overview

Phase 1 focuses on understanding the current codebase, defining clean interfaces, and establishing baselines before any code migration begins.

### Goals
- [ ] Complete comprehensive code audit
- [ ] Define storage interface traits
- [ ] Verify existing test suites are adequate
- [ ] Establish performance baselines
- [ ] Design new module structure

---

## Daily Log

### 2025-01-XX - Session 1: Initial Setup & Comprehensive Audit

**Tasks Completed:**
- ✅ Created `redesign` branch for HyperQL
- ✅ Created `redesign` branch for Hyperspatial
- ✅ Created Phase 1 journal structure
- ✅ Completed comprehensive code audit
- ✅ Created detailed AUDIT_REPORT.md

**Current Focus:** Audit complete - ready for interface design

**Audit Summary:**
- **CORRECTED**: 528+ redb API references across 20 files (not 16!)
- 620 unit tests in Hyperspatial, 346 in HyperQL
- File handle explosion confirmed: N×M×7 databases per deployment
- Custom vector/graph/timeseries storage duplicates Manifold functionality
- Test coverage excellent - migration risk is low
- Code quality high - minimal warnings, clean builds
- **Migration complexity higher than initially assessed**

**Key Findings:**
1. DatabaseManager creates 7+ separate redb databases per shard
2. With 5 collections × 3 shards × 7 databases = 105 file handles minimum
3. VectorStorageManager complex custom code duplicates manifold-vectors
4. Edge storage has no bidirectional index (O(N) reverse lookups)
5. String-based composite keys throughout (should be fixed-width tuples)
6. **CRITICAL**: 528+ redb API references across 20 files - much deeper coupling than initially assessed
7. Most coupled file: streams/core/persistence.rs with 52 references
8. 8 files have 30+ references each (critical coupling)

**Storage Patterns Documented:**
- Entity storage: String keys → UUID keys
- Property storage: String concat keys → Manifold tuple keys
- Vector storage: Custom manager → manifold-vectors VectorTable
- Graph storage: No reverse index → manifold-graph with bidirectional
- Time series: Manual encoding → manifold-timeseries with auto-encoding

**Test Suite Analysis:**
- Hyperspatial: 31 integration tests + 620 unit tests + 106 test modules
- HyperQL: 13 integration tests + 346 unit tests
- ✅ Coverage is excellent across all subsystems
- ⚠️ Many tests directly use DatabaseManager (will need updates)
- ✅ Cascade, temporal, entity logic tests should remain valid

**Next Actions:**
- Define storage abstraction traits (higher priority - needed for 20 files)
- Capture performance baselines
- Design module structure
- Document UUID generation strategy
- Create migration strategy for 528+ API references
- Revise Phase 2 timeline (6-8 weeks vs original 4-6 weeks)

---

## Code Audit Progress

### Audit Checklist

#### Hyperspatial Storage Analysis
- [x] Map all direct `redb::` imports and usage (16 files identified)
- [x] Identify DatabaseManager instantiation patterns
- [x] Document current file handle usage (N×M×7 = 105+ handles)
- [x] Catalog vector storage patterns
- [x] Catalog time series storage patterns
- [x] Catalog graph/edge storage patterns
- [x] Document entity/property storage patterns
- [x] Map Router storage dependencies
- [x] Map Collection storage dependencies
- [x] Identify schema storage patterns
- [x] Document index storage patterns (HNSW, spatial, etc.)

#### HyperQL Dependencies Analysis
- [x] Identify Hyperspatial-specific assumptions
- [x] Document DataSource trait usage
- [x] Map executor dependencies
- [x] Catalog geometric operation dependencies
- [x] Review type system for portability

#### Interface Design
- [ ] Define StorageBackend trait
- [ ] Define VectorStorage trait
- [ ] Define GraphStorage trait
- [ ] Define TimeSeriesStorage trait
- [ ] Define IndexStorage trait
- [ ] Define SchemaStorage trait

#### Test Suite Verification
- [x] Review Hyperspatial test coverage (excellent)
- [x] Review HyperQL test coverage (excellent)
- [x] Identify tests requiring updates (storage layer tests)
- [ ] Document performance test baselines (in progress)

---

## Findings

### Critical Issues Discovered

#### Issue 1: File Handle Explosion ✅ CONFIRMED
**Location**: `hyperspatial/src/persistence/storage/manager.rs`
**Problem**: Each DatabaseManager creates 7+ separate redb databases:
  - core.redb, properties.redb, edges.redb, measures.redb
  - trajectories.redb, edge_properties.redb
  - vectors/ directory (multiple DBs), timeseries/ directory
**Impact**: With 5 collections × 3 shards × 7 databases = **105 file handles minimum**
**Solution**: Migrate to Manifold column families (1 db file with N column families)
**Validation**: Audited manager.rs lines 1-100, confirmed 7+ database creations

#### Issue 2: Extensive redb Coupling Throughout Codebase ✅ CATALOGED
**CORRECTED Count**: 528+ API references across 20 files:
  - Critical coupling (30+ refs): 8 files including streams/core/persistence.rs (52), storage/core.rs (51), storage/properties.rs (47)
  - High coupling (20-29 refs): 5 files including storage/edges.rs (29), connection_pool.rs (21)
  - Moderate coupling (10-19 refs): 4 files including batch_operations.rs (19), indices/hnsw.rs (18)
  - Low coupling (1-9 refs): 3 files
**Problem**: Extensive use of redb APIs throughout - not just imports
**Impact**: 528+ places where redb API is called - deep architectural coupling
**Solution**: Comprehensive abstraction layer + domain crate migration
**Details**: See REDB_USAGE_CATALOG.md for complete breakdown with reference counts

### Storage Patterns Identified

#### Pattern 1: Entity Storage
**Current**: 
- Separate core.redb with entities table
- Key: String entity_id
- Value: Bincode-serialized Entity struct

**Target**:
- Manifold column family "entities"
- Key: UUID (16-byte fixed width)
- Value: Bincode-serialized Entity struct

#### Pattern 2: Vector Storage
**Current**:
- Custom VectorStorageManager with multiple tables
- Dynamic table creation per vector type
- Variable-width keys

**Target**:
- manifold-vectors VectorTable<DIM>
- One table per vector type/dimension
- UUID keys

#### Pattern 3: Graph Storage ✅ DOCUMENTED
**Current**: 
- edges.redb with string composite keys
- Key: "{source_id}:{edge_type}:{target_id}"
- No bidirectional index (reverse lookups are O(N))
- Separate edge_properties.redb database

**Target**:
- manifold-graph GraphTable
- UUID-based vertices, automatic bidirectional indexes
- Fixed-width (bool, f32) edge properties
- Range scans for efficient traversal

#### Pattern 4: Vector Storage ✅ DOCUMENTED
**Current**:
- Custom VectorStorageManager with separate DBs per type
- Complex caching, compression, batch operations
- String-based cache keys

**Target**:
- manifold-vectors VectorTable<DIM>
- One table per vector name/dimension
- UUID keys, zero-copy access
- Built-in Manifold optimizations

#### Pattern 5: Time Series Storage ✅ DOCUMENTED
**Current**:
- trajectories.redb with string concat keys
- Key: "{entity_id}:{timestamp}"
- Manual timestamp encoding

**Target**:
- manifold-timeseries TimeSeriesTable
- Composite key: (entity_id, timestamp)
- Automatic encoding, retention policies

See AUDIT_REPORT.md for complete pattern analysis.

### Migration Complexity Assessment ✅ UPDATED

**Original Estimate**: 16 files to migrate  
**Actual Scope**: 20 files with 528+ API references  
**Underestimation Factor**: 33x (counting references vs files)

**Why the Discrepancy:**
- Only counted `use redb::` import statements initially
- Didn't count actual API calls (begin_write, open_table, etc.)
- Didn't count TableDefinition declarations (25+ const tables)
- Didn't count error conversions and type usage

**Revised Effort:**
- 8 files with critical coupling (30+ refs each) = ~400 references
- Requires careful abstraction strategy, not simple find/replace
- Estimated 6-8 weeks for Phase 2 (up from 4-6 weeks)

---

## Interface Designs

### StorageBackend Trait (Draft)

```rust
// To be defined based on audit findings
pub trait StorageBackend {
    type Transaction;
    type Error;
    
    fn begin_write(&self) -> Result<Self::Transaction, Self::Error>;
    fn begin_read(&self) -> Result<Self::Transaction, Self::Error>;
}
```

[More interfaces to be defined]

---

## Performance Baselines

### Benchmarking Plan Created
See AUDIT_REPORT.md Section "Performance Baseline Requirements" for complete metrics list.

### Current Metrics (To be measured)

- [ ] Entity insertion throughput
- [ ] Property update throughput
- [ ] Edge creation throughput
- [ ] Vector insertion throughput
- [ ] HNSW query latency
- [ ] Multi-modal query latency
- [ ] Cascade propagation latency
- [ ] Cold start time
- [ ] Index build time (20K entities)

---

## Module Structure Design

### Hyperspatial (Proposed)

```
hyperspatial/
├── src/
│   ├── hyperbolic/          # Core hyperbolic space operations
│   ├── cascade/             # Cascade system (keep as-is)
│   ├── streams/             # Stream coordination (keep as-is)
│   ├── compute/             # Lua/WASM runtimes (keep as-is)
│   ├── indexing/            # NEW: All index implementations
│   │   ├── hnsw/           # Hyperbolic HNSW
│   │   ├── vector/         # Vector indexes
│   │   ├── graph/          # Graph indexes
│   │   └── temporal/       # Temporal indexes
│   ├── storage/             # NEW: Manifold integration layer
│   │   ├── traits/         # Storage abstractions
│   │   ├── entities/       # Entity storage via Manifold
│   │   ├── properties/     # Property storage via Manifold
│   │   ├── vectors/        # manifold-vectors integration
│   │   ├── graph/          # manifold-graph integration
│   │   └── timeseries/     # manifold-timeseries integration
│   ├── query/               # HyperQL integration (refactored)
│   ├── import/              # Import system (simplified)
│   └── types/               # Core types
├── crates/
│   └── hyperspatial-server/ # NEW: Server moved to separate crate
└── examples/
```

### HyperQL (Proposed)

```
hyperQL/
├── src/                     # Core library (general-purpose)
├── crates/
│   ├── hyperql-compile/     # NEW: Code generation
│   └── hyperql-treesitter/  # NEW: Editor support
└── examples/
```

---

## Next Steps

1. Continue comprehensive code audit
2. Document all redb usage locations
3. Design concrete interface traits
4. Verify test suite coverage
5. Measure performance baselines
6. Finalize module structure

---

## Revised Timeline Impact

### Original Phase 2 Estimate: 4-6 weeks
### Revised Phase 2 Estimate: 6-8 weeks

**Reason**: Coupling is 33x more extensive than initially assessed (528 refs vs 16 files)

**Mitigation Strategy:**
- Create comprehensive abstraction layer first (Week 1)
- Parallel implementation (old + new backends) for safety
- Incremental migration by subsystem
- Feature flags to toggle between implementations
- Run all 620 tests against both backends during transition

---

## Questions & Decisions

### Q1: UUID Generation Strategy?
**Options**: 
- v4 (random) - Better distribution, no temporal correlation
- v7 (timestamp-ordered) - Better range scan performance, temporal locality
**Decision**: **Lean toward v7** for time-based query patterns
**Rationale**: Trajectories and temporal queries benefit from ordered UUIDs
**Action**: Validate with query pattern analysis

### Q2: Vector Table Naming Convention?
**Options**: 
- Dynamic: Create table per name (e.g., "semantic_vectors_768")
- Registry: Single registry table with metadata
**Decision**: **Separate tables** - confirmed by redesign doc
**Rationale**: Type safety, zero-copy access, matches manifold-vectors design
**Implementation**: VectorTable<768> for semantic_vectors, etc.

### Q3: Schema Storage Location?
**Options**: Manifold column family vs separate database
**Decision**: TBD - lean toward Manifold CF for consistency

---

## References

- [HYPERSPATIAL_REDESIGN.md](../../HYPERSPATIAL_REDESIGN.md)
- [HYPERSPATIAL_IMPLEMENTATION_PHASES.md](../../HYPERSPATIAL_IMPLEMENTATION_PHASES.md)
- [HYPERSPATIAL_EXAMPLES.md](../../HYPERSPATIAL_EXAMPLES.md)
- [AUDIT_REPORT.md](./AUDIT_REPORT.md) - Comprehensive audit findings
- [REDB_USAGE_CATALOG.md](./REDB_USAGE_CATALOG.md) - **NEW**: Complete 528+ reference breakdown