// Web Worker for Manifold WASM Database
// This worker handles all database operations in a separate thread

import init, { WasmStorageBackend, ColumnFamilyDatabase } from "./pkg/manifold.js";

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

                log("Creating WASM storage backend...");
                const backend = await WasmStorageBackend.new("manifold-demo.db");

                log("Opening column family database...");
                db = ColumnFamilyDatabase.open_with_backend(
                    "manifold-demo.db",
                    backend,
                    0, // pool_size: 0 disables WAL (not yet implemented for WASM)
                );

                const columnFamilies = db.list_column_families();
                log(`Database initialized with ${columnFamilies.length} column families`);

                result = { columnFamilies };
                break;

            case "createCF":
                log(`Creating column family: ${data.name}`);
                db.create_column_family(data.name, null);
                result = { columnFamilies: db.list_column_families() };
                break;

            case "write":
                log(`Writing to ${data.cf}["${data.key}"]`);
                const cf = db.column_family_or_create(data.cf);
                const writeTxn = cf.begin_write();

                // Open a table (using default table for simplicity)
                const writeTable = writeTxn.open_table("default");
                writeTable.insert(data.key, data.value);

                writeTxn.commit();
                break;

            case "read":
                log(`Reading from ${data.cf}["${data.key}"]`);
                const readCf = db.column_family(data.cf);
                const readTxn = readCf.begin_read();

                const readTable = readTxn.open_table("default");
                const value = readTable.get(data.key);

                result = { value: value || null };
                break;

            case "listAll":
                log("Listing all data...");
                const allData = {};
                const cfs = db.list_column_families();

                for (const cfName of cfs) {
                    const listCf = db.column_family(cfName);
                    const listTxn = listCf.begin_read();

                    try {
                        const listTable = listTxn.open_table("default");
                        const entries = [];

                        // Iterate through all entries
                        const iter = listTable.iter();
                        while (iter.next()) {
                            const key = iter.key();
                            const value = iter.value();
                            entries.push([key, value]);
                        }

                        allData[cfName] = entries;
                    } catch (e) {
                        // Table might not exist yet
                        allData[cfName] = [];
                    }
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
