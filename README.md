# ctx

> Fast, developer-friendly project context generator and interactive code explorer for LLMs.

`ctx` is a modern command-line utility written in Rust that helps developers collect, analyze and export project context for Large Language Models (LLMs). It combines fast filesystem scanning, interactive file selection, configurable filtering, project statistics, and an experimental semantic code graph into a single tool.

Whether you're preparing context for ChatGPT, Claude, Gemini, or simply exploring a large codebase, `ctx` provides a clean and efficient workflow.

---

# Features

## Project Context Generation

- **Smart Code Artifact Gathering**: Compile complete codebase structure and file contents into a single structured artifact.
- **Smart Filtering**: Respects `.gitignore` and automatically ignores virtual environments (`venv`, `.venv`), dependency folders (`node_modules`), caches, and build folders (`target`).
- **Token Tracking**: Real-time token estimation for LLM consumption.
- **Visual Directory Tree**: Colored command-line representation of project structure.
- **Multiple Formats**: Export code artifacts to Markdown, XML, or Plain Text.

## Interactive Terminal UI

- Keyboard-driven visual file and directory selector.
- Live file preview panel.
- Fast file search and clipboard integration.
- Compile custom context artifacts on-the-fly.

## Code Graph (Experimental)

Build a local SQLite semantic index of your Rust and Python codebases.

- **Multi-language AST Parsing**: Native support for Rust and Python symbols, imports, and calls.
- **LSP Resolution**: Enriched call resolution using LSP clients (e.g. `rust-analyzer` or `pyright-langserver`) to resolve call targets.
- **Graph Queries**: Subcommands for symbols, callers, callees, forward slice trees, and symbol context.
- **Budget-Aware Context Extraction (`affect`)**: Rank and pack related code context within a specific LLM token budget.

## Model Context Protocol (MCP) Server

- **Full MCP Immersion**: Seamless integration with LLM clients (e.g., Claude desktop, Cursor, Gemini, or Claude Code) using the standard stdio protocol.
- **Interactive Context Retrieval**: Provide LLMs with tools to resolve code dependencies, lookup symbol definitions, and extract functional neighborhoods dynamically.

## Multiple Output Formats

- Markdown
- Plain text
- XML

Designed for direct consumption by:

- ChatGPT
- Claude
- Gemini
- Cursor
- Copilot
- Other LLM-powered tools

---

# Installation

## From source

```bash
git clone https://github.com/<your-org>/ctx.git

cd ctx

cargo install --path crates/ctx-cli
```

Or simply:

```bash
cargo build --release
```

The executable will be available as:

```text
target/release/ctx
```

---

# Quick Start

Display project tree

```bash
ctx
```

Generate complete markdown context

```bash
ctx -C
```

Export context to a file

```bash
ctx -C -o context.md
```

Copy context directly to clipboard

```bash
ctx -C --clipboard
```

Open interactive mode

```bash
ctx --interactive
```

Start the Model Context Protocol (MCP) server

```bash
ctx mcp
```

---

# CLI Usage

```
ctx [OPTIONS] [PATH]
```

## Common Options

| Option | Description |
|---------|-------------|
| `-C`, `--code` | Generate full project context |
| `-i`, `--interactive` | Launch interactive TUI |
| `-f`, `--format` | Output format |
| `-m`, `--mode` | Scan mode |
| `-o`, `--output` | Save output to file |
| `--clipboard` | Copy output to clipboard |
| `--max-depth` | Maximum traversal depth |
| `--max-file-size` | Maximum included file size |
| `--no-stats` | Disable project statistics |
| `--list-hidden` | Show skipped files |

---

# Scan Modes

## Smart

Default mode.

- Respects `.gitignore`
- Skips generated files
- Ignores common build artifacts
- Produces balanced LLM context

## All

Indexes every file.

Useful for:

- archives
- research
- debugging

## Code

Prioritizes source code while excluding unrelated documentation.

## Docs

Focuses on Markdown and documentation files.

Useful for documentation generation.

## LLM

Optimized output structure for language models.

Includes token statistics and improved formatting.

---

# Output Formats

## Markdown

Recommended for ChatGPT and Claude.

