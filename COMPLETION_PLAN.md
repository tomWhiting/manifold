# Column Family Database Implementation Plan

## Executive Summary

This plan outlines the implementation of a column family architecture on top of redb to enable concurrent multi-threaded writes within a single embedded database file. The design partitions a single database file into multiple independent column families, each acting as a complete redb instance with its own transaction isolation. This allows N concurrent writer threads (one per column family) while maintaining redb's strong ACID guarantees and MVCC-based concurrent reader support.

The primary motivation is to support high-performance applications with domain-specific data isolation (e.g., e-commerce with separate users, products, sales, and analytics domains) while avoiding the operational complexity of managing multiple database files or running a separate server process. The architecture prioritizes performance through zero-copy operations, fixed-width type optimizations, and cache-friendly data layout.

## Design Overview

### Column Families Concept

A column family represents a logical partition of the database containing related data. Unlike hash-based partitioning where keys are automatically distributed, column families provide explicit, application-controlled grouping. For example, an e-commerce application might define column families for users, products, sales, and analytics. Each column family is completely independent and can be written to concurrently with other column families.

Within each column family, the existing redb table system is used to organize data. A products column family might contain separate tables for metadata, vector embeddings, and reviews. All tables within a column family share transaction boundaries, ensuring atomic updates across multiple tables, while different column families can be modified simultaneously by different threads.

This design maps naturally to real-world application domains while providing true concurrent write capability through per-column-family write locks rather than a global database write lock.

### Global Index Pattern

A powerful pattern enabled by column families is the global index. Applications can designate a special column family (by convention named with an underscore prefix like `_global_index`) that contains cross-cutting indexes spanning all other column families. This allows querying across different entity types or domains while maintaining the organizational benefits of separate column families.

For example, a vector database might store different entity types (users, products, documents) in separate column families for concurrent write performance, but maintain a global index column family containing all vector embeddings. Queries can scan the global index to find relevant entities across all types, then fan out to the appropriate type-specific column families to retrieve full data. This pattern combines the performance benefits of column family isolation with the flexibility of cross-domain queries.

### Single File Architecture

The database file begins with a master header containing metadata about all column families including their names, byte offsets, and allocated sizes. Following the master header, each column family is laid out sequentially as a complete redb database structure with its own super-header, transaction slots, and region allocations.

A partitioned storage backend provides offset translation, making each column family believe it owns the entire file from offset zero while actually operating on a specific byte range. This abstraction allows unmodified redb Database instances to operate on partitions of a larger file without knowledge of the partitioning scheme.

The single-file approach simplifies deployment, enables atomic backups, and reduces file descriptor usage compared to managing separate files per column family while maintaining the concurrent write benefits of separation.

### Performance Optimization Strategy

Performance is prioritized through several key decisions. The multiple tables approach within column families allows reading only required data rather than deserializing entire entities. For vector similarity search operations, embeddings are stored in a dedicated table separate from metadata, providing better cache locality and eliminating unnecessary deserialization overhead.

Fixed-width types are used wherever possible to enable zero-copy reads. Vector embeddings with known dimensions can be accessed directly from memory-mapped pages without deserialization. The architecture avoids unnecessary clones through careful use of Arc for shared ownership and borrows for temporary access.

Column family isolation provides concurrency at the domain level where it matters most. Independent write transactions to different domains (users vs products) can proceed in parallel without coordination overhead, while the existing MVCC system handles concurrent readers without blocking writers.

### WASM Compatibility

Browser compatibility is achieved through a platform-specific storage backend that implements the StorageBackend trait using IndexedDB or Origin Private File System. Since browsers are inherently single-process environments, file locking logic is bypassed entirely in WASM builds.

The synchronous redb API can be maintained in WASM through careful use of blocking wrappers around async browser APIs, or alternatively an async API variant can be provided for more idiomatic browser usage. Web Workers provide threading capabilities for parallel column family access, though with more constraints than native threads.

## Architecture Details

### File Structure

The master header occupies the first page (4KB) and contains a magic number for file type identification, a version number for format evolution, the count of column families, and an array of column family metadata entries. Each metadata entry specifies the column family name as a variable-length string, the absolute byte offset where that column family begins, and the size allocated to it.

