use std::collections::HashSet;

use ctx_models::{HiddenReason, NodeKind};

use crate::{FilterContext, FilterEntry, FilterRule, RuleDecision};

#[derive(Debug)]
pub struct ExtensionRule {
    extension: HashSet<&'static str>,
    kind: Option<NodeKind>,
    reason: HiddenReason,
}

impl ExtensionRule {
    pub fn new(
        extension: impl IntoIterator<Item = &'static str>,
        kind: Option<NodeKind>,
        reason: HiddenReason,
    ) -> Self {
        Self {
            extension: extension.into_iter().collect(),
            kind,
            reason,
        }
    }
}

impl FilterRule for ExtensionRule {
    fn check(&self, entry: &FilterEntry, _context: &FilterContext<'_>) -> RuleDecision {
        if let Some(kind) = self.kind {
            if entry.kind != kind {
                return RuleDecision::Pass;
            }
        }

        let Some(ext) = entry.extension() else {
            return RuleDecision::Pass;
        };

        if self.extension.contains(ext) {
            return RuleDecision::Hide(self.reason.clone());
        }

        RuleDecision::Pass
    }
}

pub fn temporary_extensions() -> ExtensionRule {
    ExtensionRule::new(
        ["log", "tmp", "temp"],
        Some(NodeKind::File),
        HiddenReason::Temporary,
    )
}
