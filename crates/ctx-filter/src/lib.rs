mod engine;
mod entry;
mod rule;
pub mod rules;

pub use engine::{FilterContext, FilterEngine};
pub use entry::FilterEntry;
pub use rule::{FilterRule, RuleDecision};

use ctx_models::{ScanOptions, Visibility};

pub fn classify(entry: &FilterEntry, options: &ScanOptions) -> Visibility {
    let context = FilterContext { options };
    FilterEngine::default_smart().check(entry, &context)
}
