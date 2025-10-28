# Manifold Vectors Integration Guide

## Installation Options

There are two ways to use `manifold-vectors` in your project:

### Option 1: Separate Crate (Recommended)

Add both `manifold` and `manifold-vectors` as dependencies:

```toml
[dependencies]
manifold = "3.1"
manifold-vectors = "0.1"
tessera-embeddings = "0.1"  # For encoding
```

**Advantages:**
- Clean separation of concerns
- Optional dependency - only include if needed
- Independent versioning
- Smaller compile times if you don't use vectors

### Option 2: Manifold Feature Flags (Future)

In a future release, you could enable via features:

```toml
[dependencies]
manifold = { version = "3.1", features = ["vectors", "graph", "timeseries"] }
```

**Advantages:**
- Single dependency
- Unified versioning
- Easier to manage

**Note:** This approach is not yet implemented but could be added if desired.

## Current Recommendation

For now, **use Option 1** (separate crates). This keeps manifold core focused and allows users to opt-in to domain-specific functionality.

## Usage Patterns

### Basic Dense Vectors

```rust
use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::{VectorTable, VectorTableRead, distance};
use tessera_embeddings::TesseraDense;

let db = ColumnFamilyDatabase::open("my.db")?;
let cf = db.column_family_or_create("embeddings")?;

// Encode with Tessera
let embedder = TesseraDense::new("bge-base-en-v1.5")?;
let embedding = embedder.encode("Machine learning is amazing")?;

// Store in manifold
let write_txn = cf.begin_write()?;
let mut vectors = VectorTable::<768>::open(&write_txn, "vectors")?;

let vec_array: [f32; 768] = embedding.embedding.iter()
    .copied()
    .collect::<Vec<_>>()
    .try_into()
    .expect("dimension mismatch");

vectors.insert("doc_1", &vec_array)?;
drop(vectors);
write_txn.commit()?;

// Read with zero-copy
let read_txn = cf.begin_read()?;
let vectors = VectorTableRead::<768>::open(&read_txn, "vectors")?;
let guard = vectors.get("doc_1")?.unwrap();

// Compute similarity
let query = embedder.encode("What is machine learning?")?;
let query_vec: [f32; 768] = query.embedding.iter()
    .copied()
    .collect::<Vec<_>>()
    .try_into()
    .expect("dimension mismatch");

let similarity = distance::cosine(&query_vec, guard.value());
println!("Similarity: {:.4}", similarity);
```

### Multi-Table RAG Architecture

```rust
use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::{VectorTable, SparseVectorTable, SparseVector};
use tessera_embeddings::{TesseraDense, TesseraSparse};

let db = ColumnFamilyDatabase::open("rag.db")?;
let cf = db.column_family_or_create("knowledge_base")?;

// Encode document
let dense_embedder = TesseraDense::new("bge-base-en-v1.5")?;
let sparse_embedder = TesseraSparse::new("splade-pp-en-v1")?;

let text = "Neural networks learn from data";
let dense_emb = dense_embedder.encode(text)?;
let sparse_emb = sparse_embedder.encode(text)?;

// Atomic insert across multiple tables
{
    let write_txn = cf.begin_write()?;
    
    // Text table
    let mut text_table = write_txn.open_table::<&str, &str>(
        manifold::TableDefinition::new("articles")
    )?;
    text_table.insert("doc_1", text)?;
    
    // Dense vectors
    let mut dense_table = VectorTable::<768>::open(&write_txn, "vectors_dense")?;
    let dense_vec: [f32; 768] = dense_emb.embedding.iter()
        .copied()
        .collect::<Vec<_>>()
        .try_into()
        .expect("dimension mismatch");
    dense_table.insert("doc_1", &dense_vec)?;
    
    // Sparse vectors
    let mut sparse_table = SparseVectorTable::open(&write_txn, "vectors_sparse")?;
    let sparse_vec = SparseVector::new(sparse_emb.weights);
    sparse_table.insert("doc_1", &sparse_vec)?;
    
    // All or nothing!
    write_txn.commit()?;
}
```

### Multi-Vector ColBERT

```rust
use manifold_vectors::{MultiVectorTable, MultiVectorTableRead};
use tessera_embeddings::TesseraMultiVector;

let embedder = TesseraMultiVector::new("colbert-v2")?;
let token_embeddings = embedder.encode("What is machine learning?")?;

// Store variable-length sequence
let write_txn = cf.begin_write()?;
let mut multi_vectors = MultiVectorTable::<128>::open(&write_txn, "tokens")?;

let vectors: Vec<[f32; 128]> = (0..token_embeddings.num_tokens)
    .map(|i| {
        token_embeddings.embeddings.row(i).iter()
            .copied()
            .collect::<Vec<_>>()
            .try_into()
            .expect("dimension mismatch")
    })
    .collect();

multi_vectors.insert("query_1", &vectors)?;
drop(multi_vectors);
write_txn.commit()?;

// Read and compute MaxSim
let read_txn = cf.begin_read()?;
let multi_vectors = MultiVectorTableRead::<128>::open(&read_txn, "tokens")?;
let doc_vectors = multi_vectors.get("query_1")?.unwrap();

// MaxSim: for each query token, find max similarity with doc tokens
let mut maxsim = 0.0;
for q_idx in 0..query_token_embeddings.num_tokens {
    let query_token = query_token_embeddings.embeddings.row(q_idx);
    let mut max_sim = f32::NEG_INFINITY;
    for doc_vec in &doc_vectors {
        let dot: f32 = query_token.iter().zip(doc_vec.iter())
            .map(|(a, b)| a * b)
            .sum();
        max_sim = max_sim.max(dot);
    }
    maxsim += max_sim;
}
```

## Architecture Benefits

### Why Separate Tables in Same Column Family?

```
Column Family: "knowledge_base"
├── Table: "articles"        (String → String)
├── Table: "vectors_dense"   (String → [f32; 768])
├── Table: "vectors_sparse"  (String → Vec<(u32, f32)>)
└── Table: "metadata"        (String → String)
```

**Benefits:**
1. **Atomic Updates** - Insert article + embeddings + metadata in one transaction
2. **Efficient Queries** - Range scan metadata, lookup embedding by ID
3. **Storage Efficiency** - Related data in same segments, better cache locality
4. **Transaction Isolation** - One writer per collection

### When to Use Separate Column Families?

Only for truly independent collections:
- `news_articles` vs `user_profiles` vs `chat_messages` - **separate CFs**
- But within `news_articles`: text, embeddings, metadata - **same CF, different tables**

## See Examples

Check `examples/` directory for complete working examples:
- `dense_semantic_search.rs` - Dense vectors with BGE
- `rag_complete.rs` - Multi-table RAG architecture
- `multi_vector_colbert.rs` - ColBERT with quantization
- `sparse_hybrid_search.rs` - SPLADE hybrid retrieval
