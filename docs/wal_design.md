# Write-Ahead Log (WAL) Design Document

## Executive Summary

This document specifies the design and implementation of a Write-Ahead Log (WAL) system for the column family database. The WAL provides **fast + durable** writes by default, eliminating the need for users to manually manage `Durability::None` patterns.

**Performance Goal:** 10-15K ops/sec per thread for durable writes (200-250x improvement over current default durability)

**Key Benefit:** Append-only journal is fsynced quickly (~0.5ms) vs full B-tree fsync (~5ms), while maintaining full crash recovery and ACID guarantees.

---

## Design Decisions

### 1. Shared WAL Architecture (Phase 1)

**Decision:** Implement a single shared WAL file for all column families.

**Rationale:**
- Simpler implementation (single journal file, single checkpoint thread)
- WAL append is fast (mostly memory writes + single fsync)
- Single recovery pass on database open
- Can migrate to per-CF WAL later if benchmarks show bottleneck

**Trade-off:** Potential write serialization through single WAL file, but this is mitigated by batch writes and fast fsync.

**Future Migration Path:** If shared WAL becomes a bottleneck (>8 concurrent CFs with high write load), implement per-CF WAL journals with coordinated checkpoint.

### 2. Integration Point: WriteTransaction Layer

**Decision:** Hook WAL into `WriteTransaction::commit()` path, specifically between B-tree flush and `TransactionalMemory::commit()`.

**Rationale:**
- Clean separation of concerns (WAL is a transaction concern, not a storage concern)
- Access to serialized transaction data (table roots, freed pages, allocated pages)
- Minimal invasiveness to redb core
- Clear rollback semantics

**Integration Flow:**
```
WriteTransaction::commit()
  ├─> TableTree::flush_and_close()       [existing - produces user_root]
  ├─> store_data_freed_pages()            [existing - produces freed pages list]
  ├─> store_allocated_pages()             [existing - produces allocated pages list]
  ├─> WAL::append_transaction()           [NEW - append to WAL journal]
  ├─> fsync(WAL)                          [NEW - fast append-only fsync]
  ├─> TransactionalMemory::commit()       [existing - in-memory commit]
  └─> register_pending_checkpoint()       [NEW - mark transaction for checkpoint]
```

**Atomic Guarantee:** WAL append + fsync happens BEFORE in-memory commit becomes visible. If WAL write fails, transaction rolls back. If process crashes after WAL fsync but before in-memory commit, recovery replays WAL entry.

### 3. Checkpoint Strategy: Hybrid Time/Size-Based

**Decision:** Background checkpoint thread triggers on whichever comes first:
- **Time-based:** Every 60 seconds (configurable)
- **Size-based:** When WAL reaches 64 MB (configurable)
- **Manual:** Explicit `checkpoint()` call

**Rationale:**
- Time-based ensures regular durability even with low write volume
- Size-based prevents unbounded WAL growth on high write volume
- Hybrid provides predictable behavior across workloads

**Checkpoint Process:**
1. Acquire list of pending transactions (sequence numbers)
2. For each transaction, apply WAL entries to main database B-tree
3. Flush main database header (single fsync)
4. Truncate WAL file and reset sequence counter
5. Resume normal operation

**Write Availability:** Writes continue during checkpoint (append to WAL). Checkpoint applies older transactions while new transactions accumulate.

### 4. WAL Entry Format: Binary with CRC32 Checksums

**Decision:** Binary serialization with per-entry checksums using CRC32.

**Format:**
```
WAL Entry (Variable Length):
┌──────────────────┬────────────┬───────────────┬──────────────┬─────────────────┬──────────┐
│ Length (4 bytes) │ Seq (8 B)  │ CF Name (var) │ TxID (8 B)   │ Payload (var)   │ CRC (4B) │
└──────────────────┴────────────┴───────────────┴──────────────┴─────────────────┴──────────┘
                   ↑                                                                        ↑
                   Included in CRC calculation ───────────────────────────────────────────→

Length: u32 - total entry length (including this field)
Sequence: u64 - monotonic sequence number
CF Name: length-prefixed string (u32 length + UTF-8 bytes)
TxID: u64 - transaction ID from TransactionalMemory
Payload: Serialized transaction data (see below)
CRC32: u32 - checksum of all preceding bytes
```

