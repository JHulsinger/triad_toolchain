use anyhow::{Context, Result};
use std::process::Command;
use std::fs;
use std::env;

pub struct Verifier;

impl Verifier {
    pub fn verify(code_rust: &str, unit_id: &str) -> Result<()> {
        let temp_dir = env::temp_dir();
        let file_path = temp_dir.join(format!("{}.rs", unit_id));
        
        fs::write(&file_path, code_rust)
            .with_context(|| format!("Failed to write temp Rust file {:?}", file_path))?;
            
        let output = Command::new("rustc")
            .arg("--crate-type")
            .arg("lib")
            .arg("--emit")
            .arg("metadata") // Just check if it compiles, don't build full binary
            .arg("-o")
            .arg(temp_dir.join(format!("{}.rmeta", unit_id)))
            .arg(&file_path)
            .output()
            .context("Failed to execute rustc")?;
            
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("Compilation failed:\n{}", stderr))
        }
    }
}