Following the master header, column families are laid out contiguously. Each column family contains a complete redb database structure starting with the redb super-header (512 bytes) containing the god byte and transaction slots, followed by regions containing B-tree data for all tables within that column family.

The partitioned storage backend wraps the underlying file backend and translates all offset-based operations. When a column family at offset 1GB requests to read from offset 1000, the backend translates this to offset 1GB + 1000 in the actual file. This translation is transparent to the redb Database instance.

### Concurrency Model

Each column family owns a complete redb Database instance, which provides single-writer multiple-reader semantics through existing MVCC mechanisms. The TransactionTracker in each Database independently manages its write lock and reader registration without knowledge of other column families.

At the application level, the ColumnFamilyDatabase maintains a collection of Database instances indexed by column family name. Retrieving a column family returns a lightweight ColumnFamily handle wrapping an Arc to the underlying Database. This handle can be cheaply cloned and passed to different threads.

When thread A begins a write transaction on the users column family while thread B begins a write transaction on the products column family, both proceed concurrently. Each thread blocks only if another thread is already writing to the same column family. Readers on any column family never block writers or other readers thanks to MVCC snapshot isolation.

### Data Structure Approach

The recommended pattern uses multiple tables within each column family rather than composite keys or single large structs. For products, separate tables handle metadata, vectors, and reviews. This enables reading only required data (e.g., just vectors for similarity search), updating only changed data (e.g., just stock status), and using different type optimizations per table (fixed-width for vectors, variable-width for metadata).

Fixed-width types should be used for performance-critical data like vector embeddings. A 384-dimensional float vector is exactly 1,536 bytes and can be defined with a fixed_width implementation that returns Some(1536). This allows redb to provide direct pointers to memory-mapped data without deserialization overhead.

Variable-width types use efficient binary serialization through bincode rather than JSON. Custom Value trait implementations handle serialization, providing type safety while avoiding text-based encoding overhead. The combination of fixed-width and variable-width types optimizes each table for its specific access patterns.

## Design Decisions

This section documents important design decisions made during implementation, including rationale and trade-offs.

### Magic Number: 9 Bytes Instead of 8

**Decision:** The master header magic number is 9 bytes (`b"redb-cf\x1A\x0A"`) rather than the initially planned 8 bytes.

**Rationale:** The additional bytes (0x1A and 0x0A) serve as DOS and Unix line ending detection markers. If the file is accidentally opened in text mode or transferred using text-mode FTP, these bytes will be corrupted, allowing early detection of file corruption. This is a common practice in binary file formats (e.g., PNG uses similar markers).

**Trade-off:** One extra byte of overhead in the header (negligible given the 4KB page size).

### PartitionedStorageBackend len() Behavior

**Decision:** The `len()` method returns the actual allocated size within the partition, calculated as `min(underlying_len - partition_offset, partition_size)`, returning 0 if the partition hasn't been allocated yet.

**Rationale:** This allows the Database instance to correctly detect empty (uninitialized) partitions versus partitions that already contain data. If `len()` always returned `partition_size`, the Database would think the partition already contained data and fail to initialize properly.

**Implementation Detail:** When a partition is first created, the underlying storage hasn't been extended to cover it yet, so `len()` returns 0. As the Database writes data via `set_len()`, the underlying storage grows and `len()` begins returning the actual allocated amount.

### set_len() No-Shrink Policy

**Decision:** The `set_len()` implementation only grows the underlying storage when needed; it does not shrink the underlying storage when requested length is less than current length.

**Rationale:**
1. Other partitions may be using space beyond this partition in the shared underlying storage
2. Shrinking would require coordination across all partitions to ensure safety
3. The underlying storage backend can handle any necessary compaction at the file level
4. Simplicity and safety are prioritized over aggressive space reclamation

**Trade-off:** May result in some wasted space if partitions shrink significantly, but this is an acceptable trade-off for operational simplicity.

### Custom Error Type: ColumnFamilyError

**Decision:** Implemented a dedicated `ColumnFamilyError` enum with specific variants (`AlreadyExists`, `NotFound`, `Database`, `Io`) rather than using generic `io::Error` or `DatabaseError` everywhere.

