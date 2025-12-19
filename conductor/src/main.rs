mod db;
mod llm;
mod verifier;

use anyhow::{Context, Result};
use kernel_schema::AtomicUnit;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use db::{Database, TaskState};
use llm::{LlmClient, MockLlmClient};
use verifier::Verifier;

#[derive(Debug, Serialize, Deserialize)]
struct BuildOrderBatch {
    pub units: Vec<String>,
    pub is_super_node: bool,
    pub scc_size: Option<usize>,
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

#[tokio::main]
async fn main() -> Result<()> {
    println!("Conductor: LLM Orchestration Engine");
    
    // Paths
    let db_path = Path::new("blackboard.db");
    let build_order_path = Path::new("build_order.json");
    let units_path = Path::new("units.json");

    // Initialize database
    let db = Database::new(db_path).await
        .context("Failed to initialize database")?;
    println!("Conductor: Database initialized at {:?}", db_path);

    // Load units
    println!("Conductor: Loading units from {:?}", units_path);
    let units_json = fs::read_to_string(units_path)
        .context("Failed to read units.json")?;
    let units_vec: Vec<AtomicUnit> = serde_json::from_str(&units_json)
        .context("Failed to parse units.json")?;
    let units_map: HashMap<String, AtomicUnit> = units_vec
        .into_iter()
        .map(|u| (u.id.clone(), u))
        .collect();

    // Load build order
    println!("Conductor: Loading build order from {:?}", build_order_path);
    let build_order_json = fs::read_to_string(build_order_path)
        .context("Failed to read build_order.json")?;
    let build_order: BuildOrder = serde_json::from_str(&build_order_json)
        .context("Failed to parse build_order.json")?;

    let llm = MockLlmClient;

    println!("Conductor: Starting dispatch loop for {} batches", build_order.batches.len());

    for (i, batch) in build_order.batches.iter().enumerate() {
        println!("Conductor: [Batch {}/{}] Processing {} units", i + 1, build_order.batches.len(), batch.units.len());
        
        let mut futures = Vec::new();
        for unit_id in &batch.units {
            if let Some(unit) = units_map.get(unit_id) {
                // Register task in DB
                db.create_task(unit_id, unit_id).await?;
                db.update_task_state(unit_id, TaskState::InProgress, None, None).await?;
                
                // Dispatch transpilation and verification
                futures.push(process_unit(&db, &llm, unit));
            } else {
                eprintln!("Warning: Unit ID {} found in build order but not in units.json", unit_id);
            }
        }

        // Wait for all units in the batch to complete before moving to next batch
        let results = futures::future::join_all(futures).await;
        for res in results {
            if let Err(e) = res {
                eprintln!("Error processing unit: {:?}", e);
            }
        }
    }

    println!("Conductor: All batches processed successfully");
    
    Ok(())
}

async fn process_unit(db: &Database, llm: &impl LlmClient, unit: &AtomicUnit) -> Result<()> {
    println!("Conductor:  - Processing {}", unit.id);
    
    // 1. Transpile
    match llm.transpile(unit).await {
        Ok(rust_code) => {
            println!("Conductor:  - Transpiled {}. Verifying...", unit.id);
            
            // 2. Verify
            match Verifier::verify(&rust_code, &unit.id) {
                Ok(_) => {
                    println!("Conductor:  - Verified {}", unit.id);
                    db.update_task_state(&unit.id, TaskState::Completed, Some(&rust_code), None).await?;
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    eprintln!("Conductor:  - Verification failed for {}: {}", unit.id, err_msg);
                    db.update_task_state(&unit.id, TaskState::Failed, Some(&rust_code), Some(&err_msg)).await?;
                }
            }
        }
        Err(e) => {
            let err_msg = e.to_string();
            eprintln!("Conductor:  - Transpilation failed for {}: {}", unit.id, err_msg);
            db.update_task_state(&unit.id, TaskState::Failed, None, Some(&err_msg)).await?;
        }
    }
    Ok(())
}
