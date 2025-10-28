# Manifold Performance Optimization Plan

**Status:** Investigation Complete - Ready for Implementation  
**Date:** 2025-01-28  
**Current Performance Gap:** Manifold 746K ops/sec vs RocksDB 4.3M ops/sec (5.8x slower)

---

## Critical Issues Found

### 1. ⚠️ **CRITICAL: GROUP_COMMIT_WINDOW Disabled**

**Priority:** P0 - Immediate Fix  
**Location:** `src/column_family/wal/journal.rs:27`  
**Current State:**
```rust
const GROUP_COMMIT_WINDOW_MICROS: u64 = 0;
```

**Problem:**
- WAL group commit batching is completely disabled
- Every transaction immediately becomes leader and calls `fsync()` individually
- With 8 threads × 50 batches = **400 separate fsync operations**
- At ~0.5ms per fsync = **~200ms wasted on unnecessary synchronous I/O**

**Expected Impact:** 2-3x throughput improvement (conservative estimate)

**Fix:**
```rust
const GROUP_COMMIT_WINDOW_MICROS: u64 = 250;  // Start with 250μs, tune between 100-500
```

**Rationale:**
- 100μs = lowest latency, minimal batching
- 250μs = balanced latency/throughput (recommended starting point)
- 500μs = maximum batching, higher latency

**Testing:**
- Run `cf_comparison_benchmark` before and after
- Measure fsync count reduction with `dtrace` or `strace`
- Profile latency distribution (p50, p95, p99)

---

### 2. **write_barrier() Disk I/O Overhead**

**Priority:** P1 - High Impact  
**Location:** `src/tree_store/page_store/page_manager.rs:740`

**Problem:**
- `non_durable_commit()` calls `write_barrier()` which flushes **all dirty pages** to disk
- For 1000-item batches: ~55-60 dirty B-tree pages × 4KB = **~240KB written per commit**
- Total: 400 commits × 240KB = **~96MB unnecessary disk I/O**
- WAL already provides durability, so flushing pages to disk is redundant

**Impact:** Moderate (disk I/O is somewhat buffered by OS page cache)

**Attempted Fix:**
- Tried removing `write_barrier()` entirely → **performance got worse** (238K ops/sec)
- Write buffer filled up, triggered expensive evictions

**Recommended Approach:**
1. **Option A:** Increase `max_write_buffer_bytes` to hold more dirty pages in memory
2. **Option B:** Implement lazy/throttled flushing (flush only when buffer hits 80% capacity)
3. **Option C:** Make `write_barrier()` a no-op when WAL enabled, but increase buffer size significantly

**Testing Required:**
- Profile write buffer capacity usage during benchmark
- Measure eviction frequency
- Test with larger `max_write_buffer_bytes` (2x, 4x, 8x current size)

---

### 3. **Shared write_buffer Lock Contention**

**Priority:** P1 - High Impact  
**Location:** `src/tree_store/page_store/cached_file.rs:523`

**Problem:**
- Single `write_buffer: Mutex<LRUWriteCache>` is shared across all column families
- All 8 concurrent threads contend for the same lock on every page write
- Lock is held during read cache checks, insertions, and evictions
- Serializes independent column family writes

**Impact:** High - This is likely the #1 source of lock contention

**Solution:** Per-Column-Family Write Buffers

**Implementation Plan:**
1. Move `write_buffer` from `PagedCachedFile` to per-CF context
2. Each `ColumnFamily` gets its own `PagedCachedFile` instance (already done via pool)
3. Requires restructuring how `TransactionalMemory` interacts with storage layer

**Complexity:** High - requires architectural changes

**Alternative (Simpler):**
- Shard `write_buffer` into N independent caches (e.g., 16 shards)
- Route pages to shards based on `offset % N`
- Reduces contention by 16x with minimal code changes

---

### 4. **File Handle Kernel Serialization**

**Priority:** P2 - Platform-Specific  
**Location:** Multiple file descriptors to same inode

**Problem:**
- `FileHandlePool` creates separate file descriptors per CF (correct for avoiding userspace locks)
- However, kernel may still serialize operations on the same inode (especially macOS)
- Metadata operations (e.g., `set_len()`) are serialized via `file_growth_lock`

**Impact:** Platform-dependent (higher on macOS, lower on Linux)

**Current Mitigation:**
- `file_growth_lock` already serializes `set_len()` operations
- Prevents race conditions but doesn't eliminate kernel serialization

**Future Optimization:**
- Pre-allocate file space to avoid `set_len()` during writes
- Use `fallocate()` on Linux, `fcntl(F_PREALLOCATE)` on macOS

---

### 5. **Checkpoint Registration Overhead**

**Priority:** P3 - Low Impact  
**Location:** `src/column_family/wal/checkpoint.rs`

**Problem:**
- Every commit acquires lock to register sequence number with checkpoint manager
- 400 commits = 400 lock acquisitions
- Lock is probably not highly contended, but adds latency

**Impact:** Low (probably <5% overhead)

**Solution:**
- Batch checkpoint registrations (e.g., register every 10 sequences)
- Use atomic counter instead of lock for pending sequences
- Or eliminate registration entirely if checkpoint can scan WAL

