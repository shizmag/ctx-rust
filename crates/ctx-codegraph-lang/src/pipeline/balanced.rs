//! Tier 2: Balanced graph — name-index call resolution (no LSP).

use std::path::Path;
use std::time::Instant;

use crate::backend::{BackendRegistry, ResolveInput};
use crate::error::CodeGraphError;
use crate::index::BuildIndexOptions;
use crate::model::{
    GraphEdge, Occurrence, OccurrenceId, OccurrenceKind, ResolutionConfidence, Symbol, SymbolId,
};
use crate::pipeline::{PipelineTimings, should_use_light_lsp};
use crate::noop::SymbolNameIndex;

/// Resolve call occurrences into graph edges using the name index (Tier 2).
pub fn resolve_call_edges(
    root: &Path,
    symbols: &[Symbol],
    occurrences: &[Occurrence],
    options: &BuildIndexOptions,
    registry: &BackendRegistry,
    timings: &mut PipelineTimings,
) -> Result<Vec<GraphEdge>, CodeGraphError> {
    let started = Instant::now();
    let name_index = SymbolNameIndex::new(symbols);
    let light_lsp = should_use_light_lsp(options);

    let mut edges = Vec::new();

    for (call_site_idx, cs) in occurrences.iter().enumerate() {
        if cs.kind != OccurrenceKind::Call {
            continue;
        }

        let from_id = match cs.enclosing_symbol {
            Some(id) => id,
            None => continue,
        };

        let mut resolved_idx = None;
        let mut confidence = ResolutionConfidence::Unresolved;

        let backend = registry.find_by_path(&cs.file);
        let resolver = backend.and_then(|b| b.resolver());

        if light_lsp
            && let Some(res) = resolver
        {
            let resolve_input = ResolveInput {
                workspace_root: root,
                occurrence: cs,
                symbols,
            };
            match res.resolve(resolve_input) {
                Ok(out) => {
                    resolved_idx = out.resolved_symbol_index;
                    confidence = out.confidence;
                }
                Err(err) => {
                    eprintln!(
                        "Light LSP resolution warning for call to {}: {}",
                        cs.raw_text, err
                    );
                }
            }
        }

        if resolved_idx.is_none() {
            let (fallback_idx, fallback_conf) =
                name_index.resolve(&cs.raw_text, symbols, &cs.file);
            resolved_idx = fallback_idx;
            confidence = fallback_conf;
        }

        let edge = GraphEdge {
            id: None,
            kind: crate::model::EdgeKind::Call,
            from_file_id: cs.file_id,
            from_symbol_id: Some(from_id),
            to_symbol_id: resolved_idx.map(|idx| SymbolId(idx as i64)),
            to_external: None,
            occurrence_id: Some(OccurrenceId(call_site_idx as i64)),
            raw_text: Some(cs.raw_text.clone()),
            range: Some(cs.range.clone()),
            confidence,
            produced_by: Some(
                resolver
                    .map(|r| r.resolver_id().clone())
                    .unwrap_or_else(|| crate::backend::ResolverId::new("noop")),
            ),
        };
        edges.push(edge);
    }

    timings.record("balanced_call_graph", started);
    if light_lsp && !options.should_use_full_lsp() {
        registry.shutdown_lsp_clients();
    }
    Ok(edges)
}