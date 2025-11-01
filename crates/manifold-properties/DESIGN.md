# manifold-properties Design Document

## Executive Summary

Create a native Manifold property storage crate that eliminates string-based serialization overhead, achieving 3-8x faster numeric comparisons and 50-60% storage reduction for numeric properties.

**Status:** Scaffolding complete, ready for implementation
**Effort:** ~1,000-1,400 lines, 2-3 days development
**Impact:** High - properties are touched in every query, filter, and cascade operation

---

## Why We're Doing This

### Current Approach (Inefficient)

PropertyValue stores everything as strings with a data_type field:

```rust
// Current (hyperspatial/src/types/entity.rs)
pub struct PropertyValue {
    pub value: String,       // "42" or "3.14" or "true"
    pub data_type: String,   // "Integer" or "Float" or "Boolean"
    pub updated_at: u64,
    pub valid_from: u64,
}
```

**Problems:**
1. **String parsing overhead:** Every numeric comparison parses "42" → 42
2. **Wasteful storage:** Storing "Integer" string for every property
3. **Type unsafety:** Can store "abc" with data_type="Integer"
4. **Bincode overhead:** Serialization adds 5-10% overhead

### Proposed Approach (Efficient)

Native typed enum with Manifold Value trait:

```rust
// manifold-properties (proposed)
pub enum PropertyValue {
    Integer { value: i64, updated_at: u64, valid_from: u64 },
    Float { value: f64, updated_at: u64, valid_from: u64 },
    Boolean { value: bool, updated_at: u64, valid_from: u64 },
    String { value: String, updated_at: u64, valid_from: u64 },
    Null { updated_at: u64, valid_from: u64 },
}
```

**Benefits:**
1. **Zero-copy** for Integer/Float/Boolean (fixed-width)
2. **No parsing:** Direct i64/f64/bool access
3. **Type safety:** Impossible to store "abc" as Integer
4. **Smaller:** Discriminant (1 byte) instead of data_type string (6-15 bytes)

---

## Performance Analysis

### Storage Comparison

| Property Type | Current (bincode) | manifold-properties | Reduction |
|--------------|------------------|---------------------|-----------|
| age: 42 | ~50-80 bytes | ~25 bytes | 50-68% |
| score: 3.14 | ~50-80 bytes | ~25 bytes | 50-68% |
| active: true | ~50-80 bytes | ~17 bytes | 66-79% |
| name: "Alice" | ~50-80 bytes | ~30 bytes | 38-62% |

**Overall:** 25-30% storage reduction for typical workloads (50% numeric properties)

### Query Performance

| Operation | Current | manifold-properties | Speedup |
|-----------|---------|---------------------|---------|
| Numeric comparison (WHERE age > 25) | 80-200 ns | 10-25 ns | 3-8x faster |
| Property read (numeric) | 100-300 ns | 5-15 ns | 6-20x faster |
| Property read (string) | 100-300 ns | 50-150 ns | 2x faster |
| Batch property reads | 50-150 μs | 15-40 μs | 3-4x faster |

---

## Design Specification

### 1. PropertyValue Enum (property_value.rs)

```rust
pub enum PropertyValue {
    Integer {
        value: i64,
        updated_at: u64,
        valid_from: u64,
    },
    Float {
        value: f64,
        updated_at: u64,
        valid_from: u64,
    },
    Boolean {
        value: bool,
        updated_at: u64,
        valid_from: u64,
    },
    String {
        value: String,
        updated_at: u64,
        valid_from: u64,
    },
    Null {
        updated_at: u64,
        valid_from: u64,
    },
}
```

**Methods to implement:**
- `new_integer(value: i64) -> Self`
- `new_float(value: f64) -> Self`
- `new_boolean(value: bool) -> Self`
- `new_string(value: String) -> Self`
- `new_null() -> Self`
- `type_name(&self) -> &'static str`
- `as_integer(&self) -> Option<i64>`
- `as_float(&self) -> Option<f64>`
- `as_boolean(&self) -> Option<bool>`
- `as_string(&self) -> Option<&str>`
- `updated_at(&self) -> u64`
- `valid_from(&self) -> u64`

### 2. Manifold Value Trait (encoding.rs)

