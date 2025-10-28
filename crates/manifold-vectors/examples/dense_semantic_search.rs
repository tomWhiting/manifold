//! Dense Vector Semantic Search Example
//!
//! Demonstrates:
//! - Storage of dense embeddings using VectorTable<768>
//! - Batch insertion with Tessera BGE embeddings
//! - Cosine similarity search over stored vectors
//! - Integration with manifold column families
//!
//! This example builds a simple semantic search engine that:
//! 1. Encodes 20 sample documents using BGE-Base-EN-v1.5 (768 dimensions)
//! 2. Stores embeddings in manifold using VectorTable
//! 3. Performs semantic search to find top-K most similar documents to a query

use anyhow::Result;
use manifold::column_family::ColumnFamilyDatabase;
use manifold_vectors::{VectorTable, VectorTableRead, distance};
use tessera::TesseraDense;

fn main() -> Result<()> {
    println!("=== Dense Vector Semantic Search Example ===\n");

    // Sample document corpus
    let documents = vec![
        (
            "doc_001",
            "Machine learning is a subset of artificial intelligence",
        ),
        (
            "doc_002",
            "Deep learning uses neural networks with multiple layers",
        ),
        (
            "doc_003",
            "Natural language processing enables computers to understand text",
        ),
        (
            "doc_004",
            "Computer vision allows machines to interpret visual information",
        ),
        (
            "doc_005",
            "Reinforcement learning trains agents through rewards and penalties",
        ),
        (
            "doc_006",
            "Supervised learning requires labeled training data",
        ),
        (
            "doc_007",
            "Unsupervised learning finds patterns in unlabeled data",
        ),
        (
            "doc_008",
            "Transfer learning reuses models trained on different tasks",
        ),
        (
            "doc_009",
            "Gradient descent optimizes neural network weights",
        ),
        (
            "doc_010",
            "Backpropagation calculates gradients for training",
        ),
        (
            "doc_011",
            "Convolutional networks excel at image recognition tasks",
        ),
        (
            "doc_012",
            "Recurrent networks process sequential data effectively",
        ),
        (
            "doc_013",
            "Transformers use attention mechanisms for language tasks",
        ),
        (
            "doc_014",
            "BERT revolutionized natural language understanding",
        ),
        (
            "doc_015",
            "GPT models generate coherent and contextual text",
        ),
        ("doc_016", "Embeddings represent words as dense vectors"),
        (
            "doc_017",
            "Semantic search finds documents by meaning not keywords",
        ),
        (
            "doc_018",
            "Vector databases store and query high-dimensional embeddings",
        ),
        (
            "doc_019",
            "Retrieval augmented generation combines search with LLMs",
        ),
        (
            "doc_020",
            "Fine-tuning adapts pre-trained models to specific domains",
        ),
    ];

    println!("Document corpus: {} documents\n", documents.len());

    // Initialize Tessera dense embedder with BGE-Base-EN-v1.5
    println!("Loading BGE-Base-EN-v1.5 model (768 dimensions)...");
    let embedder = TesseraDense::new("bge-base-en-v1.5")?;
    println!("Model loaded: {}\n", embedder.model());

    // Create temporary database
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("semantic_search.db");
    let db = ColumnFamilyDatabase::open(&db_path)?;
    let cf = db.column_family_or_create("documents")?;

    // Encode and store documents
    println!("Encoding {} documents...", documents.len());
    let start = std::time::Instant::now();

    {
        let write_txn = cf.begin_write()?;
        let mut vectors = VectorTable::<768>::open(&write_txn, "embeddings")?;

        // Batch encode all document texts
        let texts: Vec<&str> = documents.iter().map(|(_, text)| *text).collect();
        let embeddings = embedder.encode_batch(&texts)?;
        println!("Encoded in {:.2}s", start.elapsed().as_secs_f32());

        // Insert embeddings with document IDs
        println!("Storing embeddings in manifold...");
        for (i, (doc_id, _)) in documents.iter().enumerate() {
            let embedding: [f32; 768] = embeddings[i]
                .embedding
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .try_into()
                .expect("embedding dimension mismatch");
            vectors.insert(doc_id, &embedding)?;
        }

        drop(vectors);
        write_txn.commit()?;
    }

    println!("Stored {} vectors\n", documents.len());

    // Perform semantic search
    let queries = vec![
        "How do neural networks learn?",
        "What is semantic similarity?",
        "Explain language models",
    ];

    let read_txn = cf.begin_read()?;
    let vectors = VectorTableRead::<768>::open(&read_txn, "embeddings")?;

    for query_text in &queries {
        println!("─────────────────────────────────────────────────────");
        println!("Query: \"{}\"", query_text);
        println!();

        // Encode query
        let query_embedding = embedder.encode(query_text)?;
        let query_vec: [f32; 768] = query_embedding
            .embedding
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .try_into()
            .expect("query embedding dimension mismatch");

        // Compute similarity with all documents
        let mut scores: Vec<(String, f32)> = Vec::new();

        for result in vectors.all_vectors()? {
            let (doc_id, doc_guard) = result?;
            let similarity = distance::cosine(&query_vec, doc_guard.value());
            scores.push((doc_id, similarity));
        }

        // Sort by similarity (descending)
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Display top-5 results
        println!("Top 5 most similar documents:");
        for (rank, (doc_id, score)) in scores.iter().take(5).enumerate() {
            let doc_text = documents
                .iter()
                .find(|(id, _)| id == doc_id)
                .map(|(_, text)| text)
                .unwrap_or(&"");
            println!("  {}. [Score: {:.4}] {}", rank + 1, score, doc_id);
            println!("     \"{}\"", doc_text);
        }
        println!();
    }

    println!("─────────────────────────────────────────────────────");
    println!("\n✓ Example complete!");
    println!("\nKey takeaways:");
    println!("  • VectorTable<768> provides type-safe storage for fixed-dimension vectors");
    println!("  • Guard-based access enables zero-copy reads with no heap allocations");
    println!("  • Batch encoding with Tessera provides 5-10x speedup over individual encodes");
    println!("  • Cosine similarity search over manifold-stored embeddings is efficient");
    println!(
        "  • {} vectors stored in {:.2} KB",
        documents.len(),
        (documents.len() * 768 * 4) as f32 / 1024.0
    );

    Ok(())
}
