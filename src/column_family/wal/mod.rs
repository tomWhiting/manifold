// Write-Ahead Log implementation for fast + durable writes
//
// The WAL provides:
// - Fast append-only writes with fsync (~0.5ms vs ~5ms for full B-tree sync)
// - Full crash recovery with CRC32 validation
// - Background checkpointing to apply WAL entries to main database
// - 200-250x performance improvement over default durability

pub mod checkpoint;
pub mod config;
pub mod entry;
pub mod journal;

pub use self::config::WALConfig;