**Payload Format:**
```rust
// Serialized using bincode for compactness
struct WALTransactionPayload {
    user_root: Option<(PageNumber, Checksum)>,      // Data B-tree root
    system_root: Option<(PageNumber, Checksum)>,    // System B-tree root
    freed_pages: Vec<PageNumber>,                    // Pages freed by this txn
    allocated_pages: Vec<PageNumber>,                // Pages allocated by this txn
    durability: Durability,                          // Original durability setting
}
```

**Rationale:**
- Binary format is compact and fast to serialize/deserialize
- CRC32 is fast (hardware-accelerated on modern CPUs) and sufficient for corruption detection
- Length prefix enables forward scanning and recovery from partial writes
- Sequence numbers provide ordering and allow duplicate detection during recovery

**Partial Write Handling:** If the last entry in the WAL has an invalid CRC or incomplete length field, it is discarded as a partial write from a crash. All prior entries with valid CRCs are applied.

### 5. File Structure

**WAL File Layout:**
```
WAL File: <database_path>.wal
┌────────────────────┐
│ WAL Header (512 B) │  Magic, version, oldest_seq, latest_seq, checksum
├────────────────────┤
│ WAL Entry 1        │  Variable length
├────────────────────┤
│ WAL Entry 2        │  Variable length
├────────────────────┤
│ ...                │
├────────────────────┤
│ WAL Entry N        │  Variable length
└────────────────────┘

WAL Header:
┌────────────────┬─────────┬───────────┬───────────┬──────────┬─────────────┐
│ Magic (8 B)    │ Ver (1) │ Oldest (8)│ Latest (8)│ CRC (4)  │ Padding...  │
└────────────────┴─────────┴───────────┴───────────┴──────────┴─────────────┘
Magic: b"REDB-WAL" - file type identifier
Version: u8 - WAL format version (starts at 1)
Oldest Seq: u64 - sequence number of first valid entry (after truncation)
Latest Seq: u64 - sequence number of last written entry
CRC32: u32 - checksum of header
Padding: Zeros to fill 512 bytes
```

**Durability Invariant:** WAL header is updated with latest_seq BEFORE fsyncing WAL file. On recovery, entries with sequence numbers between `oldest_seq` and `latest_seq` are replayed.

---

## Detailed Component Design

### Component 1: WALJournal (Core Append/Read)

**Location:** `src/column_family/wal/journal.rs`

**Responsibilities:**
- Manage WAL file lifecycle (open, append, truncate, close)
- Serialize and deserialize WAL entries
- Maintain sequence counter
- Compute and verify CRC32 checksums

**Public API:**
```rust
pub struct WALJournal {
    file: File,
    path: PathBuf,
    sequence_counter: Arc<AtomicU64>,
    write_buffer: Mutex<Vec<u8>>,  // Buffer for batching
}

impl WALJournal {
    /// Opens an existing WAL or creates a new one.
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self>;
    
    /// Appends a transaction entry to the WAL.
    /// Returns the assigned sequence number.
    pub fn append(&self, entry: &WALEntry) -> io::Result<u64>;
    
    /// Syncs all pending writes to disk.
    pub fn sync(&self) -> io::Result<()>;
    
    /// Reads all entries with sequence numbers >= start_seq.
    pub fn read_from(&self, start_seq: u64) -> io::Result<Vec<WALEntry>>;
    
    /// Truncates the WAL and resets sequence counter.
    pub fn truncate(&self, new_oldest_seq: u64) -> io::Result<()>;
    
    /// Closes the WAL file.
    pub fn close(self) -> io::Result<()>;
}

pub struct WALEntry {
    pub sequence: u64,
    pub cf_name: String,
    pub transaction_id: u64,
    pub payload: WALTransactionPayload,
}

pub struct WALTransactionPayload {
    pub user_root: Option<(PageNumber, Checksum)>,
    pub system_root: Option<(PageNumber, Checksum)>,
    pub freed_pages: Vec<PageNumber>,
    pub allocated_pages: Vec<PageNumber>,
    pub durability: Durability,
}
```

