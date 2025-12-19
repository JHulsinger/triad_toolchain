# Rust Triad Toolchain 🦀

**A language-agnostic, deterministic engine for porting large C codebases.**

The Triad Toolchain is a "Systems Agent" architecture designed to refactor operating system kernels (or any large C legacy codebase) into modern languages like Rust, Go, or Zig. It optimizes for **token efficiency**, **correctness**, and **dependency management**.

---

## 🚀 Why This Toolchain?

Refactoring a legacy kernel (e.g., Linux, xv6) to Rust is hard because LLMs have limited context windows and struggle with deep call graphs.

**The Solution:** Don't feed the whole codebase to the LLM. 
Instead, treat the codebase as a **Directed Acyclic Graph (DAG)** of isolated units.

| Step | Time (xv6) | Output |
|------|-----------|--------|
| **1. Slice** | 0.69s | Extracts 318 atomic functions with isolated context |
| **2. Map** | 0.31s | builds dependency graph & detects cycles |
| **3. Conduct** | Async | Orchestrates batch-processing for the LLM |

---

## 📐 Architecture

The toolchain consists of three decoupled CLIs:

### 1. Slicer (`tree-sitter-c`)
Parses C code into "Atomic Units".
*   **Input:** C source directory.
*   **Output:** `units.json` (Function code + exact struct definitions needed).
*   **Key Feature:** Resolves multi-file dependencies (header hunting) automatically.

### 2. Mapper (`petgraph`)
Analyzes the topology of the codebase.
*   **Input:** `units.json`.
*   **Output:** `build_order.json` + `cycle_analysis.json`.
*   **Key Feature:** Detects **Super Nodes** (circular dependencies) and suggests refactoring strategies using graph heuristics.

### 3. Conductor (`tokio` + `sqlx`)
The orchestration engine.
*   **Input:** `build_order.json`.
*   **Logic:** Dispatches tasks to an external LLM (via terminal/API) in strict dependency order (Leaf Nodes → Core Nodes).
*   **Key Feature:** **Persistent Blackboard** (SQLite) tracks the state of every function. Includes a `Verifier` loop that compiles the generated code to ensure correctness before proceeding.

---

## 🛠️ Quick Start

```bash
# 1. Build the toolchain
cargo build --release

# 2. Run the Slicer (extract C code)
./target/release/slicer --source /path/to/c_kernel --output units.json

# 3. Run the Mapper (analyze topology)
./target/release/mapper --units units.json --output build_order.json --analyze-cycles

# 4. Run the Conductor (orchestrate refactoring)
# Note: Requires configuring your target language verifier (default: rustc)
./target/release/conductor
```

---

## 🧠 Philosophy

*   **Leaf-First Topology:** We process independent "leaf" functions first. Once verified, they become the "ground truth" context for the functions that depend on them.
*   **Context Isolation:** An LLM shouldn't see the whole OS to refactor `strlen()`. It only needs `strlen` and the `size_t` definition.
*   **Determinism:** The pipeline is reproducible. The same C code always yields the same build order.
*   **Language Agnostic:** While currently optimized for C → Rust, the architecture supports any target language (Go, Zig, etc.) by swapping the Conductor's `Verifier` trait.

---

## 📊 Performance

Tested on the **xv6 kernel** (MIT teaching OS):
*   **Files:** 68 (.c and .h)
*   **Functions:** 318
*   **Processing Time:** ~1 second (End-to-End analysis)
*   **Accuracy:** 99.3% verification success rate (caught real Rust keyword collisions like `yield` and `match` in C code).

See [BENCHMARKS.md](BENCHMARKS.md) for detailed metrics.

---

## 📄 License

MIT License. See [LICENSE](LICENSE) for details.
