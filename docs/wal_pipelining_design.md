# WAL Pipelining Design for Manifold

**Status:** IN PROGRESS - Phase 1 Implementation Started  
**Date:** 2025-01-29  
**Last Updated:** 2025-01-29 (Beginning Phase 1)  
**Goal:** Improve write throughput by 2-4x through pipelined WAL writes and memtable inserts  
**Current Performance:** Manifold 945K ops/sec vs RocksDB 3.8M ops/sec (4x gap)

---

## Implementation Progress Log

### 2025-01-29: Phase 1 - CRITICAL INSIGHT DISCOVERED ⚠️

**Session Summary:**
- ✅ Completed RocksDB source code analysis
- ✅ Implemented AsyncWALJournal (506 lines) with background sync thread
- ❌ **DISCOVERED: AsyncWAL won't help current benchmark pattern**
- 🔄 **PIVOT TO PHASE 2 REQUIRED**

### Phase 1 Status: Core Complete but Won't Help This Workload ⚠️

**What We Built:**
- ✅ AsyncWALJournal struct (506 lines) - fully functional
- ✅ Background sync thread with automatic batching
- ✅ Non-blocking append() operation
- ✅ 5 passing unit tests

**Critical Discovery:**
AsyncWAL **will NOT improve benchmark performance** because:

1. **Benchmark pattern blocks on durability:**
   ```rust
   for batch in batches {
       txn.commit().unwrap();  // ← MUST wait for fsync to complete
   }
   ```

2. **AsyncWAL benefits async workloads:**
   - Helps when you fire-and-forget commits
   - Or when using futures/async-await
   - Not helpful when each commit must complete before next

3. **Benchmark results confirm this:**
   - Manifold: ~750K ops/sec (consistent across runs)
   - RocksDB: ~5M ops/sec (6.7x gap!)
   - AsyncWAL won't change this - threads still block on wait_for_sync()

**Why RocksDB is faster:**
- **Memtable architecture** - writes go to memory, not B-tree pages
- **True pipelining** - WAL sync happens while memtable writes in parallel
- **We need Phase 2** - in-memory write buffer layer

### Phase 1 Lessons Learned:
- AsyncWAL is correct for async workloads but wrong for this benchmark
- The bottleneck is **not fsync overhead** but **sequential commit pattern**
- Need architectural change: memtable-like layer (Phase 2/3)

**Files Created:**
- `src/column_family/wal/async_journal.rs` - 506 lines (working but not useful here)
- Helper method in `journal.rs` for recovery

### Phase 2 Status: Not Started
- Waiting for Phase 1 completion
- **Target:** 2.5-3.5x total improvement (2.4-3.3M ops/sec)

---

## Problem Statement

**Current Bottleneck:**
- Sequential commit pattern: Each transaction blocks on WAL fsync before completing
- No overlap between WAL write and B-tree modification
- Group commit helps but is limited by sequential transaction processing
- 400 commits in benchmark = up to 400 fsync operations (even with group commit)

**Current Flow:**
```
Transaction 1: Modify B-tree → Append WAL → wait_for_sync() [BLOCK] → non_durable_commit() → Done
Transaction 2:                                                         Modify B-tree → Append WAL → wait_for_sync() [BLOCK] → ...
```

**RocksDB Flow (Pipelined):**
```
Batch 1: Build changes → Write WAL → fsync WAL ━━━━━┓
Batch 2:                              Build changes → Write WAL → fsync WAL ━━━━━┓
Batch 3:                                                           Build changes → Write WAL → fsync WAL
                                                                   
Memtable Writer 1: ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━> Write to memtable (while batch 2 syncs WAL)
Memtable Writer 2:                                   ━━━━━━━━━━━━━━━━━━━━━━━━━━> Write to memtable (while batch 3 syncs WAL)
```

**Key Insight:** RocksDB separates WAL sync from memtable writes, allowing pipelining.

---

## Proposed Architecture

### Phase 1: Async WAL Sync Thread (Quick Win - 1 week)

**Concept:** Decouple WAL sync from transaction commit path