**Implementation Details:**

**Append Flow:**
1. Acquire write_buffer mutex
2. Serialize WALEntry to buffer using bincode
3. Compute CRC32 of serialized data
4. Append length + data + CRC to WAL file
5. Update WAL header with latest_seq
6. Release mutex (fsync happens separately)

**Batching Optimization:** Multiple threads can append concurrently (serialized by Mutex), then a single fsync commits all pending entries. This amortizes fsync cost across multiple transactions.

**CRC Verification:** On read, compute CRC32 of entry data and compare with stored CRC. If mismatch, treat as corrupt entry (stop replay if at end of WAL, skip if in middle with warning).

---

### Component 2: CheckpointManager (Background Checkpointing)

**Location:** `src/column_family/wal/checkpoint.rs`

**Responsibilities:**
- Run background thread that periodically checkpoints WAL
- Track pending transaction sequence numbers
- Apply WAL entries to main database during checkpoint
- Coordinate with active write transactions

**Public API:**
```rust
pub struct CheckpointManager {
    journal: Arc<WALJournal>,
    database: Arc<ColumnFamilyDatabase>,
    config: CheckpointConfig,
    pending_sequences: Arc<RwLock<BTreeSet<u64>>>,
    shutdown_signal: Arc<AtomicBool>,
    checkpoint_thread: Option<JoinHandle<()>>,
}

impl CheckpointManager {
    /// Creates and starts the checkpoint manager.
    pub fn start(
        journal: Arc<WALJournal>,
        database: Arc<ColumnFamilyDatabase>,
        config: CheckpointConfig,
    ) -> Self;
    
    /// Registers a transaction sequence number as pending checkpoint.
    pub fn register_pending(&self, sequence: u64);
    
    /// Manually triggers a checkpoint (blocks until complete).
    pub fn checkpoint_now(&self) -> io::Result<()>;
    
    /// Shuts down the checkpoint thread gracefully.
    pub fn shutdown(self) -> io::Result<()>;
}

pub struct CheckpointConfig {
    pub interval: Duration,         // Time-based trigger (default: 60s)
    pub max_wal_size: u64,          // Size-based trigger (default: 64MB)
}
```

**Background Thread Logic:**
```rust
loop {
    // Sleep until next checkpoint time or size threshold
    sleep_or_wait_for_threshold();
    
    if shutdown_signal.load(Ordering::Acquire) {
        break;
    }
    
    // Perform checkpoint
    checkpoint_internal()?;
}

fn checkpoint_internal() -> io::Result<()> {
    // 1. Get snapshot of pending sequences
    let sequences = pending_sequences.read().unwrap().clone();
    if sequences.is_empty() {
        return Ok(()); // Nothing to checkpoint
    }
    
    // 2. Read all pending WAL entries
    let oldest_seq = *sequences.first().unwrap();
    let entries = journal.read_from(oldest_seq)?;
    
    // 3. Apply each entry to main database
    for entry in entries {
        apply_wal_entry_to_database(&entry)?;
    }
    
    // 4. Fsync main database to persist changes
    database.sync_all()?;
    
    // 5. Truncate WAL and clear pending sequences
    let latest_seq = *sequences.last().unwrap();
    journal.truncate(latest_seq + 1)?;
    pending_sequences.write().unwrap().clear();
    
    Ok(())
}
```