---

## Comparison: Manifold vs RocksDB Architecture

**Why RocksDB is faster:**
1. **C++ with SIMD** - Hand-optimized memory operations, zero-cost abstractions
2. **Lock-free data structures** - Atomic operations, thread-local buffers, no shared locks
3. **Thread-local mem-tables** - Each thread writes to its own mem-table, merged during flush
4. **LSM-tree design** - Sequential writes to immutable SSTables (better for concurrent writes than B-trees)
5. **Mature optimization** - 10+ years of profiling and tuning by Facebook/Meta

**Manifold's Advantages:**
- Pure Rust (memory safety, no segfaults)
- Column family isolation (true concurrent writes, not just batching)
- Simpler architecture (easier to understand and maintain)
- 12x faster than LMDB (which lacks CF support)
- Competitive with Fjall (1.5x slower, both Rust implementations)

---

## Testing & Validation Strategy

### Before Making Changes
1. Run baseline benchmarks:
   ```bash
   cargo bench --package manifold-bench --bench cf_comparison_benchmark
   cargo bench --package manifold-bench --bench lmdb_benchmark
   ```

2. Capture metrics:
   - Throughput (ops/sec)
   - Latency distribution (p50, p95, p99)
   - System metrics (CPU, disk I/O, syscalls)

### For Each Optimization
1. **Profile before and after:**
   ```bash
   # macOS
   cargo instruments -t time --bench cf_comparison_benchmark
   
   # Linux
   perf record -g cargo bench --bench cf_comparison_benchmark
   perf report
   ```

2. **Measure fsync count:**
   ```bash
   # macOS
   sudo dtrace -n 'syscall::fsync:entry { @[execname] = count(); }'
   
   # Linux
   strace -c -f cargo bench --bench cf_comparison_benchmark 2>&1 | grep fsync
   ```

3. **Run full test suite:**
   ```bash
   cargo test --release
   cargo test --release --package manifold-bench
   ```

4. **Verify correctness:**
   - Crash recovery tests
   - Concurrent write isolation tests
   - WAL replay verification

---

## Implementation Roadmap

### Phase 1: Quick Wins (Days)
- [x] Document performance issues (this file)
- [ ] **Enable GROUP_COMMIT_WINDOW (250μs)** ← START HERE
- [ ] Benchmark and measure impact
- [ ] Tune window size based on results (100-500μs)

### Phase 2: Medium Effort (Weeks)
- [ ] Shard `write_buffer` lock (16-way sharding)
- [ ] Increase `max_write_buffer_bytes` 4x-8x
- [ ] Profile with actual profiler (perf/instruments)
- [ ] Identify remaining hot locks

### Phase 3: Architectural Changes (Months)
- [ ] Per-CF write buffers
- [ ] Lazy/throttled page flushing
- [ ] File pre-allocation to avoid set_len()
- [ ] Lock-free checkpoint registration

### Phase 4: Advanced Optimizations (Future)
- [ ] SIMD for checksum calculations
- [ ] Lock-free page cache (if profiling shows it's worth it)
- [ ] Custom allocator for page buffers
- [ ] Consider LSM-tree hybrid for append-heavy workloads

---

## Success Criteria

**Minimum Acceptable Performance (MVP):**
- Manifold: 1.5M - 2M ops/sec (2-3x current, competitive with Fjall)
- Within 2-3x of RocksDB (acceptable for Rust vs C++)

**Stretch Goal:**
- Manifold: 3M+ ops/sec (4x current)
- Close to RocksDB performance (within 1.5x)

**Non-Negotiable:**
- No regressions in correctness or crash recovery
- All tests must pass
- ACID guarantees maintained
- Memory safety preserved

---

## Notes & Observations

1. **GROUP_COMMIT_WINDOW = 0 is almost certainly a leftover from debugging/testing**
   - Comment suggests 100-500μs, but it's disabled
   - This is the lowest-hanging fruit for massive gains

2. **write_barrier() overhead is real but tricky to fix**
   - Can't just remove it (tried, made it worse)
   - Need to balance memory usage vs I/O overhead
   - Requires careful tuning

3. **Lock contention is architectural**
   - Shared write_buffer is a fundamental bottleneck
   - Sharding is a good intermediate step
   - Per-CF buffers is the ultimate solution

4. **Don't compare apples to oranges**
   - RocksDB is C++ with 10+ years of optimization
   - Manifold being 2-3x slower is actually respectable
   - Focus on being best-in-class for Rust embedded DBs

5. **Profile before optimizing**
   - Intuition can be wrong (write_barrier removal proved this)
   - Measure actual lock contention before changing architecture
   - Use real profilers, not guesswork

---

## References

- CF Comparison Benchmark: `crates/manifold-bench/benches/cf_comparison_benchmark.rs`
- WAL Journal: `src/column_family/wal/journal.rs`
- Page Manager: `src/tree_store/page_store/page_manager.rs`
- Cached File: `src/tree_store/page_store/cached_file.rs`
- Transaction Commit: `src/transactions.rs:1413-1570`

---

**Last Updated:** 2025-01-28  
**Next Action:** Enable GROUP_COMMIT_WINDOW and measure impact