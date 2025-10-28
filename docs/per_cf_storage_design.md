# Per-Column-Family Storage Architecture Design

**Status:** Design Proposal  
**Date:** 2025-01-28  
**Author:** Performance Investigation  
**Goal:** Eliminate lock contention between column families by giving each CF independent storage backend

---

## Problem Statement

**Current Bottleneck:**
- All column families share a single `PagedCachedFile` instance
- Single `write_buffer: Mutex<LRUWriteCache>` lock serializes all writes
- 8 concurrent column families contend for the same lock on every page write
- Performance: 746K ops/sec (5.8x slower than RocksDB)

**Root Cause:**
```rust
// Current architecture (WRONG)
ColumnFamily1 ─┐
ColumnFamily2 ─┼─> PartitionedStorageBackend ─> PagedCachedFile (SHARED LOCK)
ColumnFamily3 ─┘
```

Every `write()` call from any CF must acquire the same lock, completely serializing independent writes.

---

## Proposed Architecture

### High-Level Design

```rust
// Target architecture (CORRECT)
ColumnFamily1 ─> PagedCachedFile1 (independent lock + cache)
ColumnFamily2 ─> PagedCachedFile2 (independent lock + cache)
ColumnFamily3 ─> PagedCachedFile3 (independent lock + cache)
```

Each column family gets:
- Independent `write_buffer` (no contention)
- Independent `read_cache` (no contention)
- Independent metrics (accurate per-CF stats)
- Own file descriptor (already provided by `FileHandlePool`)

### Key Insight

`FileHandlePool` already provides separate file descriptors per CF. We just need to create separate `PagedCachedFile` instances that wrap those descriptors.

---

## Implementation Plan

### Phase 1: Core Architecture Change

**1.1 Modify `ColumnFamilyState::ensure_database()`**

Current (simplified):
```rust
pub fn ensure_database(&self, pool: &FileHandlePool, ...) -> Arc<Database> {
    let backend = pool.acquire(&self.name)?;  // Raw UnlockedFileBackend
    let partition_backend = PartitionedStorageBackend::new(backend, ...);
    Arc::new(Database::builder().create_with_backend(partition_backend)?)
}
```

Target:
```rust
pub fn ensure_database(&self, pool: &FileHandlePool, ...) -> Arc<Database> {
    let backend = pool.acquire(&self.name)?;  // Raw UnlockedFileBackend
    
    // Create PagedCachedFile for THIS column family
    let cached_backend = PagedCachedFile::new(
        backend,
        page_size,
        read_cache_size,   // Per-CF allocation
        write_buffer_size, // Per-CF allocation
    )?;
    
    // Wrap in PartitionedStorageBackend (handles segment offsets)
    let partition_backend = PartitionedStorageBackend::new_with_cache(
        cached_backend,
        segments,
        expansion_callback,
    );
    
    Arc::new(Database::builder().create_with_backend(partition_backend)?)
}
```

**1.2 Update `PartitionedStorageBackend`**

Add new constructor that accepts pre-wrapped `PagedCachedFile`:
```rust
impl PartitionedStorageBackend {
    pub fn new_with_cache(
        cached_backend: PagedCachedFile,
        segments: Vec<Segment>,
        expansion_callback: ...,
    ) -> Self {
        // Don't wrap in PagedCachedFile again - already wrapped
        Self {
            backend: Arc::new(cached_backend),
            segments: RwLock::new(segments),
            ...
        }
    }
}
```

**1.3 Database Layer Changes**

`Database::create_with_backend()` already accepts any `StorageBackend`, so no changes needed there. The `PagedCachedFile` implements `StorageBackend`, so it's transparent.

### Phase 2: Memory Management

**2.1 Per-CF Cache Budgets**

Current: Single global cache budget  
Target: Each CF gets allocated portion of total cache

```rust
// In ColumnFamilyDatabase::open_with_builder()
let total_read_cache = 4 * 1024 * 1024 * 1024; // 4GB total
let total_write_buffer = 1 * 1024 * 1024 * 1024; // 1GB total

let num_cfs = self.column_families.len().max(8); // Minimum 8 for new CFs

let per_cf_read_cache = total_read_cache / num_cfs;
let per_cf_write_buffer = total_write_buffer / num_cfs;

// Pass to ensure_database()
```