**Apply WAL Entry Logic:**
```rust
fn apply_wal_entry_to_database(entry: &WALEntry) -> io::Result<()> {
    // Get the column family
    let cf = database.column_family(&entry.cf_name)?;
    
    // Access the underlying Database's TransactionalMemory
    let db = cf.ensure_database()?;
    let mem = db.get_memory();  // pub(crate) method
    
    // Apply the transaction state from WAL to TransactionalMemory
    // This updates the in-memory B-tree roots and page allocations
    mem.apply_wal_transaction(
        entry.payload.user_root,
        entry.payload.system_root,
        entry.transaction_id,
        &entry.payload.freed_pages,
        &entry.payload.allocated_pages,
    )?;
    
    Ok(())
}
```

**Note:** This requires adding a new method `TransactionalMemory::apply_wal_transaction()` to redb core. This is a minimal, surgical change that applies pre-computed transaction state without going through the full WriteTransaction path.

---

### Component 3: Recovery (Crash Recovery on Open)

**Location:** `src/column_family/wal/recovery.rs`

**Responsibilities:**
- Detect incomplete transactions on database open
- Replay WAL entries to restore consistency
- Validate WAL integrity (checksums, sequence numbers)
- Handle partial writes and corruption gracefully

**Public API:**
```rust
pub struct WALRecovery {
    journal: Arc<WALJournal>,
    database: Arc<ColumnFamilyDatabase>,
}

impl WALRecovery {
    pub fn new(journal: Arc<WALJournal>, database: Arc<ColumnFamilyDatabase>) -> Self;
    
    /// Performs crash recovery by replaying WAL entries.
    /// Returns the number of transactions recovered.
    pub fn recover(&self) -> io::Result<usize>;
}
```

**Recovery Algorithm:**
```rust
pub fn recover(&self) -> io::Result<usize> {
    // 1. Read WAL header to get oldest_seq and latest_seq
    let header = self.journal.read_header()?;
    
    // 2. Read all entries from oldest_seq onward
    let entries = self.journal.read_from(header.oldest_seq)?;
    
    // 3. Validate sequence numbers are monotonic and within expected range
    let mut prev_seq = header.oldest_seq - 1;
    let mut valid_entries = vec![];
    
    for entry in entries {
        // Check sequence number
        if entry.sequence != prev_seq + 1 {
            eprintln!("WAL sequence gap detected: expected {}, got {}", 
                      prev_seq + 1, entry.sequence);
            if entry.sequence <= header.latest_seq {
                // Within range but out of order - corruption
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "WAL sequence corruption detected"
                ));
            } else {
                // Beyond latest_seq - partial write from crash, stop here
                break;
            }
        }
        
        valid_entries.push(entry);
        prev_seq = entry.sequence;
    }
    
    // 4. Apply valid entries to database
    for entry in &valid_entries {
        apply_wal_entry_to_database(entry)?;
    }
    
    // 5. Fsync database to persist recovery
    self.database.sync_all()?;
    
    // 6. Truncate WAL (recovery is complete)
    self.journal.truncate(prev_seq + 1)?;
    
    Ok(valid_entries.len())
}
```

**Error Handling:**
- **Invalid CRC:** Log warning and stop replay (treat as end of valid entries)
- **Sequence gap:** Error if within expected range, stop replay if beyond
- **Missing column family:** Error (cannot apply transaction without CF)
- **Partial entry:** Silently discard (incomplete write from crash)

---

### Component 4: WriteTransaction Integration

**Location:** `src/transactions.rs` (modification) and `src/column_family/database.rs`

**Responsibilities:**
- Intercept WriteTransaction::commit() to append to WAL
- Extract transaction state for WAL entry
- Coordinate fsync timing (WAL before in-memory commit)
- Handle WAL write failures gracefully

**Integration Points:**

**1. Modify WriteTransaction::commit_inner():**

