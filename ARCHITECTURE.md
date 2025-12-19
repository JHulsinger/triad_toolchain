# RustOS Triad: Architecture & Design

This document details the internal design of the Triad toolchain, focusing on the **Leaf-First Topology** and **Batch Orchestration** strategies used to deterministically refactor large codebases.

---

## 🏗️ Core Philosophy

**"Don't translate logic. Secure it."**

The goal isn't just to produce Rust code that looks like C. The goal is to produce **safe, idiomatic Rust** by leveraging the inherent structure of the codebase.

### The DAG Strategy
We treat the legacy codebase as a **Directed Acyclic Graph (DAG)** of dependencies.
1.  **Map** the graph.
2.  **Topological Sort** to find the "Leaf Nodes" (functions with 0 unverified dependencies).
3.  **Batch** the leaves for LLM processing.
4.  **Verify** the result.
5.  **Unlock** the next tier of functions.

---

## 🧩 Component Breakdown

### 1. Slicer (The Parser)
**Role:** Context Extraction.
**Stack:** Rust, `tree-sitter-c`.

Instead of feeding an LLM an entire file, the Slicer extracts distinct **Atomic Units**:
*   The function body.
*   Exact struct definitions it relies on (non-transitive).
*   Global variable declarations (if touched).
*   Macros used.

This ensures the LLM has **perfect context** without token bloat.

### 2. Mapper (The Strategist)
**Role:** Topology Analysis.
**Stack:** Rust, `petgraph`.

The Mapper builds the dependency graph and performs **Cycle Detection** using Tarjan's SCC algorithm.
*   **Super Nodes:** When circular dependencies are found (e.g., A -> B -> A), they are grouped into a "Super Node".
*   **Action Plan:** The Mapper marks Super Nodes for special handling (requires simultaneous refactoring or interface extraction).
*   **Weak Edge Detection:** Suggests where to break cycles based on graph centrality heuristics.

### 3. Conductor (The Orchestrator)
**Role:** State Management & Execution.
**Stack:** Rust, `tokio`, `sqlx` (SQLite).

The Conductor is a state machine that drives the refactoring process.
*   **Blackboard Pattern:** Uses a SQLite database (`blackboard.db`) to track the status of every function (`PENDING`, `IN_PROGRESS`, `COMPLETED`, `FAILED`).
*   **Verifier Loop:** Before marking a task `COMPLETED`, it compiles the code with the target language compiler (e.g., `rustc`). Only code that compiles is promoted.
*   **Batching:** Queries the DAG for the next batch of ready tasks (Leaf Nodes) and dispatches them in parallel.

---

## 🔄 The "Unsafe -> Safe" Workflow

The Triad is designed to support a specific refactoring workflow:

1.  **C Source** (Input)
2.  **Atomic Unit** (Slice)
3.  **Unsafe Rust** (Transpile via external tool or LLM)
4.  **Safe Rust** (Refactor via LLM)
5.  **Verified Rust** (Output)

The Conductor manages this pipeline, ensuring that step 4 only happens once the function's dependencies have already passed step 5. This provides the LLM with **verified interfaces** for all dependencies, significantly reducing hallucination risk.

---

## 🛡️ Production Readiness

*   **Zero Panic Policy:** All C parsing errors are handled gracefully (using `unwrap_or_else` or `anyhow::Context`).
*   **Persistence:** The process can be stopped and resumed at any time thanks to the SQLite blackboard.
*   **Determinism:** The Slicer and Mapper produce bit-identical outputs for the same input source.