**New Flow:**
1. Transaction commits: Append to WAL buffer → Mark as "pending sync" → Return to caller
2. Background WAL sync thread: Wakes up periodically or on signal → fsyncs all pending → Marks sequences as synced
3. Transactions can optionally wait for durability (for immediate durability mode)

**Implementation:**

```rust
pub struct AsyncWALJournal {
    backend: Arc<dyn StorageBackend>,
    sequence_counter: Arc<AtomicU64>,
    
    // Pending sync queue
    pending_sync: Arc<Mutex<BTreeSet<u64>>>,  // Sequences waiting for fsync
    last_synced: Arc<AtomicU64>,
    
    // Background sync thread
    sync_signal: Arc<Condvar>,
    sync_thread: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl AsyncWALJournal {
    pub fn append(&self, entry: &mut WALEntry) -> io::Result<u64> {
        // Assign sequence number
        let seq = self.sequence_counter.fetch_add(1, Ordering::SeqCst) + 1;
        entry.sequence = seq;
        
        // Write to WAL (buffered, no fsync)
        let entry_data = entry.to_bytes();
        let offset = self.backend.len()?;
        self.backend.write(offset, &entry_data)?;
        
        // Add to pending sync queue
        {
            let mut pending = self.pending_sync.lock().unwrap();
            pending.insert(seq);
        }
        
        // Signal sync thread (non-blocking)
        self.sync_signal.notify_one();
        
        Ok(seq)
    }
    
    pub fn wait_for_sync(&self, sequence: u64) -> io::Result<()> {
        // Fast path: already synced
        if self.last_synced.load(Ordering::Acquire) >= sequence {
            return Ok(());
        }
        
        // Wait for background thread to sync this sequence
        loop {
            let last = self.last_synced.load(Ordering::Acquire);
            if last >= sequence {
                return Ok(());
            }
            
            // Park and wait for notification
            std::thread::park_timeout(Duration::from_micros(100));
        }
    }
    
    fn sync_thread_loop(&self) {
        while !self.shutdown.load(Ordering::Acquire) {
            // Wait for pending writes or timeout
            let pending_count = {
                let pending = self.pending_sync.lock().unwrap();
                pending.len()
            };
            
            if pending_count == 0 {
                // Sleep until signaled or timeout (1ms)
                std::thread::park_timeout(Duration::from_millis(1));
                continue;
            }
            
            // Batch sync all pending writes
            if let Err(e) = self.backend.sync_data() {
                eprintln!("WAL sync failed: {}", e);
                continue;
            }
            
            // Update last_synced and clear pending
            let synced_seq = {
                let mut pending = self.pending_sync.lock().unwrap();
                let max_seq = pending.iter().max().copied().unwrap_or(0);
                pending.clear();
                max_seq
            };
            
            self.last_synced.store(synced_seq, Ordering::Release);
            
            // Wake up all waiting threads
            // (They'll check last_synced in their park loops)
        }
    }
}
```

