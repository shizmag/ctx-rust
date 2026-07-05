# Budgeted Ranked Graph-Context Queries

`ctx-codegraph` supports ranked, budget-aware context retrieval designed to fetch high-quality context snippets from a repository's code relationship graph without exceeding a user-specified token budget.

This capability is exposed via the CLI command `ctx graph affect <query>`.

---

## Command Syntax & Options

```bash
ctx graph affect <query> [options]
```

### Options

* `--mode <neighborhood|callers|callees|dependencies|dependents|forward|reverse|impact>`
  Specifies the graph traversal direction starting from the resolved query roots.
  * `neighborhood`: Traverses both inbound and outbound edges (default).
  * `callers` / `reverse`: Traverses inbound (caller) edges.
  * `callees` / `forward`: Traverses outbound (callee) edges.
  * `dependencies`: Traverses outbound references/imports.
  * `dependents`: Traverses inbound references/imports.
  * `impact`: Inbound edges, and one-level outbound edge for impact analysis.

* `--depth <n|auto>` (default: `auto`)
  Limits the maximum BFS depth.
  * `auto`: Explores dynamically up to a depth of 3, halting early if the next layer of code snippets would exceed the token budget.
  * `n`: Explores up to a fixed depth of `n` layers.

* `--token-budget <n>` (default: `12000`)
  Target budget for the packed context (in estimated tokens). The pack will attempt to fit the most relevant symbol definitions under this limit.

* `--packing <sandwich|frontloaded|balanced>` (default: `sandwich`)
  The layout strategy used to pack sections:
  * `sandwich`: To combat the "lost in the middle" effect, places the summary and highest relevance snippets at the beginning, lower-relevance supporting definitions in the middle, and a concise recap with omitted items at the end.
  * `frontloaded`: Places all snippets in descending order of relevance.
  * `balanced`: Distributes snippets based on balanced weights.

* `--ranking <hybrid|graph|lexical>` (default: `hybrid`)
  The ranking algorithm used to score graph neighbors:
  * `hybrid`: Combines topological relevance with lexical match score (BM25-like IDF).
  * `graph`: Uses topological path distance, locality, and edge confidence.
  * `lexical`: Ranks strictly by lexical query matches.

* `--format <text|json>` (default: `text`)
  Output format:
  * `text`: Human-readable Markdown sections.
  * `json`: Structured JSON containing diagnostics, token estimations, omitted items, scores, and raw sections.

* `--edge-kind <kind>` (repeatable)
  Limits edge traversal to specific relationship kinds (e.g. `--edge-kind Call --edge-kind Reference`).

* `--include-tests` (default: false)
  Include test files and test symbols. If false, test modules/symbols receive a heavy score penalty.

* `--include-unresolved` (default: false)
  Include unresolved reference edges. If false, unresolved edges are filtered out of BFS traversal.

---

## Ranking Details

Symbols discovered during BFS traversal are ranked using the following topological and lexical weights:

1. **Topological Distance**:
   - Depth 0 (Roots): `+100.0`
   - Depth 1 (Direct neighbors): `+10.0`
   - Depth 2: `+6.0`
   - Depth 3: `+3.0`
   - Depth 4+: `+1.0`

2. **Locality Boosts**:
   - Same File: `+2.0`
   - Same Module/Folder Prefix: `+1.0`

3. **Confidence Level**:
   - `LspExact` / `Exact`: `+2.0`
   - `Syntax` / `Local`: `+1.2`
   - `Heuristic` / `NameOnly` / `Ambiguous`: `+0.5`
   - `Unresolved`: `-1.0`

4. **Penalties**:
   - Test files/symbols: `-2.0`
   - Vendor/Generated files: `-4.0`

5. **Lexical Match Boost**:
   - Boosts symbols whose names, qualified paths, or file paths match terms from the query using a TF-IDF/BM25 IDF calculation.

---

## Packing Details & Snippet Extraction

For each included symbol, `ctx-codegraph` extracts the source lines containing the symbol definition.
- **Context Lines**: Includes `--context-lines` (default: 3) lines before and after the symbol span.
- **Body Truncation**: To prevent single huge functions from consuming the entire token budget, bodies longer than 80 lines (or 160 lines for root symbols) are truncated in the middle, leaving only the first 15 lines and the last 15 lines.
