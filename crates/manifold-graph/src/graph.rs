//! Graph table implementation with bidirectional edge storage.

use crate::edge::Edge;
use manifold::{
    ReadOnlyTable, ReadTransaction, ReadableTableMetadata, StorageError, Table,
    TableDefinition, TableError, WriteTransaction,
};
use uuid::Uuid;

/// A table storing graph edges with bidirectional indexes.
///
/// This table maintains two internal tables (forward and reverse) to enable
/// efficient queries for both outgoing and incoming edges. Both tables are
/// updated atomically within the same write transaction.
pub struct GraphTable<'txn> {
    forward: Table<'txn, (Uuid, &'static str, Uuid), (bool, f32)>,
    reverse: Table<'txn, (Uuid, &'static str, Uuid), (bool, f32)>,
}

impl<'txn> GraphTable<'txn> {
    /// Opens a graph table for writing.
    ///
    /// Creates two internal tables: `{name}_forward` and `{name}_reverse`.
    pub fn open(txn: &'txn WriteTransaction, name: &str) -> Result<Self, TableError> {
        let forward_name = format!("{}_forward", name);
        let reverse_name = format!("{}_reverse", name);

        let forward_def: TableDefinition<(Uuid, &str, Uuid), (bool, f32)> =
            TableDefinition::new(&forward_name);
        let reverse_def: TableDefinition<(Uuid, &str, Uuid), (bool, f32)> =
            TableDefinition::new(&reverse_name);

        let forward = txn.open_table(forward_def)?;
        let reverse = txn.open_table(reverse_def)?;

        Ok(Self { forward, reverse })
    }

    /// Adds an edge to the graph.
    ///
    /// Updates both forward and reverse indexes atomically.
    ///
    /// # Arguments
    ///
    /// * `source` - Source vertex UUID
    /// * `edge_type` - Edge type (e.g., "follows", "knows")
    /// * `target` - Target vertex UUID
    /// * `is_active` - Whether the edge is active
    /// * `weight` - Edge weight/score
    pub fn add_edge(
        &mut self,
        source: &Uuid,
        edge_type: &str,
        target: &Uuid,
        is_active: bool,
        weight: f32,
    ) -> Result<(), TableError> {
        let properties = (is_active, weight);

        // Insert into forward table: (source, edge_type, target) -> properties
        self.forward.insert(&(*source, edge_type, *target), &properties)?;

        // Insert into reverse table: (target, edge_type, source) -> properties
        self.reverse.insert(&(*target, edge_type, *source), &properties)?;

        Ok(())
    }

    /// Removes an edge from the graph.
    ///
    /// Removes from both forward and reverse indexes atomically.
    pub fn remove_edge(
        &mut self,
        source: &Uuid,
        edge_type: &str,
        target: &Uuid,
    ) -> Result<(), StorageError> {
        self.forward.remove(&(*source, edge_type, *target))?;
        self.reverse.remove(&(*target, edge_type, *source))?;
        Ok(())
    }

    /// Updates the properties of an existing edge.
    ///
    /// Updates both forward and reverse indexes atomically.
    pub fn update_edge(
        &mut self,
        source: &Uuid,
        edge_type: &str,
        target: &Uuid,
        is_active: bool,
        weight: f32,
    ) -> Result<(), TableError> {
        // Since we're updating both properties, just use add_edge which overwrites
        self.add_edge(source, edge_type, target, is_active, weight)
    }

    /// Returns the number of edges in the forward table.
    pub fn len(&self) -> Result<u64, StorageError> {
        self.forward.len()
    }

    /// Returns `true` if the table contains no edges.
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }
}

