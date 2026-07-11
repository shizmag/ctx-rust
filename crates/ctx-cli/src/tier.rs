use std::path::{Component, Path, PathBuf};

use ctx_config::Config;

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliExtractionTier {
    Fast,
    #[value(alias = "balance")]
    Balanced,
    Full,
}

impl From<CliExtractionTier> for ctx_codegraph::model::ExtractionTier {
    fn from(tier: CliExtractionTier) -> Self {
        match tier {
            CliExtractionTier::Fast => Self::Fast,
            CliExtractionTier::Balanced => Self::Balanced,
            CliExtractionTier::Full => Self::Full,
        }
    }
}

/// If `path` looks like a tier shorthand (single non-existent component matching a tier name),
/// return the tier and reset the project path to `.`.
pub fn resolve_build_path_and_tier(
    path: &Path,
    tier_flag: Option<CliExtractionTier>,
    config_tier: Option<&str>,
) -> (PathBuf, Option<ctx_codegraph::model::ExtractionTier>) {
    let mut project_path = path.to_path_buf();
    let mut tier = tier_flag.map(Into::into);

    if tier.is_none() {
        if let Some(t) = tier_from_path_shorthand(&project_path) {
            tier = Some(t);
            project_path = PathBuf::from(".");
        }
    }

    if tier.is_none() {
        tier = config_tier.and_then(ctx_codegraph::model::ExtractionTier::from_str);
    }

    (project_path, tier)
}

fn tier_from_path_shorthand(path: &Path) -> Option<ctx_codegraph::model::ExtractionTier> {
    if path.exists() {
        return None;
    }
    let mut components = path.components();
    let first = components.next()?;
    if components.next().is_some() {
        return None;
    }
    let name = match first {
        Component::Normal(s) => s.to_str()?,
        _ => return None,
    };
    ctx_codegraph::model::ExtractionTier::from_str(name)
}

/// Resolve whether LSP is enabled and which mode to use for a graph build.
pub fn resolve_graph_lsp_settings(
    tier: Option<ctx_codegraph::model::ExtractionTier>,
    cli_with_lsp: bool,
    cli_all: bool,
    no_rust_analyzer: bool,
    config: &Config,
) -> (bool, ctx_codegraph::model::LspMode) {
    use ctx_codegraph::model::{ExtractionTier, LspMode};

    let cli_lsp = (cli_with_lsp || cli_all) && !no_rust_analyzer;
    let config_use_lsp = config.use_lsp.unwrap_or(false);
    let wants_lsp = cli_lsp || config_use_lsp;
    let use_lsp = wants_lsp
        && matches!(
            tier,
            Some(ExtractionTier::Balanced) | Some(ExtractionTier::Full)
        );

    let lsp_mode = if use_lsp {
        match tier {
            Some(ExtractionTier::Full) => LspMode::Full,
            Some(ExtractionTier::Balanced) => LspMode::Light,
            _ => LspMode::Off,
        }
    } else {
        config
            .lsp_mode
            .as_deref()
            .and_then(LspMode::from_str)
            .unwrap_or(LspMode::Off)
    };

    (use_lsp, lsp_mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorthand_fast_when_missing() {
        let (path, tier) = resolve_build_path_and_tier(Path::new("fast"), None, None);
        assert_eq!(path, PathBuf::from("."));
        assert_eq!(tier, Some(ctx_codegraph::model::ExtractionTier::Fast));
    }

    #[test]
    fn shorthand_balance_alias() {
        let (_, tier) = resolve_build_path_and_tier(Path::new("balance"), None, None);
        assert_eq!(tier, Some(ctx_codegraph::model::ExtractionTier::Balanced));
    }

    #[test]
    fn explicit_tier_flag_wins() {
        let (path, tier) = resolve_build_path_and_tier(
            Path::new("fast"),
            Some(CliExtractionTier::Full),
            None,
        );
        assert_eq!(path, Path::new("fast"));
        assert_eq!(tier, Some(ctx_codegraph::model::ExtractionTier::Full));
    }

    #[test]
    fn balanced_tier_uses_light_lsp_when_config_use_lsp() {
        let config = Config {
            use_lsp: Some(true),
            ..Config::default_values()
        };
        let (use_lsp, mode) = resolve_graph_lsp_settings(
            Some(ctx_codegraph::model::ExtractionTier::Balanced),
            false,
            false,
            false,
            &config,
        );
        assert!(use_lsp);
        assert_eq!(mode, ctx_codegraph::model::LspMode::Light);
    }

    #[test]
    fn full_tier_uses_full_lsp_when_config_use_lsp() {
        let config = Config {
            use_lsp: Some(true),
            ..Config::default_values()
        };
        let (use_lsp, mode) = resolve_graph_lsp_settings(
            Some(ctx_codegraph::model::ExtractionTier::Full),
            false,
            false,
            false,
            &config,
        );
        assert!(use_lsp);
        assert_eq!(mode, ctx_codegraph::model::LspMode::Full);
    }

    #[test]
    fn fast_tier_keeps_lsp_off_even_when_config_use_lsp() {
        let config = Config {
            use_lsp: Some(true),
            ..Config::default_values()
        };
        let (use_lsp, mode) = resolve_graph_lsp_settings(
            Some(ctx_codegraph::model::ExtractionTier::Fast),
            false,
            false,
            false,
            &config,
        );
        assert!(!use_lsp);
        assert_eq!(mode, ctx_codegraph::model::LspMode::Off);
    }
}