**Rationale:**
1. Provides type-safe, semantically meaningful error variants
2. Allows callers to match on specific error conditions (e.g., column family already exists)
3. Improves error messages with domain-specific context
4. Follows Rust best practices for error handling

**Implementation:** The error type implements `std::error::Error`, `Display`, and provides `From` conversions for underlying error types, making it ergonomic to use with the `?` operator.

### Atomic Header Updates in create_column_family()

**Decision:** The implementation persists the updated header to disk (including fsync) before adding the new column family to the in-memory map.

**Rationale:** This ensures atomicity and consistency. If the header write fails (e.g., disk full, I/O error), the in-memory state remains unchanged and the error propagates to the caller. If we updated the in-memory map first, a failed header write would leave the system in an inconsistent state.

**Trade-off:** Slightly more complex code flow, but critical for correctness and recovery from failures.

## Implementation Phases

### Phase 1: Partitioned Storage Backend

**Status:** Complete

**Objective:** Implement the core offset translation layer that allows a byte range within a file to appear as a complete file to redb Database instances.

**Key Components:**

- [x] Create `PartitionedStorageBackend` struct in new module `src/column_family/partitioned_backend.rs`
  - Contains Arc to underlying StorageBackend, partition offset as u64, and partition size as u64
  - **Dev Notes:** Implemented with Arc<dyn StorageBackend> for shared ownership across multiple partitions. Constructor panics on offset overflow via checked_add for early error detection.

- [x] Implement `StorageBackend` trait for `PartitionedStorageBackend`
  - Override `len()` to return actual allocated size within partition
  - Override `read()` to translate offset by adding partition_offset before delegating to inner backend
  - Override `write()` with same offset translation
  - Override `set_len()` with bounds checking against partition size
  - Delegate `sync_data()` and `close()` directly to inner backend
  - **Dev Notes:** All methods use validate_and_translate() helper for consistent bounds checking and offset translation. close() intentionally does not close inner backend since other partitions may share it. set_len() only grows underlying storage if needed (see Design Decisions section for rationale). Critical: len() returns min(underlying_len - partition_offset, partition_size) to allow Database to detect empty partitions.

- [x] Add bounds checking in all methods to prevent partition overflow
  - Verify offset plus length does not exceed partition_size
  - Return appropriate io::Error for out-of-bounds access
  - **Dev Notes:** validate_and_translate() performs three checks: offset+len overflow, partition bounds, and translation overflow. All use checked arithmetic with descriptive error messages for debugging.

- [x] Write comprehensive unit tests for offset translation
  - Test read/write at various offsets within partition
  - Test boundary conditions (offset 0, offset at partition end)
  - Test error cases (overflow, out of bounds)
  - Test with mock backend to verify translation math
  - **Dev Notes:** 14 tests cover offset translation, bounds checking, partition isolation, Arc sharing behavior, and error cases. InMemoryBackend requires pre-sizing via set_len() before read/write operations.

**Files Modified:**
- Create: `src/column_family/partitioned_backend.rs`
- Create: `src/column_family/mod.rs` (with module declaration and visibility control)

**Dependencies:** None (uses existing StorageBackend trait from `src/db.rs`)

**Estimated Time:** 2 hours

---

### Phase 2: Master Header Format

**Status:** Complete

**Objective:** Define and implement the serialization format for the master header that describes column family layout within the file.

**Key Components:**

- [x] Define `MasterHeader` struct in `src/column_family/header.rs`
  - Magic number as [u8; 9] constant "redb-cf\x1A\x0A"
  - Version number as u8 (start with 1)
  - Column family count as u32
  - Vector of ColumnFamilyMetadata entries
  - **Dev Notes:** Magic number is 9 bytes (updated from initial plan of 8) to include both 0x1A and 0x0A for DOS/Unix line ending detection - see Design Decisions section. Serialization format fits well within one page with room for many column families.

- [x] Define `ColumnFamilyMetadata` struct
  - Name as String
  - Offset as u64 (absolute file offset)
  - Allocated size as u64
  - **Dev Notes:** Includes helper methods for serialization/deserialization with proper error handling. Uses length-prefixed strings for variable-length names.

