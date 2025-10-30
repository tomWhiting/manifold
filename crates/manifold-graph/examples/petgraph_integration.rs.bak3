//! Petgraph Integration Example
//!
//! Demonstrates how to use manifold-graph with petgraph, Rust's most popular
//! graph algorithm library. Shows:
//! - Converting manifold-graph edges to petgraph DiGraph
//! - Running PageRank for node importance
//! - Finding strongly connected components
//! - Computing shortest paths
//! - Calculating centrality measures
//!
//! This example shows how manifold-graph provides efficient storage while
//! petgraph provides algorithmic capabilities.

use manifold::column_family::ColumnFamilyDatabase;
use manifold_graph::{GraphTable, GraphTableRead};
use petgraph::algo::{dijkstra, kosaraju_scc};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct Page {
    id: Uuid,
    url: String,
}

impl Page {
    fn new(url: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            url: url.into(),
        }
    }
}

/// Simple PageRank implementation for demonstration
fn pagerank(
    graph: &DiGraph<Uuid, f32>,
    iterations: usize,
    damping: f32,
) -> HashMap<NodeIndex, f32> {
    let node_count = graph.node_count();
    let mut ranks: HashMap<NodeIndex, f32> = graph
        .node_indices()
        .map(|idx| (idx, 1.0 / node_count as f32))
        .collect();

    for _ in 0..iterations {
        let mut new_ranks = HashMap::new();

        for node in graph.node_indices() {
            let mut rank = (1.0 - damping) / node_count as f32;

            for incoming in graph.neighbors_directed(node, petgraph::Direction::Incoming) {
                let incoming_rank = ranks[&incoming];
                let outgoing_count = graph
                    .neighbors_directed(incoming, petgraph::Direction::Outgoing)
                    .count();
                if outgoing_count > 0 {
                    rank += damping * incoming_rank / outgoing_count as f32;
                }
            }

            new_ranks.insert(node, rank);
        }

        ranks = new_ranks;
    }

    ranks
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Petgraph Integration Example ===\n");

    // Create database with web page graph
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("webgraph.db");
    let db = ColumnFamilyDatabase::open(&db_path)?;
    let cf = db.column_family_or_create("web")?;

    // Create pages (nodes)
    let home = Page::new("example.com/");
    let about = Page::new("example.com/about");
    let blog = Page::new("example.com/blog");
    let post1 = Page::new("example.com/blog/post1");
    let post2 = Page::new("example.com/blog/post2");
    let contact = Page::new("example.com/contact");
    let external = Page::new("external.com/");

    let pages = vec![&home, &about, &blog, &post1, &post2, &contact, &external];

    println!("Created {} pages", pages.len());
    for page in &pages {
        println!("  - {}", page.url);
    }
    println!();

    // Build link graph using batch insertion
    println!("Building link graph with batch insertion...");
    {
        let write_txn = cf.begin_write()?;
        let mut graph = GraphTable::open(&write_txn, "links")?;

        let links = vec![
            // Home page links to everything
            (home.id, "links_to", about.id, true, 1.0),
            (home.id, "links_to", blog.id, true, 1.0),
            (home.id, "links_to", contact.id, true, 1.0),
            // About links back to home
            (about.id, "links_to", home.id, true, 1.0),
            // Blog links to posts
            (blog.id, "links_to", post1.id, true, 1.0),
            (blog.id, "links_to", post2.id, true, 1.0),
            (blog.id, "links_to", home.id, true, 1.0),
            // Posts link to each other and blog
            (post1.id, "links_to", post2.id, true, 1.0),
            (post1.id, "links_to", blog.id, true, 1.0),
            (post2.id, "links_to", post1.id, true, 1.0),
            (post2.id, "links_to", blog.id, true, 1.0),
            // Contact links to home
            (contact.id, "links_to", home.id, true, 1.0),
            // External site links in
            (external.id, "links_to", home.id, true, 1.0),
        ];

        let count = graph.add_edges_batch(&links, false)?;
        println!("Inserted {} links in batch\n", count);

        drop(graph);
        write_txn.commit()?;
    }

    // Read graph and convert to petgraph
    println!("─────────────────────────────────────────");
    println!("Converting to petgraph DiGraph...");

    let read_txn = cf.begin_read()?;
    let graph_read = GraphTableRead::open(&read_txn, "links")?;

    // Build petgraph DiGraph
    let mut petgraph_graph: DiGraph<Uuid, f32> = DiGraph::new();
    let mut node_map: HashMap<Uuid, NodeIndex> = HashMap::new();

    // Add all nodes
    for page in &pages {
        let idx = petgraph_graph.add_node(page.id);
        node_map.insert(page.id, idx);
    }

    // Add all edges using EdgeSource trait
    for edge_result in graph_read.all_edges()? {
        let edge = edge_result?;
        if edge.edge_type == "links_to" && edge.is_active {
            let source_idx = node_map[&edge.source];
            let target_idx = node_map[&edge.target];
            petgraph_graph.add_edge(source_idx, target_idx, edge.weight);
        }
    }

    println!(
        "Petgraph created: {} nodes, {} edges\n",
        petgraph_graph.node_count(),
        petgraph_graph.edge_count()
    );

    // 1. Run PageRank
    println!("─────────────────────────────────────────");
    println!("Running PageRank (15 iterations)...");

    let page_ranks = pagerank(&petgraph_graph, 15, 0.85);

    // Create reverse lookup
    let idx_to_page: HashMap<NodeIndex, &Page> =
        pages.iter().map(|p| (node_map[&p.id], *p)).collect();

    let mut ranked: Vec<_> = page_ranks.iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());

    println!("\nTop pages by PageRank:");
    for (idx, rank) in ranked.iter().take(5) {
        let page = idx_to_page[idx];
        println!("  {:.4} - {}", rank, page.url);
    }
    println!();

    // 2. Find Strongly Connected Components
    println!("─────────────────────────────────────────");
    println!("Finding Strongly Connected Components...");

    let sccs = kosaraju_scc(&petgraph_graph);

    println!("Found {} strongly connected components:", sccs.len());
    for (i, component) in sccs.iter().enumerate() {
        if component.len() > 1 {
            println!("  Component {} ({} nodes):", i + 1, component.len());
            for node_idx in component {
                let page = idx_to_page[node_idx];
                println!("    - {}", page.url);
            }
        }
    }
    println!();

    // 3. Shortest path from home to contact
    println!("─────────────────────────────────────────");
    println!("Computing shortest path: home → contact");

    let home_idx = node_map[&home.id];
    let contact_idx = node_map[&contact.id];

    let distances = dijkstra(&petgraph_graph, home_idx, Some(contact_idx), |_| 1);

    if let Some(distance) = distances.get(&contact_idx) {
        println!("Shortest path distance: {} hops", distance);
    } else {
        println!("No path found");
    }
    println!();

    // 4. Calculate in-degree (popularity)
    println!("─────────────────────────────────────────");
    println!("Page popularity (in-degree centrality):");

    let mut in_degrees: Vec<_> = pages
        .iter()
        .map(|p| {
            let idx = node_map[&p.id];
            let degree = petgraph_graph
                .neighbors_directed(idx, petgraph::Direction::Incoming)
                .count();
            (p, degree)
        })
        .collect();

    in_degrees.sort_by(|a, b| b.1.cmp(&a.1));

    for (page, degree) in in_degrees.iter().take(5) {
        println!("  {} incoming links - {}", degree, page.url);
    }
    println!();

    // 5. Statistics
    println!("─────────────────────────────────────────");
    println!("Graph Statistics:");
    println!("  Total pages: {}", petgraph_graph.node_count());
    println!("  Total links: {}", petgraph_graph.edge_count());

    let avg_out_degree = petgraph_graph.edge_count() as f32 / petgraph_graph.node_count() as f32;
    println!("  Average out-degree: {:.2}", avg_out_degree);

    let max_in_degree = in_degrees.iter().map(|(_, d)| d).max().unwrap_or(&0);
    println!("  Max in-degree: {}", max_in_degree);

    println!();
    println!("─────────────────────────────────────────");
    println!("\n✓ Example complete!");
    println!("\nKey takeaways:");
    println!("  • manifold-graph provides efficient persistent storage");
    println!("  • EdgeSource trait enables seamless petgraph integration");
    println!("  • Batch insertion improves write throughput for large graphs");
    println!("  • petgraph algorithms work directly on converted graph");
    println!("  • PageRank identifies most important nodes");
    println!("  • SCC finds cyclic subgraphs");
    println!("  • Dijkstra computes shortest paths");
    println!("  • Centrality measures reveal structural importance");

    Ok(())
}
