use ctx_models::HiddenReason;

use crate::{FilterContext, FilterEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleDecision {
    Pass,
    Hide(HiddenReason),
}

pub trait FilterRule {
    fn check(&self, entry: &FilterEntry, context: &FilterContext<'_>) -> RuleDecision;
}
