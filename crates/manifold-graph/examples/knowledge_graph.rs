//! Knowledge Graph Example
//!
//! Demonstrates building a movie/entertainment knowledge graph with:
//! - Multiple entity types (Person, Film, Studio, Genre)
//! - Rich relationship types (acted_in, directed, produced_by, genre_of)
//! - Complex multi-hop queries
//! - Pattern matching for recommendations
//! - Traversal algorithms
//!
//! This example shows how manifold-graph can model complex domain knowledge
//! with typed relationships and efficient querying.

use manifold::column_family::ColumnFamilyDatabase;
use manifold_graph::{GraphTable, GraphTableRead};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Debug, Clone)]
enum Entity {
    Person { id: Uuid, name: String },
    Film { id: Uuid, title: String, year: u16 },
    Studio { id: Uuid, name: String },
    Genre { id: Uuid, name: String },
}

impl Entity {
    fn id(&self) -> Uuid {
        match self {
            Entity::Person { id, .. } => *id,
            Entity::Film { id, .. } => *id,
            Entity::Studio { id, .. } => *id,
            Entity::Genre { id, .. } => *id,
        }
    }

    fn display(&self) -> String {
        match self {
            Entity::Person { name, .. } => format!("Person: {}", name),
            Entity::Film { title, year, .. } => format!("Film: {} ({})", title, year),
            Entity::Studio { name, .. } => format!("Studio: {}", name),
            Entity::Genre { name, .. } => format!("Genre: {}", name),
        }
    }

    fn person(name: impl Into<String>) -> Self {
        Entity::Person {
            id: Uuid::new_v4(),
            name: name.into(),
        }
    }

    fn film(title: impl Into<String>, year: u16) -> Self {
        Entity::Film {
            id: Uuid::new_v4(),
            title: title.into(),
            year,
        }
    }

    fn studio(name: impl Into<String>) -> Self {
        Entity::Studio {
            id: Uuid::new_v4(),
            name: name.into(),
        }
    }

