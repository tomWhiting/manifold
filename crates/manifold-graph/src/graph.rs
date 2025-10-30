//! Graph table implementation with bidirectional edge storage.

use crate::edge::{current_timestamp_nanos, Edge};
use manifold::{
    ReadOnlyTable, ReadTransaction, ReadableTable, ReadableTableMetadata, StorageError, Table,
    TableDefinition, TableError, WriteTransaction,
};
use uuid::Uuid;

/// A table storing graph edges with bidirectional indexes and temporal tracking.
///
/// This table maintains two internal tables (forward and reverse) to enable
/// efficient queries for both outgoing and incoming edges. Both tables are
/// updated atomically within the same write transaction.
///
/// Value tuple: (is_active, weight, created_at, deleted_at)
pub struct GraphTable<'txn> {
    forward: Table<'txn, (Uuid, &'static str, Uuid), (bool, f32, u64, Option<u64>)>,
    reverse: Table<'txn, (Uuid, &'static str, Uuid), (bool, f32, u64, Option<u64>)>,
}

impl<'txn> GraphTable<'txn> {
    /// Opens a graph table for writing.
    ///
    /// Creates two internal tables: `{name}_forward` and `{name}_reverse`.
    pub fn open(txn: &'txn WriteTransaction, name: &str) -> Result<Self, TableError> {
        let forward_name = format!("{name}_forward");
        let reverse_name = format!("{name}_reverse");

        let forward_def: TableDefinition<(Uuid, &str, Uuid), (bool, f32, u64, Option<u64>)> =
            TableDefinition::new(&forward_name);
        let reverse_def: TableDefinition<(Uuid, &str, Uuid), (bool, f32, u64, Option<u64>)> =
            TableDefinition::new(&reverse_name);

        let forward = txn.open_table(forward_def)?;
        let reverse = txn.open_table(reverse_def)?;

        Ok(Self { forward, reverse })
    }

    /// Adds an edge to the graph with optional timestamp.
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
    /// * `created_at` - Optional creation timestamp (uses current time if None)
    pub fn add_edge(
        &mut self,
        source: &Uuid,
        edge_type: &str,
        target: &Uuid,
        is_active: bool,
        weight: f32,
        created_at: Option<u64>,
    ) -> Result<(), TableError> {
        let timestamp = created_at.unwrap_or_else(current_timestamp_nanos);
        let properties = (is_active, weight, timestamp, None);

        // Insert into forward table: (source, edge_type, target) -> properties
        self.forward
            .insert(&(*source, edge_type, *target), &properties)?;

        // Insert into reverse table: (target, edge_type, source) -> properties
        self.reverse
            .insert(&(*target, edge_type, *source), &properties)?;

        Ok(())
    }

    /// Soft deletes an edge from the graph by setting deleted_at timestamp.
    ///
    /// The edge remains in storage for temporal queries but is marked as deleted.
    /// Updates both forward and reverse indexes atomically.
    pub fn remove_edge(
        &mut self,
        source: &Uuid,
        edge_type: &str,
        target: &Uuid,
    ) -> Result<(), StorageError> {
        // Get existing edge to preserve created_at
        let key = (*source, edge_type, *target);
        let edge_data = if let Some(guard) = self.forward.get(&key)? {
            let (is_active, weight, created_at, _) = guard.value();
            Some((is_active, weight, created_at))
        } else {
            None
        };

        if let Some((is_active, weight, created_at)) = edge_data {
            let deleted_at = Some(current_timestamp_nanos());
            let properties = (is_active, weight, created_at, deleted_at);

            // Update forward table with deleted_at
            self.forward.insert(&key, &properties)?;

            // Update reverse table with deleted_at
            self.reverse
                .insert(&(*target, edge_type, *source), &properties)?;
        }

        Ok(())
    }

    /// Hard deletes an edge from the graph, removing it entirely from storage.
    ///
    /// This permanently removes the edge and its history. Use remove_edge() for
    /// soft delete that preserves temporal history.
    pub fn hard_delete_edge(
        &mut self,
        source: &Uuid,
        edge_type: &str,
        target: &Uuid,
    ) -> Result<(), StorageError> {
        self.forward.remove(&(*source, edge_type, *target))?;
        self.reverse.remove(&(*target, edge_type, *source))?;
        Ok(())
    }

    /// Updates the properties of an existing edge while preserving timestamps.
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
        // Get existing edge to preserve created_at
        let key = (*source, edge_type, *target);
        let created_at = if let Some(guard) = self.forward.get(&key)? {
            let (_, _, timestamp, _) = guard.value();
            timestamp
        } else {
            current_timestamp_nanos()
        };

