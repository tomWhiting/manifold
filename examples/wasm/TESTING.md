# WASM Batch Iterator Testing Guide

## Prerequisites

1. Modern browser with OPFS support (Chrome 102+, Safari 15.2+, Firefox 111+)
2. Local web server (must use localhost or HTTPS)
3. WASM module built: `wasm-pack build --target web --out-dir examples/wasm/pkg`

## Test Setup

1. Start local server:
   ```bash
   cd examples/wasm
   python3 -m http.server 8000
   ```

2. Open browser to: `http://localhost:8000`

3. Open Developer Tools Console

## Test Cases

### Test 1: Empty Table Iteration
**Objective:** Verify iterator handles empty tables correctly

**Steps:**
1. Click "Initialize Database"
2. Type "test_empty" in Create CF input
3. Click "Create Column Family"
4. Click "List All Data"

**Expected Result:**
- Log shows: "Listed 0 entries from test_empty"
- Table shows: test_empty with empty array

**Pass Criteria:** No errors, clean empty result

---

### Test 2: Single Entry Iteration
**Objective:** Verify iterator works with one entry

**Steps:**
1. Select "test_empty" from Write dropdown
2. Enter key: "key1", value: "value1"
3. Click "Write"
4. Click "List All Data"

**Expected Result:**
- Log shows: "Listed 1 entries from test_empty"
- Table shows: test_empty with [{key: "key1", value: "value1"}]

**Pass Criteria:** Single entry displayed correctly

---

### Test 3: Multiple Entries Iteration
**Objective:** Verify batch iterator with multiple entries

**Steps:**
1. Select "test_empty" from Write dropdown
2. Write 10 entries:
   - key2/value2
   - key3/value3
   - ... up to key10/value10
3. Click "List All Data"

**Expected Result:**
- Log shows: "Listed 10 entries from test_empty"
- All 10 entries displayed in table

**Pass Criteria:** All entries present, correct order

---

### Test 4: Large Table Batch Performance
**Objective:** Verify batch mode handles larger datasets efficiently

**Steps:**
1. Create new CF: "test_large"
2. Write 50-100 entries with different keys
   (You can write them individually or use browser console for automation)
3. Click "List All Data"

**Expected Result:**
- Log shows: "Listed N entries from test_large" (where N is your count)
- All entries displayed
- Operation completes quickly (< 500ms)

**Pass Criteria:** Fast iteration, no errors, all entries present

---

### Test 5: Batch Iterator API Test (Console)
**Objective:** Test the batch iterator API directly

**Steps:**
1. Open browser console
2. Run this script:
```javascript
async function testBatchIterator() {
    // Get a column family with data
    const cf = window.db.columnFamily("test_large");
    const iter = cf.iter();
    
    console.log("Testing nextBatch(10):");
    const batch1 = iter.nextBatch(10);
    console.log(`  Batch 1 length: ${batch1.length}`);
    // Note: Use index-based iteration, not for-of
    for (let i = 0; i < Math.min(3, batch1.length); i++) {
        const pair = batch1[i];
        const k = pair[0];
        const v = pair[1];
        console.log(`    [${k}, ${v}]`);
    }
    
    console.log("Testing next():");
    const single = iter.next();
    if (single && single !== undefined) {
        const k = single[0];
        const v = single[1];
        console.log(`  Single entry: [${k}, ${v}]`);
    } else {
        console.log(`  Iterator exhausted (expected if < 11 entries)`);
    }
    
    console.log("Testing collectAll():");
    const remaining = iter.collectAll();
    console.log(`  Remaining entries: ${remaining.length}`);
    
    console.log("Test complete!");
}

testBatchIterator().catch(console.error);
```

**Expected Result:**
- Batch of 10 entries (or fewer if table is smaller)
- Single next() call works
- collectAll() gets remaining entries
- No errors

**Pass Criteria:** All API methods work correctly

---

### Test 6: Multiple Column Families
**Objective:** Verify iteration works across multiple CFs

**Steps:**
1. Create 3 different CFs: "cf1", "cf2", "cf3"
2. Write different data to each:
   - cf1: a1/b1, a2/b2
   - cf2: x1/y1, x2/y2, x3/y3
   - cf3: foo/bar
3. Click "List All Data"

**Expected Result:**
- Log shows separate counts for each CF
- Table shows all CFs with correct data
- No cross-contamination

**Pass Criteria:** Each CF's data isolated and correct

---

### Test 7: Persistence After Reload
**Objective:** Verify iterator works with persisted data

