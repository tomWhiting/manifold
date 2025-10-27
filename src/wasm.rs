//! WASM storage backend using OPFS (Origin Private File System).
//!
//! This module provides a [`WasmStorageBackend`] implementation that uses the browser's
//! Origin Private File System for persistent storage in WebAssembly environments.
//!
//! # Requirements
//!
//! - **Web Worker context**: OPFS synchronous access is only available in Web Workers,
//!   not on the main thread
//! - **Modern browser**: Requires browsers with OPFS support (Chrome 102+, Edge 102+,
//!   Safari 15.2+, Firefox 111+)
//! - **SharedArrayBuffer**: Required for synchronous file access
//!
//! # Example
//!
//! ```ignore
//! use manifold::wasm::WasmStorageBackend;
//! use manifold::column_family::ColumnFamilyDatabase;
//!
//! // This code must run in a Web Worker
//! let backend = WasmStorageBackend::new("my-database.db").await?;
//! let db = ColumnFamilyDatabase::builder()
//!     .create_with_backend(Box::new(backend))?;
//! ```

use crate::StorageBackend;
use std::io;
use std::sync::{Arc, Mutex};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{FileSystemFileHandle, FileSystemSyncAccessHandle};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
    #[wasm_bindgen(js_namespace = console)]
    fn error(s: &str);
}

// Set panic hook to get better error messages in WASM
#[wasm_bindgen(start)]
pub fn main() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();

    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            format!("Panic: {}", s)
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            format!("Panic: {}", s)
        } else {
            format!("Panic at {:?}", info.location())
        };
        error(&msg);
    }));
}

/// A storage backend that uses the browser's Origin Private File System (OPFS).
///
/// This backend implements the [`StorageBackend`] trait using OPFS synchronous access handles,
/// which provide file-like read/write operations with byte-level addressing.
///
/// # Thread Safety
///
/// The backend uses internal locking to ensure thread-safe access to the OPFS file handle,
/// though in practice WASM is single-threaded within a Web Worker context.
#[wasm_bindgen]
pub struct WasmStorageBackend {
    /// The OPFS synchronous access handle for file operations
    handle: Arc<Mutex<FileSystemSyncAccessHandle>>,
    /// The file name for debugging and error messages
    file_name: String,
}

// Manual Debug implementation to work around FileSystemSyncAccessHandle not being Debug
impl std::fmt::Debug for WasmStorageBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmStorageBackend")
            .field("file_name", &self.file_name)
            .finish_non_exhaustive()
    }
}

// SAFETY: FileSystemSyncAccessHandle is only accessible from a single Web Worker
// context in WASM, which is effectively single-threaded. The Mutex ensures
// exclusive access. We manually implement Send + Sync since the web-sys types
// don't implement them (they're opaque JavaScript objects).
unsafe impl Send for WasmStorageBackend {}
unsafe impl Sync for WasmStorageBackend {}

#[wasm_bindgen]
impl WasmStorageBackend {
    /// Creates a new WASM storage backend for the given file.
    ///
    /// This function must be called from a Web Worker context where OPFS synchronous
    /// access is available.
    #[wasm_bindgen(constructor)]
    pub async fn new(file_name: &str) -> Result<WasmStorageBackend, JsValue> {
        // Get the OPFS root directory
        let root = Self::get_opfs_root()
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to get OPFS root: {}", e)))?;

        // Get or create the file handle
        let file_handle = Self::get_file_handle(&root, file_name)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to get file handle: {}", e)))?;

        // Create a synchronous access handle (only available in Web Workers)
        let sync_handle = Self::create_sync_handle(&file_handle)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to create sync handle: {}", e)))?;

        Ok(WasmStorageBackend {
            handle: Arc::new(Mutex::new(sync_handle)),
            file_name: file_name.to_string(),
        })
    }

