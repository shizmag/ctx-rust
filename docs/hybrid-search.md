# Hybrid Search

`ctx` combines a semantic code graph with optional hybrid retrieval: BM25 lexical search, dense embeddings, reciprocal-rank fusion (RRF), and graph-aware chunking. The unified MCP tool `retrieve_context` routes queries through graph traversal or hybrid search depending on strategy and configuration.

## Overview

```
retrieve_context
    │
    ├─ strategy=graph ──► graph BFS + token-budget packing (existing ContextPack)
    │
    └─ strategy=hybrid|lexical|dense ──► HybridSearcher
            ├─ LexicalIndex (Tantivy BM25)
            ├─ DenseIndex (sqlite BLOB + cosine KNN)
            └─ RRF fusion ──► chunk expansion ──► ContextPack
```

When `embedding_model` is **not** set in `.ctxconfig`, hybrid/lexical/dense strategies are unavailable at query time. `retrieve_context` automatically falls back to `strategy=graph` so graph tools keep working without ONNX models.

## Index layout

All indexes live under `.ctx-codegraph/` in the workspace root.

| Store | Path | Contents |
|-------|------|----------|
| Graph (schema v5) | `codegraph.sqlite` | symbols, edges, files, `chunks` table |
| Lexical (Tantivy) | `lexical/` | BM25 index over chunk text |
| Dense embeddings | `dense.sqlite` | `chunk_embeddings` BLOB table (768-dim vectors) |

`Contains` edges (parent/child chunk relationships) are written during store rebuild after symbols have stable database IDs.

## Crates

| Crate | Role |
|-------|------|
| `ctx-codegraph-chunk` | Graph-aware `Chunk` / `ChunkBuilder` (symbol + occurrence chunks, parent/child) |
| `ctx-codegraph-lexical` | Tantivy `LexicalIndex` (BM25) |
| `ctx-codegraph-dense` | `DenseIndex` with brute-force cosine KNN |
| `ctx-codegraph-models` | ONNX `EmbeddingModel`, `RerankerModel`, `CodeTokenizer` via `ort` |
| `ctx-codegraph-search` | `HybridSearcher`, RRF, `HybridSearchBackend` trait |
| `ctx-codegraph-store` | Schema v5, `chunks.rs`, `search_build.rs` |
| `ctx-codegraph` | `hybrid_retrieval.rs`, `hybrid_service.rs`, `retrieve_context_for_service` |
| `ctx-codegraph-storage` | `WorkspaceHybridBackend` |

## Models

Default paths (documentation / CLI hints only — **not** auto-loaded):

- **Embeddings**: `snowflake-arctic-embed-m-v2.0` (768 dimensions)
- **Reranker**: `jina-reranker-v2-base-multilingual`

Search indexing is enabled only when `embedding_model` is **explicitly** present in `.ctxconfig`. Default model path constants in `ctx-config` are helpers for documentation; they do not trigger builds in CI or tests.

## Configuration (`.ctxconfig`)

```ini
# Required to enable hybrid search indexing and query-time embeddings
embedding_model = /path/to/snowflake-arctic-embed-m-v2.0/model.onnx

# Optional
reranker_model = /path/to/jina-reranker-v2-base-multilingual/model.onnx
tokenizer_dir = /path/to/tokenizer   # defaults to embedding model parent dir
rrf_k = 60
bm25_top_k = 50
dense_top_k = 50
rerank_top_k = 20
enable_rerank = false
default_retrieval_strategy = hybrid   # graph | hybrid | lexical | dense
```

## Building indexes

### CLI

```bash
# Graph only (default when embedding_model not configured)
ctx graph build

# All build methods: LSP + lexical + dense embeddings
ctx graph build --all

# Graph + lexical + dense embeddings (requires embedding_model in .ctxconfig)
ctx graph build --with-emb --with-lex

# Disable search indexes explicitly (overrides --all)
ctx graph build --all --without-emb --without-lex
```

### MCP `rebuild_index`

```json
{
  "name": "rebuild_index",
  "arguments": {
    "with_all": true
  }
}
```

Or explicitly:

```json
{
  "name": "rebuild_index",
  "arguments": {
    "use_lsp": true,
    "with_emb": true,
    "with_lex": true
  }
}
```

Search build failures are logged as warnings and do not block the graph index rebuild.

## Querying

### MCP `retrieve_context`

Primary unified tool (replaces `get_affected_context`, `get_graph_context`, `search_code`, `get_callers`, `get_callees`).

| Argument | Description |
|----------|-------------|
| `query` | Symbol name, qualified path, or free-text search string |
| `strategy` | `graph` (default fallback), `hybrid`, `lexical`, `dense` |
| `graph_mode` | When `strategy=graph`: `neighborhood`, `callers`, `callees`, `dependencies`, `dependents`, `impact`, etc. |
| `depth` | Integer or `"auto"` |
| `token_budget` | Default `12000` |
| `format` | `yaml` (default), `json`, `text` |

Examples:

```json
{"query": "my_func", "strategy": "hybrid", "format": "yaml", "token_budget": 8000}
```

```json
{"query": "foo", "strategy": "graph", "graph_mode": "callers", "depth": 3}
```

```json
{"query": "TODO fixme", "strategy": "lexical"}
```

### MCP tools (5 total)

1. `retrieve_context` — hybrid/graph/lexical/dense retrieval
2. `list_symbols`
3. `read_file`
4. `rebuild_index`
5. `get_project_context`

## Retrieval pipeline (hybrid)

1. **Chunk** source files during index build (`ChunkBuilder` uses symbols, `Contains` edges, occurrences).
2. **Lexical**: Tantivy BM25 over chunk text → top-K hits.
3. **Dense**: embed query with ONNX → cosine KNN over stored vectors → top-K hits.
4. **RRF**: fuse ranked lists with `score = Σ 1/(k + rank)` (default `k=60`).
5. **Pack**: map chunk hits to `ContextPack` with graph expansion, ranking, and token budget.

Reranker wiring at query time is controlled by `enable_rerank` in config; full cross-encoder reranking is optional follow-up work.

## Compatibility

Store schema version **5** adds the `chunks` table. Rebuild is required when upgrading from v4. `compat.rs` tracks `chunk_builder_version` for chunk format changes.

## Limitations and follow-up

- Dense search uses brute-force KNN (suitable for workspace-scale indexes).
- Incremental lexical/dense updates on single-file change (SA-12) are partial.
- Reranker is loaded when configured but may not yet rerank the final candidate list at query time.
- Without `embedding_model` in `.ctxconfig`, only graph retrieval is available (automatic fallback).