# Manifold Vectors Examples

This directory contains comprehensive examples demonstrating the `manifold-vectors` crate with real-world use cases using [Tessera embeddings](https://crates.io/crates/tessera-embeddings).

## Overview

The examples showcase different vector storage patterns and embedding types:

1. **Dense Semantic Search** - Single-vector embeddings for semantic similarity
2. **Comprehensive RAG System** - Multi-table architecture with atomic updates
3. **Multi-Vector ColBERT** - Token-level embeddings with MaxSim and quantization
4. **Sparse Hybrid Search** - Vocabulary-sized sparse vectors with keyword matching

## Prerequisites

The examples use Tessera embeddings, which will download models from HuggingFace on first run. Ensure you have:

- Rust 1.70+ (for const generics)
- ~2-5 GB disk space for model downloads
- Internet connection for initial model download

### GPU Acceleration

**These examples use Metal acceleration by default** (for Apple Silicon Macs). The encoding will automatically use your GPU for 2-5x faster performance.

If you're on a different platform, you can modify `Cargo.toml`:

```toml
# For NVIDIA GPUs (Linux/Windows)
tessera-embeddings = { version = "0.1", features = ["cuda"] }

# For CPU-only (any platform)
tessera-embeddings = "0.1"
```

## Running the Examples

### 1. Dense Semantic Search

Demonstrates storage and retrieval of dense 768-dimensional BGE embeddings.

```bash
cargo run --example dense_semantic_search
```

**What it shows:**
- `VectorTable<768>` for type-safe storage
- Batch encoding with Tessera
- Cosine similarity search
- Guard-based zero-copy access

**Models used:**
- BGE-Base-EN-v1.5 (768 dimensions, 110M parameters)

**Expected output:**
- Encodes 20 documents
- Performs semantic search for 3 queries
- Shows top-5 most similar documents per query

---

### 2. Comprehensive RAG System

Complete RAG pipeline with hybrid retrieval and atomic multi-table updates.

```bash
cargo run --example rag_complete
```

**What it shows:**
- Column family with 4 tables: text, dense vectors, sparse vectors, metadata
- Atomic transaction inserting across all tables
- Hybrid retrieval (70% dense + 30% sparse)
- Real-world knowledge base architecture

**Models used:**
- BGE-Base-EN-v1.5 (768 dimensions) for dense embeddings
- SPLADE++ EN v1 (30,522 vocab) for sparse embeddings

**Expected output:**
- Indexes 8 articles atomically
- Performs hybrid retrieval for 3 queries
- Shows storage breakdown and architecture details

---

### 3. Multi-Vector ColBERT

Token-level embeddings with late interaction (MaxSim) and binary quantization.

```bash
cargo run --example multi_vector_colbert
```

**What it shows:**
- `MultiVectorTable<128>` for variable-length token sequences
- MaxSim computation for late interaction
- Binary quantization for 32x compression
- Storage comparison (full-precision vs quantized)

**Models used:**
- ColBERT v2 (128 dimensions per token, 110M parameters)

**Expected output:**
- Encodes 10 passages with variable token counts
- Performs MaxSim retrieval for 3 queries
- Demonstrates binary quantization with 32x compression
- Compares retrieval quality (full vs quantized)

---

### 4. Sparse Hybrid Search

Vocabulary-sized sparse vectors with interpretable keyword weights.

```bash
cargo run --example sparse_hybrid_search
```

**What it shows:**
- `SparseVectorTable` for 30,522-dimensional sparse vectors
- 99%+ sparsity with efficient COO storage
- O(m+n) sparse dot product algorithm
- Hybrid ranking combining dense + sparse signals

**Models used:**
- BGE-Base-EN-v1.5 (768 dimensions) for dense
- SPLADE++ EN v1 (30,522 vocab) for sparse

**Expected output:**
- Encodes 15 documents with both embedders
- Shows sparse vector interpretability (top weighted tokens)
- Compares dense-only, sparse-only, and hybrid retrieval
- Displays storage efficiency metrics

---

## Performance Notes

### First Run (Model Downloads)

On first run, each example will download models from HuggingFace:

- **BGE-Base-EN-v1.5**: ~440 MB
- **ColBERT v2**: ~440 MB  
- **SPLADE++ EN v1**: ~440 MB

Models are cached in `~/.cache/huggingface/` and reused across runs.

### Encoding Speeds

On Apple M1 Max (CPU):
- Dense encoding: ~125 docs/sec (single), ~711 docs/sec (batch=32)
- ColBERT encoding: ~83 docs/sec (single), ~410 docs/sec (batch=32)
- SPLADE encoding: ~67 docs/sec (single)

With GPU acceleration (Metal/CUDA), expect 2-5x speedup.

### Storage Sizes

Typical storage per document:
- Dense (768-dim): 3.0 KB per vector
- ColBERT (128-dim × ~20 tokens): 10.0 KB per document
- ColBERT quantized: 0.3 KB per document (32x compression)
- Sparse (~100 non-zero entries): 0.8 KB per vector

## Vector Types Reference

### Dense Vectors (`VectorTable<DIM>`)

- Fixed dimension known at compile time
- Single vector per document
- Type-safe with const generics
- Best for: Semantic search, clustering, classification

### Multi-Vectors (`MultiVectorTable<DIM>`)

- Variable number of vectors per document
- Token-level granularity
- MaxSim late interaction scoring
- Best for: Precise phrase matching, passage retrieval, Q&A

### Sparse Vectors (`SparseVectorTable`)

- Vocabulary-sized (typically 30,522 dimensions)
- 99%+ sparsity, only non-zero weights stored
- Interpretable token weights
- Best for: Keyword search, hybrid retrieval, inverted indexes

## Integration Patterns

### Using with External Indexes

The examples demonstrate the `VectorSource` trait for integration with external index builders (HNSW, FAISS, etc.):

```rust
use manifold_vectors::integration::VectorSource;

// VectorTableRead implements VectorSource
let vectors = VectorTableRead::<768>::open(&read_txn, "embeddings")?;

// Iterate with zero-copy guards
for result in vectors.iter()? {
    let (doc_id, guard) = result?;
    // guard.value() provides &[f32; 768] with no allocations
    index_builder.add(doc_id, guard.value())?;
}
```

### Atomic Multi-Table Updates

See the RAG example for the pattern:

```rust
{
    let write_txn = cf.begin_write()?;
    
    // Open multiple tables
    let mut text_table = write_txn.open_table(...)?;
    let mut vectors_table = VectorTable::<768>::open(&write_txn, ...)?;
    let mut metadata_table = write_txn.open_table(...)?;
    
    // Insert across all tables
    text_table.insert(id, text)?;
    vectors_table.insert(id, &embedding)?;
    metadata_table.insert(id, metadata)?;
    
    // Single commit for atomicity
    write_txn.commit()?;
}
```

## Troubleshooting

### Model Download Fails

If model downloads fail:
1. Check internet connection
2. Verify HuggingFace is accessible
3. Manually download to `~/.cache/huggingface/hub/`

### Out of Memory

For large batches, reduce batch size or use quantization:
```rust
let embedder = TesseraMultiVector::builder()
    .model("colbert-v2")
    .quantization(QuantizationConfig::Binary)
    .build()?;
```

### Slow Encoding

- Enable GPU acceleration (Metal/CUDA features)
- Use batch encoding: `encode_batch(&texts)` instead of individual `encode()`
- For ColBERT, use smaller models: `colbert-small` (96-dim, 33M params)

## Additional Resources

- [Manifold documentation](https://github.com/yourorg/manifold)
- [Tessera embeddings](https://crates.io/crates/tessera-embeddings)
- [Domain Optimization Plan](../../DOMAIN_OPTIMIZATION_PLAN_v0.1.1.md)
- [Phase 1 Completion Summary](../../.project/DOMAIN_VECTORS_COMPLETION.md)

## Contributing

To add new examples:
1. Create a new file in this directory
2. Add to this README with description
3. Ensure it demonstrates a distinct pattern or use case
4. Include clear output documentation