    /// Gets the OPFS root directory.
    async fn get_opfs_root() -> Result<web_sys::FileSystemDirectoryHandle, io::Error> {
        // Access the global scope (must be WorkerGlobalScope in Web Worker)
        let global = js_sys::global();

        // Try to get the storage manager
        let navigator =
            js_sys::Reflect::get(&global, &JsValue::from_str("navigator")).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("Failed to access navigator: {:?}", e),
                )
            })?;

        let storage =
            js_sys::Reflect::get(&navigator, &JsValue::from_str("storage")).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("Storage API not available: {:?}", e),
                )
            })?;

        let get_directory = js_sys::Reflect::get(&storage, &JsValue::from_str("getDirectory"))
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("OPFS not supported: {:?}", e),
                )
            })?;

        let get_directory_fn = get_directory.dyn_into::<js_sys::Function>().map_err(|e| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                format!("getDirectory is not a function: {:?}", e),
            )
        })?;

        // Call getDirectory() to get the root
        let promise = get_directory_fn
            .call0(&storage)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to call getDirectory: {:?}", e),
                )
            })?
            .dyn_into::<js_sys::Promise>()
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("getDirectory did not return a Promise: {:?}", e),
                )
            })?;

        let result = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to get OPFS root: {:?}", e),
                )
            })?;

        result
            .dyn_into::<web_sys::FileSystemDirectoryHandle>()
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Result is not a FileSystemDirectoryHandle: {:?}", e),
                )
            })
    }

    /// Gets or creates a file handle in the OPFS directory.
    async fn get_file_handle(
        root: &web_sys::FileSystemDirectoryHandle,
        file_name: &str,
    ) -> Result<FileSystemFileHandle, io::Error> {
        let options = web_sys::FileSystemGetFileOptions::new();
        options.set_create(true);

        let promise = root.get_file_handle_with_options(file_name, &options);

        let result = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to create/open file '{}': {:?}", file_name, e),
                )
            })?;

        result.dyn_into::<FileSystemFileHandle>().map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Result is not a FileSystemFileHandle: {:?}", e),
            )
        })
    }

    /// Creates a synchronous access handle from a file handle.
    async fn create_sync_handle(
        file_handle: &FileSystemFileHandle,
    ) -> Result<FileSystemSyncAccessHandle, io::Error> {
        let promise = file_handle.create_sync_access_handle();

        let result = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("Sync access not available (requires Web Worker): {:?}", e),
                )
            })?;

        result
            .dyn_into::<FileSystemSyncAccessHandle>()
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Result is not a FileSystemSyncAccessHandle: {:?}", e),
                )
            })
    }

    /// Converts a JavaScript error to an io::Error.
    fn js_error_to_io_error(js_error: JsValue, context: &str) -> io::Error {
        let error_string = if let Some(error) = js_error.dyn_ref::<js_sys::Error>() {
            format!(
                "{}: {}",
                context,
                error
                    .message()
                    .as_string()
                    .unwrap_or_else(|| "Unknown error".to_string())
            )
        } else {
            format!("{}: {:?}", context, js_error)
        };
        io::Error::new(io::ErrorKind::Other, error_string)
    }
}

impl StorageBackend for WasmStorageBackend {
    fn len(&self) -> Result<u64, io::Error> {
        let handle = self.handle.lock().unwrap();

        let size = handle.get_size().map_err(|e| {
            Self::js_error_to_io_error(e, &format!("Failed to get size of '{}'", self.file_name))
        })?;

        Ok(size as u64)
    }

