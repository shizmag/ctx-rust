# ✨ ctx-rust

A beautiful, Tokyo Night-themed directory tree visualizer and LLM context gatherer written in Rust.

`ctx` allows you to explore directory trees with metadata at a glance, and compile entire codebases into structured context files (Markdown, XML, Plain Text) to feed directly into LLMs (like Gemini, Claude, and ChatGPT).

---

## 🎨 Key Features

- **Tokyo Night Palette**: True-color ANSI terminal escape sequences bring vibrant neon blue (`#7aa2f7`), green (`#9ece6a`), yellow (`#e0af68`), and purple (`#bb9af7`) theme colors to your terminal.
- **Rich Iconography**: Visual icons for different languages and folders (`📁`, `🦀`, `🐍`, `🐳`, `📦`, `⚙️`) render out-of-the-box without requiring custom Nerd Fonts.
- **Symmetrical Metadata Alignment**: Staggered dots (`....`) line up file metadata (lines, tokens, and bytes) perfectly to the right side of the terminal for a balanced, clean look.
- **Interactive TUI**: Press `-i` / `--interactive` to open a keyboard-driven terminal dashboard to cherry-pick files, check/uncheck folders, and copy selection to clipboard instantly.
- **LLM-optimized Modes**: Clean markdown (`-f md`), XML (`-f xml`), or plain text (`-f plain`) output with estimated token counts to fit context windows efficiently.
- **Smart Traversals**: Automatic respect for `.gitignore` and protection against large binary/cache files.

---

## 🚀 Quick Start

Build the project using Cargo:
```bash
cargo build --release
```

Create a symlink or add the binary `target/release/ctx` to your `PATH`.

### Basic Usage

1. **View Directory Tree & Statistics (Default)**:
   ```bash
   ctx
   ```
   *Displays a beautiful, colored tree representing your workspace with file sizes, line counts, and a summary box.*

2. **Generate Full Code Context to Stdout**:
   ```bash
   ctx -C
   ```
   *Compiles all text files into a single markdown structure.*

3. **Copy Context to Clipboard**:
   ```bash
   ctx -c
   ```
   *Gathers all project file contents, formats them, and copies them to the system clipboard for immediate pasting.*

4. **Launch Interactive TUI**:
   ```bash
   ctx -i
   ```
   *Allows browsing, checking/unchecking files with `Space`, and copying selected files with `c` or `Enter`.*

5. **Save Context to File**:
   ```bash
   ctx -o context.md
   ```

---

## ⚙️ CLI Reference

```text
Arguments:
  [PATH]  Target directory path to analyze [default: .]

Options:
  -f, --format <FORMAT>        Output format: 'markdown' (or 'md'), 'xml', 'plain' [default: markdown]
  -m, --mode <MODE>            Gathering mode: 'smart', 'all', 'code', 'docs', 'llm' [default: smart]
      --max-depth <DEPTH>      Restrict traversal to specified maximum depth
      --max-file-size <SIZE>   Exclude files exceeding size limit in bytes [default: 512 KB]
  -o, --output <PATH>          Save context output directly to specified file path
      --no-stats               Exclude summary stats from context output
      --list-hidden            Print skipped/ignored files to stderr
  -c, --clipboard              Copy context output to clipboard
  -C, --code                   Output full code context with file contents to stdout
  -i, --interactive            Launch the interactive terminal user interface (TUI)
  -h, --help                   Print help
  -V, --version                Print version
```

---

## ⌨️ TUI Keyboard Controls

- `j` / `k` or `▲` / `▼`: Move selection cursor.
- `Space`: Toggle inclusion of the selected file.
- `c` / `Enter`: Copy context of selected files to the system clipboard.
- `r`: Rescan/refresh directory contents.
- `q` / `Esc`: Exit TUI.

---

## ⚙️ Ignore Rules and Overrides

`ctx` respects standard `.gitignore` rules across your workspace, including:
- Root and nested `.gitignore` files.
- Local repository exclusions (`.git/info/exclude`).
- Global gitignore files (e.g. `~/.gitignore_global`, `~/.config/git/ignore`).

### The `#[ctx]` Bypass Block
If there are files or folders that you want to ignore in Git but keep visible for `ctx` (e.g., specific build outputs or local config templates that you want to include in your LLM context), you can use the special `#[ctx]` header block in any `.gitignore` file.

Rules placed under `#[ctx]` will be bypassed (i.e. not ignored) by `ctx`:

```text
# Standard gitignore rules (ignored by both git and ctx)
node_modules/
target/
*.log

#[ctx]
# Bypassed rules (ignored by git, but visible to ctx)
visible.log
dist/config-template.json
```

