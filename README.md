# ctx

> Fast, developer-friendly project context generator and interactive code explorer for LLMs.

`ctx` is a modern command-line utility written in Rust that helps developers collect, analyze and export project context for Large Language Models (LLMs). It combines fast filesystem scanning, interactive file selection, configurable filtering, project statistics, and an experimental semantic code graph into a single tool.

Whether you're preparing context for ChatGPT, Claude, Gemini, or simply exploring a large codebase, `ctx` provides a clean and efficient workflow.

---

# Features

## Project Context Generation

- Generate complete project context in multiple formats
- Smart filtering of unnecessary files
- Git-aware scanning
- Configurable ignore rules
- Token estimation for LLM usage
- Project statistics
- Directory tree visualization

## Interactive Terminal UI

- Keyboard-driven interface
- Interactive file selection
- Live preview
- Search
- Clipboard integration
- Fast navigation through large projects

## Code Graph (Experimental)

Build a semantic graph of your Rust project.

Features include:

- Symbol indexing
- Function call graph
- Caller lookup
- Callee lookup
- Forward slicing
- Symbol search
- Optional rust-analyzer integration for improved resolution

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

The experimental CodeGraph subsystem builds a semantic index of Rust projects.

## Build index

```bash
ctx graph build
```

---

## List symbols

```bash
ctx graph symbols
```

---

## Show callees

```bash
ctx graph calls run_pipeline
```

---

## Show callers

```bash
ctx graph callers load
```

---

## Generate forward slice

```bash
ctx graph slice run_pipeline
```

---

## Disable rust-analyzer

```bash
ctx graph build --no-rust-analyzer
```

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

- additional language support
- incremental indexing
- workspace support
- symbol references
- dependency graphs
- semantic search
- embeddings
- MCP integration
- LSP improvements
- plugin system

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