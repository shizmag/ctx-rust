use std::path::{Component, Path, PathBuf};

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
}