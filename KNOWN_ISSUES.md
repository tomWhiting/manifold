# Known Issues

## Performance Scaling Bottleneck (4+ Concurrent Threads)

**Status:** Under Investigation

### Symptoms
When using 4 or more concurrent threads writing to different column families:
- 1 thread: ~80-120K ops/sec ✅
- 2 threads: ~170-190K ops/sec ✅ (good scaling)
- 4 threads: ~40-70K ops/sec ❌ (should be ~300K+)
- 8 threads: ~40-50K ops/sec ❌ (should be ~600K+)

Performance degrades significantly beyond 2 threads instead of scaling linearly.

### Expected Behavior
With 8 separate redb database files, 8 threads achieve ~118K ops/sec total.
Our column families should achieve similar (each CF has its own Database instance + file descriptor).

### Investigation So Far

**Fixed:**
- ✅ File handle pool lock contention - moved file I/O outside critical section
- ✅ Removed fsync from segment allocation path
- ✅ Minimized header lock hold time during segment allocation

**Identified Issues:**
- WAL leader-based group commit has a deadlock or livelock at 4+ threads
- Tests hang or run very slowly with 4+ concurrent writers
- Issue occurs even WITHOUT WAL (pool_size=0), indicating column family layer problem

**Potential Remaining Bottlenecks:**
1. **Unknown shared lock** - Some mutex/rwlock being held longer than expected
2. **Segment allocation** - Despite fixes, may still have contention
3. **WAL synchronization** - Condvar wait logic may have race conditions
4. **I/O bandwidth** - Single file with many threads may hit OS-level contention

### Workaround
Use 1-2 threads per column family for optimal performance.
Or use multiple separate Database instances instead of column families.

### Test Cases
Run these to reproduce:
```bash
cargo run --example truly_no_wal --release     # Without WAL
cargo run --example test_with_pooling --release # With WAL
```

Both show the issue at 4+ threads.

---

## WAL Group Commit Implementation

**Status:** Partially Complete

### What Works
- ✅ WAL append and recovery
- ✅ Checkpoint system
- ✅ Basic group commit with 1-2 threads
- ✅ Crash recovery

### What's Broken
- ❌ Leader-based group commit deadlocks at 4+ threads
- ❌ Tests hang when testing concurrent writes

### Current State
- Reverted to simpler condvar-based wait
- No batching window (set to 0 to avoid delays)
- Still has race condition causing hangs

### Next Steps
1. Fix the deadlock in wait_for_sync() condvar logic
2. Add proper timeout/recovery from deadlocks
3. OR: Revert to background thread approach (slower but stable)

---

## Segment Allocation Header Persistence

**Status:** Disabled (Trade-off for Performance)

### Current Behavior
Segment allocations update the in-memory header but do NOT persist to disk immediately.

### Trade-offs
- **Pro:** Eliminates serialization bottleneck during expansion
- **Con:** Crash may lose segment allocation metadata (wasted space in file)
- **Impact:** Data is NOT lost (it's in the segments themselves), just some disk space may be marked as used but not tracked

### TODO
- Implement periodic header flush (every 5 seconds?)
- Ensure header is flushed on clean shutdown
- Consider making this configurable

---

## File Handle Pool Performance

**Status:** Fixed (mostly)

### What Was Fixed
- File opening now happens outside the pool lock
- Touch operations minimize lock hold time

### Remaining Concerns
- With pool_size=0, every transaction opens a new FD (very slow)
- Need to ensure users understand pool_size should be >= number of active CFs

---

## Documentation Gaps

1. Need to document optimal pool_size settings
2. Need to document performance characteristics
3. Need to explain when to use column families vs separate databases
4. Need to document WAL behavior and guarantees

