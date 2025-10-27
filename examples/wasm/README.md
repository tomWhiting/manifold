# Manifold WASM Column Family Database Demo

This example demonstrates using the Manifold column family database in a browser environment with WASM and OPFS (Origin Private File System).

## Features

- **Column Family Database** running entirely in the browser
- **OPFS Storage** for true persistence across page reloads
- **Concurrent Access** via Web Workers
- **Zero Backend** required - fully client-side

## Requirements

### Browser Support

This demo requires a modern browser with OPFS synchronous access support:

- **Chrome/Edge**: 102+
- **Safari**: 15.2+
- **Firefox**: 111+

### HTTPS Requirement

OPFS requires a secure context (HTTPS or localhost). The demo will not work over `http://` unless running on `localhost`.

## Building the Example

### 1. Install wasm-pack

```bash
cargo install wasm-pack
```

### 2. Build the WASM Module

From the repository root:

```bash
wasm-pack build --target web --out-dir examples/wasm/pkg
```

This compiles the Rust code to WASM and generates JavaScript bindings in `examples/wasm/pkg/`.

### 3. Serve the Example

You need a local web server with HTTPS support. Options:

**Option A: Using uv (recommended for Python users)**

```bash
cd examples/wasm
uv run --with http.server python -m http.server 8000
```

Then open http://localhost:8000

**Option B: Using Python directly (simple, no HTTPS)**

```bash
cd examples/wasm
python -m http.server 8000
```

Then open http://localhost:8000

**Option C: Using a proper dev server (recommended for HTTPS)**

Install a development server with HTTPS support:

```bash
npm install -g http-server
```

Then serve with HTTPS:

```bash
cd examples/wasm
http-server -S -C cert.pem -K key.pem
```

Or use any other local dev server that supports HTTPS.

**Option D: Chrome with OPFS flag (development only)**

Chrome can enable OPFS on insecure origins for testing:

```bash
chrome --enable-features=FileSystemAccessAPI,FileSystemAccessAPIExperimental
```

## Using the Demo

### Creating Column Families

1. Enter a name (e.g., "users", "products")
2. Click "Create Column Family"
3. The column family appears in the list and dropdowns

### Writing Data

1. Select a column family from the dropdown
2. Enter a key and value
3. Click "Write"
4. Data is persisted to OPFS

### Reading Data

1. Select a column family
2. Enter the key to read
3. Click "Read"
4. Value appears in the logs

### Testing Persistence

1. Write some data
2. Click "Test Persistence" to see current data count
3. Refresh the page
4. Your data is still there!

### Viewing All Data

Click "List All Data" to see all entries across all column families.

## Architecture

```
┌─────────────────┐
│   index.html    │  Main UI (runs on main thread)
└────────┬────────┘
         │ postMessage
         ▼
┌─────────────────┐
│   worker.js     │  Web Worker (separate thread)
└────────┬────────┘
         │ WASM calls
         ▼
┌─────────────────┐
│  manifold.wasm  │  Rust/WASM database
└────────┬────────┘
         │ OPFS API
         ▼
┌─────────────────┐
│      OPFS       │  Browser persistent storage
└─────────────────┘
```

### Why Web Workers?

OPFS synchronous access (which provides file-like `read()`/`write()` operations) is only available in Web Worker contexts. The main thread only has async OPFS access, which would require completely rewriting the database to be async.

By using a Web Worker, we can:
- Use the synchronous StorageBackend trait without changes
- Keep the database architecture simple
- Get better performance from synchronous I/O
- Avoid blocking the UI thread

## Current Limitations

### WAL Not Yet Implemented for WASM

The Write-Ahead Log (WAL) feature is not yet implemented for WASM. This means:

- `pool_size` is set to 0 (WAL disabled)
- Each write commits directly to storage
- Performance is lower than native with WAL enabled

This is planned for Phase 6.2. The architecture supports it; we just need to:
1. Implement async checkpoint manager with gloo-timers
2. Test recovery and checkpoint mechanisms in browser context

## Performance Characteristics

### OPFS vs Native File System

OPFS performance varies by browser and platform:

- **Chrome/Edge**: Generally good performance, especially on desktop
- **Safari**: Performance varies by iOS/macOS version
- **Firefox**: Improving with each release

Typical performance (on modern desktop Chrome):
- Small writes (< 1KB): 5,000-10,000 ops/sec
- Large writes (> 1MB): 50-100 MB/sec
- Reads: Similar to writes

Compare to native performance (with WAL):
- Small writes: 450,000 ops/sec (8 threads)
- Much higher throughput due to group commit batching

### Browser Storage Quotas

OPFS storage is subject to browser quota limits:

- **Desktop**: Typically 10% of free disk space
- **Mobile**: More restrictive (varies by device)

Check quota:

```javascript
const estimate = await navigator.storage.estimate();
console.log(`Using ${estimate.usage} of ${estimate.quota} bytes`);
```

Request persistent storage:

```javascript
await navigator.storage.persist();
```

## Troubleshooting

### "OPFS not supported" Error

**Cause**: Browser doesn't support OPFS or not running in secure context.

**Solution**:
- Use a modern browser version
- Serve over HTTPS or localhost
- Check browser compatibility above

### "Failed to create sync access handle" Error

**Cause**: Not running in Web Worker context.

**Solution**:
- Ensure worker.js is being used
- Check browser console for worker errors
- Verify WASM module loaded correctly

### Data Not Persisting

**Cause**: Browser might be in private/incognito mode.

**Solution**:
- Use normal browsing mode
- Check Storage settings in browser DevTools
- Request persistent storage permission

### Worker Initialization Fails

**Cause**: WASM module not found or CORS issues.

**Solution**:
- Check pkg/ directory exists and has manifold.js + manifold_bg.wasm
- Ensure serving from same origin
- Check browser console for specific error

## Development Tips

### Browser DevTools

1. **Application Tab** → Storage → Origin Private File System
   - View OPFS files and sizes
   - Delete storage for testing

2. **Console** → Filter to Worker context
   - See worker logs
   - Debug WASM errors

3. **Network Tab**
   - Verify WASM module loads
   - Check size of compiled binary

### Debugging WASM

Enable WASM debugging in Chrome DevTools:

1. Go to chrome://flags
2. Enable "WebAssembly Debugging"
3. Restart Chrome
4. DevTools → Sources → Can now step through Rust code!

### Performance Profiling

Use Chrome's Performance tab:

1. Record during operations
2. Look for long tasks in Worker thread
3. Check OPFS I/O timing

## Next Steps

### For Learning

1. Try creating multiple column families
2. Write various data types
3. Test concurrent access patterns
4. Experiment with large datasets

### For Production

1. Add error recovery and retry logic
2. Implement data migration strategies
3. Add schema versioning
4. Consider using typed TableDefinitions
5. Add data validation
6. Implement backup/export functionality

## License

Same as the main Manifold project: MIT OR Apache-2.0

## Contributing

Found a bug or have an improvement? Please open an issue or PR on the main repository!