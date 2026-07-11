//! Tiered feature extraction pipeline.
//!
//! - **Tier 1 (Fast)**: parallel Tree-Sitter parse, structural metrics, occurrences.
//! - **Tier 2 (Balanced)**: name-index call graph resolution.
//! - **Tier 3 (Full)**: LSP edge upgrade, semantic search (delegated to store).

pub mod balanced;
pub mod fast;
pub mod full;

use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

use crate::backend::BackendRegistry;
use crate::error::CodeGraphError;
use crate::index::BuildIndexOptions;
use crate::model::{ExtractionTier, StepTiming};

/// Per-step timing collected during indexing.
#[derive(Debug, Default, Clone)]
pub struct PipelineTimings {
    pub steps: Vec<StepTiming>,
}

impl PipelineTimings {
    pub fn record(&mut self, step: &str, started: Instant) {
        self.steps.push(StepTiming {
            step: step.to_string(),
            duration_ms: started.elapsed().as_millis() as u64,
        });
    }

    pub fn log_summary(&self) {
        if self.steps.is_empty() {
            return;
        }
        let total: u64 = self.steps.iter().map(|s| s.duration_ms).sum();
        eprintln!("Pipeline timings (total {total}ms):");
        for step in &self.steps {
            eprintln!("  {}: {}ms", step.step, step.duration_ms);
        }
    }
}

/// Discover indexable source files under `root`, respecting discovery skip rules.
pub fn discover_source_files(
    root: &Path,
    registry: &BackendRegistry,
) -> Result<Vec<PathBuf>, CodeGraphError> {
    let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
        let path = e.path();
        if path.is_dir()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
            && crate::discovery::should_skip_dir(name)
        {
            return false;
        }
        true
    });

    let mut matching_files = Vec::new();
    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if crate::index::should_index_path_with_registry(path, registry) && path.is_file() {
            matching_files.push(path.to_path_buf());
        }
    }
    Ok(matching_files)
}

/// Resolve the effective target extraction tier from build options.
pub fn target_tier(options: &BuildIndexOptions) -> ExtractionTier {
    options.extraction_tier.unwrap_or(ExtractionTier::Balanced)
}

/// Whether call-graph resolution should run for the given options.
pub fn should_resolve_calls(options: &BuildIndexOptions) -> bool {
    target_tier(options) >= ExtractionTier::Balanced
}

/// Whether full LSP resolution should run (Tier 3 only).
pub fn should_use_full_lsp(options: &BuildIndexOptions) -> bool {
    target_tier(options) >= ExtractionTier::Full && options.effective_use_lsp()
}

/// Whether light LSP (upgrade unresolved/heuristic edges only) should run at Tier 2+.
pub fn should_use_light_lsp(options: &BuildIndexOptions) -> bool {
    options.effective_use_lsp()
        && options.lsp_mode.allows_light()
        && target_tier(options) >= ExtractionTier::Balanced
        && !should_use_full_lsp(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LspMode;

    #[test]
    fn target_tier_defaults_to_balanced() {
        let options = BuildIndexOptions::default();
        assert_eq!(target_tier(&options), ExtractionTier::Balanced);
    }

    #[test]
    fn fast_tier_skips_call_resolution() {
        let options = BuildIndexOptions {
            extraction_tier: Some(ExtractionTier::Fast),
            ..Default::default()
        };
        assert!(!should_resolve_calls(&options));
    }

    #[test]
    fn full_tier_enables_full_lsp_when_use_lsp() {
        let options = BuildIndexOptions {
            extraction_tier: Some(ExtractionTier::Full),
            use_lsp: true,
            ..Default::default()
        };
        assert!(should_use_full_lsp(&options));
        assert!(!should_use_light_lsp(&options));
    }

    #[test]
    fn balanced_tier_uses_light_lsp_when_configured() {
        let options = BuildIndexOptions {
            extraction_tier: Some(ExtractionTier::Balanced),
            use_lsp: true,
            lsp_mode: LspMode::Light,
            ..Default::default()
        };
        assert!(!should_use_full_lsp(&options));
        assert!(should_use_light_lsp(&options));
    }
}