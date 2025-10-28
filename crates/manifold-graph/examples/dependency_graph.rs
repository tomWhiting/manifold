//! Dependency Graph Example
//!
//! Demonstrates using manifold-graph for build system/package management with:
//! - Package/module dependencies
//! - Cycle detection
//! - Topological sorting (build order)
//! - Critical path analysis
//! - Impact analysis (what breaks if X is removed)
//! - Dependency tree visualization
//!
//! This example shows practical software engineering use cases where
//! manifold-graph provides persistent, queryable dependency tracking.

use manifold::column_family::ColumnFamilyDatabase;
use manifold_graph::{GraphTable, GraphTableRead};
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct Package {
    id: Uuid,
    name: String,
    version: String,
}

impl Package {
    fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            version: version.into(),
        }
    }

    fn display(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

/// Detect cycles using DFS
fn detect_cycles(
    graph: &GraphTableRead,
    packages: &[&Package],
) -> Result<Vec<Vec<Uuid>>, Box<dyn std::error::Error>> {
    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();
    let mut cycles = Vec::new();

    fn dfs(
        node: Uuid,
        graph: &GraphTableRead,
        visited: &mut HashSet<Uuid>,
        rec_stack: &mut HashSet<Uuid>,
        path: &mut Vec<Uuid>,
        cycles: &mut Vec<Vec<Uuid>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        visited.insert(node);
        rec_stack.insert(node);
        path.push(node);

        for edge in graph.outgoing_edges(&node)? {
            let edge = edge?;
            if edge.edge_type == "depends_on" && edge.is_active {
                if !visited.contains(&edge.target) {
                    dfs(edge.target, graph, visited, rec_stack, path, cycles)?;
                } else if rec_stack.contains(&edge.target) {
                    // Found cycle
                    let cycle_start = path.iter().position(|&id| id == edge.target).unwrap();
                    cycles.push(path[cycle_start..].to_vec());
                }
            }
        }

        path.pop();
        rec_stack.remove(&node);
        Ok(())
    }

    for package in packages {
        if !visited.contains(&package.id) {
            let mut path = Vec::new();
            dfs(
                package.id,
                graph,
                &mut visited,
                &mut rec_stack,
                &mut path,
                &mut cycles,
            )?;
        }
    }

    Ok(cycles)
}

/// Topological sort using Kahn's algorithm
fn topological_sort(
    graph: &GraphTableRead,
    packages: &[&Package],
) -> Result<Option<Vec<Uuid>>, Box<dyn std::error::Error>> {
    // Calculate in-degrees
    let mut in_degree: HashMap<Uuid, usize> = HashMap::new();
    for package in packages {
        in_degree.insert(package.id, 0);
    }

    for package in packages {
        for edge in graph.outgoing_edges(&package.id)? {
            let edge = edge?;
            if edge.edge_type == "depends_on" && edge.is_active {
                *in_degree.entry(edge.target).or_insert(0) += 1;
            }
        }
    }

    // Find nodes with no dependencies
    let mut queue: VecDeque<Uuid> = in_degree
        .iter()
        .filter(|&(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut result = Vec::new();

    while let Some(node) = queue.pop_front() {
        result.push(node);

        for edge in graph.outgoing_edges(&node)? {
            let edge = edge?;
            if edge.edge_type == "depends_on" && edge.is_active
                && let Some(deg) = in_degree.get_mut(&edge.target) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(edge.target);
                    }
                }
        }
    }

    if result.len() == packages.len() {
        Ok(Some(result))
    } else {
        Ok(None) // Cycle detected
    }
}

/// Find all packages that depend on a given package (directly or transitively)
fn find_dependents(
    graph: &GraphTableRead,
    package_id: Uuid,
) -> Result<HashSet<Uuid>, Box<dyn std::error::Error>> {
    let mut dependents = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(package_id);

    while let Some(current) = queue.pop_front() {
        for edge in graph.incoming_edges(&current)? {
            let edge = edge?;
            if edge.edge_type == "depends_on" && edge.is_active
                && dependents.insert(edge.source) {
                    queue.push_back(edge.source);
                }
        }
    }

    Ok(dependents)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Dependency Graph Example ===\n");

    // Create database
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("dependencies.db");
    let db = ColumnFamilyDatabase::open(&db_path)?;
    let cf = db.column_family_or_create("packages")?;

    // Create packages
    println!("Creating package dependency graph...\n");

    let app = Package::new("my-app", "1.0.0");
    let web_framework = Package::new("web-framework", "2.5.0");
    let database = Package::new("database-client", "3.1.0");
    let auth = Package::new("auth-lib", "1.2.0");
    let logger = Package::new("logger", "0.9.0");
    let config = Package::new("config-parser", "2.0.0");
    let http = Package::new("http-client", "4.3.0");
    let json = Package::new("json-parser", "1.8.0");
    let crypto = Package::new("crypto", "5.0.1");

    let packages = vec![
        &app,
        &web_framework,
        &database,
        &auth,
        &logger,
        &config,
        &http,
        &json,
        &crypto,
    ];

    println!("Packages:");
    for pkg in &packages {
        println!("  - {}", pkg.display());
    }
    println!();

    // Build dependency graph
    println!("Building dependency relationships...");
    {
        let write_txn = cf.begin_write()?;
        let mut graph = GraphTable::open(&write_txn, "deps")?;

        let dependencies = vec![
            // my-app depends on everything
            (app.id, "depends_on", web_framework.id, true, 1.0),
            (app.id, "depends_on", database.id, true, 1.0),
            (app.id, "depends_on", auth.id, true, 1.0),
            (app.id, "depends_on", logger.id, true, 1.0),
            (app.id, "depends_on", config.id, true, 1.0),
            // web-framework depends on http and json
            (web_framework.id, "depends_on", http.id, true, 1.0),
            (web_framework.id, "depends_on", json.id, true, 1.0),
            (web_framework.id, "depends_on", logger.id, true, 1.0),
            // database depends on config and logger
            (database.id, "depends_on", config.id, true, 1.0),
            (database.id, "depends_on", logger.id, true, 1.0),
            // auth depends on crypto, http, and json
            (auth.id, "depends_on", crypto.id, true, 1.0),
            (auth.id, "depends_on", http.id, true, 1.0),
            (auth.id, "depends_on", json.id, true, 1.0),
            // http depends on logger
            (http.id, "depends_on", logger.id, true, 1.0),
        ];

        let count = graph.add_edges_batch(&dependencies, false)?;
        println!("Created {} dependencies\n", count);

        drop(graph);
        write_txn.commit()?;
    }

    // Query and analyze
    let read_txn = cf.begin_read()?;
    let graph = GraphTableRead::open(&read_txn, "deps")?;

    let package_map: HashMap<Uuid, &Package> = packages.iter().map(|p| (p.id, *p)).collect();

    // 1. Show dependency tree for my-app
    println!("─────────────────────────────────────────");
    println!("Dependency Tree: {}", app.display());
    println!();

    let app_deps: Vec<_> = graph
        .outgoing_edges(&app.id)?
        .filter_map(|r| r.ok())
        .filter(|e| e.edge_type == "depends_on" && e.is_active)
        .collect();

    for edge in &app_deps {
        let dep = package_map[&edge.target];
        println!("  ├─ {}", dep.display());

        // Show transitive dependencies
        for sub_edge in graph.outgoing_edges(&edge.target)? {
            let sub_edge = sub_edge?;
            if sub_edge.edge_type == "depends_on" && sub_edge.is_active {
                let sub_dep = package_map[&sub_edge.target];
                println!("  │  └─ {}", sub_dep.display());
            }
        }
    }
    println!();

    // 2. Detect cycles
    println!("─────────────────────────────────────────");
    println!("Cycle Detection:");
    println!();

    let cycles = detect_cycles(&graph, &packages)?;

    if cycles.is_empty() {
        println!("✓ No cycles detected - dependency graph is acyclic");
    } else {
        println!("✗ Cycles detected:");
        for cycle in &cycles {
            print!("  ");
            for (i, &pkg_id) in cycle.iter().enumerate() {
                if i > 0 {
                    print!(" → ");
                }
                print!("{}", package_map[&pkg_id].name);
            }
            println!();
        }
    }
    println!();

    // 3. Topological sort (build order)
    println!("─────────────────────────────────────────");
    println!("Build Order (Topological Sort):");
    println!();

    if let Some(build_order) = topological_sort(&graph, &packages)? {
        println!("Packages should be built in this order:");
        for (i, &pkg_id) in build_order.iter().enumerate() {
            let pkg = package_map[&pkg_id];
            println!("  {}. {}", i + 1, pkg.display());
        }
    } else {
        println!("Cannot determine build order - cycle detected!");
    }
    println!();

    // 4. Find most depended-upon packages
    println!("─────────────────────────────────────────");
    println!("Most Popular Packages (by in-degree):");
    println!();

    let mut in_degrees: Vec<_> = packages
        .iter()
        .map(|p| {
            let count = graph
                .incoming_edges(&p.id)
                .ok()
                .map(|iter| {
                    iter.filter_map(|r| r.ok())
                        .filter(|e| e.edge_type == "depends_on" && e.is_active)
                        .count()
                })
                .unwrap_or(0);
            (p, count)
        })
        .collect();

    in_degrees.sort_by(|a, b| b.1.cmp(&a.1));

    for (pkg, count) in in_degrees.iter().take(5) {
        if *count > 0 {
            println!("  {} - {} packages depend on this", pkg.display(), count);
        }
    }
    println!();

    // 5. Impact analysis: What breaks if we remove logger?
    println!("─────────────────────────────────────────");
    println!("Impact Analysis: Removing {}", logger.display());
    println!();

    let affected = find_dependents(&graph, logger.id)?;

    println!("Packages that would be affected:");
    for &pkg_id in &affected {
        let pkg = package_map[&pkg_id];
        println!("  - {}", pkg.display());
    }
    println!("Total affected: {} packages", affected.len());
    println!();

    // 6. Critical path (longest dependency chain)
    println!("─────────────────────────────────────────");
    println!("Dependency Depth Analysis:");
    println!();

    fn calc_depth(
        pkg_id: Uuid,
        graph: &GraphTableRead,
        memo: &mut HashMap<Uuid, usize>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if let Some(&depth) = memo.get(&pkg_id) {
            return Ok(depth);
        }

        let mut max_depth = 0;
        for edge in graph.outgoing_edges(&pkg_id)? {
            let edge = edge?;
            if edge.edge_type == "depends_on" && edge.is_active {
                let dep_depth = calc_depth(edge.target, graph, memo)?;
                max_depth = max_depth.max(dep_depth + 1);
            }
        }

        memo.insert(pkg_id, max_depth);
        Ok(max_depth)
    }

    let mut depth_memo = HashMap::new();
    let mut depths: Vec<_> = packages
        .iter()
        .map(|p| {
            let depth = calc_depth(p.id, &graph, &mut depth_memo).unwrap_or(0);
            (p, depth)
        })
        .collect();

    depths.sort_by(|a, b| b.1.cmp(&a.1));

    for (pkg, depth) in depths.iter().take(5) {
        println!("  {} - depth: {}", pkg.display(), depth);
    }
    println!();

    // 7. Find all transitive dependencies of my-app
    println!("─────────────────────────────────────────");
    println!("Complete Dependency Closure: {}", app.display());
    println!();

    fn collect_all_deps(
        pkg_id: Uuid,
        graph: &GraphTableRead,
        visited: &mut HashSet<Uuid>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for edge in graph.outgoing_edges(&pkg_id)? {
            let edge = edge?;
            if edge.edge_type == "depends_on" && edge.is_active
                && visited.insert(edge.target) {
                    collect_all_deps(edge.target, graph, visited)?;
                }
        }
        Ok(())
    }

    let mut all_deps = HashSet::new();
    collect_all_deps(app.id, &graph, &mut all_deps)?;

    println!("Total transitive dependencies: {}", all_deps.len());
    let mut sorted_deps: Vec<_> = all_deps.iter().map(|id| package_map[id]).collect();
    sorted_deps.sort_by_key(|p| &p.name);

    for pkg in &sorted_deps {
        println!("  - {}", pkg.display());
    }
    println!();

    // 8. Statistics
    println!("─────────────────────────────────────────");
    println!("Dependency Graph Statistics:");
    println!("  Total packages: {}", packages.len());
    println!("  Total dependencies: {}", graph.len()?);

    let avg_deps = graph.len()? as f32 / packages.len() as f32;
    println!("  Average dependencies per package: {:.2}", avg_deps);

    let max_depth = depths.iter().map(|(_, d)| d).max().unwrap_or(&0);
    println!("  Maximum dependency depth: {}", max_depth);

    println!();
    println!("─────────────────────────────────────────");
    println!("\n✓ Example complete!");
    println!("\nKey takeaways:");
    println!("  • Dependency graphs model package/module relationships");
    println!("  • Cycle detection prevents circular dependencies");
    println!("  • Topological sort provides correct build order");
    println!("  • Impact analysis shows what breaks when removing packages");
    println!("  • Dependency depth reveals complexity");
    println!("  • Transitive closure shows complete dependency set");
    println!("  • Bidirectional indexes enable both forward and reverse queries");
    println!("  • Persistent storage maintains dependency information");

    Ok(())
}