Implement Manifold's `Value` trait for PropertyValue to enable native serialization:

```rust
impl manifold::types::Value for PropertyValue {
    type SelfType<'a> = PropertyValueRef<'a>;
    type AsBytes<'a> = PropertyValueBytes<'a>;

    fn fixed_width() -> Option<usize> {
        None  // Enum with variable-width String variant
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a> {
        // Decode discriminant (1 byte)
        // Decode variant data based on discriminant
        // Return zero-copy reference for fixed-width variants
    }

    fn as_bytes<'a>(&'a self) -> Self::AsBytes<'a> {
        // Encode discriminant + variant data
    }
}
```

**Key insight:** Even though PropertyValue has a String variant (variable-width), the Integer/Float/Boolean variants are fixed-width and can use zero-copy access.

### 3. PropertyTable API (table.rs)

Main storage interface following manifold-vectors pattern:

```rust
pub struct PropertyTable<'txn> {
    table: manifold::Table<'txn, (Uuid, &'static str), PropertyValue>,
    // Internal state
}

impl<'txn> PropertyTable<'txn> {
    pub fn open(txn: &'txn WriteTransaction, table_name: &str) -> Result<Self>;

    pub fn set(
        &mut self,
        entity_id: &Uuid,
        property_key: &str,
        value: PropertyValue
    ) -> Result<()>;

    pub fn get(
        &self,
        entity_id: &Uuid,
        property_key: &str
    ) -> Result<Option<PropertyGuard>>;

    pub fn get_all(&self, entity_id: &Uuid) -> Result<Vec<(String, PropertyGuard)>>;

    pub fn delete(&mut self, entity_id: &Uuid, property_key: &str) -> Result<()>;
}

pub struct PropertyTableRead<'txn> {
    // Read-only access with zero-copy guards
}

pub struct PropertyGuard<'a> {
    // Zero-copy access guard (like VectorGuard in manifold-vectors)
    value: PropertyValueRef<'a>,
}
```

### 4. Bulk Operations (operations.rs)

```rust
pub fn batch_set_properties(
    table: &mut PropertyTable,
    properties: &[(Uuid, String, PropertyValue)]
) -> Result<usize>;

pub fn batch_get_properties(
    table: &PropertyTableRead,
    keys: &[(Uuid, &str)]
) -> Result<Vec<Option<PropertyGuard>>>;

pub fn bulk_delete_properties(
    table: &mut PropertyTable,
    keys: &[(Uuid, &str)]
) -> Result<usize>;
```

### 5. Temporal Queries (temporal.rs)

```rust
pub fn get_property_at(
    table: &PropertyTableRead,
    entity_id: &Uuid,
    property_key: &str,
    timestamp: u64
) -> Result<Option<PropertyGuard>>;

pub fn get_property_history(
    table: &PropertyTableRead,
    entity_id: &Uuid,
    property_key: &str
) -> Result<Vec<(u64, PropertyGuard)>>;
```

---

## Implementation Strategy

### Phase 1: Core PropertyValue Enum (4-6 hours)
- Define enum with all variants
- Implement constructor methods
- Implement accessor methods (as_integer, as_float, etc.)
- Unit tests for conversions

### Phase 2: Manifold Value Trait (6-8 hours)
- Study manifold-vectors encoding.rs as reference
- Implement Value trait with discriminant encoding
- Handle zero-copy for fixed-width variants
- Integration tests with Manifold tables

### Phase 3: PropertyTable API (4-6 hours)
- Implement PropertyTable wrapper around Manifold table
- Composite key (Uuid, &str) support
- CRUD operations (set, get, delete)
- Range queries for get_all()

### Phase 4: Bulk Operations (3-4 hours)
- batch_set_properties with transaction grouping
- batch_get_properties with range optimization
- Performance tests

### Phase 5: Temporal Support (2-3 hours)
- Historical queries using valid_from
- Version history traversal
- Temporal range queries

### Phase 6: Integration & Testing (3-4 hours)
- Comprehensive test suite
- Performance benchmarks
- Documentation examples

**Total:** 22-31 hours (~3-4 days)

---

## Reference Implementations

Study these manifold domain crates for patterns:

