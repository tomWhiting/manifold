# Implementation Instructions for manifold-properties

## Context

You are implementing the manifold-properties crate for the Manifold embedded database. This crate provides type-safe, efficient property storage using native Manifold types instead of string-based serialization.

## Getting Started

First, familiarize yourself with the Manifold codebase:

```bash
cd /Users/tom/Developer/spaces/projects/hyperspatial/main/manifold
git ls-files | head -100
```

Then explore the structure of existing domain crates:

```bash
ls -la crates/manifold-vectors/src/
ls -la crates/manifold-graph/src/
ls -la crates/manifold-timeseries/src/
```

## Your Task

Implement the manifold-properties crate at:
```
/Users/tom/Developer/spaces/projects/hyperspatial/main/manifold/crates/manifold-properties/
```

## Critical Reference Implementations

**Study these files carefully before implementing:**

1. **manifold-vectors** (CLOSEST ANALOG):
   - `crates/manifold-vectors/src/dense.rs` - Zero-copy guard pattern (VectorGuard)
   - `crates/manifold-vectors/src/lib.rs` - Crate organization pattern
   - Study how VectorTable wraps Manifold tables with type-safe API

2. **manifold-graph**:
   - `crates/manifold-graph/src/edge.rs` - Native type definitions
   - `crates/manifold-graph/src/graph.rs` - Composite key patterns

3. **Manifold core**:
   - `src/types/value.rs` - The Value trait you need to implement
   - Study how String and fixed-width types implement Value

## What You're Building

### PropertyValue Enum

Replace the current string-based PropertyValue:
```rust
// OLD (in hyperspatial):
struct PropertyValue {
    value: String,      // "42"
    data_type: String,  // "Integer"
    updated_at: u64,
    valid_from: u64,
}
```

With a native typed enum:
```rust
// NEW (manifold-properties):
enum PropertyValue {
    Integer { value: i64, updated_at: u64, valid_from: u64 },
    Float { value: f64, updated_at: u64, valid_from: u64 },
    Boolean { value: bool, updated_at: u64, valid_from: u64 },
    String { value: String, updated_at: u64, valid_from: u64 },
    Null { updated_at: u64, valid_from: u64 },
}
```

### Why This Matters

**Performance:**
- 3-8x faster numeric comparisons (no string parsing)
- Zero-copy reads for Integer/Float/Boolean
- Properties are in every hot path (queries, filters, cascades)

**Storage:**
- 50-60% smaller for numeric properties
- Eliminates data_type string (saves 6-15 bytes per property)
- 25-30% overall reduction for typical workloads

## Implementation Requirements

### File-by-File Tasks

**1. src/property_value.rs** (~200-300 lines):
- Define PropertyValue enum with 5 variants
- Each variant has: value field + updated_at + valid_from
- Implement constructors: new_integer(), new_float(), etc.
- Implement accessors: as_integer(), as_float(), etc.
- Implement type_name() returning &'static str
- Unit tests for all methods

**2. src/encoding.rs** (~300-400 lines):
- Implement `manifold::types::Value` trait for PropertyValue
- Handle discriminant byte (1 byte for variant type)
- Zero-copy SelfType<'a> = PropertyValueRef<'a> for reads
- Encode each variant efficiently
- Study manifold/src/types/value.rs lines 517-548 (String example)

**3. src/table.rs** (~250-350 lines):
- PropertyTable<'txn> struct wrapping Manifold Table
- Composite key: (Uuid, &str)
- Methods: set(), get(), get_all(), delete()
- PropertyTableRead for read-only access
- PropertyGuard for zero-copy access (like VectorGuard)
- Follow manifold-vectors/src/dense.rs pattern

**4. src/operations.rs** (~150-200 lines):
- batch_set_properties() - bulk writes
- batch_get_properties() - bulk reads
- bulk_delete_properties() - bulk deletes
- Range query optimization for get_all()

**5. src/temporal.rs** (~100-150 lines):
- get_property_at() - property value at specific timestamp
- get_property_history() - version history
- Use valid_from field for temporal filtering

### Critical Rules

**NO implementation in lib.rs:**
- lib.rs is for module organization and documentation ONLY
- All implementation goes in individual module files

**Follow manifold patterns:**
- Study manifold-vectors guard pattern for zero-copy
- Use composite keys like manifold-graph
- Temporal encoding like manifold-timeseries

**Zero-copy where possible:**
- Integer/Float/Boolean must use zero-copy access
- String can allocate (variable-length)
- Return guards, not owned values

**Test thoroughly:**
- Unit tests in each module
- Integration tests in tests/ directory
- Benchmark against bincode baseline

## Success Criteria

After implementation:
- `cargo check` - zero errors
- `cargo test` - all tests pass
- `cargo bench` - meets performance targets
- Zero-copy verified for numeric types
- Ready for Hyperspatial integration

## Design Document

Read `/Users/tom/Developer/spaces/projects/hyperspatial/main/manifold/crates/manifold-properties/DESIGN.md` for:
- Complete performance analysis
- Detailed API specifications
- Integration strategy with Hyperspatial
- 6-phase implementation plan

## Questions to Ask

If anything is unclear:
1. How does manifold-vectors achieve zero-copy? (Study VectorGuard)
2. How do composite keys work? (Study manifold-graph)
3. How does the Value trait work? (Study manifold/src/types/value.rs)
4. What's the encoding format? (Study existing domain crates)

## Estimated Timeline

- Phase 1 (PropertyValue enum): 4-6 hours
- Phase 2 (Value trait): 6-8 hours
- Phase 3 (PropertyTable API): 4-6 hours
- Phase 4 (Bulk operations): 3-4 hours
- Phase 5 (Temporal support): 2-3 hours
- Phase 6 (Testing): 3-4 hours

**Total:** 22-31 hours (~3-4 days)

## Ready to Start

The scaffolding is complete. All module files exist as placeholders. The design is documented. Reference implementations are identified.

Begin with src/property_value.rs - define the enum and basic methods. Then move to encoding.rs to implement the Value trait. Good luck!