```bash
ctx -C --format markdown
```

---

## Plain Text

```bash
ctx -C --format plain
```

---

## XML

Suitable for custom parsers and tooling.

```bash
ctx -C --format xml
```

---

# Interactive Mode

Launch with

```bash
ctx --interactive
```

Features include:

- project browser
- search
- preview pane
- file selection
- clipboard export
- keyboard shortcuts

---

# Code Graph

The CodeGraph subsystem builds a semantic index of Rust and Python projects in a local SQLite database (`.ctx-codegraph/codegraph.sqlite`).

## Build index

Build the semantic symbol and call graph. By default, it uses fast Tree-Sitter AST parsing.

```bash
ctx graph build
```

Options:
- `--with-lsp`: Enables language server fallback (e.g., `rust-analyzer` or `pyright-langserver`) to enrich calls with precise `LspExact` resolution.
- `--no-rust-analyzer`: Disables language server integration, forcing tree-sitter fallback only.
- `--verbose`, `-v`: Displays detailed build statistics, parsed files, edge kinds, and timings.

## List symbols

List all indexed symbols grouped by file, or search for a specific symbol:

```bash
ctx graph symbols [query]
```

## Show callees

List direct callees (called symbols/functions) of a symbol:

```bash
ctx graph callees run_pipeline
```
*(Also available via `ctx graph calls <symbol>`)*

## Show callers

List direct callers of a symbol:

```bash
ctx graph callers load
```

## Generate forward slice tree

Compute and print the hierarchical forward call slice tree starting from a target symbol:

```bash
ctx graph slice run_pipeline
```

## Extract graph context

Retrieve semantic neighborhood context around a target symbol:

```bash
ctx graph context auth_service --mode callers --depth 2 --max-nodes 50
```

Available modes:
- `callers` / `callees`
- `dependencies` / `dependents`
- `neighborhood` (default)
- `forward-slice` / `reverse-slice`

## Retrieve ranked context under token budget (`affect`)

Retrieve a ranked, token-budgeted semantic context containing code snippets around a symbol (e.g., `auth_service`), optimized for LLM prompting:

```bash
ctx graph affect auth_service --depth auto --token-budget 12000
```

Options:
- `--mode <mode>`: Traversal mode (`callers`, `callees`, `dependencies`, `dependents`, `forward`, `reverse`, `neighborhood`, `impact`).
- `--packing <packing>`: Snippet packing strategy (`sandwich`, `frontloaded`, `balanced`).
- `--ranking <ranking>`: Symbol ranking strategy (`hybrid`, `graph`, `lexical`).
- `--format <format>`: Output format (`text`, `json`).
- `--no-snippets`: Disable inline code snippets and retrieve names only.

---

# Model Context Protocol (MCP) Server

`ctx` embeds a fully featured **Model Context Protocol (MCP)** server over standard input/output (stdio), allowing LLM agents (like Claude desktop, Cursor, Gemini, or Claude Code) to interact with your codebase context dynamically.

## Run MCP Server

```bash
ctx mcp
```

This starts a JSON-RPC 2.0 stdio server. When initialized by an MCP client, the server:
1. Automatically loads or rebuilds/updates the local SQLite codegraph index for the initialized workspace path.
2. Registers context retrieval tools to the LLM agent.

## Exposed Tools

### `get_graph_context`
Allows the agent to query the codebase's call graph and symbol definitions.
- **Arguments**:
  - `query` (string, required): The symbol name or qualified path to resolve.
  - `mode` (string, optional): Traversal mode (`neighborhood`, `callers`, `callees`, `dependencies`, `dependents`, `impact`). Default is `neighborhood`.
  - `depth` (integer, optional): BFS traversal depth. Default is `2`.
- **Behavior**:
  - Automatically handles ambiguity (if multiple symbols match, it prompts the agent with list of candidates).
  - Fetches matching code snippets and renders a clean Markdown output containing file paths, line numbers, call graph diagrams, and the code blocks.

---

# Collecting Code Artifacts

One of the core features of `ctx` is compiling codebase context into a clean, unified **Code Artifact** that can be directly fed into LLMs.

## CLI Artifact Generation

