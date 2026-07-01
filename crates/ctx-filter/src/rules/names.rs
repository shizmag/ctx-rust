use std::collections::HashSet;

use ctx_models::{HiddenReason, NodeKind};

use crate::{FilterContext, FilterEntry, FilterRule, RuleDecision};

#[derive(Debug)]
pub struct NameRule {
    name: HashSet<&'static str>,
    kind: Option<NodeKind>,
    reason: HiddenReason,
}

impl NameRule {
    pub fn new(
        names: impl IntoIterator<Item = &'static str>,
        kind: Option<NodeKind>,
        reason: HiddenReason,
    ) -> Self {
        Self {
            name: names.into_iter().collect(),
            kind,
            reason,
        }
    }
}

impl FilterRule for NameRule {
    fn check(&self, entry: &FilterEntry, _context: &FilterContext<'_>) -> RuleDecision {
        if let Some(kind) = self.kind {
            if entry.kind != kind {
                return RuleDecision::Pass;
            }
        }

        if self.name.contains(entry.name.as_str()) {
            return RuleDecision::Hide(self.reason.clone());
        }

        RuleDecision::Pass
    }
}

pub fn vcs_dirs() -> NameRule {
    NameRule::new(
        [".git", ".jj", ".hg", ".svn"],
        Some(NodeKind::Directory),
        HiddenReason::VcsInternals,
    )
}

pub fn dependency_dirs() -> NameRule {
    NameRule::new(
        ["node_modules", ".venv", "venv"],
        Some(NodeKind::Directory),
        HiddenReason::Dependencies,
    )
}

pub fn build_dirs() -> NameRule {
    NameRule::new(
        ["target", "dist", "build", ".next"],
        Some(NodeKind::Directory),
        HiddenReason::BuildArtifacts,
    )
}

pub fn cache_dirs() -> NameRule {
    NameRule::new(
        ["__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache"],
        Some(NodeKind::Directory),
        HiddenReason::Cache,
    )
}

pub fn lockfiles() -> NameRule {
    NameRule::new(
        [
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "Cargo.lock",
        ],
        Some(NodeKind::File),
        HiddenReason::Lockfile,
    )
}

pub fn temporary_files() -> NameRule {
    NameRule::new([".DS_Store"], Some(NodeKind::File), HiddenReason::Temporary)
}