```rust
fn commit_inner(&mut self) -> Result<(), CommitError> {
    // ... existing code to flush B-trees ...
    let (user_root, allocated_pages, data_freed) =
        self.tables.lock().unwrap().table_tree.flush_and_close()?;
    
    self.store_data_freed_pages(data_freed.clone())?;  // Clone for WAL
    self.store_allocated_pages(allocated_pages.clone())?;  // Clone for WAL
    
    // NEW: Append to WAL if enabled
    if let Some(wal_journal) = self.wal_journal.as_ref() {
        let entry = WALEntry {
            sequence: 0,  // Will be assigned by journal
            cf_name: self.cf_name.clone(),
            transaction_id: self.transaction_id.raw_id(),
            payload: WALTransactionPayload {
                user_root,
                system_root: self.system_tables.lock().unwrap().table_tree.get_root(),
                freed_pages: data_freed,
                allocated_pages: allocated_pages.into_iter().collect(),
                durability: self.durability.into(),
            },
        };
        
        let sequence = wal_journal.append(&entry)
            .map_err(|e| CommitError::Storage(StorageError::from(e)))?;
        
        // Fsync WAL (fast append-only sync)
        wal_journal.sync()
            .map_err(|e| CommitError::Storage(StorageError::from(e)))?;
        
        // Register for checkpoint
        if let Some(checkpoint_mgr) = self.checkpoint_manager.as_ref() {
            checkpoint_mgr.register_pending(sequence);
        }
    }
    
    // ... existing commit code ...
    match self.durability {
        InternalDurability::None => self.non_durable_commit(user_root)?,
        InternalDurability::Immediate => {
            // With WAL, we can skip the expensive B-tree fsync
            // because we've already fsynced the WAL
            if self.wal_journal.is_some() {
                self.non_durable_commit(user_root)?;  // In-memory only
            } else {
                self.durable_commit(user_root)?;  // Legacy path
            }
        }
    }
    
    Ok(())
}
```

**2. Add WAL fields to WriteTransaction:**

```rust
pub struct WriteTransaction {
    // ... existing fields ...
    
    // NEW: WAL integration
    cf_name: String,  // Column family name for WAL entry
    wal_journal: Option<Arc<WALJournal>>,
    checkpoint_manager: Option<Arc<CheckpointManager>>,
}
```

**3. Modify ColumnFamily::begin_write():**

```rust
pub fn begin_write(&self) -> Result<WriteTransaction, TransactionError> {
    let db = self.ensure_database()?;
    let mut txn = db.begin_write()?;
    
    // Inject WAL context if enabled
    if let Some(wal_ctx) = &self.wal_context {
        txn.set_wal_context(
            self.name.clone(),
            wal_ctx.journal.clone(),
            wal_ctx.checkpoint_manager.clone(),
        );
    }
    
    Ok(txn)
}
```

---

### Component 5: Configuration and Builder

**Location:** `src/column_family/builder.rs` (modification)

**Responsibilities:**
- Provide WAL configuration options
- Enable/disable WAL at database level
- Configure checkpoint behavior

**Builder API:**

```rust
pub struct ColumnFamilyDatabaseBuilder {
    pool_size: usize,
    wal_enabled: bool,               // NEW
    wal_config: Option<WALConfig>,   // NEW
}

pub struct WALConfig {
    pub checkpoint_interval: Duration,
    pub max_wal_size: u64,
}

impl Default for WALConfig {
    fn default() -> Self {
        Self {
            checkpoint_interval: Duration::from_secs(60),
            max_wal_size: 64 * 1024 * 1024,  // 64 MB
        }
    }
}

impl ColumnFamilyDatabaseBuilder {
    /// Enables WAL with default configuration.
    pub fn enable_wal(mut self) -> Self {
        self.wal_enabled = true;
        self.wal_config = Some(WALConfig::default());
        self
    }
    
    /// Enables WAL with custom configuration.
    pub fn wal_config(mut self, config: WALConfig) -> Self {
        self.wal_enabled = true;
        self.wal_config = Some(config);
        self
    }
    
    /// Disables WAL (uses legacy durability behavior).
    pub fn disable_wal(mut self) -> Self {
        self.wal_enabled = false;
        self.wal_config = None;
        self
    }
}
```

**Database Initialization with WAL:**