To generate a full code artifact containing the file tree structure and the file contents:

```bash
ctx -C
```

### Saving & Copying Artifacts

- **Write to a file**:
  ```bash
  ctx -C -o context.md
  ```
- **Copy to Clipboard**:
  ```bash
  ctx -C --clipboard
  ```

### Formats
Specify output formats using `-f` or `--format`:
- `markdown` (or `md`): Formatted with markdown headers and fenced code blocks. (Recommended for Claude/ChatGPT).
- `xml`: Structures context using XML tags, ideal for Claude's XML parsing behavior.
- `plain` (or `text`/`txt`): Standard text export.

### Smart Exclusions & Filtering
By default, `ctx` filters files carefully to keep context sizes within LLM token budgets:
- Respects project `.gitignore` files.
- Automatically excludes virtual environments (`venv`, `.venv`), package folders (`node_modules`), cache directories (`.git`, `.ctx-codegraph`), and build artifacts (`target`).
- Configurable maximum depth (`--max-depth`) and file size limits (`--max-file-size`).
- Custom skip rules can be specified in `.ctx.toml` configuration.

## Interactive TUI Artifacts

Launch the interactive terminal:

```bash
ctx --interactive
```

- Navigate the project structure visually.
- Select/deselect specific files and directories using spacebar/keys.
- Compile and copy the custom-tailored code artifact immediately onto the system clipboard.

---

# Under the Hood

### Incremental Updates
The CodeGraph index is backed by a local SQLite database (`.ctx-codegraph/codegraph.sqlite`).
`ctx` checks file modification times (`mtime`) and file sizes (`size`) to identify modified, added, or deleted files. Upon loading the service (either via CLI or MCP), `ctx` updates the index **incrementally** in milliseconds, avoiding expensive rebuilds.

### Dual Resolution Strategies
- **Tree-Sitter Parsing**: High-speed local AST parsing of symbols, functions, and import/calls for Rust and Python.
- **Language Server (LSP) Fallback**: Connects to active LSPs (like `rust-analyzer` or `pyright-langserver`) to resolve call targets with absolute semantic accuracy, marking resolved edges as `LspExact`.

---

# Configuration

`ctx` automatically loads a project configuration file when available.

Example:

```toml
mode = "smart"

max_depth = 6

max_file_size = 524288

exclude = [
    "target",
    "node_modules",
    "*.log"
]
```

CLI arguments always override configuration values.

---

# Project Structure

```
ctx/
├── ctx-cli
├── ctx-core
├── ctx-render
├── ctx-filter
├── ctx-config
├── ctx-models
├── ctx-codegraph
├── ctx-tui
├── ctx-stats
├── ctx-llm
└── ctx-test
```

Each crate is focused on a single responsibility to keep the architecture modular and easy to extend.

---

# Architecture

```
Filesystem
      │
      ▼
ctx-core
      │
      ▼
Filtering
      │
      ▼
Statistics
      │
      ▼
Rendering
      │
      ├────────► Markdown
      ├────────► XML
      └────────► Plain Text

             ▲

Interactive TUI

             ▲

CodeGraph
```

---

# Technologies

The project is built using:

- Rust
- Clap
- Ratatui
- Crossterm
- Tree-sitter
- rust-analyzer (optional)
- SQLite
- Rusqlite
- Walkdir
- Arboard
- ThisError

---

# Performance

Designed for:

- very large repositories
- low memory overhead
- incremental indexing
- fast filesystem traversal
- efficient rendering

---

# Supported Platforms

- Linux
- macOS
- Windows

---

# Roadmap

Planned features include:

- Additional language support (Go, TypeScript, C++)
- Workspace/multi-project support
- Global symbol references cross-referencing
- Dependency graphs visualizer
- Semantic search & embeddings
- Local LSP improvements
- Plugin system

---

# Contributing

Contributions are welcome.

Feel free to open:

- Issues
- Feature Requests
- Pull Requests

Before submitting a PR, please ensure:

- tests pass
- formatting is correct
- clippy reports no warnings

```bash
cargo fmt

cargo clippy

cargo test
```

---

# License

Licensed under the MIT License.

See the LICENSE file for details.