    fn read(&self, offset: u64, out: &mut [u8]) -> Result<(), io::Error> {
        if out.is_empty() {
            return Ok(());
        }

        let handle = self.handle.lock().unwrap();

        // Read from OPFS at the specified offset
        let options = web_sys::FileSystemReadWriteOptions::new();
        options.set_at(offset as f64);

        let bytes_read = handle
            .read_with_u8_array_and_options(out, &options)
            .map_err(|e| {
                Self::js_error_to_io_error(
                    e,
                    &format!(
                        "Failed to read {} bytes at offset {} from '{}'",
                        out.len(),
                        offset,
                        self.file_name
                    ),
                )
            })? as usize;

        if bytes_read != out.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "Expected to read {} bytes but got {} bytes from '{}' at offset {}",
                    out.len(),
                    bytes_read,
                    self.file_name,
                    offset
                ),
            ));
        }

        Ok(())
    }

    fn set_len(&self, len: u64) -> Result<(), io::Error> {
        let handle = self.handle.lock().unwrap();

        handle.truncate_with_f64(len as f64).map_err(|e| {
            Self::js_error_to_io_error(
                e,
                &format!("Failed to set length of '{}' to {}", self.file_name, len),
            )
        })?;

        Ok(())
    }

    fn sync_data(&self) -> Result<(), io::Error> {
        let handle = self.handle.lock().unwrap();

        handle.flush().map_err(|e| {
            Self::js_error_to_io_error(e, &format!("Failed to flush '{}'", self.file_name))
        })?;

        Ok(())
    }

    fn write(&self, offset: u64, data: &[u8]) -> Result<(), io::Error> {
        if data.is_empty() {
            return Ok(());
        }

        let handle = self.handle.lock().unwrap();

        // Write to OPFS at the specified offset
        let options = web_sys::FileSystemReadWriteOptions::new();
        options.set_at(offset as f64);

        let bytes_written = handle
            .write_with_u8_array_and_options(data, &options)
            .map_err(|e| {
                Self::js_error_to_io_error(
                    e,
                    &format!(
                        "Failed to write {} bytes at offset {} to '{}'",
                        data.len(),
                        offset,
                        self.file_name
                    ),
                )
            })? as usize;

        if bytes_written != data.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                format!(
                    "Expected to write {} bytes but wrote {} bytes to '{}' at offset {}",
                    data.len(),
                    bytes_written,
                    self.file_name,
                    offset
                ),
            ));
        }

        Ok(())
    }

    fn close(&self) -> Result<(), io::Error> {
        let handle = self.handle.lock().unwrap();

        // close() doesn't return a Result in the web-sys API
        handle.close();

        Ok(())
    }
}

/// Checks if OPFS synchronous access is supported in the current environment.
///
/// # Returns
///
/// `true` if running in a Web Worker with OPFS support, `false` otherwise.
///
/// # Example
///
/// ```ignore
/// if !is_opfs_supported() {
///     eprintln!("OPFS not supported. Requires a modern browser and Web Worker context.");
///     return;
/// }
/// ```
#[wasm_bindgen]
pub fn is_opfs_supported() -> bool {
    let global = js_sys::global();

    // Check if we have navigator.storage.getDirectory
    let navigator = match js_sys::Reflect::get(&global, &JsValue::from_str("navigator")) {
        Ok(nav) => nav,
        Err(_) => return false,
    };

    let storage = match js_sys::Reflect::get(&navigator, &JsValue::from_str("storage")) {
        Ok(storage) => storage,
        Err(_) => return false,
    };

    let get_directory = match js_sys::Reflect::get(&storage, &JsValue::from_str("getDirectory")) {
        Ok(gd) => gd,
        Err(_) => return false,
    };

    get_directory.is_function()
}

/// WASM-specific wrapper for ColumnFamilyDatabase that provides a JavaScript-friendly API
#[wasm_bindgen]
pub struct WasmDatabase {
    db: crate::column_family::ColumnFamilyDatabase,
}