```rust
impl ColumnFamilyDatabaseBuilder {
    pub fn open<P: AsRef<Path>>(self, path: P) -> Result<ColumnFamilyDatabase, ColumnFamilyError> {
        // ... existing open logic ...
        
        // Initialize WAL if enabled
        let wal_context = if self.wal_enabled {
            let wal_path = path.as_ref().with_extension("wal");
            let journal = Arc::new(WALJournal::open(&wal_path)?);
            
            // Perform recovery if WAL exists and has entries
            let recovery = WALRecovery::new(journal.clone(), db.clone());
            let recovered = recovery.recover()?;
            if recovered > 0 {
                eprintln!("WAL recovery: applied {recovered} transactions");
            }
            
            // Start checkpoint manager
            let checkpoint_config = CheckpointConfig {
                interval: self.wal_config.as_ref().unwrap().checkpoint_interval,
                max_wal_size: self.wal_config.as_ref().unwrap().max_wal_size,
            };
            let checkpoint_mgr = CheckpointManager::start(
                journal.clone(),
                db.clone(),
                checkpoint_config,
            );
            
            Some(WALContext {
                journal,
                checkpoint_manager: Arc::new(checkpoint_mgr),
            })
        } else {
            None
        };
        
        // Store WAL context in database
        db.wal_context = wal_context;
        
        Ok(db)
    }
}
```

---

## File Structure and Module Organization

**New Files:**

```
src/column_family/wal/
├── mod.rs              # Module declaration and public re-exports
├── journal.rs          # WALJournal - core append/read operations
├── checkpoint.rs       # CheckpointManager - background checkpointing
├── recovery.rs         # WALRecovery - crash recovery logic
├── entry.rs            # WALEntry and payload types
└── config.rs           # WALConfig and CheckpointConfig
```

**Modified Files:**

```
src/column_family/
├── database.rs         # Add WAL context, modify begin_write()
├── builder.rs          # Add WAL configuration options
└── mod.rs              # Add wal module declaration

src/
├── transactions.rs     # Modify WriteTransaction::commit_inner()
└── tree_store/page_store/page_manager.rs  # Add apply_wal_transaction()
```

**Module Hierarchy:**

```rust
// src/column_family/wal/mod.rs

pub mod journal;
pub mod checkpoint;
pub mod recovery;
pub mod entry;
pub mod config;

pub use self::checkpoint::{CheckpointConfig, CheckpointManager};
pub use self::config::WALConfig;
pub use self::entry::{WALEntry, WALTransactionPayload};
pub use self::journal::WALJournal;
pub use self::recovery::WALRecovery;
```

**Design Principle:** Each module handles a single responsibility. No mixing of concerns.

---

## Testing Strategy

### Unit Tests

**journal.rs:**
- Test append and read operations
- Test CRC verification (corrupt data detection)
- Test sequence number assignment
- Test truncate operation

**checkpoint.rs:**
- Test checkpoint triggers (time, size)
- Test applying WAL entries to database
- Test pending sequence tracking

**recovery.rs:**
- Test recovery with valid WAL
- Test recovery with partial writes
- Test recovery with CRC corruption
- Test recovery with sequence gaps

### Integration Tests

**tests/wal_tests.rs:**
- Test write → crash → recover → verify data
- Test concurrent writes with WAL enabled
- Test checkpoint during active writes
- Test WAL disabled fallback to legacy behavior

### Stress Tests

**tests/wal_stress.rs:**
- High-volume writes with periodic crashes
- Multiple CFs with WAL enabled
- Long-running checkpoint stress test

---

## Performance Expectations

**Without WAL (Current):**
- Durable writes: ~60 ops/sec per thread (full B-tree fsync)
- Non-durable writes: ~16K ops/sec per thread (no fsync)

**With WAL (Target):**
- Durable writes: 10-15K ops/sec per thread (WAL append + fsync)
- Speedup: **200-250x over current durable**, near-parity with Durability::None

**Fsync Comparison:**
- Full B-tree fsync: ~5ms (many random writes across file)
- WAL append fsync: ~0.5ms (sequential append to single file)

