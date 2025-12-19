use clap::Parser;
use kernel_schema::AtomicUnit;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tree_sitter::{Parser as TSParser, Query, QueryCursor};
use anyhow::{Context, Result};

#[derive(Parser, Debug)]
#[command(author, version, about = "Extract Atomic Units from C source files")]
struct Args {
    /// Path to the C source file or directory
    #[arg(short, long)]
    source: PathBuf,

    /// Path to the output units.json file
    #[arg(short, long, default_value = "units.json")]
    output: PathBuf,
}

/// Global type registry for cross-file type resolution
#[derive(Default)]
struct TypeRegistry {
    /// Maps type name -> full definition text
    types: HashMap<String, String>,
    /// Maps type name -> file where it was defined (for debugging)
    type_sources: HashMap<String, PathBuf>,
    /// Tracks #include directives per file
    includes: HashMap<PathBuf, Vec<String>>,
    /// Macro definitions (#define)
    macros: HashMap<String, String>,
}

impl TypeRegistry {
    fn new() -> Self {
        Self::default()
    }

    fn register_type(&mut self, name: String, definition: String, source_file: PathBuf) {
        // Prefer longer definitions (full struct body over forward declaration)
        // Forward declarations like "struct proc;" are shorter than full definitions
        let should_insert = match self.types.get(&name) {
            None => true,
            Some(existing) => {
                // Replace if new definition is longer (more complete)
                // Also check if existing is just a forward declaration (contains '{')
                let new_has_body = definition.contains('{');
                let old_has_body = existing.contains('{');
                (new_has_body && !old_has_body) || (!old_has_body && definition.len() > existing.len())
            }
        };
        
        if should_insert {
            self.types.insert(name.clone(), definition);
            self.type_sources.insert(name, source_file);
        }
    }

    fn get_type(&self, name: &str) -> Option<&String> {
        self.types.get(name)
    }

    fn register_include(&mut self, file: PathBuf, include: String) {
        self.includes.entry(file).or_default().push(include);
    }

    fn register_macro(&mut self, name: String, definition: String) {
        if !self.macros.contains_key(&name) {
            self.macros.insert(name, definition);
        }
    }

    fn get_macro(&self, name: &str) -> Option<&String> {
        self.macros.get(name)
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Slicer: Analyzing source at {:?}", args.source);

    let mut units = Vec::new();
    let mut type_registry = TypeRegistry::new();
    let mut files_to_process: Vec<PathBuf> = Vec::new();

    // Collect all files to process
    collect_source_files(&args.source, &mut files_to_process)?;

    println!("Slicer: Found {} source files", files_to_process.len());

    // First pass: collect all type definitions across all files
    for path in &files_to_process {
        collect_types_from_file(path, &mut type_registry)?;
    }

    println!("Slicer: Registered {} types and {} macros across all files", 
        type_registry.types.len(), type_registry.macros.len());

    // Second pass: extract functions with cross-file type resolution
    for path in &files_to_process {
        extract_functions_from_file(path, &mut units, &type_registry)?;
    }

    let json = serde_json::to_string_pretty(&units)?;
    fs::write(&args.output, json)?;

    println!("Slicer: Extracted {} units to {:?}", units.len(), args.output);

    Ok(())
}

/// Recursively collect all .c and .h files from a path
fn collect_source_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        let ext = path.extension().and_then(|s| s.to_str());
        if ext == Some("c") || ext == Some("h") {
            files.push(path.to_path_buf());
        }
    } else if path.is_dir() {
        for entry in fs::read_dir(path).with_context(|| format!("Failed to read directory {:?}", path))? {
            let entry = entry?;
            let entry_path = entry.path();
            collect_source_files(&entry_path, files)?;
        }
    }
    Ok(())
}

/// First pass: collect all type definitions from a file
fn collect_types_from_file(path: &PathBuf, registry: &mut TypeRegistry) -> Result<()> {
    let code_raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read file {:?}", path))?;
    let code = code_raw.as_bytes();

    let mut parser = TSParser::new();
    parser.set_language(tree_sitter_c::language())
        .context("Error loading C grammar")?;

    let tree = match parser.parse(&code_raw, None) {
        Some(t) => t,
        None => {
            eprintln!("Warning: Failed to parse {:?}, skipping", path);
            return Ok(());
        }
    };
    let root_node = tree.root_node();

    // Query for type definitions (struct, union, enum, typedef)
    let type_query = Query::new(tree_sitter_c::language(), "
        (struct_specifier) @type
        (union_specifier) @type
        (enum_specifier) @type
        (type_definition) @type
    ").context("Error creating type query")?;

    // Query for #include directives
    let include_query = Query::new(tree_sitter_c::language(), 
        "(preproc_include path: (_) @path)"
    ).context("Error creating include query")?;

    let mut cursor = QueryCursor::new();

    // Extract #include directives
    let matches = cursor.matches(&include_query, root_node, code);
    for m in matches {
        for capture in m.captures {
            if let Ok(text) = capture.node.utf8_text(code) {
                registry.register_include(path.clone(), text.to_string());
            }
        }
    }

    // Extract type definitions
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&type_query, root_node, code);
    for m in matches {
        for capture in m.captures {
            let node = capture.node;
            if let Some(name) = extract_type_name(node, code) {
                if let Ok(def_text) = node.utf8_text(code) {
                    registry.register_type(name, def_text.to_string(), path.clone());
                }
            }
        }
    }

    // Query for #define macros
    let macro_query = Query::new(tree_sitter_c::language(),
        "(preproc_def name: (identifier) @name) @def"
    ).context("Error creating macro query")?;

    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&macro_query, root_node, code);
    for m in matches {
        let mut name_text: Option<String> = None;
        let mut def_text: Option<String> = None;
        for capture in m.captures {
            let node = capture.node;
            if node.kind() == "identifier" {
                if let Ok(text) = node.utf8_text(code) {
                    name_text = Some(text.to_string());
                }
            } else if node.kind() == "preproc_def" {
                if let Ok(text) = node.utf8_text(code) {
                    def_text = Some(text.to_string());
                }
            }
        }
        if let (Some(name), Some(def)) = (name_text, def_text) {
            registry.register_macro(name, def);
        }
    }

    Ok(())
}

