use std::path::PathBuf;

use ctx_filter::{FilterEntry, classify};
use ctx_models::{HiddenReason, Mode, NodeKind, ScanOptions, Visibility};

fn options() -> ScanOptions {
    ScanOptions {
        max_depth: None,
        max_file_size: 512 * 1024,
        mode: Mode::Smart,
    }
}

fn entry(path: &str, kind: NodeKind) -> FilterEntry {
    FilterEntry::new(PathBuf::from(path), kind, 0, None)
}

#[test]
fn hides_vcs_directories() {
    let item = entry(".git", NodeKind::Directory);

    let result = classify(&item, &options());

    assert_eq!(result, Visibility::Hidden(HiddenReason::VcsInternals));
}

#[test]
fn hides_dependency_directories() {
    let item = entry("node_modules", NodeKind::Directory);

    let result = classify(&item, &options());

    assert_eq!(result, Visibility::Hidden(HiddenReason::Dependencies));
}

#[test]
fn hides_rust_target_directory() {
    let item = entry("target", NodeKind::Directory);

    let result = classify(&item, &options());

    assert_eq!(result, Visibility::Hidden(HiddenReason::BuildArtifacts));
}

#[test]
fn hides_lockfiles() {
    let item = entry("package-lock.json", NodeKind::File);

    let result = classify(&item, &options());

    assert_eq!(result, Visibility::Hidden(HiddenReason::Lockfile));
}

#[test]
fn all_mode_disables_builtin_hiding() {
    let options = ScanOptions {
        mode: Mode::All,
        max_depth: None,
        max_file_size: 512 * 1024,
    };

    let item = entry("node_modules", NodeKind::Directory);

    let result = classify(&item, &options);

    assert_eq!(result, Visibility::Visible);
}

#[test]
fn symlink_is_not_hidden_by_directory_rule() {
    let item = entry("node_modules", NodeKind::Symlink);

    let result = classify(&item, &options());

    assert_eq!(result, Visibility::Visible);
}

#[test]
fn code_mode_keeps_code_config_and_readme() {
    let code_opts = ScanOptions {
        mode: Mode::Code,
        max_depth: None,
        max_file_size: 512 * 1024,
    };

    // Keep code
    assert_eq!(classify(&entry("main.rs", NodeKind::File), &code_opts), Visibility::Visible);
    assert_eq!(classify(&entry("utils.py", NodeKind::File), &code_opts), Visibility::Visible);
    
    // Keep configs
    assert_eq!(classify(&entry("Cargo.toml", NodeKind::File), &code_opts), Visibility::Visible);
    assert_eq!(classify(&entry(".gitignore", NodeKind::File), &code_opts), Visibility::Visible);
    
    // Keep readme-like
    assert_eq!(classify(&entry("README.md", NodeKind::File), &code_opts), Visibility::Visible);
    assert_eq!(classify(&entry("LICENSE", NodeKind::File), &code_opts), Visibility::Visible);
    
    // Hide others
    assert_eq!(classify(&entry("image.png", NodeKind::File), &code_opts), Visibility::Hidden(HiddenReason::NonCode));
    assert_eq!(classify(&entry("doc.pdf", NodeKind::File), &code_opts), Visibility::Hidden(HiddenReason::NonCode));
    assert_eq!(classify(&entry("notes.txt", NodeKind::File), &code_opts), Visibility::Hidden(HiddenReason::NonCode));
}

#[test]
fn docs_mode_keeps_docs_and_text() {
    let docs_opts = ScanOptions {
        mode: Mode::Docs,
        max_depth: None,
        max_file_size: 512 * 1024,
    };

    // Keep docs
    assert_eq!(classify(&entry("README.md", NodeKind::File), &docs_opts), Visibility::Visible);
    assert_eq!(classify(&entry("notes.txt", NodeKind::File), &docs_opts), Visibility::Visible);
    assert_eq!(classify(&entry("doc.pdf", NodeKind::File), &docs_opts), Visibility::Visible);
    
    // Hide others
    assert_eq!(classify(&entry("main.rs", NodeKind::File), &docs_opts), Visibility::Hidden(HiddenReason::NonDocs));
    assert_eq!(classify(&entry("image.png", NodeKind::File), &docs_opts), Visibility::Hidden(HiddenReason::NonDocs));
}

#[test]
fn llm_mode_ignores_media_and_binaries() {
    let llm_opts = ScanOptions {
        mode: Mode::Llm,
        max_depth: None,
        max_file_size: 512 * 1024,
    };

    // Keep code and docs
    assert_eq!(classify(&entry("main.rs", NodeKind::File), &llm_opts), Visibility::Visible);
    assert_eq!(classify(&entry("README.md", NodeKind::File), &llm_opts), Visibility::Visible);
    
    // Hide binaries/media
    assert_eq!(classify(&entry("image.png", NodeKind::File), &llm_opts), Visibility::Hidden(HiddenReason::Binary));
    assert_eq!(classify(&entry("archive.zip", NodeKind::File), &llm_opts), Visibility::Hidden(HiddenReason::Binary));
    assert_eq!(classify(&entry("run.exe", NodeKind::File), &llm_opts), Visibility::Hidden(HiddenReason::Binary));
}