**Benefits:**
- Transactions don't block on fsync
- Background thread batches multiple fsyncs automatically
- Better CPU utilization (threads don't block waiting for I/O)

**Expected Improvement:** 1.5-2x (reduces fsync blocking overhead)

---

### Phase 2: Pipelined B-tree Writes (Medium - 2 weeks)

**Concept:** Overlap B-tree modification with WAL fsync (like RocksDB's pipelined writes)

**Challenges:**
- RocksDB writes to memtable (fast, in-memory)
- We write to B-tree pages (slower, involves allocations)
- Need to ensure consistency between WAL and B-tree

**Approach:**

1. **Separate commit stages:**
   ```rust
   pub enum CommitStage {
       BuildChanges,      // Construct B-tree modifications
       AppendWAL,         // Write to WAL (async)
       WaitForSync,       // Wait for WAL fsync
       ApplyChanges,      // Update B-tree in-memory state
   }
   ```

2. **Pipeline stages:**
   ```
   Txn 1: Build → Append WAL → [WAL Sync Thread syncing]
   Txn 2:                       Build → Append WAL → [WAL Sync Thread syncing]
   Txn 3:                                            Build → Append WAL
   
   [After WAL sync completes for Txn 1]
   Txn 1:                                            Apply Changes (non_durable_commit)
   ```

3. **Implementation:**
   ```rust
   pub fn commit_pipelined(&mut self) -> Result<(), CommitError> {
       // Stage 1: Build changes (already done in transaction)
       let (user_root, system_root) = self.flush_tables()?;
       
       // Stage 2: Append to WAL (async)
       let sequence = self.wal_journal.append(&entry)?;
       
       // Stage 3: Don't wait for sync - queue for background apply
       self.pending_apply.push(PendingCommit {
           sequence,
           user_root,
           system_root,
           transaction_id: self.transaction_id,
       });
       
       // Return immediately - commit will complete in background
       Ok(())
   }
   
   // Background thread applies commits after WAL sync
   fn apply_thread_loop(&self) {
       while !self.shutdown.load(Ordering::Acquire) {
           let last_synced = self.wal_journal.last_synced();
           
           // Find all pending commits with sequence <= last_synced
           let ready_commits = self.pending_apply.drain_ready(last_synced);
           
           for commit in ready_commits {
               // Apply to in-memory B-tree state
               self.mem.non_durable_commit(
                   commit.user_root,
                   commit.system_root,
                   commit.transaction_id,
               )?;
           }
           
           std::thread::sleep(Duration::from_micros(100));
       }
   }
   ```

**Benefits:**
- B-tree updates happen in parallel with WAL fsyncs
- Multiple transactions can be in-flight simultaneously
- Better CPU and I/O utilization

**Expected Improvement:** Additional 1.5-2x on top of Phase 1 (total 2-4x)

**Risks:**
- More complex error handling (what if apply fails after WAL synced?)
- Need careful synchronization for reads (must see pending commits)
- Recovery needs to handle partially-applied commits

---

### Phase 3: Write-Optimized B-tree Layer (Advanced - 3-4 weeks)

**Concept:** Add a write buffer layer above B-tree (similar to memtable)

**Architecture:**
```
Writes → WriteBuffer (in-memory HashMap/SkipList) → Background flush → B-tree
Reads  → Check WriteBuffer first → Then B-tree
WAL    → Protects WriteBuffer
```

**Benefits:**
- Writes become pure memory operations (very fast)
- Background thread batches B-tree updates
- Natural fit with LSM-like architecture

**Implementation Sketch:**
```rust
pub struct WriteBufferLayer {
    // In-memory write buffer (newest writes)
    active_buffer: Arc<RwLock<SkipMap<Vec<u8>, Vec<u8>>>>,
    
    // Immutable buffer being flushed to B-tree
    flushing_buffer: Arc<RwLock<Option<SkipMap<Vec<u8>, Vec<u8>>>>>,
    
    // Background flush thread
    flush_thread: Option<JoinHandle<()>>,
    
    // Underlying B-tree
    btree: Arc<Database>,
}

impl WriteBufferLayer {
    pub fn insert(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let buffer = self.active_buffer.read().unwrap();
        buffer.insert(key.to_vec(), value.to_vec());
        
        // Check if buffer is full
        if buffer.len() > FLUSH_THRESHOLD {
            self.trigger_flush();
        }
        
        Ok(())
    }
    
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Check active buffer
        {
            let buffer = self.active_buffer.read().unwrap();
            if let Some(value) = buffer.get(key) {
                return Ok(Some(value.clone()));
            }
        }
        
        // Check flushing buffer
        {
            let flushing = self.flushing_buffer.read().unwrap();
            if let Some(buffer) = flushing.as_ref() {
                if let Some(value) = buffer.get(key) {
                    return Ok(Some(value.clone()));
                }
            }
        }
        
        // Check B-tree
        self.btree.get(key)
    }
    
    fn trigger_flush(&self) {
        // Swap active and flushing buffers
        let old_active = {
            let mut active = self.active_buffer.write().unwrap();
            let new_buffer = SkipMap::new();
            std::mem::replace(&mut *active, new_buffer)
        };
        
        {
            let mut flushing = self.flushing_buffer.write().unwrap();
            *flushing = Some(old_active);
        }
        
        // Signal flush thread
    }
    
    fn flush_thread_loop(&self) {
        while !self.shutdown.load(Ordering::Acquire) {
            // Wait for flushing buffer to be populated
            let buffer = {
                let mut flushing = self.flushing_buffer.write().unwrap();
                flushing.take()
            };
            
            if let Some(buffer) = buffer {
                // Flush to B-tree
                let txn = self.btree.begin_write()?;
                {
                    let mut table = txn.open_table(TABLE)?;
                    for (key, value) in buffer.iter() {
                        table.insert(key.as_slice(), value.as_slice())?;
                    }
                }
                txn.commit()?;
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
```

**Benefits:**
- Drastically faster writes (memory-only)
- Natural batching of B-tree updates
- Better write amplification

**Expected Improvement:** Could reach 3-4M ops/sec (match RocksDB)

**Challenges:**
- Significant architectural change
- Memory overhead (need to keep write buffer in memory)
- More complex recovery (need to replay WAL into write buffer)
- Read path becomes more complex (check multiple layers)

---

## Implementation Roadmap

### Week 1-2: Phase 1 (Async WAL Sync) - IN PROGRESS
- ✅ **Day 1-2:** Design and implement `AsyncWALJournal`
  - Status: COMPLETE - 506 lines, background sync thread working
  - Files created: `src/column_family/wal/async_journal.rs`
  - Features: Non-blocking append, automatic batching, efficient wait
- 🚧 **Day 3-4:** Integrate with transaction commit path
  - Status: IN PROGRESS - Adding helper methods
  - Next: Wire up AsyncWALJournal in ColumnFamilyDatabase
- ⏸️ **Day 5:** Unit tests for async sync
  - Note: Basic unit tests already written (5 tests in async_journal.rs)
  - Need: Integration tests with full transaction flow
- ⏸️ **Day 6-7:** Integration tests and benchmarking

**Success Criteria:**
- No regressions in correctness
- 1.5-2x throughput improvement
- Latency p99 remains acceptable

### Week 3-4: Phase 2 (Pipelined Commits) - NOT STARTED
- ⏸️ **Day 1-3:** Implement commit staging and pending apply queue
- ⏸️ **Day 4-5:** Background apply thread
- ⏸️ **Day 6-7:** Error handling and recovery
- ⏸️ **Day 8-10:** Testing and benchmarking

**Success Criteria:**
- 2.5-3.5x total throughput improvement
- All ACID guarantees maintained
- Clean error handling

### Week 5-8: Phase 3 (Write Buffer Layer) - DEFERRED
- Only if Phase 1+2 don't reach target performance
- Can be deferred to future release

**Legend:**
- ✅ Complete
- 🚧 In Progress
- ⏸️ Not Started

---

## Alternative Approaches Considered

### 1. Increase GROUP_COMMIT_WINDOW (REJECTED)
**Tried:** Spinning for 50-250μs to batch more writes  
**Result:** Actually decreased performance (threads already waiting)  
**Reason:** Transactions commit sequentially, not concurrently overlapping

### 2. Remove write_barrier() (REJECTED)
**Tried:** Skip flushing dirty B-tree pages on every commit  
**Result:** No significant improvement (~3% gain)  
**Reason:** Write coalescing already optimized this path

### 3. Lock-Free WAL Queue (DEFERRED)
**Concept:** Replace mutex with lock-free queue for WAL appends  
**Decision:** Do async sync first, then evaluate if still needed  
**Reason:** Async sync should reduce lock contention sufficiently

### 4. Full LSM-tree Conversion (REJECTED)
**Concept:** Replace B-tree with LSM-tree  
**Decision:** Too large a change, loses B-tree benefits  
**Reason:** B-tree provides better read performance and simpler implementation

---

## Risks and Mitigations

### Risk 1: Async Commits Break Semantics
**Risk:** Transactions return before durability guaranteed  
**Mitigation:**
- Provide durability mode option (immediate vs async)
- Default to immediate (wait for sync) for backward compatibility
- Document async mode clearly

### Risk 2: Increased Memory Usage
**Risk:** Pending commits queue up in memory  
**Mitigation:**
- Bound pending queue size
- Apply backpressure if queue too large
- Monitor memory usage

### Risk 3: Complex Error Handling
**Risk:** Failures after WAL sync but before apply  
**Mitigation:**
- WAL is source of truth - recovery replays WAL
- If apply fails, mark transaction as failed and retry on recovery
- Add extensive error handling tests

### Risk 4: Read Consistency
**Risk:** Reads don't see pending but synced commits  
**Mitigation:**
- Option 1: Reads wait for pending applies before their sequence
- Option 2: Reads check pending queue for recent writes
- Option 3: Only allow reads after all pending applies complete

---

## Performance Targets

**Current:** 945K ops/sec

**Phase 1 (Async WAL):**
- Conservative: 1.4M ops/sec (1.5x)
- Optimistic: 1.9M ops/sec (2x)

**Phase 2 (Pipelined Commits):**
- Conservative: 2.4M ops/sec (2.5x total)
- Optimistic: 3.3M ops/sec (3.5x total)

**Phase 3 (Write Buffer):**
- Conservative: 3.3M ops/sec (3.5x total)
- Optimistic: 4.2M ops/sec (4.5x total, exceeds RocksDB!)

---

## Comparison: Manifold vs RocksDB After Optimization

**RocksDB Advantages:**
- Mature C++ implementation with SIMD
- LSM-tree architecture optimized for writes
- 10+ years of production tuning

**Manifold Advantages (After This Work):**
- Pure Rust (memory safety)
- Pipelined writes (matching RocksDB)
- B-tree provides better read performance
- Simpler architecture (easier to maintain)

**Expected Performance After Phase 2:**
- Manifold: ~3M ops/sec
- RocksDB: ~3.8M ops/sec
- Gap: **1.3x** (down from 4x!)

This would make Manifold **highly competitive** with RocksDB while maintaining Rust safety and B-tree read advantages.

---

## References

**RocksDB Implementation:**
- `db/db_impl/db_impl_write.cc` - PipelinedWriteImpl
- `db/write_thread.cc` - WriteThread group commit
- `db/memtable/` - Memtable implementations

**Manifold Current Implementation:**
- `src/column_family/wal/journal.rs` - Current WAL implementation
- `src/transactions.rs` - Transaction commit path
- `src/tree_store/page_store/page_manager.rs` - non_durable_commit

---

**Last Updated:** 2025-01-29 17:15 PST  
**Status:** Phase 1 Core Implementation COMPLETE - Ready for Integration  
**Next Action:** [NEXT SESSION] Wire AsyncWALJournal into ColumnFamilyDatabase and transaction commit path  
**Current Task:** Core implementation complete - 506 lines written, 5 tests passing  
**Lines Written:** 506 lines (async_journal.rs) + helper method in journal.rs

---

## Session Notes

### Session 1 (2025-01-29)
- Analyzed current performance gap (4x slower than RocksDB)
- Reviewed RocksDB source code for pipelined write implementation
- Discovered key optimizations: async WAL sync + pipelined memtable writes
- Created comprehensive 3-phase implementation plan
- **✅ COMPLETED AsyncWALJournal Implementation (506 lines)**
  - Background sync thread with automatic batching (100μs poll, 1ms max delay)
  - Non-blocking append() operation - transactions don't block on fsync
  - Efficient wait_for_sync() with spin-wait (10μs intervals)
  - 5 unit tests covering basic functionality - all passing
  - Helper method added to sync WALJournal for recovery
  - Module properly exported in wal/mod.rs
- **Status at end of session:** Phase 1 core COMPLETE ✅
- **Next session:** Integration into transaction path + benchmarking
- Context approaching limit - this document tracks all progress for continuation