**Steps:**
1. Write several entries to a CF
2. Click "List All Data" (verify they show up)
3. Refresh the page (F5)
4. Click "Initialize Database"
5. Click "List All Data"

**Expected Result:**
- All previously written entries still present
- Iterator works on persisted data

**Pass Criteria:** Data persists, iteration works after reload

---

### Test 8: Iterator Resource Cleanup
**Objective:** Verify iterators don't leak resources

**Steps:**
1. Open browser console
2. Run this script:
```javascript
async function testCleanup() {
    const cf = window.db.columnFamily("test_large");
    
    // Create and abandon 100 iterators
    for (let i = 0; i < 100; i++) {
        const iter = cf.iter();
        // Don't call next, just create and abandon
    }
    
    console.log("Created 100 iterators (should be GC'd)");
    
    // Create one and use it
    const iter = cf.iter();
    const batch = iter.nextBatch(5);
    console.log(`Final iterator works: ${batch.length} entries`);
    
    // Explicit close
    iter.close();
    console.log("Explicit close successful");
}

testCleanup().catch(console.error);
```

**Expected Result:**
- No memory leaks or errors
- Final iterator works
- close() succeeds

**Pass Criteria:** No browser warnings, final iterator functional

---

### Test 9: Error Handling
**Objective:** Verify graceful error handling

**Steps:**
1. Open console
2. Try iterating non-existent CF:
```javascript
try {
    const cf = window.db.columnFamily("doesnotexist");
    const iter = cf.iter();
} catch (e) {
    console.log("Caught error:", e);
}
```

**Expected Result:**
- Clear error message about missing CF
- No crash

**Pass Criteria:** Clean error, no crash

---

### Test 10: Concurrent Operations
**Objective:** Verify iteration works during writes

**Steps:**
1. Create CF with some data
2. Open two console tabs (or use setTimeout in one)
3. In one: Start a long iteration (collectAll on large dataset)
4. In other: Write new data simultaneously

**Expected Result:**
- Both operations succeed
- No data corruption
- Iterator may or may not see new writes (depends on transaction timing)

**Pass Criteria:** No errors, no corruption

---

## Performance Benchmarks

### Benchmark 1: Batch vs Single-Entry Iteration

**Setup:**
```javascript
async function benchmarkIterator() {
    const cf = window.db.columnFamily("test_large");
    
    // Batch mode (100 entries per batch)
    console.time("Batch iteration (size=100)");
    const iter1 = cf.iter();
    let count1 = 0;
    let batch;
    while ((batch = iter1.nextBatch(100)).length > 0) {
        count1 += batch.length;
    }
    console.timeEnd("Batch iteration (size=100)");
    console.log(`  Total entries: ${count1}`);
    
    // Single-entry mode
    console.time("Single-entry iteration");
    const iter2 = cf.iter();
    let count2 = 0;
    let entry;
    while ((entry = iter2.next()) !== undefined) {
        count2++;
    }
    console.timeEnd("Single-entry iteration");
    console.log(`  Total entries: ${count2}`);
}

benchmarkIterator().catch(console.error);

// Note: When iterating over batches, use index-based loops:
// for (let i = 0; i < batch.length; i++) { const [k, v] = [batch[i][0], batch[i][1]]; }
// Not: for (const [k, v] of batch) - this won't work with WASM-returned arrays
```

**Expected Result:**
- Batch mode significantly faster (10-100x for large tables)
- Both modes return same count

---

## Current Status

**Fully Implemented:**
- ✅ High-performance batch iteration with `nextBatch(size)`
- ✅ Range queries with `iterRange(startKey, endKey)`
- ✅ Full table iteration with `iter()`
- ✅ OPFS persistence across page reloads
- ✅ Multiple column families

**Planned (Phase 6.2):**
- WAL (Write-Ahead Log) for improved write performance
  - Currently each write commits immediately
  - WAL will enable batched commits with group commit optimization

---

## Troubleshooting

### "Iterator not working"
- Check browser console for errors
- Verify OPFS support: `navigator.storage && navigator.storage.getDirectory`
- Ensure running in localhost or HTTPS

### "Empty results but data exists"
- Check you're querying the right CF
- Try refreshing after write
- Verify write succeeded (check logs)

### "Slow iteration"
- Are you using batch mode? nextBatch(100) vs next()
- Check table size (too many entries?)
- Look for errors in console

---

## Success Criteria Summary

All tests should:
- Execute without errors
- Return expected results
- Complete in reasonable time (< 1s for < 1000 entries)
- Not cause memory leaks
- Handle edge cases gracefully
