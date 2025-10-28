# Manifold Performance Optimization Plan

**Status:** Investigation Complete - Architectural Changes Required  
**Date:** 2025-01-28  
**Current Performance Gap:** Manifold 746K ops/sec vs RocksDB 4.3M ops/sec (5.8x slower)

**Update:** GROUP_COMMIT_WINDOW testing showed batching window is architecturally flawed for our use case. Focus on real architectural fixes.

---

## Critical Issues Found

### 1. ❌ **GROUP_COMMIT_WINDOW Testing Results: NO BENEFIT**

**Priority:** N/A - Tested and Rejected  
**Location:** `src/column_family/wal/journal.rs:27`  
**Status:** TESTED - Does not improve performance

**Tests Performed:**
- `GROUP_COMMIT_WINDOW_MICROS = 0` → 746K ops/sec (baseline)
- `GROUP_COMMIT_WINDOW_MICROS = 50` → 734K ops/sec (slightly worse)
- `GROUP_COMMIT_WINDOW_MICROS = 250` → 605K ops/sec (significantly worse)

**Why It Doesn't Work:**

The batching window is architecturally flawed for our workload:

1. **Threads append concurrently** (fast, no blocking)
2. **All threads call `wait_for_sync()` nearly simultaneously**
3. **First thread becomes leader, others become followers** (blocked on condvar)
4. **Leader spins for batching window** → followers are ALREADY waiting
5. **Window adds pure latency with no benefit**

The fundamental issue: batching window spins **after** followers are already in the wait queue. It doesn't collect additional transactions because they arrived before the spin started.

**Actual Commit Pattern:**
- Benchmark has threads doing **sequential commits** in loops
- Not truly concurrent overlapping commits
- Most commits have only 1-2 threads actually committing simultaneously
- Group commit already works via `wait_for_sync()` without needing a spin window

**Conclusion:** Keep `GROUP_COMMIT_WINDOW_MICROS = 0`. The current leader-follower mechanism provides natural batching without artificial delays.

---

### 2. ⚠️ **CRITICAL: write_barrier() Disk I/O Overhead**

**Priority:** P0 - Highest Impact (Real Bottleneck)  
**Location:** `src/tree_store/page_store/page_manager.rs:740`

**Problem:**
- `non_durable_commit()` calls `write_barrier()` which flushes **all dirty pages** to disk
- For 1000-item batches: ~55-60 dirty B-tree pages × 4KB = **~240KB written per commit**
- Total: 400 commits × 240KB = **~96MB unnecessary disk I/O**
- WAL already provides durability, so flushing pages to disk is redundant

**Impact:** HIGH - This is a primary bottleneck causing unnecessary disk I/O

