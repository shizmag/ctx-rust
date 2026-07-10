use ctx_codegraph_lang::backend::BackendRegistry;
use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::CodeIndex;
use ctx_codegraph_store::storage::{open_db, rebuild_index_db_with_registry};
use ctx_lang_python::PythonBackend;
use ctx_lang_rust::RustBackend;
use std::fs;
use std::path::{Path, PathBuf};

pub fn production_registry() -> BackendRegistry {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(RustBackend::new()));
    reg.register(Box::new(PythonBackend::new()));
    reg
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
) -> (rusqlite::Connection, CodeIndex, BackendRegistry) {
    let registry = production_registry();
    let (index, _) = rebuild_index_db_with_registry(root, options, &registry).unwrap();
    let conn = open_db(root, &registry).unwrap();
    (conn, index, registry)
}

pub fn lexical_search_options() -> BuildIndexOptions {
    BuildIndexOptions {
        with_lexical: Some(true),
        with_embeddings: Some(false),
        ..Default::default()
    }
}

pub fn no_search_options() -> BuildIndexOptions {
    BuildIndexOptions {
        with_lexical: Some(false),
        with_embeddings: Some(false),
        ..Default::default()
    }
}

pub fn lexical_index_dir(root: &Path) -> PathBuf {
    root.join(".ctx-codegraph").join("lexical")
}