    fn genre(name: impl Into<String>) -> Self {
        Entity::Genre {
            id: Uuid::new_v4(),
            name: name.into(),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Knowledge Graph Example ===\n");

    // Create database
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("movies.db");
    let db = ColumnFamilyDatabase::open(&db_path)?;
    let cf = db.column_family_or_create("entertainment")?;

    // Create entities
    println!("Creating knowledge graph entities...\n");

    // People
    let nolan = Entity::person("Christopher Nolan");
    let cillian = Entity::person("Cillian Murphy");
    let emily = Entity::person("Emily Blunt");
    let rdj = Entity::person("Robert Downey Jr.");
    let matt = Entity::person("Matt Damon");
    let florence = Entity::person("Florence Pugh");

    // Films
    let oppenheimer = Entity::film("Oppenheimer", 2023);
    let inception = Entity::film("Inception", 2010);
    let interstellar = Entity::film("Interstellar", 2014);
    let dunkirk = Entity::film("Dunkirk", 2017);

    // Studios
    let universal = Entity::studio("Universal Pictures");
    let warner = Entity::studio("Warner Bros");
    let syncopy = Entity::studio("Syncopy");

    // Genres
    let drama = Entity::genre("Drama");
    let scifi = Entity::genre("Sci-Fi");
    let thriller = Entity::genre("Thriller");
    let history = Entity::genre("Historical");
    let war = Entity::genre("War");

    let entities = [
        &nolan,
        &cillian,
        &emily,
        &rdj,
        &matt,
        &florence,
        &oppenheimer,
        &inception,
        &interstellar,
        &dunkirk,
        &universal,
        &warner,
        &syncopy,
        &drama,
        &scifi,
        &thriller,
        &history,
        &war,
    ];

    // Build knowledge graph with batch insertion
    println!(
        "Building knowledge graph with {} entities...",
        entities.len()
    );
    {
        let write_txn = cf.begin_write()?;
        let mut graph = GraphTable::open(&write_txn, "knowledge")?;

        #[allow(clippy::vec_init_then_push)]
        let relationships = {
            let mut relationships = vec![];

            // Oppenheimer relationships
            relationships.push((nolan.id(), "directed", oppenheimer.id(), true, 1.0));
            relationships.push((cillian.id(), "acted_in", oppenheimer.id(), true, 1.0));
            relationships.push((emily.id(), "acted_in", oppenheimer.id(), true, 0.9));
            relationships.push((rdj.id(), "acted_in", oppenheimer.id(), true, 0.95));
            relationships.push((matt.id(), "acted_in", oppenheimer.id(), true, 0.7));
            relationships.push((florence.id(), "acted_in", oppenheimer.id(), true, 0.8));
            relationships.push((oppenheimer.id(), "produced_by", universal.id(), true, 1.0));
            relationships.push((oppenheimer.id(), "produced_by", syncopy.id(), true, 1.0));
            relationships.push((oppenheimer.id(), "genre_of", drama.id(), true, 1.0));
            relationships.push((oppenheimer.id(), "genre_of", history.id(), true, 1.0));
            relationships.push((oppenheimer.id(), "genre_of", thriller.id(), true, 0.8));

            // Inception relationships
            relationships.push((nolan.id(), "directed", inception.id(), true, 1.0));
            relationships.push((cillian.id(), "acted_in", inception.id(), true, 0.7));
            relationships.push((inception.id(), "produced_by", warner.id(), true, 1.0));
            relationships.push((inception.id(), "produced_by", syncopy.id(), true, 1.0));
            relationships.push((inception.id(), "genre_of", scifi.id(), true, 1.0));
            relationships.push((inception.id(), "genre_of", thriller.id(), true, 0.9));

            // Interstellar relationships
            relationships.push((nolan.id(), "directed", interstellar.id(), true, 1.0));
            relationships.push((matt.id(), "acted_in", interstellar.id(), true, 1.0));
            relationships.push((interstellar.id(), "produced_by", warner.id(), true, 1.0));
            relationships.push((interstellar.id(), "produced_by", syncopy.id(), true, 1.0));
            relationships.push((interstellar.id(), "genre_of", scifi.id(), true, 1.0));
            relationships.push((interstellar.id(), "genre_of", drama.id(), true, 0.8));

            // Dunkirk relationships
            relationships.push((nolan.id(), "directed", dunkirk.id(), true, 1.0));
            relationships.push((dunkirk.id(), "produced_by", warner.id(), true, 1.0));
            relationships.push((dunkirk.id(), "produced_by", syncopy.id(), true, 1.0));
            relationships.push((dunkirk.id(), "genre_of", war.id(), true, 1.0));
            relationships.push((dunkirk.id(), "genre_of", history.id(), true, 1.0));
            relationships.push((dunkirk.id(), "genre_of", thriller.id(), true, 0.7));

            relationships
        };

        let count = graph.add_edges_batch(&relationships, false)?;
        println!("Created {} relationships\n", count);

        drop(graph);
        write_txn.commit()?;
    }

    // Query the knowledge graph
    let read_txn = cf.begin_read()?;
    let graph = GraphTableRead::open(&read_txn, "knowledge")?;

    // Create entity lookup map
    let entity_map: HashMap<Uuid, &Entity> = entities.iter().map(|e| (e.id(), *e)).collect();

    // 1. Find all films directed by Nolan
    println!("─────────────────────────────────────────");
    println!("Query: Films directed by Christopher Nolan");
    println!();

    let nolan_films: Vec<_> = graph
        .outgoing_edges(&nolan.id())?
        .filter_map(|r| r.ok())
        .filter(|e| e.edge_type == "directed" && e.is_active)
        .collect();

    println!("Found {} films:", nolan_films.len());
    for edge in &nolan_films {
        let film = entity_map[&edge.target];
        println!("  - {}", film.display());
    }
    println!();

    // 2. Find all actors in Oppenheimer
    println!("─────────────────────────────────────────");
    println!("Query: Actors in Oppenheimer");
    println!();

    let actors: Vec<_> = graph
        .incoming_edges(&oppenheimer.id())?
        .filter_map(|r| r.ok())
        .filter(|e| e.edge_type == "acted_in" && e.is_active)
        .collect();

    println!("Cast ({} actors):", actors.len());
    let mut sorted_actors = actors.clone();
    sorted_actors.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());

    for edge in &sorted_actors {
        let actor = entity_map[&edge.source];
        println!("  - {} (weight: {:.2})", actor.display(), edge.weight);
    }
    println!();

    // 3. Find actors who worked with Nolan on multiple films
    println!("─────────────────────────────────────────");
    println!("Query: Actors who worked with Nolan on multiple films");
    println!();

    let mut actor_film_count: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

    for nolan_film in &nolan_films {
        let film_id = nolan_film.target;
        for edge in graph.incoming_edges(&film_id)? {
            let edge = edge?;
            if edge.edge_type == "acted_in" && edge.is_active {
                actor_film_count
                    .entry(edge.source)
                    .or_default()
                    .push(film_id);
            }
        }
    }

    let frequent_collaborators: Vec<_> = actor_film_count
        .iter()
        .filter(|(_, films)| films.len() > 1)
        .collect();

    println!(
        "Found {} frequent collaborators:",
        frequent_collaborators.len()
    );
    for (actor_id, film_ids) in &frequent_collaborators {
        let actor = entity_map[actor_id];
        println!("  {} - {} films:", actor.display(), film_ids.len());
        for film_id in *film_ids {
            let film = entity_map[film_id];
            println!("    • {}", film.display());
        }
    }
    println!();

    // 4. Find films in the same genres as Oppenheimer
    println!("─────────────────────────────────────────");
    println!("Query: Films similar to Oppenheimer (by genre)");
    println!();

    // Get Oppenheimer's genres
    let opp_genres: HashSet<Uuid> = graph
        .outgoing_edges(&oppenheimer.id())?
        .filter_map(|r| r.ok())
        .filter(|e| e.edge_type == "genre_of" && e.is_active)
        .map(|e| e.target)
        .collect();

    println!("Oppenheimer genres:");
    for genre_id in &opp_genres {
        let genre = entity_map[genre_id];
        println!("  - {}", genre.display());
    }
    println!();

    // Find other films with overlapping genres
    let mut film_genre_overlap: HashMap<Uuid, usize> = HashMap::new();

    for genre_id in &opp_genres {
        for edge in graph.incoming_edges(genre_id)? {
            let edge = edge?;
            if edge.edge_type == "genre_of" && edge.is_active && edge.source != oppenheimer.id() {
                *film_genre_overlap.entry(edge.source).or_insert(0) += 1;
            }
        }
    }

    let mut similar_films: Vec<_> = film_genre_overlap.iter().collect();
    similar_films.sort_by(|a, b| b.1.cmp(a.1));

    println!("Similar films (by genre overlap):");
    for (film_id, overlap_count) in &similar_films {
        let film = entity_map[film_id];
        println!("  - {} ({} shared genres)", film.display(), overlap_count);
    }
    println!();

    // 5. Find all studios that produced Nolan films
    println!("─────────────────────────────────────────");
    println!("Query: Studios that produced Nolan films");
    println!();

    let mut nolan_studios: HashSet<Uuid> = HashSet::new();

    for nolan_film in &nolan_films {
        for edge in graph.outgoing_edges(&nolan_film.target)? {
            let edge = edge?;
            if edge.edge_type == "produced_by" && edge.is_active {
                nolan_studios.insert(edge.target);
            }
        }
    }

    println!("Studios ({}):", nolan_studios.len());
    for studio_id in &nolan_studios {
        let studio = entity_map[studio_id];
        println!("  - {}", studio.display());
    }
    println!();

    // 6. Recommendation: If you liked Oppenheimer, you might like...
    println!("─────────────────────────────────────────");
    println!("Recommendation Engine: Based on Oppenheimer");
    println!();

    println!("Recommendations:");
    println!("  1. Same director (Nolan):");
    for edge in &nolan_films {
        if edge.target != oppenheimer.id() {
            let film = entity_map[&edge.target];
            println!("     → {}", film.display());
        }
    }

    println!("  2. Similar genres:");
    for (film_id, overlap) in similar_films.iter().take(2) {
        let film = entity_map[film_id];
        println!("     → {} ({} shared genres)", film.display(), overlap);
    }
    println!();

    // 7. Statistics
    println!("─────────────────────────────────────────");
    println!("Knowledge Graph Statistics:");
    println!("  Total entities: {}", entities.len());
    println!("  Total relationships: {}", graph.len()?);

    let edge_types: HashSet<String> = graph
        .all_edges()?
        .filter_map(|r| r.ok())
        .map(|e| e.edge_type.to_string())
        .collect();
    println!("  Relationship types: {}", edge_types.len());
    println!(
        "    Types: {}",
        edge_types
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    println!();
    println!("─────────────────────────────────────────");
    println!("\n✓ Example complete!");
    println!("\nKey takeaways:");
    println!("  • Knowledge graphs model complex domain relationships");
    println!("  • Multiple entity types stored with UUID identifiers");
    println!("  • Rich relationship types (directed, acted_in, genre_of, etc.)");
    println!("  • Multi-hop queries traverse relationships efficiently");
    println!("  • Pattern matching enables recommendations");
    println!("  • Bidirectional indexes support both directions");
    println!("  • Edge weights can represent relationship strength");
    println!("  • Batch insertion efficient for initial graph construction");

    Ok(())
}
