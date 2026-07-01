mod extensions;
mod names;

use crate::FilterRule;

pub fn default_rules() -> Vec<Box<dyn FilterRule>> {
    vec![
        Box::new(names::vcs_dirs()),
        Box::new(names::dependency_dirs()),
        Box::new(names::build_dirs()),
        Box::new(names::cache_dirs()),
        Box::new(names::lockfiles()),
        Box::new(names::temporary_files()),
        Box::new(extensions::temporary_extensions()),
    ]
}