1. **manifold-vectors** (`/main/manifold/crates/manifold-vectors/`):
   - Zero-copy access via guards (src/dense.rs)
   - Value trait implementation for fixed-width arrays
   - VectorTable/VectorTableRead pattern

2. **manifold-graph** (`/main/manifold/crates/manifold-graph/`):
   - Edge struct with native types (src/edge.rs)
   - GraphTable wrapper (src/graph.rs)
   - Bidirectional indexing

3. **manifold-timeseries** (`/main/manifold/crates/manifold-timeseries/`):
   - Temporal encoding patterns
   - Composite key handling
   - Batch operations

---

## Integration with Hyperspatial

### Files to Update

1. **Add dependency** to `hyperspatial/Cargo.toml`:
```toml
manifold-properties = { version = "0.1.0", path = "../manifold/crates/manifold-properties" }
```

2. **Update PropertyValue import** in `hyperspatial/src/types/entity.rs`:
```rust
pub use manifold_properties::PropertyValue;
// Remove old PropertyValue struct definition
```

3. **Update PropertyStorage** in `hyperspatial/src/storage/properties.rs`:
```rust
use manifold_properties::{PropertyTable, PropertyTableRead};

// Replace bincode serialization with direct PropertyTable usage
// Remove serialization::serialize/deserialize calls
// Use PropertyTable::set/get directly
```

4. **Update type conversions** throughout Hyperspatial:
- Remove string parsing in query filters
- Direct numeric access in cascade aggregations
- Updated type checking in validators

---

## Testing Strategy

### Unit Tests (in manifold-properties)
- PropertyValue variant creation and access
- Type conversions and comparisons
- Temporal metadata handling
- Value trait encoding/decoding

### Integration Tests (in manifold-properties)
- PropertyTable CRUD operations
- Bulk operations performance
- Temporal queries correctness
- Range query efficiency

### System Tests (in hyperspatial)
- End-to-end property operations
- Query filter performance
- Cascade aggregation correctness
- Migration from old PropertyValue (verify data compatibility)

---

## Performance Targets

### Storage
- 50% reduction for numeric properties
- 25-30% overall reduction (50% numeric workload)

### Query Performance
- 3-8x faster WHERE clause numeric comparisons
- 6-20x faster numeric property reads
- 3-4x faster bulk property operations

### Memory
- Zero allocations for numeric property reads
- Reduced memory copying in aggregations

---

## Risk Mitigation

### Low Risk Factors
- **No backwards compatibility:** Greenfield project eliminates migration complexity
- **We control Manifold:** Can modify if needed
- **Clear reference implementations:** manifold-vectors, manifold-graph proven patterns

### Success Criteria
- All hyperspatial tests pass after integration
- Performance targets met in benchmarks
- Zero-copy access verified for fixed-width variants
- Storage reduction measured and documented

---

## File Structure

```
manifold-properties/
├── Cargo.toml                 # Dependencies and metadata
├── DESIGN.md                  # This document
├── src/
│   ├── lib.rs                 # Module organization (documentation only)
│   ├── property_value.rs      # PropertyValue enum definition
│   ├── encoding.rs            # Manifold Value trait implementation
│   ├── table.rs               # PropertyTable CRUD API
│   ├── operations.rs          # Bulk operations
│   └── temporal.rs            # Temporal query support
└── tests/
    └── integration.rs         # Integration tests (to be created)
```

---

## Next Steps for Implementation Agent

1. **Start with property_value.rs:** Define PropertyValue enum with all variants and methods
2. **Implement encoding.rs:** Value trait following manifold-vectors pattern
3. **Build table.rs:** PropertyTable wrapper with composite key support
4. **Add operations.rs:** Bulk operations for efficiency
5. **Implement temporal.rs:** Historical queries
6. **Test thoroughly:** Unit tests, integration tests, benchmarks

**Reference pattern:** manifold-vectors is the closest analog - study src/dense.rs for zero-copy guard pattern.

---

## Success Metrics

After integration into Hyperspatial:
- WHERE age > 25 queries: 3-8x faster
- Cascade aggregations: 2-4x faster (less deserialization)
- Storage: 25-30% reduction overall
- All 586+ tests pass
- Zero regressions in functionality
