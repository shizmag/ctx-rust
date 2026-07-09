use std::path::PathBuf;

use ctx_filter::{FilterContext, FilterEngine, FilterEntry, FilterRule, RuleDecision};
use ctx_models::{HiddenReason, Mode, NodeKind, ScanOptions, Visibility};

fn smart_options() -> ScanOptions {
    ScanOptions {
        max_depth: None,
        max_file_size: 512 * 1024,
        mode: Mode::Smart,
        exclude: Vec::new(),
    }
}

fn options_with_mode(mode: Mode) -> ScanOptions {
    ScanOptions {
        mode,
        max_depth: None,
        max_file_size: 512 * 1024,
        exclude: Vec::new(),
    }
}

fn entry(path: &str, kind: NodeKind) -> FilterEntry {
    FilterEntry::new(PathBuf::from(path), kind, 0, None)
}

fn entry_with_bytes(path: &str, kind: NodeKind, bytes: u64) -> FilterEntry {
    FilterEntry::new(PathBuf::from(path), kind, 0, Some(bytes))
}

fn classify(entry: &FilterEntry, options: &ScanOptions) -> Visibility {
    let context = FilterContext { options };
    FilterEngine::default_smart().check(entry, &context)
}

// --- Cache directories ---

#[test]
fn hides_cache_directories() {
    let cache_dirs = [
        ".cache",
        "__pycache__",
        ".pytest_cache",
        ".mypy_cache",
        ".ruff_cache",
        ".tox",
    ];

    for dir in cache_dirs {
        let item = entry(dir, NodeKind::Directory);
        assert_eq!(
            classify(&item, &smart_options()),
            Visibility::Hidden(HiddenReason::Cache),
            "expected {dir} to be hidden as cache"
        );
    }
}

#[test]
fn cache_directory_rule_does_not_apply_to_files() {
    let item = entry("__pycache__", NodeKind::File);
    assert_eq!(classify(&item, &smart_options()), Visibility::Visible);
}

// --- Temporary / generated files ---

#[test]
fn hides_temporary_and_generated_files() {
    let cases = [
        (".DS_Store", HiddenReason::Temporary),
        ("scratch.tmp", HiddenReason::Temporary),
        ("backup.bak", HiddenReason::Temporary),
        ("session.swp", HiddenReason::Temporary),
        ("debug.log", HiddenReason::Temporary),
        ("scratch.temp", HiddenReason::Temporary),
    ];

    for (path, reason) in cases {
        let item = entry(path, NodeKind::File);
        assert_eq!(
            classify(&item, &smart_options()),
            Visibility::Hidden(reason),
            "expected {path} to be hidden"
        );
    }
}

// --- Nested paths (rules match basename only) ---

#[test]
fn nested_dependency_path_file_is_visible_by_basename() {
    let item = entry("node_modules/pkg/index.js", NodeKind::File);
    assert_eq!(classify(&item, &smart_options()), Visibility::Visible);
}

#[test]
fn nested_cache_path_directory_is_hidden_by_basename() {
    let item = entry("src/__pycache__", NodeKind::Directory);
    assert_eq!(
        classify(&item, &smart_options()),
        Visibility::Hidden(HiddenReason::Cache)
    );
}

#[test]
fn nested_vcs_path_directory_is_hidden_by_basename() {
    let item = entry("project/.git", NodeKind::Directory);
    assert_eq!(
        classify(&item, &smart_options()),
        Visibility::Hidden(HiddenReason::VcsInternals)
    );
}

// --- Large file bytes field (no large-file rule yet) ---

#[test]
fn large_file_bytes_do_not_affect_visibility_without_rule() {
    let opts = ScanOptions {
        max_depth: None,
        max_file_size: 1024,
        mode: Mode::Smart,
        exclude: Vec::new(),
    };

    let small = entry_with_bytes("small.txt", NodeKind::File, 512);
    let large = entry_with_bytes("huge.bin", NodeKind::File, 10 * 1024 * 1024);

    assert_eq!(classify(&small, &opts), Visibility::Visible);
    assert_eq!(classify(&large, &opts), Visibility::Visible);
}

// --- Smart mode vs specialized modes ---

#[test]
fn specialized_modes_still_apply_builtin_hiding_rules() {
    for mode in [Mode::Code, Mode::Docs, Mode::Llm] {
        let opts = options_with_mode(mode);

        assert_eq!(
            classify(&entry("node_modules", NodeKind::Directory), &opts),
            Visibility::Hidden(HiddenReason::Dependencies),
            "node_modules should stay hidden in {mode:?}"
        );
        assert_eq!(
            classify(&entry(".git", NodeKind::Directory), &opts),
            Visibility::Hidden(HiddenReason::VcsInternals),
            ".git should stay hidden in {mode:?}"
        );
        assert_eq!(
            classify(&entry("Cargo.lock", NodeKind::File), &opts),
            Visibility::Hidden(HiddenReason::Lockfile),
            "lockfiles should stay hidden in {mode:?}"
        );
    }
}

#[test]
fn code_mode_hides_non_code_after_builtin_rules_pass() {
    let opts = options_with_mode(Mode::Code);

    assert_eq!(
        classify(&entry("photo.png", NodeKind::File), &opts),
        Visibility::Hidden(HiddenReason::NonCode)
    );
    assert_eq!(
        classify(&entry("main.rs", NodeKind::File), &opts),
        Visibility::Visible
    );
}

