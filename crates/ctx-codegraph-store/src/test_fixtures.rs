//! Shared test helpers for index/storage integration tests.
//!
//! Enabled with the `test-fixtures` crate feature.

use ctx_codegraph_lang::backend::BackendRegistry;
use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::{CodeIndex, FileChangeDetection};
use crate::storage::{open_db, rebuild_index_db_with_registry};
use ctx_lang_python::PythonBackend;
use ctx_lang_rust::RustBackend;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static REGISTRY: OnceLock<BackendRegistry> = OnceLock::new();

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Build options that disable lexical/embedding search indexes (fast tests).
///
/// Uses content-hash change detection so incremental tests do not need
/// `thread::sleep` to bump mtimes.
pub fn no_search_options() -> BuildIndexOptions {
    BuildIndexOptions {
        with_lexical: Some(false),
        with_embeddings: Some(false),
        change_detection: FileChangeDetection::ContentHash,
        ..Default::default()
    }
}

struct IsolatedXdgGuard {
    _env_lock: MutexGuard<'static, ()>,
    _temp_dir: tempfile::TempDir,
}

impl Drop for IsolatedXdgGuard {
    fn drop(&mut self) {
        // SAFETY: guarded test-only env mutation; restored on scope exit (incl. panic).
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }
}

/// Run `f` with an empty isolated `XDG_CONFIG_HOME` (mutex-guarded).
pub fn with_isolated_global_config<F: FnOnce()>(f: F) {
    let temp_dir = tempfile::tempdir().unwrap();
    let xdg = temp_dir.path().join("xdg-config");
    fs::create_dir_all(&xdg).unwrap();
    let _guard = IsolatedXdgGuard {
        _env_lock: env_lock(),
        _temp_dir: temp_dir,
    };
    // SAFETY: guarded test-only env mutation.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &xdg) };
    f();
}

/// Production-like language backend registry (cached).
pub fn production_registry() -> &'static BackendRegistry {
    REGISTRY.get_or_init(|| {
        let mut reg = BackendRegistry::new();
        reg.register(Box::new(RustBackend::new()));
        reg.register(Box::new(PythonBackend::new()));
        reg
    })
}

/// Minimal Cargo workspace with nested modules and a cross-file call edge.
pub fn setup_mini_rust_project(root: &Path) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"search_test\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"pub mod util;

pub fn greet() {
    util::helper();
}

pub fn farewell() {}
"#,
    )
    .unwrap();
    fs::write(
        src.join("util.rs"),
        r#"pub fn helper() {
    println!("help");
}
"#,
    )
    .unwrap();
}

pub fn indexed_db(
    root: &Path,
    options: BuildIndexOptions,
) -> (rusqlite::Connection, CodeIndex, &'static BackendRegistry) {
    let registry = production_registry();
    let (index, _) = rebuild_index_db_with_registry(root, options, registry).unwrap();
    let conn = open_db(root, registry).unwrap();
    (conn, index, registry)
}

pub fn lexical_search_options() -> BuildIndexOptions {
    BuildIndexOptions {
        with_lexical: Some(true),
        with_embeddings: Some(false),
        change_detection: FileChangeDetection::ContentHash,
        ..Default::default()
    }
}

pub fn lexical_index_dir(root: &Path) -> PathBuf {
    root.join(".ctx-codegraph").join("lexical")
}