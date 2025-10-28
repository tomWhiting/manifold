//! Edge types and property access guards.

use manifold::AccessGuard;
use std::ops::Deref;
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

/// A guard providing access to edge properties.
///
/// The properties are deserialized once when the guard is created,
/// then cached for subsequent accesses. This follows the same pattern
/// as `VectorGuard` from manifold-vectors.
pub struct EdgeGuard<'a> {
    properties_cached: (bool, f32),
    _guard: AccessGuard<'a, (bool, f32)>,
}

impl<'a> EdgeGuard<'a> {
    pub(crate) fn new(guard: AccessGuard<'a, (bool, f32)>) -> Self {
        let properties_cached = guard.value();
        Self {
            properties_cached,
            _guard: guard,
        }
    }

    /// Returns whether this edge is active
    pub fn is_active(&self) -> bool {
        self.properties_cached.0
    }

    /// Returns the edge weight
    pub fn weight(&self) -> f32 {
        self.properties_cached.1
    }

    /// Returns the properties as a tuple
    pub fn properties(&self) -> (bool, f32) {
        self.properties_cached
    }
}

impl<'a> Deref for EdgeGuard<'a> {
    type Target = (bool, f32);

    fn deref(&self) -> &Self::Target {
        &self.properties_cached
    }
}