**2.2 Dynamic Rebalancing (Future Enhancement)**

For now: Static allocation per CF  
Future: Monitor CF activity and rebalance cache dynamically

### Phase 3: Metrics & Monitoring

**3.1 Per-CF Cache Stats**

```rust
impl ColumnFamily {
    pub fn cache_stats(&self) -> CacheStats {
        // Get stats from this CF's PagedCachedFile
        self.database.cache_stats()
    }
}
```

**3.2 Aggregate Stats**

```rust
impl ColumnFamilyDatabase {
    pub fn aggregate_cache_stats(&self) -> CacheStats {
        // Sum stats from all CFs
        let mut total = CacheStats::default();
        for cf in self.column_families.values() {
            total += cf.cache_stats();
        }
        total
    }
}
```

---

## Technical Details

### File Descriptor Coordination

**Q: Won't multiple file descriptors to the same file cause issues?**

A: No, this already works:
- `FileHandlePool` already creates multiple FDs to the same file
- Each FD has independent file position pointer
- OS handles synchronization of writes to the same inode
- `file_growth_lock` already serializes `set_len()` operations

### Write Ordering

**Q: How do we ensure write ordering across CFs?**

A: We don't need to:
- Each CF writes to independent page ranges (handled by `PartitionedStorageBackend`)
- Segments don't overlap (enforced by header allocation)
- WAL provides cross-CF ordering for recovery

### Page Range Isolation

Each CF's pages are isolated by segment offsets:
```
File Layout:
[Header][CF1 Segment 1][CF2 Segment 1][CF1 Segment 2][CF3 Segment 1]...

CF1 writes: Offset by segment1.offset + segment2.offset
CF2 writes: Offset by segment1.offset
CF3 writes: Offset by segment1.offset
```

`PartitionedStorageBackend` already handles this mapping, so it's transparent to `PagedCachedFile`.

### Crash Recovery

**Q: Does this affect crash recovery?**

A: No:
- WAL already logs all CF transactions with CF names
- Recovery replays transactions to correct CFs
- Each CF's `PagedCachedFile` is recreated on open
- No cross-CF dependencies in recovery

---

## Migration Path

### Backward Compatibility

**File Format:** No changes needed  
**Header Format:** No changes needed  
**WAL Format:** No changes needed  
**Recovery:** Works identically

### Testing Strategy

**Unit Tests:**
- Per-CF cache isolation
- Independent lock verification
- Memory budget allocation

**Integration Tests:**
- Concurrent multi-CF writes
- CF creation/deletion with dynamic cache allocation
- Crash recovery with per-CF backends

**Benchmarks:**
- cf_comparison_benchmark (expect 2-4x improvement)
- Verify lock contention reduction with profiler
- Measure per-CF cache hit rates

---

## Expected Performance Impact

### Conservative Estimate

**Before:** 746K ops/sec (8 CFs, shared lock)  
**After:** 2M-2.5M ops/sec (3x improvement)

**Reasoning:**
- Eliminate lock contention between 8 CFs
- Each CF writes at ~250K-300K ops/sec independently
- 8 × 250K = 2M ops/sec total

### Optimistic Estimate

**After:** 3M-3.5M ops/sec (4-5x improvement)

**Reasoning:**
- Per-CF caching improves hit rates (better locality)
- Reduced lock contention → less CPU overhead
- Better CPU cache utilization (each core owns a CF)

---

## Implementation Checklist

### Core Changes
- [ ] Add `PagedCachedFile::new_with_cache()` constructor
- [ ] Update `ColumnFamilyState::ensure_database()` to create `PagedCachedFile`
- [ ] Add `PartitionedStorageBackend::new_with_cache()` constructor
- [ ] Implement per-CF cache budget allocation
- [ ] Add per-CF cache stats methods

### Memory Management
- [ ] Calculate per-CF cache sizes in `ColumnFamilyDatabase::open()`
- [ ] Pass cache sizes to `ensure_database()`
- [ ] Handle dynamic CF creation (allocate from pool)
- [ ] Handle CF deletion (return cache to pool)

### Testing
- [ ] Unit test: Verify each CF has independent `PagedCachedFile`
- [ ] Unit test: Verify lock isolation (lock CF1, can still write CF2)
- [ ] Integration test: Concurrent writes to 8 CFs
- [ ] Benchmark: cf_comparison_benchmark shows improvement
- [ ] Profile: Confirm lock contention eliminated