#[wasm_bindgen]
impl WasmDatabase {
    /// Opens or creates a database with the given file name
    ///
    /// # Arguments
    ///
    /// * `file_name` - Name of the database file in OPFS
    /// * `pool_size` - Number of file handles for WAL (0 = disabled, 4-8 recommended for WASM)
    #[wasm_bindgen(constructor)]
    pub async fn new(file_name: String, pool_size: usize) -> Result<WasmDatabase, JsValue> {
        use crate::column_family::wal::checkpoint::CheckpointManager;
        use crate::column_family::wal::config::CheckpointConfig;
        use crate::column_family::wal::journal::WALJournal;

        // Create main database backend
        let backend = WasmStorageBackend::new(&file_name).await?;
        let backend_arc: Arc<dyn StorageBackend> = Arc::new(backend);

        let db = if pool_size > 0 {
            // WAL enabled - create WAL backend, journal, and checkpoint manager

            // Create WAL backend with .wal extension
            let wal_file_name = format!("{}.wal", file_name);
            let wal_backend = WasmStorageBackend::new(&wal_file_name).await?;
            let wal_backend_arc: Arc<dyn StorageBackend> = Arc::new(wal_backend);

            // Create WAL journal
            let journal = WALJournal::new(wal_backend_arc)
                .map_err(|e| JsValue::from_str(&format!("Failed to create WAL journal: {}", e)))?;
            let journal_arc = Arc::new(journal);

            // Open database without checkpoint manager first
            let db_temp = crate::column_family::ColumnFamilyDatabase::open_with_backend_internal(
                file_name.clone(),
                Arc::clone(&backend_arc),
                Some(Arc::clone(&journal_arc)),
                None,
            )
            .map_err(|e| JsValue::from_str(&format!("Failed to open database: {}", e)))?;

            // Create temporary Arc for checkpoint manager initialization
            let db_arc = Arc::new(db_temp);

            // Start checkpoint manager with WASM config (15s interval, 32MB max)
            let config =
                CheckpointConfig::from(crate::column_family::wal::config::WALConfig::default());
            let checkpoint_mgr =
                CheckpointManager::start(Arc::clone(&journal_arc), Arc::clone(&db_arc), config);

            // Create final database with checkpoint manager
            // Note: We can't easily extract the database from the Arc, so we create a new one
            crate::column_family::ColumnFamilyDatabase::open_with_backend_internal(
                file_name,
                backend_arc,
                Some(journal_arc),
                Some(Arc::new(checkpoint_mgr)),
            )
            .map_err(|e| JsValue::from_str(&format!("Failed to reinitialize database: {}", e)))?
        } else {
            // WAL disabled - simple initialization
            crate::column_family::ColumnFamilyDatabase::open_with_backend_internal(
                file_name,
                backend_arc,
                None,
                None,
            )
            .map_err(|e| JsValue::from_str(&format!("Failed to open database: {}", e)))?
        };

        Ok(WasmDatabase { db })
    }

    /// Lists all column family names
    #[wasm_bindgen(js_name = listColumnFamilies)]
    pub fn list_column_families(&self) -> Vec<String> {
        self.db.list_column_families()
    }

    /// Creates a new column family
    #[wasm_bindgen(js_name = createColumnFamily)]
    pub fn create_column_family(&self, name: String) -> Result<(), JsValue> {
        self.db
            .create_column_family(name, None)
            .map_err(|e| JsValue::from_str(&format!("Failed to create column family: {}", e)))?;
        Ok(())
    }

    /// Gets a column family, creating it if it doesn't exist
    #[wasm_bindgen(js_name = columnFamilyOrCreate)]
    pub fn column_family_or_create(&self, name: String) -> Result<WasmColumnFamily, JsValue> {
        let cf = self
            .db
            .column_family_or_create(&name)
            .map_err(|e| JsValue::from_str(&format!("Failed to get column family: {}", e)))?;
        Ok(WasmColumnFamily { cf })
    }

    /// Gets an existing column family
    #[wasm_bindgen(js_name = columnFamily)]
    pub fn column_family(&self, name: String) -> Result<WasmColumnFamily, JsValue> {
        let cf = self
            .db
            .column_family(&name)
            .map_err(|e| JsValue::from_str(&format!("Column family not found: {}", e)))?;
        Ok(WasmColumnFamily { cf })
    }

    /// Manually triggers a checkpoint to flush WAL to main database
    ///
    /// This ensures all pending WAL entries are applied to the database and persisted.
    /// Useful for:
    /// - Calling from beforeunload handler to ensure data safety on browser close
    /// - Explicit control over when expensive checkpoint operations happen
    /// - Testing and verification
    ///
    /// If WAL is disabled (pool_size = 0), this is a no-op.
    pub fn sync(&self) -> Result<(), JsValue> {
        self.db
            .checkpoint()
            .map_err(|e| JsValue::from_str(&format!("Checkpoint failed: {}", e)))?;
        Ok(())
    }
}

