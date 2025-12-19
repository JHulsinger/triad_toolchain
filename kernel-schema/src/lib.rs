use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct AtomicUnit {
    pub id: String,
    pub code: String,
    pub dependencies: Vec<String>,
    pub required_headers: Vec<String>,
}

impl AtomicUnit {
    pub fn new(id: String, code: String, dependencies: Vec<String>, required_headers: Vec<String>) -> Self {
        Self {
            id,
            code,
            dependencies,
            required_headers,
        }
    }
}
