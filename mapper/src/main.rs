use anyhow::{Context, Result};
use clap::Parser;
use kernel_schema::AtomicUnit;
use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::DiGraph;
use petgraph::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "Map dependencies and detect cycles for Atomic Units")]
struct Args {
    /// Path to the units.json file
    #[arg(short, long, default_value = "units.json")]
    units: PathBuf,

    /// Path to the output build_order.json file
    #[arg(short, long, default_value = "build_order.json")]
    output: PathBuf,

    /// Analyze cycles and suggest refactoring strategies
    #[arg(long)]
    analyze_cycles: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct BuildOrderBatch {
    pub units: Vec<String>,
    pub is_super_node: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scc_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refactoring_difficulty: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BuildOrder {
    pub metadata: BuildMetadata,
    pub batches: Vec<BuildOrderBatch>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BuildMetadata {
    pub total_units: usize,
    pub total_batches: usize,
    pub super_nodes: usize,
    pub largest_super_node: usize,
    pub average_batch_size: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct CycleAnalysis {
    pub super_node: Vec<String>,
    pub size: usize,
    pub weak_edges: Vec<(String, String)>,
    pub refactoring_suggestions: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Mapper: Loading units from {:?}", args.units);
    let units_json = fs::read_to_string(&args.units)
        .with_context(|| format!("Failed to read units file {:?}", args.units))?;
    let units: Vec<AtomicUnit> = serde_json::from_str(&units_json)
        .context("Failed to parse units.json")?;

    println!("Mapper: Constructing dependency graph for {} units", units.len());
    let mut graph = DiGraph::<String, ()>::new();
    let mut nodes = HashMap::new();

    // Add nodes
    for unit in &units {
        let idx = graph.add_node(unit.id.clone());
        nodes.insert(unit.id.clone(), idx);
    }

    // Add edges
    for unit in &units {
        // Safe: we just inserted this node above, so it must exist
        let from_idx = nodes.get(&unit.id)
            .expect("Node was just inserted; this is a bug");
        for dep in &unit.dependencies {
            if let Some(to_idx) = nodes.get(dep) {
                graph.add_edge(*from_idx, *to_idx, ());
            }
        }
    }

    println!("Mapper: Running cycle detection (Tarjan's SCC)");
    let sccs = tarjan_scc(&graph);
    
    // Build a map from node index to SCC index
    let mut node_to_scc = HashMap::new();
    for (scc_idx, scc) in sccs.iter().enumerate() {
        for node_idx in scc {
            node_to_scc.insert(*node_idx, scc_idx);
        }
    }

    // Construct the condensed graph (DAG of SCCs)
    let mut cond_graph = DiGraph::<usize, ()>::new();
    for i in 0..sccs.len() {
        cond_graph.add_node(i);
    }

    for edge in graph.edge_indices() {
        // Safe: edge_endpoints only returns None for invalid edge indices,
        // but we're iterating over graph.edge_indices() so they must be valid
        let (u, v) = graph.edge_endpoints(edge)
            .expect("Edge index from edge_indices() must be valid");
        // Safe: all nodes were assigned SCC indices in the loop above
        let u_scc = *node_to_scc.get(&u)
            .expect("All nodes must have SCC assignments");
        let v_scc = *node_to_scc.get(&v)
            .expect("All nodes must have SCC assignments");
        if u_scc != v_scc {
            cond_graph.update_edge(NodeIndex::new(u_scc), NodeIndex::new(v_scc), ());
        }
    }

    println!("Mapper: Generating topological sort");
    let mut order = toposort(&cond_graph, None)
        .map_err(|_| anyhow::anyhow!("Cycle detected in condensed graph (should be impossible)"))?;
    
    order.reverse();

    let mut batches = Vec::new();
    let mut super_node_count = 0;
    let mut largest_super_node = 0;

    for scc_idx in order {
        let scc = &sccs[scc_idx.index()];
        let unit_ids: Vec<String> = scc.iter().map(|idx| graph[*idx].clone()).collect();
        let is_super_node = unit_ids.len() > 1;
        
        if is_super_node {
            super_node_count += 1;
            largest_super_node = largest_super_node.max(unit_ids.len());
            println!("Mapper: Detected Super Node: {:?}", unit_ids);
        }

        let difficulty = if unit_ids.len() > 20 {
            Some("Very High".to_string())
        } else if unit_ids.len() > 10 {
            Some("High".to_string())
        } else if unit_ids.len() > 5 {
            Some("Medium".to_string())
        } else if is_super_node {
            Some("Low".to_string())
        } else {
            None
        };

        batches.push(BuildOrderBatch {
            units: unit_ids,
            is_super_node,
            scc_size: if is_super_node { Some(scc.len()) } else { None },
            refactoring_difficulty: difficulty,
        });
    }

    // Validation warnings
    for batch in &batches {
        if let Some(size) = batch.scc_size {
            if size > 20 {
                eprintln!("WARNING: Super Node with {} functions detected. Consider breaking this cycle.", size);
            }
        }
    }

    let metadata = BuildMetadata {
        total_units: units.len(),
        total_batches: batches.len(),
        super_nodes: super_node_count,
        largest_super_node,
        average_batch_size: units.len() as f64 / batches.len() as f64,
    };

    let build_order = BuildOrder { metadata, batches };

    let output_json = serde_json::to_string_pretty(&build_order)?;
    fs::write(&args.output, output_json)
        .with_context(|| format!("Failed to write build order to {:?}", args.output))?;

    println!("Mapper: Generated {} batches to {:?}", build_order.batches.len(), args.output);

    // Cycle analysis
    if args.analyze_cycles {
        println!("Mapper: Analyzing cycles...");
        let mut analyses = Vec::new();

        for scc in &sccs {
            if scc.len() > 1 {
                let unit_ids: Vec<String> = scc.iter().map(|idx| graph[*idx].clone()).collect();
                let weak_edges = find_weak_edges(&graph, scc);
                let suggestions = generate_refactoring_suggestions(scc.len(), &weak_edges);

                analyses.push(CycleAnalysis {
                    super_node: unit_ids,
                    size: scc.len(),
                    weak_edges,
                    refactoring_suggestions: suggestions,
                });
            }
        }

        let analysis_json = serde_json::to_string_pretty(&analyses)?;
        fs::write("cycle_analysis.json", analysis_json)?;
        println!("Mapper: Cycle analysis written to cycle_analysis.json");
    }

    Ok(())
}

/// Find edges that might be good candidates for breaking the cycle.
/// 
/// NOTE: This is a HEURISTIC, not an optimal minimum feedback arc set.
/// We identify edges where the target has low in-degree within the SCC,
/// reasoning that such edges might be easier to refactor.
/// 
/// For production use, consider implementing a proper minimum FAS algorithm.
fn find_weak_edges(graph: &DiGraph<String, ()>, scc: &[NodeIndex]) -> Vec<(String, String)> {
    let scc_set: HashSet<_> = scc.iter().copied().collect();
    let mut weak_edges = Vec::new();

    // A weak edge is one whose removal would break the SCC
    // For simplicity, we identify edges that are part of the minimum feedback arc set
    // Here we use a heuristic: edges with low in-degree targets
    for &node in scc {
        for edge in graph.edges(node) {
            let target = edge.target();
            if scc_set.contains(&target) {
                // Count in-degree within SCC
                let in_degree = graph.edges_directed(target, petgraph::Direction::Incoming)
                    .filter(|e| scc_set.contains(&e.source()))
                    .count();
                
                if in_degree <= 2 {
                    weak_edges.push((graph[node].clone(), graph[target].clone()));
                }
            }
        }
    }

    weak_edges
}

fn generate_refactoring_suggestions(size: usize, weak_edges: &[(String, String)]) -> Vec<String> {
    let mut suggestions = Vec::new();

    // Add confidence notice - this is a heuristic, not an optimal solution
    suggestions.push("NOTE: Suggestions are based on heuristic analysis (low in-degree edges).".to_string());

    if size > 20 {
        suggestions.push("CRITICAL: This Super Node is very large. Consider architectural refactoring.".to_string());
    }

    if !weak_edges.is_empty() {
        suggestions.push(format!(
            "Consider breaking {} weak edge(s) to simplify the cycle. [Confidence: Medium]",
            weak_edges.len()
        ));
        
        for (from, to) in weak_edges.iter().take(3) {
            suggestions.push(format!(
                "  - Extract interface between '{}' and '{}'",
                from, to
            ));
        }
    }

    if size <= 5 {
        suggestions.push("This is a small cycle. Refactor all functions together atomically. [Confidence: High]".to_string());
    }

    suggestions
}