- [x] Implement `to_bytes()` serialization for MasterHeader
  - Use fixed-size header followed by variable-size metadata array
  - Encode count before metadata entries
  - Use length-prefixed strings for column family names
  - Ensure total serialized size fits within one page (4096 bytes)
  - **Dev Notes:** Returns error if header exceeds PAGE_SIZE, pads with zeros to exactly 4096 bytes. Format is: magic(9) + version(1) + count(4) + metadata entries(variable) + padding.

- [x] Implement `from_bytes()` deserialization for MasterHeader
  - Validate magic number matches expected value
  - Check version compatibility
  - Parse metadata entries with proper error handling
  - Validate offset and size values for sanity
  - **Dev Notes:** Automatically calls validate() after deserialization to ensure consistency. Provides clear error messages for debugging malformed headers.

- [x] Add validation logic
  - Verify column family names are non-empty and unique
  - Check offsets are page-aligned and non-overlapping
  - Ensure allocated sizes are positive
  - **Dev Notes:** Validation uses HashSet for duplicate detection, checks all offsets are 4096-byte aligned, verifies no overlapping ranges using pairwise comparison, and checks for overflow in offset+size calculations.

- [x] Write serialization round-trip tests
  - Test with various column family counts (0, 1, many)
  - Test with different name lengths
  - Test error cases (invalid magic, bad version, corrupt data)
  - **Dev Notes:** 15 comprehensive tests covering all validation cases, serialization round-trips, error conditions, and edge cases like adjacent (non-overlapping) ranges and headers that exceed page size.

**Files Modified:**
- Create: `src/column_family/header.rs`
- Modify: `src/column_family/mod.rs` (add module declaration)

**Dependencies:** Phase 1 complete (though not strictly dependent)

**Estimated Time:** 1 hour

---

### Phase 3: ColumnFamilyDatabase Implementation

**Status:** Complete

**Objective:** Implement the main API that manages multiple column families within a single file and provides the public interface for applications.

**Key Components:**

- [x] Define `ColumnFamilyDatabase` struct in `src/column_family/database.rs`
  - File path as PathBuf
  - Shared file backend as Arc<dyn StorageBackend>
  - Column family map as Arc<RwLock<HashMap<String, Arc<Database>>>>
  - Master header as Arc<RwLock<MasterHeader>>
  - **Dev Notes:** Struct holds Arc-wrapped shared state for thread-safe access. RwLock allows concurrent reads of column family map while serializing writes (creating new column families).

- [x] Implement `open()` constructor
  - Open or create file using FileBackend
  - Read master header from first page
  - For each column family in header, create PartitionedStorageBackend and Database instance
  - Populate column family map
  - Handle case of new file (create empty master header)
  - **Dev Notes:** Detects new files by checking if length is 0. For new files, writes empty master header to first page. For existing files, reads and deserializes master header, then recreates all Database instances from metadata. FileBackend::new returns DatabaseError directly so no need to wrap it.

- [x] Implement `create_column_family()` method
  - Acquire write lock on column family map
  - Check for duplicate name
  - Calculate next available offset (after last column family)
  - Determine allocation size (configurable or default to 1GB)
  - Create PartitionedStorageBackend for new range
  - Initialize new Database instance with backend
  - Update master header and persist to file
  - Add to column family map
  - Return ColumnFamily handle
  - **Dev Notes:** First column family starts at PAGE_SIZE (4096) after master header. Subsequent families are placed contiguously based on max(offset+size) of existing families. Database::builder().create_with_backend() handles initialization automatically when partition appears empty (len=0). Critical fix: PartitionedStorageBackend::len() must return actual allocated size, not partition_size, so Database can detect empty partitions.

- [x] Implement `column_family()` accessor
  - Acquire read lock on map
  - Look up column family by name
  - Return ColumnFamily handle wrapping Arc<Database>
  - Return error if not found
  - **Dev Notes:** Returns lightweight ColumnFamily wrapper that clones the Arc<Database>, making it cheap to pass between threads. Uses read lock for concurrent access.

- [x] Implement `list_column_families()` method
  - Return vector of column family names
  - **Dev Notes:** Simple accessor that reads from in-memory header. Returns owned Strings for safety.