**Checkpoint Overhead:**
- Checkpoint every 60s with 64MB WAL: ~100-200ms
- Non-blocking for writes (accumulate in new WAL space)
- Amortized cost: negligible (<0.5% of total time)

---

## Rollout Plan

### Phase 5.6a: Core WAL (8-10 hours)
- Implement WALJournal with append/read/truncate
- Implement WALEntry serialization with CRC32
- Unit tests for journal operations

### Phase 5.6b: Transaction Integration (6-8 hours)
- Modify WriteTransaction::commit_inner() to append to WAL
- Add WAL context to WriteTransaction
- Modify ColumnFamily::begin_write() to inject WAL
- Integration tests for basic WAL write path

### Phase 5.6c: Checkpoint System (6-8 hours)
- Implement CheckpointManager with background thread
- Implement apply_wal_entry_to_database logic
- Add TransactionalMemory::apply_wal_transaction() to redb
- Integration tests for checkpoint correctness

### Phase 5.6d: Crash Recovery (6-8 hours)
- Implement WALRecovery with full validation
- Handle partial writes and corruption
- Integrate recovery into database open path
- Crash simulation tests

### Phase 5.6e: Testing & Benchmarking (4-6 hours)
- Comprehensive integration tests
- Stress tests with simulated crashes
- Performance benchmarks vs baseline
- Validate 10K+ ops/sec target

### Phase 5.6f: Documentation (2-3 hours)
- Update COMPLETION_PLAN.md
- Document WAL behavior and configuration
- Provide migration guide from Durability::None pattern

**Total Estimated Time:** 32-43 hours

---

## Success Criteria

1. **Performance:** Durable writes achieve >10K ops/sec per thread (200x improvement)
2. **Correctness:** All crash recovery tests pass with 100% data integrity
3. **Concurrency:** Concurrent writes with WAL show similar scaling to Durability::None
4. **Overhead:** WAL overhead is <10% compared to Durability::None (without fsync)
5. **Reliability:** Stress tests with 10K+ transactions show zero data loss on simulated crashes

---

## Open Questions

1. **Do we need per-transaction durability control with WAL enabled?**
   - Proposal: Keep `set_durability()` API, but with WAL it controls checkpoint urgency, not fsync behavior
   - `Durability::Immediate`: Triggers immediate checkpoint after WAL append
   - `Durability::None`: Defers checkpoint to background thread

2. **Should checkpoint block new writes or allow concurrent WAL growth?**
   - Proposal: Allow concurrent writes (complexity) for maximum throughput
   - Alternative: Block writes during checkpoint (simpler, brief pause)

3. **How to handle WAL file size on disk for long-running databases?**
   - Proposal: Checkpoint frequency ensures WAL stays <64MB
   - Alternative: Add WAL compaction for very long-running databases

4. **Should we support disabling WAL at runtime (not just at open)?**
   - Proposal: No - WAL state is database-level, changing at runtime is complex
   - Alternative: Provide `checkpoint_and_disable_wal()` method for migration

---

## Future Enhancements (Post Phase 5.6)

1. **Per-CF WAL Journals:** Eliminate shared WAL bottleneck for >8 concurrent CFs
2. **Async I/O Integration:** Use io_uring for zero-copy WAL writes on Linux
3. **WAL Compression:** Compress WAL entries for space savings (zstd)
4. **Incremental Checkpoint:** Apply WAL entries incrementally rather than all-at-once
5. **WAL Archiving:** Keep old WAL files for point-in-time recovery

---

## Conclusion

This WAL design provides production-ready fast + durable writes while maintaining redb's strong consistency guarantees. The phased implementation approach allows for incremental validation and testing. The shared WAL architecture balances simplicity with performance, with a clear migration path to per-CF WAL if needed.

**Key Innovation:** By fsyncing an append-only journal instead of random B-tree writes, we achieve 200-250x durability performance improvement while preserving full ACID semantics and crash recovery.