#[test]
fn docs_mode_hides_non_docs_after_builtin_rules_pass() {
    let opts = options_with_mode(Mode::Docs);

    assert_eq!(
        classify(&entry("main.rs", NodeKind::File), &opts),
        Visibility::Hidden(HiddenReason::NonDocs)
    );
    assert_eq!(
        classify(&entry("guide.md", NodeKind::File), &opts),
        Visibility::Visible
    );
}

#[test]
fn llm_mode_hides_binaries_after_builtin_rules_pass() {
    let opts = options_with_mode(Mode::Llm);

    assert_eq!(
        classify(&entry("image.png", NodeKind::File), &opts),
        Visibility::Hidden(HiddenReason::Binary)
    );
    assert_eq!(
        classify(&entry("lib.rs", NodeKind::File), &opts),
        Visibility::Visible
    );
}

// --- Case sensitivity ---

#[test]
fn directory_name_rules_are_case_sensitive() {
    let cases = [
        ("NODE_MODULES", HiddenReason::Dependencies),
        ("Node_Modules", HiddenReason::Dependencies),
        (".GIT", HiddenReason::VcsInternals),
        ("TARGET", HiddenReason::BuildArtifacts),
        ("__PYCACHE__", HiddenReason::Cache),
    ];

    for (name, _) in cases {
        let item = entry(name, NodeKind::Directory);
        assert_eq!(
            classify(&item, &smart_options()),
            Visibility::Visible,
            "expected {name} to remain visible (case-sensitive match)"
        );
    }
}

#[test]
fn extension_rules_are_case_sensitive() {
    let item = entry("notes.TXT", NodeKind::File);
    assert_eq!(classify(&item, &smart_options()), Visibility::Visible);

    let item = entry("scratch.TMP", NodeKind::File);
    assert_eq!(classify(&item, &smart_options()), Visibility::Visible);
}

// --- Files with no extension ---

#[test]
fn extensionless_files_in_smart_mode_are_visible() {
    for name in ["Makefile", "LICENSE", "Dockerfile", "README", "notes"] {
        let item = entry(name, NodeKind::File);
        assert_eq!(
            classify(&item, &smart_options()),
            Visibility::Visible,
            "{name} should be visible in Smart mode"
        );
    }
}

#[test]
fn extensionless_files_in_code_mode() {
    let opts = options_with_mode(Mode::Code);

    let kept = ["Makefile", "LICENSE", "Dockerfile", "README", "Cargo.toml"];
    for name in kept {
        assert_eq!(
            classify(&entry(name, NodeKind::File), &opts),
            Visibility::Visible,
            "{name} should be kept in Code mode"
        );
    }

    assert_eq!(
        classify(&entry("notes", NodeKind::File), &opts),
        Visibility::Hidden(HiddenReason::NonCode)
    );
}

#[test]
fn extensionless_files_in_docs_mode() {
    let opts = options_with_mode(Mode::Docs);

    for name in ["README", "LICENSE", "CHANGELOG"] {
        assert_eq!(
            classify(&entry(name, NodeKind::File), &opts),
            Visibility::Visible,
            "{name} should be kept in Docs mode"
        );
    }

    assert_eq!(
        classify(&entry("Makefile", NodeKind::File), &opts),
        Visibility::Hidden(HiddenReason::NonDocs)
    );
}

#[test]
fn extensionless_files_in_llm_mode_are_visible() {
    let opts = options_with_mode(Mode::Llm);

    for name in ["Makefile", "LICENSE", "Dockerfile"] {
        assert_eq!(
            classify(&entry(name, NodeKind::File), &opts),
            Visibility::Visible,
            "{name} should be visible in LLM mode (no extension to ignore)"
        );
    }
}

// --- Regression: first matching rule wins ---

struct StubRule {
    name: &'static str,
    reason: HiddenReason,
}

impl FilterRule for StubRule {
    fn check(&self, entry: &FilterEntry, _context: &FilterContext<'_>) -> RuleDecision {
        if entry.name == self.name {
            RuleDecision::Hide(self.reason.clone())
        } else {
            RuleDecision::Pass
        }
    }
}

#[test]
fn first_matching_rule_wins_in_engine_order() {
    let engine = FilterEngine::new(vec![
        Box::new(StubRule {
            name: "contested",
            reason: HiddenReason::VcsInternals,
        }),
        Box::new(StubRule {
            name: "contested",
            reason: HiddenReason::Dependencies,
        }),
    ]);

    let item = entry("contested", NodeKind::File);
    let context = FilterContext {
        options: &smart_options(),
    };

    assert_eq!(
        engine.check(&item, &context),
        Visibility::Hidden(HiddenReason::VcsInternals)
    );
}

#[test]
fn lockfile_rule_takes_precedence_over_later_temporary_extension_rule() {
    // Regression: default rule order places lockfiles before temporary extensions.
    // A lockfile must not be misclassified if a later rule could also match.
    let item = entry("pnpm-lock.yaml", NodeKind::File);
    assert_eq!(
        classify(&item, &smart_options()),
        Visibility::Hidden(HiddenReason::Lockfile)
    );
}