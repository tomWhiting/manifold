use std::time::Duration;

/// Configuration for the Write-Ahead Log system.
#[derive(Debug, Clone)]
pub struct WALConfig {
    /// How often to checkpoint the WAL to the main database.
    ///
    /// Default: 60 seconds
    pub checkpoint_interval: Duration,

    /// Maximum size of the WAL file before triggering a checkpoint.
    ///
    /// Default: 64 MB
    pub max_wal_size: u64,
}

impl Default for WALConfig {
    fn default() -> Self {
        Self {
            checkpoint_interval: Duration::from_secs(60),
            max_wal_size: 64 * 1024 * 1024, // 64 MB
        }
    }
}

/// Configuration for the checkpoint manager.
#[derive(Debug, Clone)]
pub struct CheckpointConfig {
    /// Time-based checkpoint trigger.
    pub interval: Duration,

    /// Size-based checkpoint trigger.
    pub max_wal_size: u64,
}

impl From<WALConfig> for CheckpointConfig {
    fn from(config: WALConfig) -> Self {
        Self {
            interval: config.checkpoint_interval,
            max_wal_size: config.max_wal_size,
        }
    }
}
