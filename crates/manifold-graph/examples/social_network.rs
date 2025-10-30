//! Social Network Graph Example
//!
//! Demonstrates:
//! - Creating a social network with multiple edge types
//! - Bidirectional edge queries (followers/following)
//! - Edge properties (active/passive, weights)
//! - Atomic multi-edge updates
//! - Edge type filtering
//!
//! This example models a Twitter-like social network where users can:
//! - Follow other users (directed edges)
//! - Block users (prevents interaction)
//! - Mute users (hidden from feed)

use manifold::column_family::ColumnFamilyDatabase;
use manifold_graph::{Edge, GraphTable, GraphTableRead};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug)]
struct User {
    id: Uuid,
    username: String,
}

impl User {
    fn new(username: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            username: username.into(),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Social Network Graph Example ===\n");

    // Create database
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("social_network.db");
    let db = ColumnFamilyDatabase::open(&db_path)?;
    let cf = db.column_family_or_create("social")?;

    // Create users
    let alice = User::new("alice");
    let bob = User::new("bob");
    let charlie = User::new("charlie");
    let diana = User::new("diana");
    let eve = User::new("eve");

    let users = vec![&alice, &bob, &charlie, &diana, &eve];
    let user_map: HashMap<Uuid, &str> = users
        .iter()
        .map(|u| (u.id, u.username.as_str()))
        .collect();

    println!("Created {} users:", users.len());
    for user in &users {
        println!("  - {} ({})", user.username, user.id);
    }
    println!();

    // Build social graph
    println!("Building social network...");
    {
        let write_txn = cf.begin_write()?;
        let mut graph = GraphTable::open(&write_txn, "connections")?;

        // Alice follows Bob and Charlie
        graph.add_edge(&alice.id, "follows", &bob.id, true, 1.0, None)?;
        graph.add_edge(&alice.id, "follows", &charlie.id, true, 0.8, None)?;

        // Bob follows Alice and Diana
        graph.add_edge(&bob.id, "follows", &alice.id, true, 1.0, None)?;
        graph.add_edge(&bob.id, "follows", &diana.id, true, 0.9, None)?;

        // Charlie follows everyone except Eve
        graph.add_edge(&charlie.id, "follows", &alice.id, true, 0.7, None)?;
        graph.add_edge(&charlie.id, "follows", &bob.id, true, 0.6, None)?;
        graph.add_edge(&charlie.id, "follows", &diana.id, true, 0.5, None)?;

        // Diana follows Alice
        graph.add_edge(&diana.id, "follows", &alice.id, true, 0.9, None)?;

        // Alice blocks Eve (active=false indicates blocked)
        graph.add_edge(&alice.id, "blocks", &eve.id, true, 1.0, None)?;

        // Bob mutes Charlie
        graph.add_edge(&bob.id, "mutes", &charlie.id, true, 1.0, None)?;

        drop(graph);
        write_txn.commit()?;
    }
    println!("Social graph created!\n");

    // Query the graph
    let read_txn = cf.begin_read()?;
    let graph = GraphTableRead::open(&read_txn, "connections")?;

    // 1. Who does Alice follow?
    println!("─────────────────────────────────────────");
    println!("Who does Alice follow?");
    let following: Vec<Edge> = graph
        .outgoing_edges(&alice.id)?
        .filter_map(|r| r.ok())
        .filter(|e| e.edge_type == "follows" && e.is_active)
        .collect();

    for edge in &following {
        let username = user_map.get(&edge.target).unwrap_or(&"unknown");
        println!(
            "  → {} (weight: {:.1})",
            username, edge.weight
        );
    }
    println!("  Total: {} users\n", following.len());

    // 2. Who follows Alice?
    println!("─────────────────────────────────────────");
    println!("Who follows Alice?");
    let followers: Vec<Edge> = graph
        .incoming_edges(&alice.id)?
        .filter_map(|r| r.ok())
        .filter(|e| e.edge_type == "follows" && e.is_active)
        .collect();

    for edge in &followers {
        let username = user_map.get(&edge.source).unwrap_or(&"unknown");
        println!(
            "  ← {} (weight: {:.1})",
            username, edge.weight
        );
    }
    println!("  Total: {} followers\n", followers.len());

    // 3. Mutual follows (Alice follows them AND they follow Alice)
    println!("─────────────────────────────────────────");
    println!("Alice's mutual connections:");
    let alice_following: HashMap<Uuid, f32> = following
        .iter()
        .map(|e| (e.target, e.weight))
        .collect();

    for edge in &followers {
        if alice_following.contains_key(&edge.source) {
            let username = user_map.get(&edge.source).unwrap_or(&"unknown");
            println!("  ⟷ {} (mutual)", username);
        }
    }
    println!();

    // 4. Most popular user (most followers)
    println!("─────────────────────────────────────────");
    println!("Most popular users (by follower count):");
    let mut follower_counts: Vec<(Uuid, usize)> = users
        .iter()
        .map(|user| {
            let count = graph
                .incoming_edges(&user.id)
                .ok()
                .map(|iter| {
                    iter.filter_map(|r| r.ok())
                        .filter(|e| e.edge_type == "follows" && e.is_active)
                        .count()
                })
                .unwrap_or(0);
            (user.id, count)
        })
        .collect();

    follower_counts.sort_by(|a, b| b.1.cmp(&a.1));

    for (user_id, count) in follower_counts.iter().take(3) {
        let username = user_map.get(user_id).unwrap_or(&"unknown");
        println!("  {}. {} - {} followers", 
            follower_counts.iter().position(|(id, _)| id == user_id).unwrap() + 1,
            username, 
            count
        );
    }
    println!();

    // 5. Check blocks and mutes
    println!("─────────────────────────────────────────");
    println!("Blocked and muted users:");

    // Alice's blocks
    let blocks: Vec<Edge> = graph
        .outgoing_edges(&alice.id)?
        .filter_map(|r| r.ok())
        .filter(|e| e.edge_type == "blocks")
        .collect();

    for edge in &blocks {
        let username = user_map.get(&edge.target).unwrap_or(&"unknown");
        println!("  Alice blocked: {}", username);
    }

    // Bob's mutes
    let mutes: Vec<Edge> = graph
        .outgoing_edges(&bob.id)?
        .filter_map(|r| r.ok())
        .filter(|e| e.edge_type == "mutes")
        .collect();

    for edge in &mutes {
        let username = user_map.get(&edge.target).unwrap_or(&"unknown");
        println!("  Bob muted: {}", username);
    }
    println!();

    // 6. Demonstrate edge update (unfollow)
    println!("─────────────────────────────────────────");
    println!("Demonstrating edge update:");
    println!("  Alice unfollows Charlie...");
    
    {
        let write_txn = cf.begin_write()?;
        let mut graph_write = GraphTable::open(&write_txn, "connections")?;
        
        // Set is_active to false to indicate unfollowed
        graph_write.update_edge(&alice.id, "follows", &charlie.id, false, 0.0)?;
        
        drop(graph_write);
        write_txn.commit()?;
    }

    // Re-query to show the update
    let read_txn2 = cf.begin_read()?;
    let graph2 = GraphTableRead::open(&read_txn2, "connections")?;
    
    let alice_following_updated: Vec<Edge> = graph2
        .outgoing_edges(&alice.id)?
        .filter_map(|r| r.ok())
        .filter(|e| e.edge_type == "follows" && e.is_active)
        .collect();

    println!("  Alice now follows {} users", alice_following_updated.len());
    for edge in &alice_following_updated {
        let username = user_map.get(&edge.target).unwrap_or(&"unknown");
        println!("    → {}", username);
    }
    println!();

    // 7. Statistics
    println!("─────────────────────────────────────────");
    println!("Graph Statistics:");
    
    let total_edges = graph2.len()?;
    println!("  Total edges: {}", total_edges);
    
    println!("  Edge types used: follows, blocks, mutes");
    println!("  Average edges per user: {:.1}", total_edges as f64 / users.len() as f64);
    println!();

    println!("─────────────────────────────────────────");
    println!("\n✓ Example complete!");
    println!("\nKey takeaways:");
    println!("  • Bidirectional queries enable efficient follower/following lookups");
    println!("  • Edge types (follows, blocks, mutes) stored in single graph");
    println!("  • is_active property enables soft deletes and state management");
    println!("  • Weight property can represent relationship strength");
    println!("  • Atomic updates ensure consistency across forward/reverse indexes");
    println!("  • Range scans provide O(k) traversal where k = edges per vertex");

    Ok(())
}