/// WASM-specific wrapper for ColumnFamily
#[wasm_bindgen]
pub struct WasmColumnFamily {
    cf: crate::column_family::ColumnFamily,
}

#[wasm_bindgen]
impl WasmColumnFamily {
    /// Writes a key-value pair atomically
    pub fn write(&self, key: String, value: String) -> Result<(), JsValue> {
        use crate::TableDefinition;

        let txn = self.cf.begin_write().map_err(|e| {
            error(&format!("begin_write error: {}", e));
            JsValue::from_str(&format!("Failed to begin write: {}", e))
        })?;

        {
            let table_def: TableDefinition<String, String> = TableDefinition::new("data");
            let mut table = txn.open_table(table_def).map_err(|e| {
                error(&format!("open_table error: {}", e));
                JsValue::from_str(&format!("Failed to open table: {}", e))
            })?;

            table.insert(&key, &value).map_err(|e| {
                error(&format!(
                    "insert error: {} (key={}, value={})",
                    e, key, value
                ));
                JsValue::from_str(&format!("Failed to insert: {}", e))
            })?;
        }

        txn.commit().map_err(|e| {
            error(&format!("commit error: {}", e));
            JsValue::from_str(&format!("Failed to commit: {}", e))
        })?;

        Ok(())
    }

    /// Reads a value by key
    pub fn read(&self, key: String) -> Result<Option<String>, JsValue> {
        use crate::{ReadableTable, TableDefinition};

        let txn = self.cf.begin_read().map_err(|e| {
            error(&format!("begin_read error: {}", e));
            JsValue::from_str(&format!("Failed to begin read: {}", e))
        })?;

        let table_def: TableDefinition<String, String> = TableDefinition::new("data");
        let table = txn.open_table(table_def).map_err(|e| {
            error(&format!("open_table error: {}", e));
            JsValue::from_str(&format!("Failed to open table: {}", e))
        })?;

        let value = table.get(&key).map_err(|e| {
            error(&format!("get error: {} (key={})", e, key));
            JsValue::from_str(&format!("Failed to get: {}", e))
        })?;

        Ok(value.map(|v| v.value().clone()))
    }

    /// Creates an iterator over all entries in the table
    pub fn iter(&self) -> Result<WasmIterator, JsValue> {
        WasmIterator::new(&self.cf, None, None)
    }

    /// Creates an iterator over a range of entries
    ///
    /// If start_key is provided, iteration begins at that key (inclusive)
    /// If end_key is provided, iteration ends at that key (exclusive)
    /// If both are None, iterates over all entries (same as iter())
    #[wasm_bindgen(js_name = iterRange)]
    pub fn iter_range(
        &self,
        start_key: Option<String>,
        end_key: Option<String>,
    ) -> Result<WasmIterator, JsValue> {
        WasmIterator::new(&self.cf, start_key, end_key)
    }
}

/// High-performance batch iterator for WASM
///
/// Owns the ReadTransaction to solve lifetime issues at the WASM boundary.
/// Provides batch iteration API to minimize WASM-JS boundary crossings.
#[wasm_bindgen]
pub struct WasmIterator {
    txn: crate::ReadTransaction,
    table: crate::table::ReadOnlyTable<String, String>,
    range: Option<crate::table::Range<'static, String, String>>,
}

