//! Edge types for graph storage.

use uuid::Uuid;

/// An edge in the graph with properties.
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
}

impl Edge {
    /// Creates a new edge
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
        }
    }
}