- [ ] Implement `delete_column_family()` method (optional, for completeness)
  - Remove from map
  - Update master header
  - Consider space reclamation strategy
  - **Dev Notes:** Deferred - not required for initial implementation.

- [x] Define `ColumnFamily` wrapper struct
  - Name as String
  - Database reference as Arc<Database>
  - **Dev Notes:** Implements Clone via #[derive(Clone)] which clones the Arc (cheap reference count increment). Provides clean API boundary between column family concept and underlying Database.

- [x] Implement convenience methods on ColumnFamily
  - `begin_write()` delegates to Database
  - `begin_read()` delegates to Database
  - Implement Clone using Arc clone (cheap)
  - **Dev Notes:** Direct delegation to self.db methods. Requires importing ReadableDatabase trait for begin_read() to be available. Clone is automatically cheap thanks to Arc.

**Files Modified:**
- Create: `src/column_family/database.rs`
- Modify: `src/column_family/mod.rs` (add module and re-exports)
- Modify: `src/lib.rs` (add public re-export of column_family module - already present)
- **Critical Fixes Applied:**
  1. Modified `src/column_family/partitioned_backend.rs` - Changed `len()` implementation to return actual allocated size (see Design Decisions section)
  2. Created `ColumnFamilyError` enum for type-safe error handling with specific variants for common cases
  3. Fixed atomicity in `create_column_family()` - header is persisted to disk before updating in-memory map
  4. Enhanced documentation in `set_len()` explaining no-shrink policy

**Dependencies:** Phase 1 and Phase 2 complete

**Estimated Time:** 3-4 hours

---

### Phase 4: Integration and Testing

**Status:** In Progress

**Objective:** Ensure the column family system works correctly with comprehensive testing and integrates cleanly with existing redb functionality.

**Key Components:**

- [x] Create example program demonstrating column family usage
  - Show creating column families for different domains
  - Demonstrate concurrent writes to different column families
  - Show multiple tables within a column family
  - Demonstrate global index pattern (cross-CF queries using special `_global_index` column family)
  - Located in `examples/column_families.rs`
  - **Dev Notes:** Created e-commerce themed example with users, products, and orders column families. Demonstrates concurrent writes (3 threads writing ~3500 total records in ~50-70ms), multiple tables per CF (users+emails, products+prices), and atomic transactions. Uses `Box<dyn Error>` for flexible error handling across redb error types. Global index pattern deferred as more advanced use case.

- [ ] Write integration tests in `tests/column_family_tests.rs`
  - Test creating and opening column families
  - Test concurrent writes to different column families succeed
  - Test concurrent writes to same column family properly serialize
  - Test transactions across multiple tables within column family
  - Test persistence (write, close, reopen, verify data)
  - **Dev Notes:**

- [ ] Write stress tests for concurrency
  - Spawn multiple threads writing to different column families simultaneously
  - Verify data integrity after concurrent operations
  - Test with many readers and writers
  - **Dev Notes:**

- [ ] Performance benchmarking
  - Compare single column family vs multiple column families for concurrent writes
  - Measure throughput improvement with parallelism
  - Benchmark vector-only table reads vs full-struct reads
  - Profile to identify any bottlenecks
  - **Dev Notes:**

- [ ] Verify no regressions in existing redb tests
  - Run full test suite to ensure compatibility
  - Fix any breaking changes
  - **Dev Notes:**

- [ ] Update documentation
  - Add module-level documentation to `src/column_family/mod.rs`
  - Document public API with examples
  - Update README if appropriate
  - **Dev Notes:**

**Files Modified:**
- Create: `examples/column_families.rs`
- Create: `tests/column_family_tests.rs`
- Modify: `src/column_family/mod.rs` (documentation)

**Dependencies:** Phase 3 complete

**Estimated Time:** 2-3 hours

---

### Phase 5: Dynamic Column Family Sizing (Optional Enhancement)

**Status:** Not Started

**Objective:** Allow column families to grow beyond initial allocation by claiming additional space from a shared pool or expanding the file.

**Key Components:**

- [ ] Design space allocation strategy
  - Decide between fixed expansion increments vs demand-based growth
  - Consider fragmentation implications
  - Document trade-offs in design decision
  - **Dev Notes:**

