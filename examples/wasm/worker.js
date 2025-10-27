// Web Worker for Manifold WASM Database
// This worker handles all database operations in a separate thread

import init, { WasmDatabase } from "./pkg/manifold.js";

let db = null;
let wasmInitialized = false;

// Initialize WASM module
async function initWasm() {
    if (!wasmInitialized) {
        await init();
        wasmInitialized = true;
    }
}

// Send log message to main thread
function log(message, level = "info") {
    postMessage({
        type: "log",
        data: { message, level },
    });
}

// Handle messages from main thread
self.onmessage = async function (e) {
    const { id, type, data } = e.data;

    try {
        let result = {};

        switch (type) {
            case "init":
                log("Initializing WASM module...");
                await initWasm();

                log("Checking OPFS support...");

                // Check if OPFS is supported
                if (!navigator.storage || !navigator.storage.getDirectory) {
                    throw new Error(
                        "OPFS not supported. Requires modern browser and HTTPS/localhost.",
                    );
                }

                log("Opening column family database...");
                const poolSize = 8; // WAL enabled with 8 file handles for group commit
                db = await new WasmDatabase("manifold-demo.db", poolSize);

                const columnFamilies = db.listColumnFamilies();
                log(`Database initialized with ${columnFamilies.length} column families`);

                log("WAL ENABLED (pool_size=8) - Fast writes with group commit", "success");
                log("  - Checkpoints: 15s interval or 32 MB WAL size", "info");
                log("  - Manual checkpoint available via Sync button", "info");

                result = { columnFamilies };
                break;

            case "createCF":
                log(`Creating column family: ${data.name}`);
                db.createColumnFamily(data.name);
                result = { columnFamilies: db.listColumnFamilies() };
                break;

            case "write":
                log(`Writing to ${data.cf}["${data.key}"]`, "info");
                const cf = db.columnFamilyOrCreate(data.cf);
                const writeStart = performance.now();
                cf.write(data.key, data.value);
                const writeTime = (performance.now() - writeStart).toFixed(2);
                log(`Write complete in ${writeTime}ms (WAL + group commit)`, "success");
                break;

            case "read":
                log(`Reading from ${data.cf}["${data.key}"]`);
                const readCf = db.columnFamily(data.cf);
                const value = readCf.read(data.key);
                result = { value: value || null };
                break;

            case "listAll":
                log("Listing all data using batch iteration...");
                const allData = {};
                const cfs = db.listColumnFamilies();

                for (const cfName of cfs) {
                    const cf = db.columnFamily(cfName);
                    const iter = cf.iter();
                    const entries = [];

                    // Use batch iteration for performance (100 entries at a time)
                    let batch;
                    while ((batch = iter.nextBatch(100)).length > 0) {
                        // Convert to regular JavaScript array for iteration
                        for (let i = 0; i < batch.length; i++) {
                            const pair = batch[i];
                            const key = pair[0];
                            const value = pair[1];
                            entries.push({ key, value });
                        }
                    }

                    allData[cfName] = entries;
                    log(`Listed ${entries.length} entries from ${cfName}`);
                }

                result = { data: allData };
                break;

            case "listRange":
                log(`Listing range: ${data.startKey || "(start)"} to ${data.endKey || "(end)"}`);
                const rangeCf = db.columnFamily(data.cf);
                const rangeIter = rangeCf.iterRange(data.startKey || null, data.endKey || null);
                const rangeEntries = [];

                let rangeBatch;
                while ((rangeBatch = rangeIter.nextBatch(100)).length > 0) {
                    for (let i = 0; i < rangeBatch.length; i++) {
                        const pair = rangeBatch[i];
                        const key = pair[0];
                        const value = pair[1];
                        rangeEntries.push({ key, value });
                    }
                }

                log(`Found ${rangeEntries.length} entries in range`);
                result = { entries: rangeEntries };
                break;

            case "sync":
                log("Manual sync: Triggering WAL checkpoint...", "info");
                db.sync();
                log("Checkpoint complete - WAL flushed to main database", "success");
                break;

            case "walStatus":
                log("Checking WAL status in OPFS...", "info");
                try {
                    const root = await navigator.storage.getDirectory();
                    let walExists = false;
                    let walSize = 0;

                    try {
                        const walHandle = await root.getFileHandle("manifold-demo.db.wal");
                        const file = await walHandle.getFile();
                        walExists = true;
                        walSize = file.size;
                    } catch (e) {
                        // WAL file doesn't exist yet
                    }

                    if (walExists) {
                        log(`WAL file exists: ${walSize} bytes`, "success");
                        log(`WAL will checkpoint at 32 MB or 15 seconds`, "info");
                    } else {
                        log("WAL file not yet created (no writes yet)", "info");
                    }

                    result = { walExists, walSize };
                } catch (error) {
                    log(`WAL status check failed: ${error.message}`, "error");
                }
                break;

            default:
                throw new Error(`Unknown message type: ${type}`);
        }

        // Send success response
        postMessage({
            id,
            type: "response",
            data: result,
        });
    } catch (error) {
        log(`Error: ${error.message}`, "error");

        // Send error response
        postMessage({
            id,
            type: "error",
            error: error.message,
        });
    }
};

// beforeunload example: Sync database before page closes
// This ensures any pending WAL entries are checkpointed to the main database
// Uncomment the following to enable automatic sync on page close:
//
// self.addEventListener("beforeunload", async (e) => {
//     if (db) {
//         try {
//             db.sync();
//         } catch (error) {
//             console.error("Failed to sync database on close:", error);
//         }
//     }
// });