/// Read-only graph table providing efficient edge traversal.
pub struct GraphTableRead {
    forward: ReadOnlyTable<(Uuid, &'static str, Uuid), (bool, f32)>,
    reverse: ReadOnlyTable<(Uuid, &'static str, Uuid), (bool, f32)>,
}

impl GraphTableRead {
    /// Opens a graph table for reading.
    pub fn open(txn: &ReadTransaction, name: &str) -> Result<Self, StorageError> {
        let forward_name = format!("{}_forward", name);
        let reverse_name = format!("{}_reverse", name);

        let forward_def: TableDefinition<(Uuid, &str, Uuid), (bool, f32)> =
            TableDefinition::new(&forward_name);
        let reverse_def: TableDefinition<(Uuid, &str, Uuid), (bool, f32)> =
            TableDefinition::new(&reverse_name);

        let forward = txn.open_table(forward_def).map_err(|e| match e {
            TableError::Storage(s) => s,
            _ => StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)),
        })?;

        let reverse = txn.open_table(reverse_def).map_err(|e| match e {
            TableError::Storage(s) => s,
            _ => StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)),
        })?;

        Ok(Self { forward, reverse })
    }

    /// Retrieves a specific edge by source, edge_type, and target.
    pub fn get_edge(
        &self,
        source: &Uuid,
        edge_type: &str,
        target: &Uuid,
    ) -> Result<Option<Edge>, StorageError> {
        Ok(self.forward.get(&(*source, edge_type, *target))?.map(|guard| {
            let (is_active, weight) = guard.value();
            Edge::new(*source, edge_type, *target, is_active, weight)
        }))
    }

    /// Returns an iterator over all outgoing edges from the given source vertex.
    ///
    /// Uses a range scan on the forward table for efficiency.
    pub fn outgoing_edges(
        &self,
        source: &Uuid,
    ) -> Result<OutgoingEdgeIter<'_>, StorageError> {
        // Range from (source, "", nil_uuid) to (source, max_str, max_uuid)
        let start = (*source, "", Uuid::nil());
        let end = (*source, "\u{FFFF}", Uuid::max());

        Ok(OutgoingEdgeIter {
            inner: self.forward.range(start..end)?,
        })
    }

    /// Returns an iterator over all incoming edges to the given target vertex.
    ///
    /// Uses a range scan on the reverse table for efficiency.
    pub fn incoming_edges(
        &self,
        target: &Uuid,
    ) -> Result<IncomingEdgeIter<'_>, StorageError> {
        // Range from (target, "", nil_uuid) to (target, max_str, max_uuid)
        let start = (*target, "", Uuid::nil());
        let end = (*target, "\u{FFFF}", Uuid::max());

        Ok(IncomingEdgeIter {
            inner: self.reverse.range(start..end)?,
        })
    }

    /// Returns the number of edges stored in this table.
    pub fn len(&self) -> Result<u64, StorageError> {
        self.forward.len()
    }

    /// Returns `true` if the table contains no edges.
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }
}

/// Iterator over outgoing edges from a source vertex.
pub struct OutgoingEdgeIter<'a> {
    inner: manifold::Range<'a, (Uuid, &'static str, Uuid), (bool, f32)>,
}

impl<'a> Iterator for OutgoingEdgeIter<'a> {
    type Item = Result<Edge, StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|result| {
            result.map(|(key_guard, value_guard)| {
                let (source, edge_type, target) = key_guard.value();
                let (is_active, weight) = value_guard.value();
                Edge::new(source, edge_type, target, is_active, weight)
            })
        })
    }
}

/// Iterator over incoming edges to a target vertex.
pub struct IncomingEdgeIter<'a> {
    inner: manifold::Range<'a, (Uuid, &'static str, Uuid), (bool, f32)>,
}

impl<'a> Iterator for IncomingEdgeIter<'a> {
    type Item = Result<Edge, StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|result| {
            result.map(|(key_guard, value_guard)| {
                let (target, edge_type, source) = key_guard.value();
                let (is_active, weight) = value_guard.value();
                // Note: In reverse table, first UUID is target, third is source
                Edge::new(source, edge_type, target, is_active, weight)
            })
        })
    }
}