/// Extract the name from a type definition node
fn extract_type_name(node: tree_sitter::Node, code: &[u8]) -> Option<String> {
    match node.kind() {
        "struct_specifier" | "union_specifier" | "enum_specifier" => {
            node.child_by_field_name("name")
                .and_then(|n| n.utf8_text(code).ok())
                .map(|s| s.to_string())
        }
        "type_definition" => {
            // For typedef, the name is in the declarator
            node.child_by_field_name("declarator")
                .and_then(|n| extract_identifier_text(n, code))
        }
        _ => None
    }
}

/// Recursively find an identifier's text
fn extract_identifier_text(node: tree_sitter::Node, code: &[u8]) -> Option<String> {
    if node.kind() == "identifier" || node.kind() == "type_identifier" {
        return node.utf8_text(code).ok().map(|s| s.to_string());
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if let Some(text) = extract_identifier_text(child, code) {
                return Some(text);
            }
        }
    }
    None
}

/// Second pass: extract functions from a file using the global type registry
fn extract_functions_from_file(
    path: &PathBuf,
    units: &mut Vec<AtomicUnit>,
    type_registry: &TypeRegistry,
) -> Result<()> {
    let code_raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read file {:?}", path))?;
    let code = code_raw.as_bytes();

    let mut parser = TSParser::new();
    parser.set_language(tree_sitter_c::language())
        .context("Error loading C grammar")?;

    let tree = match parser.parse(&code_raw, None) {
        Some(t) => t,
        None => {
            eprintln!("Warning: Failed to parse {:?}, skipping", path);
            return Ok(());
        }
    };
    let root_node = tree.root_node();

    let func_query = Query::new(tree_sitter_c::language(), "(function_definition) @func")
        .context("Error creating func query")?;

    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&func_query, root_node, code);

    for m in matches {
        for capture in m.captures {
            let node = capture.node;

            // Extract function name safely (no unwrap)
            let name = extract_function_name(node, code)
                .unwrap_or_else(|| "unknown_fn".to_string());

            let func_code = node.utf8_text(code)
                .with_context(|| format!("Failed to extract function code for {}", name))?
                .to_string();

            // Trace dependencies and types (no unwrap)
            let mut dependencies = Vec::new();
            let mut used_types = Vec::new();
            extract_info_safe(node, code, &mut dependencies, &mut used_types);

            // Collect required type definitions from global registry
            let mut required_headers = Vec::new();
            for type_name in &used_types {
                if let Some(def) = type_registry.get_type(type_name) {
                    if !required_headers.contains(def) {
                        required_headers.push(def.clone());
                    }
                }
            }

            units.push(AtomicUnit::new(
                name,
                func_code,
                dependencies,
                required_headers,
            ));
        }
    }

    Ok(())
}

/// Safely extract function name without unwrap
fn extract_function_name(node: tree_sitter::Node, code: &[u8]) -> Option<String> {
    if node.kind() == "function_definition" {
        if let Some(decl) = node.child_by_field_name("declarator") {
            return find_identifier_safe(decl, code);
        }
    }
    find_identifier_safe(node, code)
}

/// Safely find identifier without unwrap
fn find_identifier_safe(node: tree_sitter::Node, code: &[u8]) -> Option<String> {
    if node.kind() == "identifier" {
        return node.utf8_text(code).ok().map(|s| s.to_string());
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if let Some(name) = find_identifier_safe(child, code) {
                return Some(name);
            }
        }
    }
    None
}

/// Extract dependencies and types safely (no unwrap)
fn extract_info_safe(
    node: tree_sitter::Node,
    code: &[u8],
    deps: &mut Vec<String>,
    types: &mut Vec<String>,
) {
    match node.kind() {
        "call_expression" => {
            if let Some(func_node) = node.child_by_field_name("function") {
                if let Ok(text) = func_node.utf8_text(code) {
                    let text = text.to_string();
                    if !deps.contains(&text) {
                        deps.push(text);
                    }
                }
            }
        }
        "type_identifier" => {
            if let Ok(text) = node.utf8_text(code) {
                let text = text.to_string();
                if !types.contains(&text) {
                    types.push(text);
                }
            }
        }
        _ => {}
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            extract_info_safe(child, code, deps, types);
        }
    }
}
