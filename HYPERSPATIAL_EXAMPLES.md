# Hyperspatial Redesign - Code Examples

**Date:** 2024-10-29  
**Parent Document:** [HYPERSPATIAL_REDESIGN.md](./HYPERSPATIAL_REDESIGN.md)

This document provides detailed code examples showing how the redesigned Hyperspatial ecosystem uses Manifold's storage primitives.

---

## Table of Contents

1. [UUID Keys and Fixed-Width Storage](#uuid-keys-and-fixed-width-storage)
2. [Vector Storage with Multiple Named Vectors](#vector-storage-with-multiple-named-vectors)
3. [Trajectory Storage with Custom Types](#trajectory-storage-with-custom-types)
4. [Graph Storage with Simplified Edge Properties](#graph-storage-with-simplified-edge-properties)
5. [Compiled Query Examples](#compiled-query-examples)
6. [Multi-Modal Positioning](#multi-modal-positioning)
7. [Cascade Integration](#cascade-integration)
8. [Stream Workflows](#stream-workflows)

---

## UUID Keys and Fixed-Width Storage

### Entity Storage with UUID Keys

```rust
use manifold::column_family::ColumnFamilyDatabase;
use manifold::{TableDefinition, ReadableTable};
use uuid::Uuid;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct EntityData {
    name: String,
    entity_type: String,
    created_at: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open database
    let db = ColumnFamilyDatabase::builder()
        .open("hyperspatial.manifold")?;
    
    // Get or create entities column family
    let entities_cf = db.column_family_or_create("entities")?;
    
    // Create entity with UUID key
    let entity_id = Uuid::new_v4();
    let entity_data = EntityData {
        name: "Document 1".to_string(),
        entity_type: "Document".to_string(),
        created_at: 1698765432000,
    };
    
    // Store entity - UUID is 16 bytes fixed-width
    {
        let write_txn = entities_cf.begin_write()?;
        let mut table: Table<[u8; 16], Vec<u8>> = write_txn.open_table(
            TableDefinition::new("entities")
        )?;
        
        let serialized = bincode::serialize(&entity_data)?;
        table.insert(entity_id.as_bytes(), &serialized)?;
        
        drop(table);
        write_txn.commit()?;
    }
    
    // Retrieve entity - zero-copy key access
    {
        let read_txn = entities_cf.begin_read()?;
        let table = read_txn.open_table(
            TableDefinition::<&[u8; 16], &[u8]>::new("entities")
        )?;
        
        if let Some(guard) = table.get(entity_id.as_bytes())? {
            let data: EntityData = bincode::deserialize(guard.value())?;
            println!("Entity: {:?}", data);
        }
    }
    
    Ok(())
}
```

### Composite Keys with UUID Components

```rust
use uuid::Uuid;

// Property key: (entity_uuid, property_name)
type PropertyKey = ([u8; 16], String);

fn store_property(
    cf: &ColumnFamily,
    entity_id: Uuid,
    name: &str,
    value: &PropertyValue,
) -> Result<(), Error> {
    let write_txn = cf.begin_write()?;
    let mut table = write_txn.open_table(
        TableDefinition::<PropertyKey, Vec<u8>>::new("properties")
    )?;
    
    let key = (*entity_id.as_bytes(), name.to_string());
    let serialized = bincode::serialize(value)?;
    
    table.insert(&key, &serialized)?;
    drop(table);
    write_txn.commit()?;
    
    Ok(())
}
```

---

## Vector Storage with Multiple Named Vectors

### Separate Tables per Vector Type

```rust
use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::{VectorTable, VectorTableRead, VectorGuard};
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = ColumnFamilyDatabase::builder()
        .open("vectors.manifold")?;
    
    // Create separate column families for different vector types
    let semantic_cf = db.column_family_or_create("semantic_vectors")?;
    let code_cf = db.column_family_or_create("code_vectors")?;
    let sparse_cf = db.column_family_or_create("sparse_vectors")?;
    
    let entity_id = Uuid::new_v4();
    let entity_key = entity_id.to_string();
    
    // Store semantic embedding (768D)
    {
        let write_txn = semantic_cf.begin_write()?;
        let mut table = VectorTable::<768>::open(&write_txn, "vectors")?;
        
        let semantic_embedding: [f32; 768] = [0.1; 768]; // Example
        table.insert(&entity_key, &semantic_embedding)?;
        
        drop(table);
        write_txn.commit()?;
    }
    
    // Store code embedding (512D) - same entity_id, different table
    {
        let write_txn = code_cf.begin_write()?;
        let mut table = VectorTable::<512>::open(&write_txn, "vectors")?;
        
        let code_embedding: [f32; 512] = [0.2; 512]; // Example
        table.insert(&entity_key, &code_embedding)?;
        
        drop(table);
        write_txn.commit()?;
    }
    
    // Retrieve both embeddings for same entity
    {
        let read_txn_semantic = semantic_cf.begin_read()?;
        let semantic_table = VectorTableRead::<768>::open(&read_txn_semantic, "vectors")?;
        
        let read_txn_code = code_cf.begin_read()?;
        let code_table = VectorTableRead::<512>::open(&read_txn_code, "vectors")?;
        
        if let Some(semantic_guard) = semantic_table.get(&entity_key)? {
            println!("Semantic embedding dimension: {}", semantic_guard.value().len());
        }
        
        if let Some(code_guard) = code_table.get(&entity_key)? {
            println!("Code embedding dimension: {}", code_guard.value().len());
        }
    }
    
    Ok(())
}
```

### Vector Registry for Dynamic Access

```rust
use std::collections::HashMap;
use std::sync::Arc;

pub struct VectorRegistry {
    // Map vector name to column family and dimension
    registrations: HashMap<String, VectorTableInfo>,
    db: Arc<ColumnFamilyDatabase>,
}

enum VectorTableInfo {
    Dense { cf_name: String, dimension: usize },
    Sparse { cf_name: String },
    Multi { cf_name: String, dimension: usize },
}

impl VectorRegistry {
    pub fn new(db: Arc<ColumnFamilyDatabase>) -> Self {
        Self {
            registrations: HashMap::new(),
            db,
        }
    }
    
    pub fn register_dense_vectors(
        &mut self,
        name: &str,
        dimension: usize,
    ) -> Result<(), Error> {
        self.registrations.insert(
            name.to_string(),
            VectorTableInfo::Dense {
                cf_name: format!("{}_vectors", name),
                dimension,
            }
        );
        Ok(())
    }
    
    pub fn get_vector<const DIM: usize>(
        &self,
        vector_name: &str,
        entity_id: &str,
    ) -> Result<Option<[f32; DIM]>, Error> {
        let info = self.registrations.get(vector_name)
            .ok_or_else(|| Error::VectorNotRegistered(vector_name.to_string()))?;
        
        match info {
            VectorTableInfo::Dense { cf_name, dimension } => {
                if *dimension != DIM {
                    return Err(Error::DimensionMismatch);
                }
                
                let cf = self.db.column_family(cf_name)?;
                let read_txn = cf.begin_read()?;
                let table = VectorTableRead::<DIM>::open(&read_txn, "vectors")?;
                
                if let Some(guard) = table.get(entity_id)? {
                    Ok(Some(*guard.value()))
                } else {
                    Ok(None)
                }
            }
            _ => Err(Error::WrongVectorType),
        }
    }
}
```

---

## Trajectory Storage with Custom Types

### Position Snapshot Type

```rust
use manifold_timeseries::{TimeSeriesTable, AbsoluteEncoding};
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PositionSnapshot {
    pub graph_position: [f32; 17],
    pub embedding_position: [f32; 17],
    pub property_position: [f32; 17],
}

impl PositionSnapshot {
    pub fn new(
        graph: [f32; 17],
        embedding: [f32; 17],
        property: [f32; 17],
    ) -> Self {
        Self {
            graph_position: graph,
            embedding_position: embedding,
            property_position: property,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = ColumnFamilyDatabase::builder()
        .open("trajectories.manifold")?;
    
    let trajectories_cf = db.column_family_or_create("trajectories")?;
    
    let entity_id = Uuid::new_v4();
    let entity_key = entity_id.to_string();
    
    // Store position snapshots over time
    let write_txn = trajectories_cf.begin_write()?;
    let mut ts_table = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "positions")?;
    
    // Snapshot at T0
    let t0 = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let snapshot_t0 = PositionSnapshot::new(
        [0.1; 17], // Initial graph position
        [0.2; 17], // Initial embedding position
        [0.3; 17], // Initial property position
    );
    
    ts_table.write(
        &entity_key,
        t0,
        serde_json::to_value(&snapshot_t0)?.as_f64().unwrap() as f32,
    )?;
    
    // Snapshot at T1 (after some drift)
    let t1 = t0 + 3600_000; // 1 hour later
    let snapshot_t1 = PositionSnapshot::new(
        [0.15; 17], // Drifted graph position
        [0.22; 17], // Drifted embedding position
        [0.31; 17], // Drifted property position
    );
    
    ts_table.write(
        &entity_key,
        t1,
        serde_json::to_value(&snapshot_t1)?.as_f64().unwrap() as f32,
    )?;
    
    drop(ts_table);
    write_txn.commit()?;
    
    // Query trajectory over time range
    let read_txn = trajectories_cf.begin_read()?;
    let ts_read = manifold_timeseries::TimeSeriesTableRead::<AbsoluteEncoding>::open(
        &read_txn,
        "positions"
    )?;
    
    println!("Trajectory for entity {}:", entity_id);
    for point in ts_read.range(&entity_key, t0, t1 + 1)? {
        let (timestamp, _value) = point?;
        println!("  Snapshot at timestamp: {}", timestamp);
        // Deserialize full position snapshot for analysis
    }
    
    Ok(())
}
```

### Trajectory Analysis

```rust
pub struct TrajectoryAnalyzer {
    trajectories_cf: ColumnFamily,
}

impl TrajectoryAnalyzer {
    pub fn calculate_drift(
        &self,
        entity_id: Uuid,
        start_time: u64,
        end_time: u64,
    ) -> Result<DriftMetrics, Error> {
        let read_txn = self.trajectories_cf.begin_read()?;
        let ts_table = TimeSeriesTableRead::<AbsoluteEncoding>::open(
            &read_txn,
            "positions"
        )?;
        
        let entity_key = entity_id.to_string();
        let mut snapshots = Vec::new();
        
        for point in ts_table.range(&entity_key, start_time, end_time)? {
            let (timestamp, value) = point?;
            let snapshot: PositionSnapshot = serde_json::from_value(
                serde_json::Value::from(value)
            )?;
            snapshots.push((timestamp, snapshot));
        }
        
        // Calculate drift for each modality
        let graph_drift = self.calculate_position_drift(
            snapshots.iter().map(|(_, s)| s.graph_position)
        );
        
        let embedding_drift = self.calculate_position_drift(
            snapshots.iter().map(|(_, s)| s.embedding_position)
        );
        
        let property_drift = self.calculate_position_drift(
            snapshots.iter().map(|(_, s)| s.property_position)
        );
        
        Ok(DriftMetrics {
            graph_drift,
            embedding_drift,
            property_drift,
            total_snapshots: snapshots.len(),
            time_span_ms: end_time - start_time,
        })
    }
    
    fn calculate_position_drift<I>(&self, positions: I) -> f32
    where
        I: Iterator<Item = [f32; 17]>,
    {
        // Calculate total movement between consecutive positions
        let positions: Vec<_> = positions.collect();
        let mut total_drift = 0.0;
        
        for window in positions.windows(2) {
            let distance = euclidean_distance(&window[0], &window[1]);
            total_drift += distance;
        }
        
        total_drift
    }
}

pub struct DriftMetrics {
    pub graph_drift: f32,
    pub embedding_drift: f32,
    pub property_drift: f32,
    pub total_snapshots: usize,
    pub time_span_ms: u64,
}
```

---

## Graph Storage with Simplified Edge Properties

### Using manifold-graph

```rust
use manifold_graph::{GraphTable, GraphTableRead};
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = ColumnFamilyDatabase::builder()
        .open("graph.manifold")?;
    
    let edges_cf = db.column_family_or_create("edges")?;
    
    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    
    // Add edge with simplified properties: bool (active) + f32 (weight)
    {
        let write_txn = edges_cf.begin_write()?;
        let mut graph = GraphTable::open(&write_txn, "social")?;
        
        // Active edge with weight 1.0
        graph.add_edge(
            &user1,
            "follows",
            &user2,
            true,   // is_active: true
            1.0,    // weight: 1.0
        )?;
        
        // Passive edge (e.g., suggested connection)
        let user3 = Uuid::new_v4();
        graph.add_edge(
            &user1,
            "suggested",
            &user3,
            false,  // is_active: false (passive/suggested)
            0.7,    // weight: 0.7 (confidence score)
        )?;
        
        drop(graph);
        write_txn.commit()?;
    }
    
    // Query edges
    {
        let read_txn = edges_cf.begin_read()?;
        let graph = GraphTableRead::open(&read_txn, "social")?;
        
        // Get outgoing edges
        println!("Outgoing edges from user1:");
        for edge_result in graph.outgoing_edges(&user1)? {
            let edge = edge_result?;
            println!("  {} -[{}]-> {} (active: {}, weight: {})",
                edge.source, edge.edge_type, edge.target,
                edge.properties.0, edge.properties.1
            );
        }
        
        // Filter by active status
        for edge_result in graph.outgoing_edges(&user1)? {
            let edge = edge_result?;
            if edge.properties.0 { // is_active
                println!("Active connection: {} -> {}", edge.source, edge.target);
            }
        }
    }
    
    Ok(())
}
```

### Storing Complex Edge Metadata Separately

```rust
// For rare cases needing more than bool + f32, use separate properties table

#[derive(Serialize, Deserialize)]
struct EdgeMetadata {
    confidence: f32,
    provenance: String,
    timestamp: u64,
    source_system: String,
}

fn store_edge_with_metadata(
    graph_cf: &ColumnFamily,
    properties_cf: &ColumnFamily,
    source: Uuid,
    edge_type: &str,
    target: Uuid,
    is_active: bool,
    weight: f32,
    metadata: EdgeMetadata,
) -> Result<(), Error> {
    // Store edge with simple properties
    {
        let write_txn = graph_cf.begin_write()?;
        let mut graph = GraphTable::open(&write_txn, "edges")?;
        graph.add_edge(&source, edge_type, &target, is_active, weight)?;
        drop(graph);
        write_txn.commit()?;
    }
    
    // Store complex metadata separately
    {
        let write_txn = properties_cf.begin_write()?;
        let mut table = write_txn.open_table(
            TableDefinition::<(Uuid, String, Uuid), Vec<u8>>::new("edge_metadata")
        )?;
        
        let key = (source, edge_type.to_string(), target);
        let value = bincode::serialize(&metadata)?;
        table.insert(&key, &value)?;
        
        drop(table);
        write_txn.commit()?;
    }
    
    Ok(())
}
```

---

## Compiled Query Examples

### Input: HyperQL Query File

**File: `queries/similar_documents.hql`**

```hyperql
-- Find similar documents by semantic similarity
-- Parameters: $query_vector, $category, $threshold
SELECT 
    d.id,
    d.title,
    d.created_at,
    similarity(d.semantic_embedding, $query_vector) AS score
FROM documents d
WHERE d.category = $category
  AND similarity(d.semantic_embedding, $query_vector) > $threshold
ORDER BY score DESC
LIMIT 10
```

### Generated: Rust Code

**File: `generated/queries.rs`**

```rust
// Auto-generated by hyperql-compile
// DO NOT EDIT - regenerate with: hyperql-compile queries/
use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::VectorTableRead;
use uuid::Uuid;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarDocumentsResult {
    pub id: Uuid,
    pub title: String,
    pub created_at: u64,
    pub score: f32,
}

/// Find similar documents by semantic similarity
///
/// # Parameters
/// - `query_vector`: 768-dimensional query embedding
/// - `category`: Document category filter
/// - `threshold`: Minimum similarity threshold (0.0-1.0)
///
/// # Returns
/// Up to 10 most similar documents ordered by score
pub fn find_similar_documents(
    db: &ColumnFamilyDatabase,
    query_vector: &[f32; 768],
    category: &str,
    threshold: f32,
) -> Result<Vec<SimilarDocumentsResult>, QueryError> {
    // Open column families
    let docs_cf = db.column_family("documents")
        .map_err(|e| QueryError::ColumnFamilyNotFound("documents".to_string()))?;
    
    // Open tables
    let read_txn = docs_cf.begin_read()?;
    let semantic_table = VectorTableRead::<768>::open(&read_txn, "semantic_vectors")?;
    let metadata_table = read_txn.open_table(
        manifold::TableDefinition::<Uuid, DocumentMetadata>::new("metadata")
    )?;
    
    let mut results = Vec::new();
    
    // Scan documents (TODO: use index for category filter)
    for entry in metadata_table.iter()? {
        let (id_bytes, metadata) = entry?;
        let doc_id = Uuid::from_bytes(*id_bytes.value());
        
        // Filter by category
        if metadata.category != category {
            continue;
        }
        
        // Calculate similarity
        if let Some(vec_guard) = semantic_table.get(&doc_id.to_string())? {
            let score = cosine_similarity(query_vector, vec_guard.value());
            
            // Filter by threshold
            if score > threshold {
                results.push(SimilarDocumentsResult {
                    id: doc_id,
                    title: metadata.title.clone(),
                    created_at: metadata.created_at,
                    score,
                });
            }
        }
    }
    
    // Sort by score descending
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    
    // Limit to 10
    results.truncate(10);
    
    Ok(results)
}

// Helper types
#[derive(Deserialize)]
struct DocumentMetadata {
    title: String,
    category: String,
    created_at: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("Column family not found: {0}")]
    ColumnFamilyNotFound(String),
    #[error("Manifold error: {0}")]
    Manifold(#[from] manifold::Error),
}

// Helper function (could be in shared module)
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b)
}
```

### Usage in Application

```rust
use generated::queries::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open database
    let db = ColumnFamilyDatabase::builder()
        .open("app.manifold")?;
    
    // Query embedding (from user input or Tessera)
    let query_embedding: [f32; 768] = get_query_embedding("machine learning")?;
    
    // Execute compiled query - zero parsing overhead!
    let results = find_similar_documents(
        &db,
        &query_embedding,
        "technology",
        0.7, // 70% similarity threshold
    )?;
    
    // Process results with full type safety
    for doc in results {
        println!("{}: {} (score: {:.2})", 
            doc.id, doc.title, doc.score);
    }
    
    Ok(())
}
```

---

## Multi-Modal Positioning

### Storing Three Positions per Entity

```rust
pub struct MultiModalPositions {
    graph_position: [f32; 17],
    embedding_position: [f32; 17],
    property_position: [f32; 17],
}

impl MultiModalPositions {
    pub fn store(
        &self,
        db: &ColumnFamilyDatabase,
        entity_id: Uuid,
    ) -> Result<(), Error> {
        let entity_key = entity_id.to_string();
        
        // Store graph position
        {
            let cf = db.column_family("graph_positions")?;
            let write_txn = cf.begin_write()?;
            let mut table = VectorTable::<17>::open(&write_txn, "positions")?;
            table.insert(&entity_key, &self.graph_position)?;
            drop(table);
            write_txn.commit()?;
        }
        
        // Store embedding position
        {
            let cf = db.column_family("embedding_positions")?;
            let write_txn = cf.begin_write()?;
            let mut table = VectorTable::<17>::open(&write_txn, "positions")?;
            table.insert(&entity_key, &self.embedding_position)?;
            drop(table);
            write_txn.commit()?;
        }
        
        // Store property position
        {
            let cf = db.column_family("property_positions")?;
            let write_txn = cf.begin_write()?;
            let mut table = VectorTable::<17>::open(&write_txn, "positions")?;
            table.insert(&entity_key, &self.property_position)?;
            drop(table);
            write_txn.commit()?;
        }
        
        Ok(())
    }
    
    pub fn calculate_distance(
        &self,
        other: &MultiModalPositions,
        weights: (f32, f32, f32), // (α, β, γ)
    ) -> f32 {
        let (alpha, beta, gamma) = weights;
        
        let d_graph = hyperbolic_distance(&self.graph_position, &other.graph_position);
        let d_embedding = hyperbolic_distance(&self.embedding_position, &other.embedding_position);
        let d_property = hyperbolic_distance(&self.property_position, &other.property_position);
        
        // Weighted combination
        alpha * d_graph + beta * d_embedding + gamma * d_property
    }
}

fn hyperbolic_distance(a: &[f32; 17], b: &[f32; 17]) -> f32 {
    // Hyperboloid distance calculation
    // d = acosh(-⟨a, b⟩_L) where ⟨·,·⟩_L is Minkowski inner product
    let minkowski_product = minkowski_inner_product(a, b);
    (-minkowski_product).acosh()
}

fn minkowski_inner_product(a: &[f32; 17], b: &[f32; 17]) -> f32 {
    // ⟨a, b⟩_L = -a₀b₀ + a₁b₁ + ... + a₁₆b₁₆
    -a[0] * b[0] + a[1..].iter().zip(&b[1..]).map(|(x, y)| x * y).sum::<f32>()
}
```

---

## Cascade Integration

### Schema-Driven Cascade with Manifold Storage

```rust
use manifold_timeseries::TimeSeriesTable;

pub struct CascadeEngine {
    measures_cf: ColumnFamily,
    dependency_graph: CascadeDependencyGraph,
}

impl CascadeEngine {
    pub fn propagate_cascade(
        &self,
        root_entity: Uuid,
        measure_name: &str,
        initial_value: f32,
    ) -> Result<(), Error> {
        // Get entities to propagate to
        let targets = self.dependency_graph.get_cascade_targets(
            &root_entity,
            measure_name,
        )?;
        
        let write_txn = self.measures_cf.begin_write()?;
        let mut measures_table = TimeSeriesTable::<AbsoluteEncoding>::open(
            &write_txn,
            "cascade_measures"
        )?;
        
        let timestamp = current_timestamp_ms();
        
        // Store root value
        measures_table.write(
            &format!("{}::{}", root_entity, measure_name),
            timestamp,
            initial_value,
        )?;
        
        // Propagate to dependencies
        for (target_entity, decay_factor) in targets {
            let propagated_value = initial_value * decay_factor;
            
            measures_table.write(
                &format!("{}::{}", target_entity, measure_name),
                timestamp,
                propagated_value,
            )?;
        }
        
        drop(measures_table);
        write_txn.commit()?;
        
        Ok(())
    }
}
```

---

## Stream Workflows

### Stream Coordination with Loop-Back

```rust
pub struct StreamCoordinator {
    streams: HashMap<String, StreamChannel>,
}

impl StreamCoordinator {
    /// Send entity to stream for external processing
    pub async fn trigger_stream(
        &self,
        stream_name: &str,
        entity: Entity,
    ) -> Result<(), Error> {
        let channel = self.streams.get(stream_name)
            .ok_or(Error::StreamNotFound)?;
        
        channel.send(StreamMessage::Process(entity)).await?;
        Ok(())
    }
    
    /// Process entity from stream and update database
    pub async fn handle_stream_result(
        &self,
        db: &ColumnFamilyDatabase,
        result: ProcessedEntity,
    ) -> Result<(), Error> {
        // Update entity with processing results
        let entities_cf = db.column_family("entities")?;
        let write_txn = entities_cf.begin_write()?;
        
        // Store updated entity (loop-back to database)
        let mut table = write_txn.open_table(
            TableDefinition::<Uuid, Vec<u8>>::new("entities")
        )?;
        
        let serialized = bincode::serialize(&result.entity_data)?;
        table.insert(result.entity_id.as_bytes(), &serialized)?;
        
        drop(table);
        write_txn.commit()?;
        
        Ok(())
    }
}

/// Example: Entity → Stream → Tessera → Database update
pub async fn embedding_workflow(
    coordinator: &StreamCoordinator,
    db: &ColumnFamilyDatabase,
    entity_id: Uuid,
    text_content: String,
) -> Result<(), Error> {
    // 1. Send to embedding stream
    coordinator.trigger_stream(
        "embedding_generation",
        Entity {
            id: entity_id,
            content: text_content,
        },
    ).await?;
    
    // 2. External Tessera service processes (not shown)
    //    - Receives entity from stream
    //    - Generates embedding
    //    - Sends back to result stream
    
    // 3. Handle result (loop-back)
    let result = coordinator.receive_result("embedding_generation").await?;
    
    // 4. Store embedding in database
    let vectors_cf = db.column_family("semantic_vectors")?;
    let write_txn = vectors_cf.begin_write()?;
    let mut table = VectorTable::<768>::open(&write_txn, "vectors")?;
    
    table.insert(&entity_id.to_string(), &result.embedding)?;
    drop(table);
    write_txn.commit()?;
    
    Ok(())
}
```

---

## Summary

These examples demonstrate:

1. **Fixed-width UUID keys** for zero-copy performance
2. **Multiple named vectors** using separate column families
3. **Custom types in time series** for trajectory tracking
4. **Simplified edge properties** (bool + f32)
5. **Compiled queries** generating type-safe Rust code
6. **Multi-modal positioning** with three separate position vectors
7. **Cascade integration** using manifold-timeseries
8. **Stream workflows** with database loop-back

All examples follow Manifold's patterns and leverage its domain crates for clean, performant implementations.