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
                log("Listing all data...");
                const allData = {};
                const cfs = db.listColumnFamilies();

                for (const cfName of cfs) {
                    // For now, just return empty - full iteration would need more WASM API
                    allData[cfName] = [];
                }

                result = { data: allData };
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
