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
                db = await new WasmDatabase("manifold-demo.db");

                const columnFamilies = db.listColumnFamilies();
                log(`Database initialized with ${columnFamilies.length} column families`);

                result = { columnFamilies };
                break;

            case "createCF":
                log(`Creating column family: ${data.name}`);
                db.createColumnFamily(data.name);
                result = { columnFamilies: db.listColumnFamilies() };
                break;

            case "write":
                log(`Writing to ${data.cf}["${data.key}"]`);
                const cf = db.columnFamilyOrCreate(data.cf);
                cf.write(data.key, data.value);
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
