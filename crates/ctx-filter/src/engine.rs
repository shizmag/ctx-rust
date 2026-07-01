use ctx_models::{Mode, ScanOptions, Visibility};

use crate::FilterEntry;
use crate::rule::FilterRule;
use crate::rules;

pub struct FilterContext<'a> {
    pub options: &'a ScanOptions,
}

pub struct FilterEngine {
    pub rules: Vec<Box<dyn FilterRule>>,
}

impl FilterEngine {
    pub fn new(rules: Vec<Box<dyn FilterRule>>) -> Self {
        Self { rules }
    }

    pub fn default_smart() -> Self {
        Self {
            rules: rules::default_rules(),
        }
    }

    pub fn check(&self, entry: &FilterEntry, context: &FilterContext<'_>) -> Visibility {
        if context.options.mode == Mode::All {
            return Visibility::Visible;
        }

        for rule in &self.rules {
            match rule.check(entry, context) {
                crate::RuleDecision::Pass => {}
                crate::RuleDecision::Hide(reason) => {
                    return Visibility::Hidden(reason);
                }
            }
        }
        Visibility::Visible
    }
}