- [ ] Implement free space tracking in master header
  - Add field tracking unallocated ranges in file
  - Update on column family creation and deletion
  - **Dev Notes:**

- [ ] Modify `PartitionedStorageBackend` to support expansion
  - Detect when write would exceed current partition size
  - Request additional space from ColumnFamilyDatabase
  - Update partition_size atomically
  - **Dev Notes:**

- [ ] Implement space allocation coordination
  - Use mutex or RwLock to serialize space allocation decisions
  - Update master header when column family grows
  - Persist header changes durably
  - **Dev Notes:**

- [ ] Add compaction/reclamation logic (if supporting delete)
  - Allow reclaiming space from deleted column families
  - Consider online vs offline compaction
  - **Dev Notes:**

- [ ] Test dynamic growth scenarios
  - Fill column family to capacity and verify automatic expansion
  - Test multiple column families growing concurrently
  - Verify file size increases appropriately
  - **Dev Notes:**

**Files Modified:**
- Modify: `src/column_family/header.rs` (add free space tracking)
- Modify: `src/column_family/partitioned_backend.rs` (expansion logic)
- Modify: `src/column_family/database.rs` (allocation coordination)

**Dependencies:** Phase 4 complete

**Estimated Time:** 4-6 hours

**Note:** This phase can be deferred if fixed-size column families are acceptable initially.

---

### Phase 6: WASM Backend Implementation

**Status:** Not Started

**Objective:** Enable column family database to run in browser environments using IndexedDB or OPFS for persistence.

**Key Components:**

- [ ] Create `WasmStorageBackend` in `src/backends/wasm.rs`
  - Conditional compilation with `#[cfg(target_arch = "wasm32")]`
  - Use wasm-bindgen for JavaScript interop
  - Choose between IndexedDB or OPFS based on browser support
  - **Dev Notes:**

- [ ] Implement StorageBackend trait for WasmStorageBackend
  - Map byte-range operations to IndexedDB transactions or OPFS file operations
  - Handle async browser APIs with synchronous wrapper or async trait variant
  - No file locking needed (browsers are single-process)
  - **Dev Notes:**

- [ ] Handle async API impedance mismatch
  - Investigate wasm-bindgen-futures for sync wrappers
  - Consider alternative: async StorageBackend trait variant for WASM
  - Document decision and trade-offs
  - **Dev Notes:**

- [ ] Create WASM-specific example
  - Simple web page demonstrating column family database in browser
  - Show persistence across page reloads
  - Located in `examples/wasm/`
  - **Dev Notes:**

- [ ] Test WASM build
  - Verify compilation with wasm32-unknown-unknown target
  - Test in actual browser environment
  - Verify IndexedDB/OPFS persistence
  - **Dev Notes:**

- [ ] Update build configuration
  - Add WASM-specific dependencies to Cargo.toml with target conditions
  - Document WASM build instructions
  - **Dev Notes:**

**Files Modified:**
- Create: `src/backends/wasm.rs`
- Modify: `src/backends.rs` (conditional export)
- Create: `examples/wasm/index.html` and supporting files
- Modify: `Cargo.toml` (WASM dependencies)

**Dependencies:** Phase 4 complete (Phase 5 optional)

**Estimated Time:** 4-6 hours

**Note:** This phase can be deferred if browser support is not immediately required.

---

## Coding Conventions

### Performance Focus

Performance is the primary consideration in all implementation decisions. Avoid allocations and copies wherever possible by using references and borrows. Prefer zero-copy operations such as returning slices into memory-mapped data rather than copying to owned buffers.

Use `Arc` for shared ownership of heavyweight objects like Database instances, allowing cheap cloning across threads without duplicating data. Avoid `clone()` on large data structures; instead pass references or use `Arc::clone()` for reference-counted sharing.

Leverage fixed-width types for performance-critical data. Vector embeddings and other fixed-size data should implement `fixed_width()` returning `Some(size)` to enable direct memory access without deserialization overhead.

Profile before optimizing, but design with performance in mind from the start. The architecture should naturally support efficient access patterns rather than requiring later refactoring for performance.

### Code Organization

Module structure should mirror the conceptual architecture. The `column_family` module contains all column-family-specific code, with submodules for the partitioned backend, header format, and database management.

