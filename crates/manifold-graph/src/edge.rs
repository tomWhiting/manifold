//! Edge types for graph storage.

use uuid::Uuid;

/// An edge in the graph with properties and temporal tracking.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Source vertex ID
    pub source: Uuid,
    /// Edge type (e.g., "follows", "knows", "contains")
    pub edge_type: String,
    /// Target vertex ID
    pub target: Uuid,
    /// Whether this edge is active (vs passive/hidden/deleted)
    pub is_active: bool,
    /// Edge weight or score
    pub weight: f32,
    /// Creation timestamp in nanoseconds since Unix epoch
    pub created_at: u64,
    /// Deletion timestamp in nanoseconds since Unix epoch (None if not deleted)
    pub deleted_at: Option<u64>,
}

impl Edge {
    /// Creates a new edge with current timestamp
    pub fn new(
        source: Uuid,
        edge_type: impl Into<String>,
        target: Uuid,
        is_active: bool,
        weight: f32,
    ) -> Self {
        Self {
            source,
            edge_type: edge_type.into(),
            target,
            is_active,
            weight,
            created_at: current_timestamp_nanos(),
            deleted_at: None,
        }
    }

    /// Creates a new edge with explicit timestamps
    pub fn with_timestamps(
        source: Uuid,
        edge_type: impl Into<String>,
        target: Uuid,
        is_active: bool,
        weight: f32,
        created_at: u64,
        deleted_at: Option<u64>,
    ) -> Self {
        Self {
            source,
            edge_type: edge_type.into(),
            target,
            is_active,
            weight,
            created_at,
            deleted_at,
        }
    }

    /// Checks if this edge was active at the given timestamp
    pub fn is_active_at(&self, timestamp: u64) -> bool {
        self.created_at <= timestamp
            && self.deleted_at.map_or(true, |deleted| deleted > timestamp)
    }
}

/// Returns the current timestamp in nanoseconds since Unix epoch
pub fn current_timestamp_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System time before Unix epoch")
        .as_nanos() as u64
}