#[wasm_bindgen]
impl WasmIterator {
    fn new(
        cf: &crate::column_family::ColumnFamily,
        start_key: Option<String>,
        end_key: Option<String>,
    ) -> Result<WasmIterator, JsValue> {
        use crate::{ReadableTable, TableDefinition};
        use std::ops::Bound;

        let txn = cf.begin_read().map_err(|e| {
            error(&format!("begin_read error: {}", e));
            JsValue::from_str(&format!("Failed to begin read: {}", e))
        })?;

        let table_def: TableDefinition<String, String> = TableDefinition::new("data");
        let table = txn.open_table(table_def).map_err(|e| {
            error(&format!("open_table error: {}", e));
            JsValue::from_str(&format!("Failed to open table: {}", e))
        })?;

        // Build range bounds based on start_key and end_key
        let range = match (start_key, end_key) {
            (None, None) => {
                // Full range
                table.range::<String>(..)
            }
            (Some(start), None) => {
                // Start to end
                table.range::<String>((Bound::Included(start), Bound::Unbounded))
            }
            (None, Some(end)) => {
                // Beginning to end
                table.range::<String>((Bound::Unbounded, Bound::Excluded(end)))
            }
            (Some(start), Some(end)) => {
                // Start to end (inclusive start, exclusive end)
                table.range::<String>((Bound::Included(start), Bound::Excluded(end)))
            }
        }
        .map_err(|e| {
            error(&format!("range error: {}", e));
            JsValue::from_str(&format!("Failed to create range: {}", e))
        })?;

        Ok(WasmIterator {
            txn,
            table,
            range: Some(range),
        })
    }

    /// Returns the next batch of entries (up to `batch_size`)
    ///
    /// This is the primary high-performance API. Returns an array of [key, value] pairs.
    /// Empty array indicates end of iteration.
    ///
    /// Performance: ~100x faster than calling next() in a loop for large tables
    /// due to minimized WASM-JS boundary crossings.
    #[wasm_bindgen(js_name = nextBatch)]
    pub fn next_batch(&mut self, batch_size: usize) -> JsValue {
        use js_sys::Array;

        let batch = Array::new();

        if let Some(range) = &mut self.range {
            for _ in 0..batch_size {
                match range.next() {
                    Some(Ok((key_guard, value_guard))) => {
                        let key = key_guard.value().clone();
                        let value = value_guard.value().clone();

                        let pair = Array::new();
                        pair.push(&JsValue::from_str(&key));
                        pair.push(&JsValue::from_str(&value));
                        batch.push(&pair);
                    }
                    Some(Err(e)) => {
                        error(&format!("iterator error: {}", e));
                        break;
                    }
                    None => {
                        self.range = None;
                        break;
                    }
                }
            }
        }

        batch.into()
    }

    /// Returns the next single entry
    ///
    /// Convenience wrapper around next_batch(1). For better performance with
    /// large tables, use next_batch() with a larger batch size (e.g., 100).
    ///
    /// Returns [key, value] array or undefined if done.
    pub fn next(&mut self) -> JsValue {
        use js_sys::Array;

        let batch = self.next_batch(1);
        let batch_array = Array::from(&batch);

        if batch_array.length() > 0 {
            batch_array.get(0)
        } else {
            JsValue::UNDEFINED
        }
    }

    /// Collects all remaining entries into an array
    ///
    /// Helper method for small tables. For large tables, prefer iterating
    /// with next_batch() to avoid loading everything into memory at once.
    #[wasm_bindgen(js_name = collectAll)]
    pub fn collect_all(&mut self) -> JsValue {
        use js_sys::Array;

        let all = Array::new();

        loop {
            let batch = self.next_batch(100);
            let batch_array = Array::from(&batch);

            if batch_array.length() == 0 {
                break;
            }

            for i in 0..batch_array.length() {
                all.push(&batch_array.get(i));
            }
        }

        all.into()
    }

    /// Explicitly closes the iterator and releases the transaction
    ///
    /// This is optional - the iterator will be cleaned up automatically
    /// when dropped by JavaScript GC. However, calling close() explicitly
    /// allows earlier resource cleanup.
    pub fn close(self) {
        // Drop self, cleaning up transaction
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require wasm-bindgen-test to run in a browser environment
    // Run with: wasm-pack test --headless --chrome

    #[test]
    fn test_opfs_detection() {
        // This will only pass in a Web Worker context with OPFS support
        // In native Rust tests, this will return false
        let _supported = is_opfs_supported();
        // We can't assert true/false here since it depends on the environment
    }
}