### Documentation
- [ ] Update `OPTIMIZATION_PLAN.md` when complete
- [ ] Document per-CF cache budgeting in README
- [ ] Add architecture diagram to docs

---

## Alternative Approaches Considered

### 1. Lock Sharding (REJECTED)

**Idea:** Shard `write_buffer` into 16 independent caches, route by page offset

**Pros:**
- Simpler implementation
- Reduces contention by 16x

**Cons:**
- Still has contention (just less)
- Doesn't give true per-CF isolation
- Band-aid solution, not architecturally correct
- Similar implementation complexity to proper fix

**Decision:** Do it right with per-CF backends

### 2. Lock-Free Write Buffer (REJECTED FOR NOW)

**Idea:** Replace `Mutex<LRUWriteCache>` with lock-free data structure

**Pros:**
- Zero lock contention
- Maximum performance

**Cons:**
- Very complex implementation
- Hard to maintain
- Unclear if it's actually faster (need profiling)
- Can do later if per-CF still not fast enough

**Decision:** Per-CF is simpler and likely sufficient

### 3. Single Backend with Per-CF Locks (REJECTED)

**Idea:** Keep shared `PagedCachedFile`, but add per-CF lock map

**Pros:**
- Minimal changes

**Cons:**
- Adds complexity (lock map)
- Still shares read cache (not fully isolated)
- Harder to get per-CF metrics
- Worse cache locality

**Decision:** Per-CF backends is cleaner

---

## Risks & Mitigations

### Risk 1: Memory Usage Increase

**Risk:** Each CF has its own cache → more memory usage

**Mitigation:**
- Total memory budget stays the same (4GB read + 1GB write)
- Divided equally among active CFs
- Dynamic rebalancing in future if needed

### Risk 2: File Descriptor Limit

**Risk:** More FDs open simultaneously

**Mitigation:**
- `FileHandlePool` already handles this with LRU eviction
- Max pool size configurable (default 64)
- Same number of FDs as before (one per active CF)

### Risk 3: Implementation Complexity

**Risk:** Larger change than anticipated

**Mitigation:**
- Clean separation of concerns
- `PagedCachedFile` already implements `StorageBackend`
- `PartitionedStorageBackend` already handles segment mapping
- Just connecting existing pieces differently

### Risk 4: Performance Doesn't Improve

**Risk:** Bottleneck is elsewhere (not lock contention)

**Mitigation:**
- Profile BEFORE implementing to confirm bottleneck
- Measure lock wait times with perf/instruments
- If profiling shows different bottleneck, adjust plan

---

## Success Metrics

### Must Achieve
- ✅ 2M ops/sec minimum (2.7x improvement)
- ✅ Each CF has independent write_buffer lock
- ✅ All tests pass
- ✅ No memory leaks

### Should Achieve
- ✅ 2.5M ops/sec (3.4x improvement)
- ✅ Per-CF cache stats working
- ✅ Lock contention eliminated (verified with profiler)

### Nice to Have
- ✅ 3M+ ops/sec (4x improvement)
- ✅ Dynamic cache rebalancing
- ✅ Automated performance regression tests

---

## Timeline Estimate

**Week 1: Core Implementation**
- Days 1-2: Modify `ColumnFamilyState` and `PartitionedStorageBackend`
- Days 3-4: Implement per-CF cache budgeting
- Day 5: Unit tests

**Week 2: Integration & Testing**
- Days 1-2: Integration tests
- Days 3-4: Benchmark and profile
- Day 5: Bug fixes

**Week 3: Polish**
- Days 1-2: Documentation
- Days 3-4: Code review and refinements
- Day 5: Final benchmarks and merge

**Total:** 2-3 weeks for complete implementation

---

## References

- `src/column_family/state.rs` - `ColumnFamilyState::ensure_database()`
- `src/column_family/file_handle_pool.rs` - `FileHandlePool` implementation
- `src/column_family/partitioned_backend.rs` - `PartitionedStorageBackend`
- `src/tree_store/page_store/cached_file.rs` - `PagedCachedFile`
- `src/tree_store/page_store/page_manager.rs` - `TransactionalMemory`

---

**Last Updated:** 2025-01-28  
**Status:** Ready for Implementation