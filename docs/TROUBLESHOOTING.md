# Troubleshooting Guide

This guide covers common errors you may encounter when using Manifold and how to resolve them.

## Table of Contents

- [Database Opening Errors](#database-opening-errors)
- [Table Operation Errors](#table-operation-errors)
- [Transaction Errors](#transaction-errors)
- [I/O and Storage Errors](#io-and-storage-errors)
- [Corruption and Recovery](#corruption-and-recovery)
- [Column Family Errors](#column-family-errors)
- [WASM-Specific Errors](#wasm-specific-errors)
- [Performance Issues](#performance-issues)

---

## Database Opening Errors

### Error: "Database already open. Cannot acquire lock."

**Cause:** Another process or thread already has the database open with write access.

**Solutions:**
1. Check for other processes using the database file:
   ```bash
   # On Unix-like systems
   lsof /path/to/database.manifold
   ```
2. Ensure you're not opening the same database multiple times in your application
3. If you need concurrent read access, use `begin_read()` instead of `begin_write()`
4. For legitimate concurrent read-only access across processes, consider using the read-only mode (if supported in future versions)

### Error: "Manual upgrade required. Expected file format version 3, but file is version X"

**Cause:** The database file was created with an older or incompatible version of Manifold.

**Solutions:**
1. Check the database file version and your Manifold library version
2. If upgrading from an older version, follow the migration guide in CHANGELOG.md
3. For production systems, test migrations on a copy of the database first
4. Consider exporting data from the old database and importing into a new one if automatic migration is not available

### Error: "DB corrupted: [details]"

**Cause:** The database file has been corrupted due to disk errors, crashes, or bugs.

**Solutions:**
1. Check if you have a backup of the database
2. Review system logs for disk errors or crashes around the time of corruption
3. If WAL is enabled, reopening the database may automatically recover from the WAL
4. For severe corruption, you may need to restore from backup
5. Enable WAL for future protection against crashes: `ColumnFamilyDatabaseBuilder::default().build(path)` (WAL is enabled by default)

---

## Table Operation Errors

### Error: "Table 'X' is of type Table<Y, Z>"

**Cause:** You're trying to open a table with different key/value types than it was created with.

**Solutions:**
1. Check the table definition at the point of creation
2. Ensure consistent `TableDefinition` across your codebase
3. Use type aliases for commonly used table definitions:
   ```rust
   type UsersTable = TableDefinition<u64, &str>;
   const USERS: UsersTable = TableDefinition::new("users");
   ```

### Error: "Table 'X' does not exist"

**Cause:** The table hasn't been created yet, or the name is misspelled.

**Solutions:**
1. Check for typos in the table name
2. Create the table in a write transaction before reading:
   ```rust
   let write_txn = db.begin_write()?;
   {
       let mut table = write_txn.open_table(MY_TABLE)?;
       // Table is now created
   }
   write_txn.commit()?;
   ```
3. Table names are case-sensitive - ensure exact match

### Error: "Table 'X' already exists"

**Cause:** Attempting to create a table that already exists with incompatible schema.

**Solutions:**
1. Use `open_table()` instead of creating explicitly
2. Check if table already exists before creation
3. Tables are automatically created on first `open_table()` call

### Error: "X is a multimap table" / "X is not a multimap table"

**Cause:** Using multimap methods on regular tables or vice versa.

**Solutions:**
1. Use `MultimapTableDefinition` for tables that need multiple values per key
2. Use regular `TableDefinition` for single value per key
3. These are distinct table types - you cannot convert between them

---

## Transaction Errors

### Error: "Transaction still in use"

**Cause:** Attempting to begin a new write transaction while another is still active.

**Solutions:**
1. Ensure previous transactions are committed or aborted:
   ```rust
   {
       let txn = db.begin_write()?;
       // ... work ...
       txn.commit()?;
   } // txn dropped here
   
   let txn2 = db.begin_write()?; // Now OK
   ```
2. Remember: only one write transaction can be active at a time per column family
3. Multiple read transactions can coexist with writes

### Error: "Previous I/O error occurred. Please close and re-open the database."

**Cause:** A previous I/O operation failed and the database is in an error state.

**Solutions:**
1. Close the database handle and reopen it
2. Check the underlying filesystem for errors
3. Ensure sufficient disk space is available
4. Check file permissions

---

## I/O and Storage Errors

### Error: "I/O error: [system error]"

**Cause:** Underlying filesystem or storage error.

**Common Scenarios:**

#### "No space left on device"
- **Solution:** Free up disk space, or increase the storage quota
- Database files can grow; ensure adequate space is available

#### "Permission denied"
- **Solution:** Check file/directory permissions
- Ensure the process has read/write access to the database file and directory

#### "Too many open files"
- **Solution:** Increase the file descriptor limit:
  ```bash
  # Temporary (Unix)
  ulimit -n 4096
  
  # Permanent: edit /etc/security/limits.conf
  ```
- Close unused database connections

### Error: "The value (length=X) being inserted exceeds the maximum of 3GiB"

**Cause:** Attempting to store a value larger than the maximum supported size.

**Solutions:**
1. Split large values into smaller chunks
2. Store large data externally and keep references in the database
3. For binary data, consider compression before storage
4. Maximum value size is 3GiB - this is a hard limit

---

## Corruption and Recovery

### Detecting Corruption

Manifold detects corruption through:
- CRC32 checksums on headers
- B-tree structure validation
- WAL entry checksums

### Recovery Steps

1. **If WAL is enabled** (default):
   - Simply reopen the database
   - WAL replay will recover committed transactions
   - Uncommitted transactions are lost (expected behavior)

2. **If corruption persists**:
   ```rust
   // Check if database opens at all
   match ColumnFamilyDatabase::open(path) {
       Ok(db) => {
           // Database opened - try reading data
           // Corruption may be in specific tables
       }
       Err(e) => {
           // Header or critical structure corrupted
           // Restore from backup
       }
   }
   ```

3. **Prevention**:
   - Enable WAL (enabled by default)
   - Regular backups
   - Monitor disk health
   - Use journaling filesystems (ext4, XFS, APFS, NTFS)

---

## Column Family Errors

### Error: "column family 'X' not found"

**Cause:** The column family hasn't been created, or name is misspelled.

**Solutions:**
1. Create the column family first:
   ```rust
   db.create_column_family("my_cf", None)?;
   ```
2. Or use auto-create:
   ```rust
   let cf = db.column_family_or_create("my_cf")?;
   ```
3. List existing column families:
   ```rust
   let families = db.list_column_families();
   ```

### Error: "column family 'X' already exists"

**Cause:** Attempting to create a column family that already exists.

**Solutions:**
1. Use `column_family()` to get existing CF
2. Use `column_family_or_create()` for idempotent creation
3. Check with `list_column_families()` first

---

## WASM-Specific Errors

### Error: "OPFS not supported"

**Cause:** Origin Private File System (OPFS) is not available in the browser.

**Solutions:**
1. Ensure you're using a modern browser (Chrome 102+, Edge 102+)
2. OPFS requires a secure context (HTTPS or localhost)
3. Check browser compatibility: https://caniuse.com/native-filesystem-api

### Error: "Sync access not available (requires Web Worker)"

**Cause:** Attempting to use synchronous OPFS access outside of a Web Worker.

**Solutions:**
1. Move database operations to a Web Worker:
   ```javascript
   const worker = new Worker('database-worker.js');
   ```
2. OPFS synchronous access is only available in Web Worker contexts
3. See `examples/wasm/` for reference implementation

### Error: "Failed to get OPFS root: [details]"

**Cause:** Browser denied access to OPFS, or quota exceeded.

**Solutions:**
1. Check if the user has denied storage permissions
2. Check storage quota:
   ```javascript
   const estimate = await navigator.storage.estimate();
   console.log(`Used: ${estimate.usage}, Quota: ${estimate.quota}`);
   ```
3. Request persistent storage:
   ```javascript
   await navigator.storage.persist();
   ```

---

## Performance Issues

### Slow Writes

**Symptoms:** Write operations take longer than expected.

**Solutions:**
1. **Use WAL** (enabled by default):
   - Provides ~200x faster durable writes
2. **Batch operations**:
   ```rust
   let txn = db.begin_write()?;
   {
       let mut table = txn.open_table(TABLE)?;
       for item in items {
           table.insert(&item.key, &item.value)?;
       }
   }
   txn.commit()?; // Single commit for all inserts
   ```
3. **Reduce fsync frequency** (if you can tolerate some data loss):
   ```rust
   // For non-critical data, consider adjusting durability
   // (Check documentation for durability options)
   ```

### Slow Reads

**Symptoms:** Read operations are slower than expected.

**Solutions:**
1. **Use appropriate key types**:
   - Prefer fixed-size types (u64, u32) over variable-size (&str)
   - Fixed-size keys enable better B-tree optimizations
2. **Check query patterns**:
   - Sequential access is faster than random access
   - Use range queries when appropriate
3. **Monitor cache efficiency**:
   - Increase pool size if you have available memory:
     ```rust
     ColumnFamilyDatabase::builder()
         .pool_size(256)  // Default is 64
         .open(path)?
     ```

### High Memory Usage

**Symptoms:** Database using more memory than expected.

**Solutions:**
1. **Reduce pool size**:
   ```rust
   ColumnFamilyDatabase::builder()
       .pool_size(32)  // Reduce from default 64
       .open(path)?
   ```
2. **Close unused column families**:
   - Drop column family handles when not in use
3. **Monitor transaction lifetime**:
   - Long-running read transactions prevent old pages from being reclaimed
   - Close read transactions as soon as possible

---

## Getting Help

If you're experiencing an issue not covered here:

1. **Check the documentation**: https://docs.rs/manifold
2. **Search existing issues**: Check the GitHub issue tracker
3. **Enable debug logging**: Set `RUST_LOG=manifold=debug` for detailed logs
4. **File an issue**: Provide:
   - Manifold version
   - Platform (OS, architecture)
   - Minimal reproduction case
   - Error message and stack trace
   - Database file size and approximate entry count

## Preventive Measures

To avoid common issues:

- ✓ Enable WAL (enabled by default) for crash protection
- ✓ Implement regular backups
- ✓ Monitor disk space and I/O errors
- ✓ Use consistent type definitions across your codebase
- ✓ Close transactions promptly
- ✓ Test error handling paths in your application
- ✓ Monitor error rates in production
- ✓ Keep Manifold updated for bug fixes and improvements