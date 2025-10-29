//! Comprehensive RAG (Retrieval Augmented Generation) Example
//!
//! Demonstrates:
//! - Column family with multiple tables (text, dense vectors, sparse vectors, metadata)
//! - Atomic updates across multiple tables in single transaction
//! - Hybrid retrieval combining dense semantic search + sparse keyword matching
//! - Integration with VectorSource trait for external index builders
//! - Real-world RAG pipeline architecture
//!
//! Architecture:
//! ```
//! Column Family: "knowledge_base"
//! ├── Table: "articles"        (Uuid → String)        - Full article text
//! ├── Table: "vectors_dense"   (Uuid → [f32; 768])   - BGE embeddings
//! ├── Table: "vectors_sparse"  (Uuid → Vec<(u32, f32)>) - SPLADE embeddings
//! └── Table: "metadata"        (Uuid → String)        - JSON metadata
//! ```

use anyhow::Result;
use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::{
    SparseVector, SparseVectorTable, SparseVectorTableRead, VectorTable, VectorTableRead, distance,
};
use tessera::{TesseraDense, TesseraSparse};
use uuid::Uuid;

#[derive(Debug)]
struct Article {
    id: Uuid,
    title: String,
    content: String,
    category: String,
}

impl Article {
    fn full_text(&self) -> String {
        format!("{}\n\n{}", self.title, self.content)
    }
}