        // Use add_edge with preserved created_at
        self.add_edge(source, edge_type, target, is_active, weight, Some(created_at))
    }

    /// Adds multiple edges to the graph in a single batch operation.
    ///
    /// This method leverages Manifold's bulk insertion API for improved throughput,
    /// especially beneficial when loading large graphs. Both forward and reverse
    /// indexes are updated atomically within the same transaction.
    ///
    /// # Arguments
    ///
    /// * `edges` - Vector of edge tuples: (source, `edge_type`, target, `is_active`, `weight`, `created_at`)
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
    /// let now = 1234567890;
    ///
    /// let edges = vec![
    ///     (u1, "follows", u2, true, 1.0, now),
    ///     (u1, "follows", u3, true, 0.8, now),
    ///     (u2, "follows", u3, true, 0.9, now),
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
        edges: &[(Uuid, &str, Uuid, bool, f32, u64)],
        sorted: bool,
    ) -> Result<usize, StorageError> {
        // Prepare forward table items: (source, edge_type, target) -> (is_active, weight, created_at, deleted_at)
        let forward_items: Vec<((Uuid, &str, Uuid), (bool, f32, u64, Option<u64>))> = edges
            .iter()
            .map(|(source, edge_type, target, is_active, weight, created_at)| {
                (
                    (*source, *edge_type, *target),
                    (*is_active, *weight, *created_at, None),
                )
            })
            .collect();

        // Prepare reverse table items: (target, edge_type, source) -> (is_active, weight, created_at, deleted_at)
        let reverse_items: Vec<((Uuid, &str, Uuid), (bool, f32, u64, Option<u64>))> = edges
            .iter()
            .map(|(source, edge_type, target, is_active, weight, created_at)| {
                (
                    (*target, *edge_type, *source),
                    (*is_active, *weight, *created_at, None),
                )
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

/// Read-only graph table providing efficient edge traversal with temporal support.
pub struct GraphTableRead {
    forward: ReadOnlyTable<(Uuid, &'static str, Uuid), (bool, f32, u64, Option<u64>)>,
    reverse: ReadOnlyTable<(Uuid, &'static str, Uuid), (bool, f32, u64, Option<u64>)>,
}

impl GraphTableRead {
    /// Opens a graph table for reading.
    pub fn open(txn: &ReadTransaction, name: &str) -> Result<Self, StorageError> {
        let forward_name = format!("{name}_forward");
        let reverse_name = format!("{name}_reverse");

        let forward_def: TableDefinition<(Uuid, &str, Uuid), (bool, f32, u64, Option<u64>)> =
            TableDefinition::new(&forward_name);
        let reverse_def: TableDefinition<(Uuid, &str, Uuid), (bool, f32, u64, Option<u64>)> =
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

    /// Retrieves a specific edge by source, edge_type, and target.
    ///
    /// Returns None if the edge doesn't exist or has been soft-deleted.
    /// Use get_edge_at() for temporal queries.
    pub fn get_edge(
        &self,
        source: &Uuid,
        edge_type: &str,
        target: &Uuid,
    ) -> Result<Option<Edge>, StorageError> {
        Ok(self
            .forward
            .get(&(*source, edge_type, *target))?
            .and_then(|guard| {
                let (is_active, weight, created_at, deleted_at) = guard.value();
                // Only return if not deleted
                if deleted_at.is_none() {
                    Some(Edge::with_timestamps(
                        *source,
                        edge_type,
                        *target,
                        is_active,
                        weight,
                        created_at,
                        deleted_at,
                    ))
                } else {
                    None
                }
            }))
    }

    /// Retrieves a specific edge at a given timestamp.
    ///
    /// Returns the edge if it existed at the specified timestamp (created_at <= timestamp
    /// and either not deleted or deleted_at > timestamp).
    pub fn get_edge_at(
        &self,
        source: &Uuid,
        edge_type: &str,
        target: &Uuid,
        timestamp: u64,
    ) -> Result<Option<Edge>, StorageError> {
        Ok(self
            .forward
            .get(&(*source, edge_type, *target))?
            .and_then(|guard| {
                let (is_active, weight, created_at, deleted_at) = guard.value();
                let edge = Edge::with_timestamps(
                    *source,
                    edge_type,
                    *target,
                    is_active,
                    weight,
                    created_at,
                    deleted_at,
                );

                if edge.is_active_at(timestamp) {
                    Some(edge)
                } else {
                    None
                }
            }))
    }

    /// Returns an iterator over all outgoing edges from the given source vertex.
    ///
    /// By default, excludes soft-deleted edges. Use outgoing_edges_with_deleted() to include them.
    pub fn outgoing_edges(&self, source: &Uuid) -> Result<OutgoingEdgeIter<'_>, StorageError> {
        // Range from (source, "", nil_uuid) to (source, max_str, max_uuid)
        let start = (*source, "", Uuid::nil());
        let end = (*source, "\u{FFFF}", Uuid::max());

        Ok(OutgoingEdgeIter {
            inner: self.forward.range(start..end)?,
            include_deleted: false,
        })
    }

    /// Returns an iterator over all outgoing edges including soft-deleted ones.
    pub fn outgoing_edges_with_deleted(
        &self,
        source: &Uuid,
    ) -> Result<OutgoingEdgeIter<'_>, StorageError> {
        let start = (*source, "", Uuid::nil());
        let end = (*source, "\u{FFFF}", Uuid::max());

        Ok(OutgoingEdgeIter {
            inner: self.forward.range(start..end)?,
            include_deleted: true,
        })
    }

    /// Returns an iterator over all incoming edges to the given target vertex.
    ///
    /// By default, excludes soft-deleted edges. Use incoming_edges_with_deleted() to include them.
    pub fn incoming_edges(&self, target: &Uuid) -> Result<IncomingEdgeIter<'_>, StorageError> {
        // Range from (target, "", nil_uuid) to (target, max_str, max_uuid)
        let start = (*target, "", Uuid::nil());
        let end = (*target, "\u{FFFF}", Uuid::max());

        Ok(IncomingEdgeIter {
            inner: self.reverse.range(start..end)?,
            include_deleted: false,
        })
    }

    /// Returns an iterator over all incoming edges including soft-deleted ones.
    pub fn incoming_edges_with_deleted(
        &self,
        target: &Uuid,
    ) -> Result<IncomingEdgeIter<'_>, StorageError> {
        let start = (*target, "", Uuid::nil());
        let end = (*target, "\u{FFFF}", Uuid::max());

        Ok(IncomingEdgeIter {
            inner: self.reverse.range(start..end)?,
            include_deleted: true,
        })
    }

    /// Returns an iterator over all edges in the graph.
    ///
    /// By default, excludes soft-deleted edges. Use all_edges_with_deleted() to include them.
    pub fn all_edges(&self) -> Result<AllEdgesIter<'_>, StorageError> {
        Ok(AllEdgesIter {
            inner: self.forward.iter()?,
            include_deleted: false,
        })
    }

    /// Returns an iterator over all edges including soft-deleted ones.
    pub fn all_edges_with_deleted(&self) -> Result<AllEdgesIter<'_>, StorageError> {
        Ok(AllEdgesIter {
            inner: self.forward.iter()?,
            include_deleted: true,
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
///
/// By default, only returns non-deleted edges. Use all_edges_with_deleted() to include soft-deleted edges.
pub struct OutgoingEdgeIter<'a> {
    inner: manifold::Range<'a, (Uuid, &'static str, Uuid), (bool, f32, u64, Option<u64>)>,
    include_deleted: bool,
}

impl Iterator for OutgoingEdgeIter<'_> {
    type Item = Result<Edge, StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let result = self.inner.next()?;

            match result {
                Ok((key_guard, value_guard)) => {
                    let (source, edge_type, target) = key_guard.value();
                    let (is_active, weight, created_at, deleted_at) = value_guard.value();

                    // Skip deleted edges unless include_deleted is true
                    if !self.include_deleted && deleted_at.is_some() {
                        continue;
                    }

                    return Some(Ok(Edge::with_timestamps(
                        source,
                        edge_type,
                        target,
                        is_active,
                        weight,
                        created_at,
                        deleted_at,
                    )));
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

/// Iterator over all edges in the graph.
///
/// By default, only returns non-deleted edges.
pub struct AllEdgesIter<'a> {
    inner: manifold::Range<'a, (Uuid, &'static str, Uuid), (bool, f32, u64, Option<u64>)>,
    include_deleted: bool,
}

impl Iterator for AllEdgesIter<'_> {
    type Item = Result<Edge, StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let result = self.inner.next()?;

            match result {
                Ok((key_guard, value_guard)) => {
                    let (source, edge_type, target) = key_guard.value();
                    let (is_active, weight, created_at, deleted_at) = value_guard.value();

                    // Skip deleted edges unless include_deleted is true
                    if !self.include_deleted && deleted_at.is_some() {
                        continue;
                    }

                    return Some(Ok(Edge::with_timestamps(
                        source,
                        edge_type,
                        target,
                        is_active,
                        weight,
                        created_at,
                        deleted_at,
                    )));
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

/// Iterator over incoming edges to a target vertex.
///
/// By default, only returns non-deleted edges.
pub struct IncomingEdgeIter<'a> {
    inner: manifold::Range<'a, (Uuid, &'static str, Uuid), (bool, f32, u64, Option<u64>)>,
    include_deleted: bool,
}

impl Iterator for IncomingEdgeIter<'_> {
    type Item = Result<Edge, StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let result = self.inner.next()?;

            match result {
                Ok((key_guard, value_guard)) => {
                    let (target, edge_type, source) = key_guard.value();
                    let (is_active, weight, created_at, deleted_at) = value_guard.value();

                    // Skip deleted edges unless include_deleted is true
                    if !self.include_deleted && deleted_at.is_some() {
                        continue;
                    }

                    // Note: In reverse table, first UUID is target, third is source
                    return Some(Ok(Edge::with_timestamps(
                        source,
                        edge_type,
                        target,
                        is_active,
                        weight,
                        created_at,
                        deleted_at,
                    )));
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}