**Root Cause Analysis:**
- WAL already provides durability (fsync'd to disk)
- Writing dirty pages to disk is redundant when WAL is enabled
- BUT: write buffer has limited capacity
- Removing `write_barrier()` causes buffer overflow → expensive evictions

**Previous Attempt:**
- Tried removing `write_barrier()` entirely → **performance got worse** (238K ops/sec)
- Write buffer filled up and triggered evictions during page writes
- Evictions are even more expensive than periodic flushing

**Best Solution - Multi-Part Fix:**

1. **Significantly increase write buffer capacity**
   - Current: Small (causes evictions)
   - Target: 512MB - 1GB to hold all dirty pages from concurrent transactions
   - Allows keeping pages in memory until checkpoint

2. **Make `write_barrier()` conditional on WAL**
   - When WAL enabled: Keep data in write buffer (no disk flush)
   - When WAL disabled: Flush to disk (existing behavior)
   - Requires passing WAL state to `TransactionalMemory`

3. **Implement smart eviction policy**
   - When buffer hits 80% capacity: flush oldest pages
   - When checkpoint runs: flush all dirty pages
   - Prevents buffer overflow while minimizing disk I/O

**Implementation Plan:**
1. Add `wal_enabled: bool` flag to `TransactionalMemory::new()`
2. Pass flag from `WriteTransaction` (knows WAL state)
3. In `non_durable_commit()`: skip `write_barrier()` if `wal_enabled`
4. Increase `max_write_buffer_bytes` from current to 512MB
5. Add buffer pressure monitoring and smart eviction

---

### 3. ⚠️ **CRITICAL: Shared write_buffer Lock Contention**

**Priority:** P0 - Highest Impact (Architectural Bottleneck)  
**Location:** `src/tree_store/page_store/cached_file.rs:523`

**Problem:**
- Single `write_buffer: Mutex<LRUWriteCache>` is shared across all column families
- All 8 concurrent threads contend for the same lock on every page write
- Lock is held during read cache checks, insertions, and evictions
- Serializes independent column family writes

**Impact:** CRITICAL - This is the #1 source of lock contention serializing all column families

**Current Architecture Problem:**
- All 8 column families share ONE `write_buffer` lock in `PagedCachedFile`
- Every page write from any CF must acquire this single lock
- Lock is held during: read cache check, buffer insertion, eviction checks
- Completely serializes independent concurrent writes

**Best Solution: Per-Column-Family Storage Backends**

Instead of half-measures (sharding), do it right:

1. **Each `ColumnFamily` gets its own `PagedCachedFile` instance**
   - Currently: All CFs share one backend via `PartitionedStorageBackend`
   - Target: Each CF has independent `PagedCachedFile` with own write_buffer
   - Already partially there: `FileHandlePool` gives separate file descriptors

2. **Architecture Change:**
   ```
   Current:  CF1, CF2, CF3 → PartitionedBackend → PagedCachedFile (SHARED lock)
   
   Target:   CF1 → PagedCachedFile1 (independent lock)
             CF2 → PagedCachedFile2 (independent lock)  
             CF3 → PagedCachedFile3 (independent lock)
   ```

3. **Implementation Steps:**
   - Create `PagedCachedFile` in `ColumnFamilyState::ensure_database()`
   - Pass to `Database::create_with_backend()` instead of raw backend
   - Each CF gets isolated write buffer, read cache, and metrics
   - Zero lock contention between CFs

**Complexity:** Medium - clean architectural separation, no hacks needed

**Why Not Shard?**
- Sharding (16-way) still has lock contention, just reduced
- Per-CF is the actual correct architecture for column families
- Similar implementation complexity but cleaner result

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

### Phase 1: Foundation (1-2 weeks)
- [x] Document performance issues (this file)
- [x] Test GROUP_COMMIT_WINDOW → Confirmed no benefit
- [ ] Profile with perf/instruments to confirm bottlenecks
- [ ] Design per-CF storage architecture

### Phase 2: Per-CF Storage Backends (2-3 weeks) ← **PRIMARY EFFORT**
This is the core architectural fix that addresses the real bottleneck:

- [ ] **Design:** Each CF gets its own `PagedCachedFile` instance
- [ ] Modify `ColumnFamilyState` to create `PagedCachedFile` instead of raw backend
- [ ] Update `Database::create_with_backend()` to accept `PagedCachedFile`
- [ ] Test: Verify each CF has isolated write_buffer lock
- [ ] Benchmark: Expect 2-4x improvement from eliminating lock contention

### Phase 3: WAL-Aware write_barrier() (1 week)
- [ ] Add `wal_enabled` flag to `TransactionalMemory`
- [ ] Thread flag from `WriteTransaction` → `Database` → `TransactionalMemory`
- [ ] Skip `write_barrier()` in `non_durable_commit()` when WAL enabled
- [ ] Increase `max_write_buffer_bytes` to 512MB-1GB
- [ ] Implement buffer pressure monitoring (flush at 80% capacity)
- [ ] Test: Verify no evictions during normal operation
- [ ] Benchmark: Expect 1.5-2x improvement from reduced disk I/O

### Phase 4: Polish & Optimization (1 week)
- [ ] File pre-allocation to avoid set_len() serialization
- [ ] Optimize checkpoint registration (atomic counter vs lock)
- [ ] Re-run full benchmark suite
- [ ] Profile again to find any remaining bottlenecks

### Expected Results After All Phases:
- **Conservative:** 2M-3M ops/sec (2.7-4x improvement)
- **Optimistic:** 3M-4M ops/sec (4-5.4x improvement, close to RocksDB)

### Phase 5: Advanced (If Needed)
- [ ] SIMD for checksum calculations
- [ ] Lock-free structures (only if profiling shows benefit)
- [ ] Custom allocator for page buffers

---

## Success Criteria

**Minimum Acceptable Performance (MVP):**
- Manifold: 2M ops/sec (2.7x current, exceeds Fjall)
- Within 2x of RocksDB (respectable for Rust vs C++)

**Target Goal (Realistic):**
- Manifold: 3M ops/sec (4x current)
- Within 1.5x of RocksDB (excellent performance)

**Stretch Goal (Optimistic):**
- Manifold: 4M+ ops/sec (5.4x current)
- Match or exceed RocksDB (world-class performance)

**Non-Negotiable:**
- No regressions in correctness or crash recovery
- All tests must pass
- ACID guarantees maintained
- Memory safety preserved

---

## Notes & Observations

1. **GROUP_COMMIT_WINDOW Testing Confirmed Architecture Issue**
   - Tested 0μs, 50μs, 250μs → No improvement, actually worse
   - Batching window spins AFTER followers already waiting
   - Current leader-follower mechanism already provides natural batching
   - Confirmed: Not a quick fix, focus on real bottlenecks

2. **write_barrier() is a real bottleneck but requires careful fix**
   - Can't just remove it (tried, caused evictions, worse performance)
   - Solution: Increase buffer capacity + make conditional on WAL
   - Must implement buffer pressure monitoring
   - Best done AFTER per-CF backends (independent buffers)

3. **Shared write_buffer lock is THE architectural bottleneck**
   - Single lock serializing 8 concurrent CFs is unacceptable
   - Per-CF backends is the correct architecture, not a workaround
   - This should be the PRIMARY focus of optimization effort
   - Expected to provide 2-4x improvement alone

4. **Don't half-measure the solution**
   - Sharding is a band-aid, per-CF is the right design
   - Do it correctly once rather than incrementally
   - Cleaner code, better performance, easier to maintain

5. **Performance targets are achievable**
   - Current: 746K ops/sec
   - Per-CF backends: ~2M-3M ops/sec (eliminate lock contention)
   - WAL-aware write_barrier: ~3M-4M ops/sec (reduce disk I/O)
   - Reaching RocksDB performance (4.3M) is realistic

6. **Profile to validate assumptions**
   - Use real profilers (perf/instruments) to confirm bottlenecks
   - Measure lock wait times, fsync counts, disk I/O
   - Validate that fixes actually help before merging

---

## References

- CF Comparison Benchmark: `crates/manifold-bench/benches/cf_comparison_benchmark.rs`
- WAL Journal: `src/column_family/wal/journal.rs`
- Page Manager: `src/tree_store/page_store/page_manager.rs`
- Cached File: `src/tree_store/page_store/cached_file.rs`
- Transaction Commit: `src/transactions.rs:1413-1570`

---

**Last Updated:** 2025-01-28  
**Next Action:** Design and implement per-CF `PagedCachedFile` architecture (highest impact fix)