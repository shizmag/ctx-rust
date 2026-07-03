# ctx-codegraph

`ctx-codegraph` is a best-effort code graph construction engine for Rust, integrated into `ctx-rust`.

It uses **Tree-sitter Rust** for fast, high-coverage syntax indexing of functions, methods, impls, traits, and call sites, and integrates with **rust-analyzer** over the Language Server Protocol (LSP) to resolve exact code references.

---

## 🎨 Key Features

- **Tree-sitter Syntax Indexing**: Fast indexing of functions, methods, structs, enums, traits, test functions, and call expressions directly from the AST.
- **rust-analyzer LSP Resolver**: Optional resolution of call expressions to their exact definitions if `rust-analyzer` is available in your `PATH`.
- **Name-Only Fallback**: If `rust-analyzer` is unavailable, calls are resolved via a name-only search of candidate definitions.
- **Persistent SQLite Storage**: The constructed code graph is stored in `.ctx-codegraph/codegraph.sqlite` for quick querying of symbols, callers, callees, and slices.
- **Semantic Slicing**: Computes forward and reverse code slices recursively to determine the exact call dependencies of any given function.

---

## 🚀 CLI Commands

To use the code graph, use the `ctx graph` subcommand:

### 1. Build the index
```bash
ctx graph build
```
Scans the project files, parses them, resolves calls (optionally via `rust-analyzer`), and saves the index to SQLite.

### 2. List symbols
```bash
ctx graph symbols
```
Lists all indexed symbols (functions, methods, impls, structs, etc.) grouped by file.

### 3. Query callees
```bash
ctx graph calls <symbol>
```
Finds the target symbol and prints all functions/methods called by it.

### 4. Query callers
```bash
ctx graph callers <symbol>
```
Finds the target symbol and prints all functions/methods calling it.

### 5. Semantic forward slice
```bash
ctx graph slice <symbol>
```
Prints the recursive forward dependency tree of the symbol (all transitive callees).

---

## ⚠️ Limitations

- **Best-Effort Graph**: `ctx-codegraph` is a best-effort index. It does not perform full semantic analysis on its own.
- **rust-analyzer Integration**: Exact symbol resolution requires `rust-analyzer` to be installed and available in your `PATH`.
- **Name-Only Fallback**: If `rust-analyzer` is unavailable, `ctx-codegraph` falls back to name-based resolution which may result in `Ambiguous` or `Unresolved` edges.
- **Dynamic & Generic Calls**: Macro-generated code, trait dynamic dispatch, and complex generic methods/calls might remain unresolved.
- **Raw Names**: Unresolved calls are still preserved and displayed as raw call names.
- **SQLite Storage**: The index database is saved locally at `.ctx-codegraph/codegraph.sqlite`.
- **Language Support**: Python support is not part of this first implementation; only Rust code indexing is supported at this stage.
