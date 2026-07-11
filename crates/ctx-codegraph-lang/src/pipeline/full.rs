//! Tier 3: Full semantic — LSP edge upgrade and semantic enrichment hooks.

use std::path::Path;
use std::time::Instant;

use crate::backend::{BackendRegistry, ResolveInput};
use crate::error::CodeGraphError;
use crate::index::BuildIndexOptions;
use crate::model::{
    GraphEdge, Occurrence, OccurrenceKind, ResolutionConfidence, Symbol,
};
use crate::pipeline::{PipelineTimings, should_use_full_lsp};

/// Upgrade call edges with full LSP resolution (Tier 3).
///
/// Re-resolves all call sites when full LSP is enabled, replacing heuristic/syntax edges
/// with `LspExact` where the language server can provide precise targets.
pub fn upgrade_edges_with_lsp(
    root: &Path,
    edges: &mut [GraphEdge],
    symbols: &[Symbol],
    occurrences: &[Occurrence],
    options: &BuildIndexOptions,
    registry: &BackendRegistry,
    timings: &mut PipelineTimings,
) -> Result<(), CodeGraphError> {
    if !should_use_full_lsp(options) {
        return Ok(());
    }

    let started = Instant::now();

    for edge in edges.iter_mut() {
        let occ_id = match edge.occurrence_id {
            Some(id) => id.0 as usize,
            None => continue,
        };
        let cs = match occurrences.get(occ_id) {
            Some(o) if o.kind == OccurrenceKind::Call => o,
            _ => continue,
        };

        let backend = registry.find_by_path(&cs.file);
        let resolver = match backend.and_then(|b| b.resolver()) {
            Some(r) => r,
            None => continue,
        };

        let resolve_input = ResolveInput {
            workspace_root: root,
            occurrence: cs,
            symbols,
        };

        match resolver.resolve(resolve_input) {
            Ok(out) => {
                if let Some(idx) = out.resolved_symbol_index {
                    edge.to_symbol_id = Some(crate::model::SymbolId(idx as i64));
                    edge.confidence = out.confidence;
                    edge.produced_by = Some(resolver.resolver_id().clone());
                } else if out.confidence == ResolutionConfidence::LspExact {
                    edge.confidence = ResolutionConfidence::Unresolved;
                }
            }
            Err(err) => {
                eprintln!(
                    "Full LSP resolution warning for call to {}: {}",
                    cs.raw_text, err
                );
            }
        }
    }

    timings.record("full_lsp_upgrade", started);
    Ok(())
}