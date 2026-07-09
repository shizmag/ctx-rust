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
        ["log", "tmp", "temp", "swp", "bak"],
        Some(NodeKind::File),
        HiddenReason::Temporary,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FilterContext, FilterEntry, RuleDecision};
    use ctx_models::{Mode, NodeKind, ScanOptions};

    fn context() -> FilterContext<'static> {
        static OPTIONS: ScanOptions = ScanOptions {
            max_depth: None,
            max_file_size: 512 * 1024,
            mode: Mode::Smart,
            exclude: Vec::new(),
        };
        FilterContext {
            options: &OPTIONS,
        }
    }

    #[test]
    fn extension_rule_matches_file_suffix() {
        let rule = temporary_extensions();
        let entry = FilterEntry::new("backup.bak".into(), NodeKind::File, 0, None);

        assert_eq!(
            rule.check(&entry, &context()),
            RuleDecision::Hide(HiddenReason::Temporary)
        );
    }

    #[test]
    fn extension_rule_passes_for_extensionless_files() {
        let rule = temporary_extensions();
        let entry = FilterEntry::new("Makefile".into(), NodeKind::File, 0, None);

        assert_eq!(rule.check(&entry, &context()), RuleDecision::Pass);
    }

    #[test]
    fn extension_rule_is_case_sensitive() {
        let rule = temporary_extensions();
        let entry = FilterEntry::new("file.TMP".into(), NodeKind::File, 0, None);

        assert_eq!(rule.check(&entry, &context()), RuleDecision::Pass);
    }
}
