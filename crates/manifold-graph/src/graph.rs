//! Graph table implementation with bidirectional edge storage.

use crate::edge::Edge;
use manifold::{
    ReadOnlyTable, ReadTransaction, ReadableTable, ReadableTableMetadata, StorageError, Table,
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
        let forward_name = format!("{name}_forward");
        let reverse_name = format!("{name}_reverse");

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
        self.forward
            .insert(&(*source, edge_type, *target), &properties)?;

        // Insert into reverse table: (target, edge_type, source) -> properties
        self.reverse
            .insert(&(*target, edge_type, *source), &properties)?;

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

    /// Adds multiple edges to the graph in a single batch operation.
    ///
    /// This method leverages Manifold's bulk insertion API for improved throughput,
    /// especially beneficial when loading large graphs. Both forward and reverse
    /// indexes are updated atomically within the same transaction.
    ///
    /// # Arguments
    ///
    /// * `edges` - Vector of edge tuples: (source, `edge_type`, target, `is_active`, `weight`)
    /// * `sorted` - Whether the input is pre-sorted by (source, `edge_type`, target).
    ///   Set to `true` if your data is already sorted for best performance.
    ///
    /// # Returns
    ///
    /// Returns the number of edges inserted.
    ///
    /// # Performance
    ///
    /// - Sorted data (`sorted = true`): Uses optimized insertion with minimal tree rebalancing
    /// - Unsorted data (`sorted = false`): Chunks and sorts data internally for good performance
    /// - Batch operations benefit from WAL group commit for higher throughput
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use manifold::column_family::ColumnFamilyDatabase;
    /// # use manifold_graph::GraphTable;
    /// # use uuid::Uuid;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let db = ColumnFamilyDatabase::open("test.db")?;
    /// # let cf = db.column_family_or_create("graph")?;
    /// # let write_txn = cf.begin_write()?;
    /// # let mut graph = GraphTable::open(&write_txn, "edges")?;
    /// let u1 = Uuid::new_v4();
    /// let u2 = Uuid::new_v4();
    /// let u3 = Uuid::new_v4();
    ///
    /// let edges = vec![
    ///     (u1, "follows", u2, true, 1.0),
    ///     (u1, "follows", u3, true, 0.8),
    ///     (u2, "follows", u3, true, 0.9),
    /// ];
    ///
    /// let count = graph.add_edges_batch(&edges, false)?;
    /// assert_eq!(count, 3);
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::type_complexity)]
    pub fn add_edges_batch(
        &mut self,
        edges: &[(Uuid, &str, Uuid, bool, f32)],
        sorted: bool,
    ) -> Result<usize, StorageError> {
        // Prepare forward table items: (source, edge_type, target) -> (is_active, weight)
        let forward_items: Vec<((Uuid, &str, Uuid), (bool, f32))> = edges
            .iter()
            .map(|(source, edge_type, target, is_active, weight)| {
                ((*source, *edge_type, *target), (*is_active, *weight))
            })
            .collect();

        // Prepare reverse table items: (target, edge_type, source) -> (is_active, weight)
        let reverse_items: Vec<((Uuid, &str, Uuid), (bool, f32))> = edges
            .iter()
            .map(|(source, edge_type, target, is_active, weight)| {
                ((*target, *edge_type, *source), (*is_active, *weight))
            })
            .collect();

        // Note: reverse items are NOT sorted even if forward items are,
        // so we always use sorted=false for reverse table
        let count = self.forward.insert_bulk(forward_items, sorted)?;

        self.reverse.insert_bulk(reverse_items, false)?;

        Ok(count)
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
        let forward_name = format!("{name}_forward");
        let reverse_name = format!("{name}_reverse");

        let forward_def: TableDefinition<(Uuid, &str, Uuid), (bool, f32)> =
            TableDefinition::new(&forward_name);
        let reverse_def: TableDefinition<(Uuid, &str, Uuid), (bool, f32)> =
            TableDefinition::new(&reverse_name);

        let forward = txn.open_table(forward_def).map_err(|e| match e {
            TableError::Storage(s) => s,
            _ => StorageError::Io(std::io::Error::other(e)),
        })?;

        let reverse = txn.open_table(reverse_def).map_err(|e| match e {
            TableError::Storage(s) => s,
            _ => StorageError::Io(std::io::Error::other(e)),
        })?;

        Ok(Self { forward, reverse })
    }

    /// Retrieves a specific edge by source, `edge_type`, and target.
    pub fn get_edge(
        &self,
        source: &Uuid,
        edge_type: &str,
        target: &Uuid,
    ) -> Result<Option<Edge>, StorageError> {
        Ok(self
            .forward
            .get(&(*source, edge_type, *target))?
            .map(|guard| {
                let (is_active, weight) = guard.value();
                Edge::new(*source, edge_type, *target, is_active, weight)
            }))
    }

    /// Returns an iterator over all outgoing edges from the given source vertex.
    ///
    /// Uses a range scan on the forward table for efficiency.
    pub fn outgoing_edges(&self, source: &Uuid) -> Result<OutgoingEdgeIter<'_>, StorageError> {
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
    pub fn incoming_edges(&self, target: &Uuid) -> Result<IncomingEdgeIter<'_>, StorageError> {
        // Range from (target, "", nil_uuid) to (target, max_str, max_uuid)
        let start = (*target, "", Uuid::nil());
        let end = (*target, "\u{FFFF}", Uuid::max());

        Ok(IncomingEdgeIter {
            inner: self.reverse.range(start..end)?,
        })
    }

    /// Returns an iterator over all edges in the graph.
    ///
    /// This method iterates over the forward table only to avoid returning
    /// duplicate edges. Use this for full-graph traversal or when you need
    /// to process every edge exactly once.
    pub fn all_edges(&self) -> Result<AllEdgesIter<'_>, StorageError> {
        Ok(AllEdgesIter {
            inner: self.forward.iter()?,
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

impl Iterator for OutgoingEdgeIter<'_> {
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

/// Iterator over all edges in the graph.
pub struct AllEdgesIter<'a> {
    inner: manifold::Range<'a, (Uuid, &'static str, Uuid), (bool, f32)>,
}

impl Iterator for AllEdgesIter<'_> {
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

impl Iterator for IncomingEdgeIter<'_> {
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
