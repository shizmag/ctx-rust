pub const ROOT_LONG_ABOUT: &str = "\
ctx is a highly informative, interactive directory tree visualizer and LLM context gatherer.\n\n\
Default mode prints a Tokyo Night colored tree plus a project summary. Use -C/--code for full file \
contents, -i/--interactive for the keyboard-driven TUI, or -o/--output to write to a file.";

pub const ROOT_AFTER_HELP: &str = "\
Examples:\n  \
ctx\n  \
ctx -i\n  \
ctx -C -f markdown -o context.md\n  \
ctx graph build fast\n  \
ctx graph symbols\n  \
ctx mcp install\n  \
ctx healthcheck --probe";

pub const GRAPH_AFTER_HELP: &str = "\
Examples:\n  \
ctx graph build\n  \
ctx graph build fast\n  \
ctx graph build balance\n  \
ctx graph build full\n  \
ctx graph build --tier full --with-lsp\n  \
ctx graph build --all\n  \
ctx graph symbols\n  \
ctx graph calls my_function\n  \
ctx graph callers my_function\n  \
ctx graph slice my_function\n  \
ctx graph affect run_pipeline --token-budget 12000\n  \
ctx graph info\n  \
ctx g symbols\n  \
ctx g info";

pub const GRAPH_BUILD_LONG_ABOUT: &str = "\
Build or rebuild the local SQLite codegraph index.\n\n\
Extraction tiers (positional shorthand or --tier):\n  \
fast      Tree-sitter structural index only (fastest)\n  \
balance   Call graph via syntax + heuristics (default)\n  \
full      LSP enrichment + hybrid search indexes when configured\n\n\
Use an explicit project path when it could be confused with a tier name: \
ctx graph build /path/to/project";

pub const GRAPH_INFO_LONG_ABOUT: &str = "\
Show codegraph index status, symbol counts, and hybrid search configuration.\n\n\
Use ctx stats for filesystem scan totals and MCP session notes. \
Use ctx healthcheck for parser/LSP/search probe results.";

pub const GRAPH_SYMBOLS_LONG_ABOUT: &str = "\
List indexed symbols grouped by file, or resolve a specific symbol query.\n\n\
If the query is a directory path, symbols in that directory are listed. \
The index is built automatically when missing.";

pub const GRAPH_CALLS_ABOUT: &str =
    "List direct callees of a symbol. Ambiguous names print candidates.";
pub const GRAPH_CALLERS_ABOUT: &str =
    "List direct callers of a symbol. Ambiguous names print candidates.";
pub const GRAPH_SLICE_ABOUT: &str =
    "Display a forward call-slice tree from the target symbol.";
pub const GRAPH_CONTEXT_LONG_ABOUT: &str = "\
Extract a graph neighborhood around a symbol and render it as markdown context.\n\n\
Modes: callers, callees, dependencies, dependents, forward-slice, reverse-slice, neighborhood.";

pub const GRAPH_AFFECT_LONG_ABOUT: &str = "\
Retrieve ranked code snippets around a query under a token budget.\n\n\
Combines graph traversal, lexical/dense hybrid ranking, and packing strategies \
(sandwich, frontloaded, balanced).";

pub const GRAPH_AFFECT_AFTER_HELP: &str = "\
Examples:\n  \
ctx graph affect run_pipeline\n  \
ctx graph affect load --mode callers --depth 2\n  \
ctx graph affect my_fn --format json --explain-ranking";

pub const MCP_ABOUT: &str =
    "Model Context Protocol server: `ctx mcp` (or `ctx mcp serve`) starts stdio MCP; `ctx mcp install` registers with agents";

pub const MCP_SERVE_LONG_ABOUT: &str = "\
Start the ctx MCP server over stdio for coding agents (Cursor, Claude, Gemini, etc.).\n\n\
Run `ctx mcp install` first, then restart the target application.";

pub const MCP_INSTALL_LONG_ABOUT: &str = "\
Auto-install / register the ctx MCP server into popular coding agents.\n\n\
Supported clients: claude, cursor, gemini, continue, code/vscode.\n\
Writes JSON entries pointing at this binary with args [\"mcp\"]. Use --dry-run to preview.";

pub const MCP_INSTALL_AFTER_HELP: &str = "\
Examples:\n  \
ctx mcp install\n  \
ctx mcp install --clients cursor,claude\n  \
ctx mcp install --dry-run";

pub const SETTING_LONG_ABOUT: &str = "\
Open interactive TUI to view/edit global settings (~/.config/ctx/config).\n\n\
The optional PATH merges legacy project-local .ctxconfig overrides for preview.";

pub const STATS_LONG_ABOUT: &str = "\
Show project scan totals (files, lines, tokens), codegraph index metadata, and last MCP stats.\n\n\
For index-only status use ctx graph info. For subsystem probes use ctx healthcheck --probe.";

pub const HEALTHCHECK_LONG_ABOUT: &str = "\
Report health of tree-sitter parsers, LSP servers, hybrid search backends, and the codegraph index.\n\n\
--probe runs live checks (LSP init, ONNX inference). Exit code is 1 when any check fails.";