fn main() -> Result<()> {
    println!("=== Comprehensive RAG System Example ===\n");

    // Sample knowledge base articles
    let articles = vec![
        Article {
            id: Uuid::new_v4(),
            title: "Introduction to Machine Learning".to_string(),
            content: "Machine learning is a subset of artificial intelligence that enables systems to learn and improve from experience without being explicitly programmed. It focuses on developing algorithms that can access data and learn from it.".to_string(),
            category: "fundamentals".to_string(),
        },
        Article {
            id: Uuid::new_v4(),
            title: "Neural Networks Explained".to_string(),
            content: "Neural networks are computing systems inspired by biological neural networks. They consist of interconnected nodes (neurons) organized in layers. Deep learning uses neural networks with multiple hidden layers to learn hierarchical representations.".to_string(),
            category: "architecture".to_string(),
        },
        Article {
            id: Uuid::new_v4(),
            title: "The Transformer Architecture".to_string(),
            content: "Transformers revolutionized natural language processing through self-attention mechanisms. Unlike recurrent networks, transformers process entire sequences in parallel, enabling efficient training on large datasets. BERT and GPT are prominent transformer-based models.".to_string(),
            category: "architecture".to_string(),
        },
        Article {
            id: Uuid::new_v4(),
            title: "Understanding Word Embeddings".to_string(),
            content: "Word embeddings represent words as dense vectors in continuous space, capturing semantic relationships. Similar words have similar vector representations. Common techniques include Word2Vec, GloVe, and modern contextual embeddings from transformers.".to_string(),
            category: "fundamentals".to_string(),
        },
        Article {
            id: Uuid::new_v4(),
            title: "Semantic Search Systems".to_string(),
            content: "Semantic search finds information based on meaning rather than keyword matching. It uses vector embeddings to represent queries and documents in shared semantic space. Similarity is measured using cosine distance or dot product.".to_string(),
            category: "applications".to_string(),
        },
        Article {
            id: Uuid::new_v4(),
            title: "Retrieval Augmented Generation".to_string(),
            content: "RAG combines information retrieval with language models to generate accurate, grounded responses. The system retrieves relevant context from a knowledge base, then conditions the language model on this context to produce informed answers.".to_string(),
            category: "applications".to_string(),
        },
        Article {
            id: Uuid::new_v4(),
            title: "Vector Databases".to_string(),
            content: "Vector databases specialize in storing and querying high-dimensional embeddings. They provide efficient nearest neighbor search through indexing structures like HNSW, IVF, or product quantization. Essential for semantic search at scale.".to_string(),
            category: "infrastructure".to_string(),
        },
        Article {
            id: Uuid::new_v4(),
            title: "Fine-tuning Language Models".to_string(),
            content: "Fine-tuning adapts pre-trained models to specific tasks or domains. It continues training on task-specific data, adjusting weights to improve performance. Techniques like LoRA enable efficient fine-tuning with fewer parameters.".to_string(),
            category: "training".to_string(),
        },
    ];

    println!("Knowledge base: {} articles\n", articles.len());

    // Initialize embedders
    println!("Loading embedding models...");
    println!("  • Dense: BGE-Base-EN-v1.5 (768 dimensions)");
    let dense_embedder = TesseraDense::new("bge-base-en-v1.5")?;

    println!("  • Sparse: SPLADE++ EN v1 (30522 vocabulary)");
    let sparse_embedder = TesseraSparse::new("splade-pp-en-v1")?;
    println!("Models loaded\n");

    // Create database with column family
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("rag_system.db");
    let db = ColumnFamilyDatabase::open(&db_path)?;
    let cf = db.column_family_or_create("knowledge_base")?;

    // Index all articles with ATOMIC updates
    println!("Indexing articles...");
    let start = std::time::Instant::now();

    for article in &articles {
        // Encode with both dense and sparse embedders
        let full_text = article.full_text();
        let dense_emb = dense_embedder.encode(&full_text)?;
        let sparse_emb = sparse_embedder.encode(&full_text)?;

        // Convert dense embedding to fixed-size array
        let dense_vec: [f32; 768] = dense_emb
            .embedding
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .try_into()
            .expect("dense embedding dimension mismatch");

        // Convert sparse embedding to SparseVector
        let sparse_vec = SparseVector::new(
            sparse_emb
                .weights
                .into_iter()
                .map(|(idx, w)| (idx as u32, w))
                .collect(),
        );

        // ATOMIC TRANSACTION: Insert text + dense vector + sparse vector + metadata
        {
            let write_txn = cf.begin_write()?;

            // Store article text
            let mut articles_table =
                write_txn.open_table::<Uuid, &str>(manifold::TableDefinition::new("articles"))?;
            articles_table.insert(&article.id, full_text.as_str())?;

            // Store dense embedding
            let mut dense_table = VectorTable::<768>::open(&write_txn, "vectors_dense")?;
            dense_table.insert(&article.id, &dense_vec)?;

            // Store sparse embedding
            let mut sparse_table = SparseVectorTable::open(&write_txn, "vectors_sparse")?;
            sparse_table.insert(&article.id, &sparse_vec)?;

            // Store metadata (JSON)
            let metadata = format!(
                r#"{{"title":"{}","category":"{}"}}"#,
                article.title, article.category
            );
            let mut metadata_table =
                write_txn.open_table::<Uuid, &str>(manifold::TableDefinition::new("metadata"))?;
            metadata_table.insert(&article.id, metadata.as_str())?;

            // Ensure all table borrows are dropped before committing the write transaction
            drop(metadata_table);
            drop(sparse_table);
            drop(dense_table);
            drop(articles_table);

            write_txn.commit()?;
        }

        println!("  ✓ Indexed: {}", article.id);
    }

    println!(
        "Indexed {} articles in {:.2}s\n",
        articles.len(),
        start.elapsed().as_secs_f32()
    );

    // Perform hybrid retrieval queries
    let queries = vec![
        "How do language models retrieve information?",
        "What are vector representations of words?",
        "Explain deep neural networks",
    ];

    println!("═══════════════════════════════════════════════════════════");
    println!("HYBRID RETRIEVAL QUERIES");
    println!("═══════════════════════════════════════════════════════════\n");

    let read_txn = cf.begin_read()?;
    let dense_table = VectorTableRead::<768>::open(&read_txn, "vectors_dense")?;
    let sparse_table = SparseVectorTableRead::open(&read_txn, "vectors_sparse")?;

    // Hybrid scoring weights
    let dense_weight = 0.7;
    let sparse_weight = 0.3;

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
            query_sparse
                .weights
                .into_iter()
                .map(|(idx, w)| (idx as u32, w))
                .collect(),
        );

        // Compute dense similarity scores
        let mut dense_scores: Vec<(Uuid, f32)> = Vec::new();
        for result in dense_table.all_vectors()? {
            let (doc_id, doc_guard) = result?;
            let similarity = distance::cosine(&query_dense_vec, doc_guard.value());
            dense_scores.push((doc_id, similarity));
        }

        // Compute sparse similarity scores
        let mut sparse_scores: Vec<(Uuid, f32)> = Vec::new();
        for article in &articles {
            if let Some(doc_sparse) = sparse_table.get(&article.id)? {
                let similarity = query_sparse_vec.dot(&doc_sparse);
                sparse_scores.push((article.id, similarity));
            }
        }

        // Hybrid scoring: combine dense and sparse (weighted average)
        let mut hybrid_scores: Vec<(Uuid, f32, f32, f32)> = Vec::new();
        for article in &articles {
            let dense_score = dense_scores
                .iter()
                .find(|(id, _)| id == &article.id)
                .map(|(_, score)| *score)
                .unwrap_or(0.0);

            let sparse_score = sparse_scores
                .iter()
                .find(|(id, _)| id == &article.id)
                .map(|(_, score)| *score)
                .unwrap_or(0.0);

            // Normalize sparse score (rough normalization)
            let sparse_normalized = sparse_score / 100.0;

            let hybrid_score = (dense_weight * dense_score) + (sparse_weight * sparse_normalized);

            hybrid_scores.push((article.id, hybrid_score, dense_score, sparse_normalized));
        }

        // Sort by hybrid score
        hybrid_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        println!("Top 3 results (Hybrid Retrieval):");
        for (rank, (doc_id, hybrid, dense, sparse)) in hybrid_scores.iter().take(3).enumerate() {
            let article = articles.iter().find(|a| &a.id == doc_id).unwrap();
            println!(
                "  {}. [Hybrid: {:.4}] (Dense: {:.4}, Sparse: {:.4})",
                rank + 1,
                hybrid,
                dense,
                sparse
            );
            println!("     {:?} → \"{}\"", doc_id, article.title);
        }
        println!();
    }

    println!("═══════════════════════════════════════════════════════════");
    println!("\n✓ RAG System Example Complete!");
    println!("\nArchitecture Highlights:");
    println!("  • Atomic updates: Text + Dense + Sparse + Metadata in single transaction");
    println!(
        "  • Hybrid retrieval: {:.0}% dense + {:.0}% sparse for best results",
        dense_weight * 100.0,
        sparse_weight * 100.0
    );
    println!("  • Multiple tables in single column family for logical grouping");
    println!("  • Type-safe vector storage with VectorTable<768> and SparseVectorTable");
    println!("  • Zero-copy reads via guard-based access pattern");
    println!("\nStorage Breakdown:");
    println!("  • Articles table: {} texts", articles.len());
    println!(
        "  • Dense vectors: {} × 768 × 4 bytes = {:.2} KB",
        articles.len(),
        (articles.len() * 768 * 4) as f32 / 1024.0
    );
    println!(
        "  • Sparse vectors: {} × ~100 entries (99% sparsity)",
        articles.len()
    );
    println!("  • Metadata: {} JSON records", articles.len());

    Ok(())
}
