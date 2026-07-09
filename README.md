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

- **stdio MCP server**: Integrates with Cursor, Claude Desktop, and other MCP clients.
- **Full tool catalog**: `get_affected_context`, graph traversal, project context, symbol search, and index rebuild.
- **Resources & prompts**: Index status, project tree summary, and guided symbol exploration.

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

`ctx` embeds a **Model Context Protocol (MCP)** server over standard input/output (stdio), allowing LLM agents (Cursor, Claude Desktop, Gemini, Claude Code, etc.) to interact with your codebase context dynamically.

## Prerequisites

Build the codegraph index **before** first use:

```bash
ctx graph build --with-lsp
```

The MCP server opens an existing index on `initialize` but does **not** auto-build. If the index is missing, initialization fails with a message pointing to the command above. Agents can also call the `rebuild_index` tool when a rebuild is needed.

## Run MCP Server

```bash
ctx mcp
```

This starts a JSON-RPC 2.0 stdio server (protocol version `2024-11-05`). Progress and status messages are logged to **stderr** (e.g. `Index loaded`, `Index not found — run ctx graph build --with-lsp`).

## Client Configuration

Example configs are in `examples/mcp/`. Replace `/path/to/ctx` with the absolute path to your `ctx` binary, or ensure `ctx` is on `PATH` and use `"command": "ctx"`.

### Cursor

Copy or merge into `.cursor/mcp.json` (see `examples/mcp/cursor-mcp.json`):

```json
{
  "mcpServers": {
    "ctx": {
      "command": "/path/to/ctx",
      "args": ["mcp"]
    }
  }
}
```

### Claude Desktop

Add to Claude Desktop MCP config (see `examples/mcp/claude-desktop-config.json`):

```json
{
  "mcpServers": {
    "ctx": {
      "command": "/path/to/ctx",
      "args": ["mcp"]
    }
  }
}
```

## Tools

| Tool | Description |
|------|-------------|
| `get_affected_context` | **Primary LLM tool.** Budget-aware ranked context (same as `ctx graph affect`). |
| `get_graph_context` | Graph neighborhood with code snippets. |
| `get_project_context` | Full project context (same as `ctx -C`). |
| `list_symbols` | List or search indexed symbols. |
| `get_callers` | Direct callers of a symbol. |
| `get_callees` | Direct callees of a symbol. |
| `rebuild_index` | Rebuild the codegraph index. |

### `get_affected_context`

- `query` (required): Symbol name or qualified path.
- `mode` (optional): `neighborhood`, `callers`, `callees`, `dependencies`, `dependents`, `forward`, `reverse`, `forward-slice`, `reverse-slice`, `impact`. Default: `neighborhood`.
- `depth` (optional): Integer or `"auto"`. Default: `auto`.
- `max_nodes`, `max_files` (optional): Graph limits. Defaults: `200`, `50`.
- `token_budget` (optional): Default `12000`.
- `model_context_window` (optional): Default `128000`.
- `packing` (optional): `sandwich`, `frontloaded`, `balanced`. Default: `sandwich`.
- `ranking` (optional): `hybrid`, `graph`, `lexical`. Default: `hybrid`.
- `include_tests`, `include_unresolved`, `no_snippets` (optional booleans).
- `edge_kind` (optional array): e.g. `Call`, `Import`.
- `context_lines` (optional): Snippet padding. Default: `3`.
- `format` (optional): `text` or `json`. Default: `text`.

### `get_graph_context`

- `query` (required): Symbol name or qualified path.
- `mode` (optional): Same modes as above. Default: `neighborhood`.
- `depth` (optional): BFS depth. Default: `2`.
- `max_nodes` (optional): Default `40`.
- `max_files` (optional): Default `20` (`0` = unlimited).

### `get_project_context`

- `format` (optional): `markdown`, `xml`, `plain`. Default: `markdown`.
- `mode` (optional): `smart`, `code`, `docs`, `llm`, `all`. Default: `smart`.
- `max_depth`, `max_file_size` (optional).
- `include_stats` (optional): Default `true`.

### `list_symbols`

- `query` (optional): Filter symbols; omit to list.
- `limit` (optional): Default `50`.

### `get_callers` / `get_callees`

- `query` (required): Symbol name or qualified path.

### `rebuild_index`

- `use_lsp` (optional): Use LSP resolution. Default: `true`.

When symbol resolution is ambiguous, tools return structured text listing candidate symbols so the agent can refine `query` and retry.

## Resources

| URI | Description |
|-----|-------------|
| `ctx://index/status` | Index build status and metadata (files, symbols, edges). |
| `ctx://project/tree` | Brief project tree summary. |

## Prompts

| Prompt | Arguments | Description |
|--------|-----------|-------------|
| `explore-symbol` | `symbol` (required) | Guided workflow for exploring a symbol with ctx tools. |

## Protocol Methods

- `initialize`, `notifications/initialized`
- `ping`
- `tools/list`, `tools/call`
- `resources/list`, `resources/read`
- `prompts/list`, `prompts/get`

## Operational Notes

- **Pre-build required**: Run `ctx graph build --with-lsp` before connecting an MCP client.
- **No mid-session workspace switch**: The workspace is fixed at `initialize`; re-connect to change projects.
- **stderr logging**: Status messages go to stderr; JSON-RPC responses go to stdout.
- **Disambiguation**: Ambiguous symbols return candidate lists instead of errors — this is the intended MCP-interactive pattern.

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
`ctx` checks file modification times (`mtime`) and file sizes (`size`) to identify modified, added, or deleted files. The CLI updates the index **incrementally** in milliseconds when loading graph commands. The MCP server opens the existing index without auto-building; use `ctx graph build` or the `rebuild_index` MCP tool to refresh.

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