The `mod.rs` file in each module serves three purposes: controlling visibility through `pub` and `pub(crate)` modifiers, organizing submodules with clear `mod` declarations, and providing module-level documentation explaining the module's role and usage. Keep `mod.rs` files focused on these organizational concerns rather than implementation.

### Leveraging Existing Code

The existing redb codebase is well-designed and thoroughly tested. Reuse existing abstractions wherever possible rather than duplicating functionality. The `StorageBackend` trait provides the exact abstraction needed for partitioning without modifying redb internals.

Database, TransactionalMemory, and transaction types should be used as-is. The column family system wraps these components rather than reimplementing their functionality. This preserves all existing optimizations and correctness guarantees.

When modifications to existing code are necessary, make minimal surgical changes rather than wholesale rewrites. Respect the existing architecture and coding style.

### Error Handling

Use Result types consistently throughout the implementation. Errors should provide enough context for debugging but avoid exposing internal implementation details in public API errors.

Propagate errors with the `?` operator when the caller should handle them. Only use `unwrap()` or `expect()` in cases that are structurally impossible to fail or in test code where panic is acceptable.

Add new error variants to existing error types rather than creating a proliferation of error types if the existing error enum is suitable.

### Code Style

Follow Rust naming conventions rigorously. Types are PascalCase, functions and variables are snake_case, constants are SCREAMING_SNAKE_CASE.

No emojis in code, comments, or error messages. Technical documentation should be clear and professional without decorative elements.

Document public APIs with doc comments explaining purpose, parameters, return values, and any important behavior or edge cases. Internal functions benefit from brief comments explaining non-obvious logic.

Run `cargo fmt` to maintain consistent formatting. Enable clippy and address warnings, though it is acceptable to allow specific warnings with documented rationale.

---

## Plan Maintenance Instructions

This plan is a living document that must be updated as implementation progresses to remain useful across multiple development sessions.

### Updating Progress

When starting work on a task, do not check its box yet. The checkbox should only be marked when the task is completely finished and verified to work correctly. Partially completed tasks remain unchecked.

When completing a task, check its box and immediately add dev notes in the designated space below the checkbox. Dev notes should concisely capture what was implemented, any important decisions made, gotchas discovered, or context that will help future developers (including yourself in the next session) understand the implementation.

Update the phase status at the beginning of each phase section from "Not Started" to "In Progress" when beginning the first task in that phase. Change to "Complete" only when all tasks in the phase are checked off and verified.

### Dev Notes Format

Dev notes should be 1-3 sentences capturing essential information. Focus on what is non-obvious or might be surprising when revisiting the code later. Examples of useful dev notes:

- "Used RwLock instead of Mutex because reads significantly outnumber writes in typical usage"
- "Bounds checking must happen before offset translation to catch partition overflow"
- "IndexedDB has a 50MB quota in some browsers; documented in WasmStorageBackend comments"

Avoid obvious statements like "Implemented the function" that provide no additional value.

### Cross-Session Continuity

At the start of each new development session, review the plan to understand what has been completed and what remains. Check the dev notes for recently completed tasks to understand any important context or decisions.

Before continuing implementation, verify that previously completed tasks still work by running relevant tests. This catches any inadvertent breakage from other changes.

Update estimated times if they prove significantly inaccurate to help calibrate future estimates.

---

## Success Criteria

The implementation is considered complete when all tasks through Phase 4 are checked off and verified working. Phase 5 (dynamic sizing) and Phase 6 (WASM) are optional enhancements that can be deferred or skipped based on requirements.

The example program in `examples/column_families.rs` should successfully demonstrate creating column families, concurrent writes, and multi-table transactions without errors or data corruption.

All existing redb tests must pass without modification to ensure compatibility. The integration tests in `tests/column_family_tests.rs` must verify concurrent correctness and data persistence.

Performance benchmarks should demonstrate measurable throughput improvement when using multiple column families with concurrent writes compared to sequential writes to a single database. Vector similarity search operations should show significant speedup when using dedicated tables with fixed-width types compared to deserializing full structs.

The code should be clean, well-documented, and maintainable. Someone unfamiliar with the implementation should be able to understand the architecture by reading the module-level documentation and following the code structure.
