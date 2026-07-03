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
Usage: ctx [OPTIONS] [PATH]

Arguments:
  [PATH]  Target directory path to analyze [default: .]

Options:
  -f, --format <FORMAT>
          Format for the full context output. Choose from: 'markdown' (or 'md'), 'xml', 'plain' (or 'text', 'txt') [possible values: markdown, xml, plain]

  -m, --mode <MODE>
          Gathering strategy mode: 'smart' (respects gitignore + sensible skips), 'all' (scans all files), 'code' (prioritizes code files), 'docs' (prioritizes docs/markdown), 'llm' (structures with token counts) [possible values: smart, all, code, docs, llm]

      --max-depth <MAX_DEPTH>
          Restrict directory traversal to the specified maximum depth

      --max-file-size <MAX_FILE_SIZE>
          Exclude files exceeding this size limit in bytes from the final context contents [default: 512 KB]

  -o, --output <OUTPUT>
          Save the compiled context output directly to the specified file path instead of printing to stdout

      --no-stats
          Exclude the project summary tables and statistics from the generated context output

      --list-hidden
          Print lists of skipped, gitignored, or hidden files to stderr for transparency

  -c, --clipboard
          Copy the fully compiled context output straight to the system clipboard

  -C, --code
          Output the full code context (file structure and contents) to stdout instead of only showing the colored directory tree

  -i, --interactive
          Launch the interactive, keyboard-driven terminal user interface (TUI) for selecting files

  -h, --help
          Print help

  -V, --version
          Print version
```

---

## 🔍 Gathering Modes

`ctx` provides several modes (via `-m` or `--mode`) to filter files and prioritize what content gets compiled into your context:

- **`smart` (Default)**: Automatically respects `.gitignore` files and applies sensible defaults. It skips build directories (`target/`, `dist/`, etc.), dependency directories (`node_modules/`, `venv/`), version control systems (`.git/`), lockfiles (`Cargo.lock`, `package-lock.json`), cache folders, and temporary files.
- **`all`**: Bypasses all filtering rules entirely and makes every single file visible and included in the output.
- **`code`**: Filters the tree to keep only source code files (e.g., `.rs`, `.py`, `.js`, `.cpp`), configuration files (`Cargo.toml`, `package.json`, etc.), and project readmes/documentation root files.
- **`docs`**: Keeps only documentation and text-based files (e.g., `.md`, `.txt`, `.pdf`, `.docx`, `.xml`, `.json`, `.csv`, `.html`), hiding other file types.
- **`llm`**: Excludes binary/media files, archives, and executables (e.g., `.png`, `.jpg`, `.zip`, `.tar.gz`, `.exe`, `.so`, `.class`), while retaining all other files.

---

## ⌨️ TUI Keyboard Controls

When running in interactive TUI mode (`ctx -i`), use the following shortcuts:

- **Navigation**:
  - `j` / `k` or `▲` / `▼`: Move the selection cursor up or down.
  - `g` / `G`: Jump to the top or bottom of the list.
  - `h` / `l` or `◀` / `▶`: Collapse or expand the selected directory.
- **Selection**:
  - `Space` or `x`: Toggle inclusion of the selected file or directory (recursively checks/unchecks children).
- **Actions**:
  - `c`: Copy the compiled context of all checked files to the system clipboard.
  - `C` (Shift+C): Copy the absolute path of the selected item to the clipboard.
  - `Enter`: Expand/collapse a directory, or open a text file in a temporary view pager.
  - `o`: Open the selected file in a temporary view pager.
  - `f`: Enter search mode to filter files dynamically by name. Press `Esc` or `Enter` to exit search mode.
  - `r`: Force rescan and refresh directory contents.
  - `q` / `Esc`: Exit the TUI.

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

