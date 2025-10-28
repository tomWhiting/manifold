//! Integration traits for external graph algorithm libraries.

use crate::{AllEdgesIter, Edge, GraphTableRead};
use manifold::StorageError;

/// Trait for edge sources consumable by graph algorithm libraries.
///
/// This trait enables external graph algorithm libraries (BFS, DFS, PageRank, etc.)
/// to efficiently iterate over all edges in the graph.
pub trait EdgeSource {
    /// Iterator type over edges
    type Iter<'a>: Iterator<Item = Result<Edge, StorageError>>
    where
        Self: 'a;

    /// Returns an iterator over all edges in the graph.
    ///
    /// The iterator provides access to all edges with their properties.
    fn iter_edges(&self) -> Result<Self::Iter<'_>, StorageError>;

    /// Returns the number of edges.
    fn edge_count(&self) -> Result<u64, StorageError>;

    /// Returns true if empty.
    fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.edge_count()? == 0)
    }
}

impl EdgeSource for GraphTableRead {
    type Iter<'a>
        = AllEdgesIter<'a>
    where
        Self: 'a;

    fn iter_edges(&self) -> Result<Self::Iter<'_>, StorageError> {
        self.iter()
    }

    fn edge_count(&self) -> Result<u64, StorageError> {
        self.len()
    }
}
