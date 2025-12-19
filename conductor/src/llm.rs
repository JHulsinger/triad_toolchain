use anyhow::Result;
use kernel_schema::AtomicUnit;
use async_trait::async_trait;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn transpile(&self, unit: &AtomicUnit) -> Result<String>;
}

pub struct MockLlmClient;

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn transpile(&self, unit: &AtomicUnit) -> Result<String> {
        // Simulated transpilation delay
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        
        // Mock output: just a comment wrapping the C code for now
        let rust_code = format!(
            "// Transpiled from C function: {}\n// Dependencies: {:?}\n\nfn {}() {{\n    println!(\"Simulated Rust version of {}\");\n}}",
            unit.id, unit.dependencies, unit.id, unit.id
        );
        
        Ok(rust_code)
    }
}
