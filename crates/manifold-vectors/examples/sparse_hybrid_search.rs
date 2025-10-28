//! Sparse Vector Hybrid Search Example
//!
//! Demonstrates:
//! - SparseVectorTable for SPLADE embeddings (vocabulary-sized vectors)
//! - COO (Coordinate) format storage with 99% sparsity
//! - Efficient sparse dot product computation
//! - Hybrid retrieval combining dense semantic search + sparse keyword matching
//! - Interpretable sparse weights showing keyword expansion
//!
//! SPLADE (Sparse Lexical and Expansion) produces vocabulary-sized vectors
//! (30,522 dimensions for BERT vocab) where each dimension corresponds to a
//! token. Most dimensions are zero (99%+ sparsity), making it efficient for
//! inverted index storage while maintaining learned semantic expansion.

use anyhow::Result;
use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::{
    SparseVector, SparseVectorTable, SparseVectorTableRead, VectorTable, VectorTableRead, distance,
};
use tessera::{TesseraDense, TesseraSparse};

fn main() -> Result<()> {
    println!("=== Sparse Vector Hybrid Search Example ===\n");

    // Sample documents for hybrid search
    let documents = vec![
        (
            "doc_001",
            "Machine learning algorithms learn patterns from training data to make predictions on new data.",
        ),
        (
            "doc_002",
            "Deep neural networks use multiple layers to extract hierarchical features from raw input.",
        ),
        (
            "doc_003",
            "Natural language processing enables computers to understand and generate human language.",
        ),
        (
            "doc_004",
            "Computer vision systems analyze and interpret visual information from images and videos.",
        ),
        (
            "doc_005",
            "Reinforcement learning trains agents to maximize rewards through trial and error interactions.",
        ),
        (
            "doc_006",
            "Supervised learning requires labeled examples to train classification and regression models.",
        ),
        (
            "doc_007",
            "Unsupervised learning discovers hidden patterns and structures in unlabeled datasets.",
        ),
        (
            "doc_008",
            "Transfer learning leverages pre-trained models to solve new tasks with limited data.",
        ),
        (
            "doc_009",
            "Gradient descent optimization iteratively adjusts model parameters to minimize loss functions.",
        ),
        (
            "doc_010",
            "Backpropagation computes gradients through neural network layers using the chain rule.",
        ),
        (
            "doc_011",
            "Convolutional neural networks apply spatial filters to extract features from image data.",
        ),
        (
            "doc_012",
            "Recurrent neural networks process sequential data by maintaining hidden state across time steps.",
        ),
        (
            "doc_013",
            "Transformer models use self-attention mechanisms to capture long-range dependencies in sequences.",
        ),
        (
            "doc_014",
            "BERT employs bidirectional transformers to create contextual word representations for understanding.",
        ),
        (
            "doc_015",
            "GPT generates text autoregressively by predicting the next token conditioned on previous context.",
        ),
    ];

    println!("Document corpus: {} documents\n", documents.len());

    // Initialize embedders
    println!("Loading embedding models...");
    println!("  • Dense: BGE-Base-EN-v1.5 (768 dimensions)");
    let dense_embedder = TesseraDense::new("bge-base-en-v1.5")?;

    println!("  • Sparse: SPLADE++ EN v1 (30,522 vocabulary)");
    let sparse_embedder = TesseraSparse::new("splade-pp-en-v1")?;
    println!("Models loaded\n");

    // Create database
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("hybrid_search.db");
    let db = ColumnFamilyDatabase::open(&db_path)?;
    let cf = db.column_family_or_create("documents")?;

    // Encode and store documents
    println!("Encoding documents...");
    let start = std::time::Instant::now();

    let mut total_sparse_entries = 0;

    {
        let write_txn = cf.begin_write()?;
        let mut dense_table = VectorTable::<768>::open(&write_txn, "vectors_dense")?;
        let mut sparse_table = SparseVectorTable::open(&write_txn, "vectors_sparse")?;

        for (doc_id, text) in &documents {
            // Encode with both embedders
            let dense_emb = dense_embedder.encode(text)?;
            let sparse_emb = sparse_embedder.encode(text)?;

            // Convert dense to fixed array
            let dense_vec: [f32; 768] = dense_emb
                .embedding
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .try_into()
                .expect("dense embedding dimension mismatch");

            // Convert sparse to SparseVector (convert usize to u32)
            let sparse_vec = SparseVector::new(
                sparse_emb.weights.into_iter()
                    .map(|(idx, weight)| (idx as u32, weight))
                    .collect()
            );
            total_sparse_entries += sparse_vec.len();

            // Store both
            dense_table.insert(doc_id, &dense_vec)?;
            sparse_table.insert(doc_id, &sparse_vec)?;
        }

        drop(dense_table);
        drop(sparse_table);
        write_txn.commit()?;
    }

    println!("Encoded in {:.2}s", start.elapsed().as_secs_f32());
    println!(
        "Average sparse entries per document: {:.1}",
        total_sparse_entries as f32 / documents.len() as f32
    );
    println!(
        "Sparsity: {:.2}% (only {:.2}% non-zero)",
        (1.0 - (total_sparse_entries as f32 / documents.len() as f32) / 30522.0) * 100.0,
        ((total_sparse_entries as f32 / documents.len() as f32) / 30522.0) * 100.0
    );
    println!();

    // Demonstrate sparse vector interpretability
    println!("─────────────────────────────────────────────────────");
    println!("SPARSE VECTOR INTERPRETABILITY");
    println!("─────────────────────────────────────────────────────\n");

    let sample_text = documents[0].1;
    println!("Sample text: \"{}\"", sample_text);
    println!();

    let sample_sparse = sparse_embedder.encode(sample_text)?;
    println!("Sparse representation:");
    println!("  Vocabulary size: {}", sparse_embedder.vocab_size());
    println!("  Non-zero entries: {}", sample_sparse.weights.len());
    println!(
        "  Sparsity: {:.2}%",
        (1.0 - sample_sparse.weights.len() as f32 / sparse_embedder.vocab_size() as f32) * 100.0
    );
    println!();

    // Show top weighted terms (would need tokenizer to show actual tokens)
    let mut sorted_weights = sample_sparse.weights.clone();
    sorted_weights.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    println!("  Top 10 weighted dimensions (token indices):");
    for (idx, (token_idx, weight)) in sorted_weights.iter().take(10).enumerate() {
        println!(
            "    {}. Token {} → weight {:.4}",
            idx + 1,
            token_idx,
            weight
        );
    }
    println!();

    // Hybrid retrieval queries
    let queries = vec![
        "How do machine learning models learn from data?",
        "What are neural network layers?",
        "Explain text generation with language models",
    ];

    println!("═══════════════════════════════════════════════════════════");
    println!("HYBRID SEARCH (Dense + Sparse)");
    println!("═══════════════════════════════════════════════════════════\n");

    let read_txn = cf.begin_read()?;
    let dense_table = VectorTableRead::<768>::open(&read_txn, "vectors_dense")?;
    let sparse_table = SparseVectorTableRead::open(&read_txn, "vectors_sparse")?;

    for query_text in &queries {
        println!("─────────────────────────────────────────────────────");
        println!("Query: \"{}\"", query_text);
        println!();

        // Encode query with both embedders
        let query_dense = dense_embedder.encode(query_text)?;
        let query_sparse = sparse_embedder.encode(query_text)?;

        let query_dense_vec: [f32; 768] = query_dense
            .embedding
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .try_into()
            .expect("query dense dimension mismatch");

        let query_sparse_vec = SparseVector::new(
            query_sparse.weights.into_iter()
                .map(|(idx, weight)| (idx as u32, weight))
                .collect()
        );

        println!("  Query sparse entries: {}", query_sparse_vec.len());
        println!();

        // Compute dense scores
        let mut dense_scores: Vec<(String, f32)> = Vec::new();
        for result in dense_table.all_vectors()? {
            let (doc_id, doc_guard) = result?;
            let similarity = distance::cosine(&query_dense_vec, doc_guard.value());
            dense_scores.push((doc_id, similarity));
        }

        // Compute sparse scores using efficient sparse dot product
        let mut sparse_scores: Vec<(String, f32)> = Vec::new();
        for (doc_id, _) in &documents {
            if let Some(doc_sparse) = sparse_table.get(doc_id)? {
                // Efficient O(m + n) sparse dot product (sorted merge)
                let similarity = query_sparse_vec.dot(&doc_sparse);
                sparse_scores.push((doc_id.to_string(), similarity));
            }
        }

        // Display dense-only results
        let mut dense_ranked = dense_scores.clone();
        dense_ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        println!("  Dense-only Top 3:");
        for (rank, (doc_id, score)) in dense_ranked.iter().take(3).enumerate() {
            let doc_text = documents
                .iter()
                .find(|(id, _)| id == doc_id)
                .map(|(_, text)| text)
                .unwrap_or(&"");
            println!("    {}. [Cosine: {:.4}] {}", rank + 1, score, doc_id);
            println!("       \"{}...\"", &doc_text[..doc_text.len().min(80)]);
        }
        println!();

        // Display sparse-only results
        let mut sparse_ranked = sparse_scores.clone();
        sparse_ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        println!("  Sparse-only Top 3:");
        for (rank, (doc_id, score)) in sparse_ranked.iter().take(3).enumerate() {
            let doc_text = documents
                .iter()
                .find(|(id, _)| id == doc_id)
                .map(|(_, text)| text)
                .unwrap_or(&"");
            println!("    {}. [Sparse dot: {:.2}] {}", rank + 1, score, doc_id);
            println!("       \"{}...\"", &doc_text[..doc_text.len().min(80)]);
        }
        println!();

        // Hybrid scoring (weighted combination)
        let dense_weight = 0.6;
        let sparse_weight = 0.4;

        let mut hybrid_scores: Vec<(String, f32, f32, f32)> = Vec::new();
        for (doc_id, _) in &documents {
            let dense_score = dense_scores
                .iter()
                .find(|(id, _)| id == doc_id)
                .map(|(_, score)| *score)
                .unwrap_or(0.0);

            let sparse_score = sparse_scores
                .iter()
                .find(|(id, _)| id == doc_id)
                .map(|(_, score)| *score)
                .unwrap_or(0.0);

            // Normalize sparse scores (rough normalization to [0, 1] range)
            let sparse_normalized = sparse_score / 100.0;

            let hybrid_score = (dense_weight * dense_score) + (sparse_weight * sparse_normalized);

            hybrid_scores.push((
                doc_id.to_string(),
                hybrid_score,
                dense_score,
                sparse_normalized,
            ));
        }

        // Sort by hybrid score
        hybrid_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        println!(
            "  Hybrid Top 3 ({}% dense + {}% sparse):",
            (dense_weight * 100.0) as u32,
            (sparse_weight * 100.0) as u32
        );
        for (rank, (doc_id, hybrid, dense, sparse)) in hybrid_scores.iter().take(3).enumerate() {
            let doc_text = documents
                .iter()
                .find(|(id, _)| id == doc_id)
                .map(|(_, text)| text)
                .unwrap_or(&"");
            println!(
                "    {}. [Hybrid: {:.4}] (D:{:.4}, S:{:.4}) {}",
                rank + 1,
                hybrid,
                dense,
                sparse,
                doc_id
            );
            println!("       \"{}...\"", &doc_text[..doc_text.len().min(80)]);
        }
        println!();
    }

    println!("═══════════════════════════════════════════════════════════");
    println!("\n✓ Sparse Hybrid Search Example Complete!");
    println!("\nKey Takeaways:");
    println!("  • SparseVectorTable stores vocabulary-sized vectors with 99%+ sparsity");
    println!("  • COO format: only non-zero (index, weight) pairs are stored");
    println!("  • Efficient O(m+n) sparse dot product via sorted merge algorithm");
    println!(
        "  • Hybrid search combines semantic understanding (dense) + keyword matching (sparse)"
    );
    println!("  • Sparse vectors are interpretable - each dimension maps to a vocabulary token");
    println!();
    println!("Storage Efficiency:");
    println!(
        "  • Dense: {} × 768 × 4 bytes = {:.2} KB",
        documents.len(),
        (documents.len() * 768 * 4) as f32 / 1024.0
    );
    println!(
        "  • Sparse: {} × ~{} entries × 8 bytes = {:.2} KB",
        documents.len(),
        total_sparse_entries / documents.len(),
        (total_sparse_entries * 8) as f32 / 1024.0
    );
    println!(
        "  • Sparsity enables efficient inverted index storage (99.{:.0}% zeros)",
        (1.0 - (total_sparse_entries as f32 / documents.len() as f32) / 30522.0) * 1000.0
    );

    Ok(())
}
