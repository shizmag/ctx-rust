<<<<<<<< HEAD:crates/ctx-lang-rust/src/parser_tests.rs
use crate::parser::{RustParser, parse_rust_file};
use ctx_codegraph_lang::backend::{ParseInput, ParserBackend};
use ctx_codegraph_lang::model::{OccurrenceKind, SymbolKind};
|||||||| parent of f9ba449 (Fix clippy warnings and consolidate lang ID types after crate split):crates/ctx-codegraph-storage/src/languages/rust/parser_tests.rs
use super::parser::{RustParser, parse_rust_file};
use crate::backend::{ParseInput, ParserBackend};
use crate::model::{OccurrenceKind, SymbolKind};
========
use ctx_lang_rust::parser::{RustParser, parse_rust_file};
use ctx_codegraph_lang::backend::{ParseInput, ParserBackend};
use ctx_codegraph_lang::model::{OccurrenceKind, SymbolKind};
use std::fs;
>>>>>>>> f9ba449 (Fix clippy warnings and consolidate lang ID types after crate split):crates/ctx-lang-rust/tests/parser_tests.rs
use std::io::Write;
use tempfile::NamedTempFile;

fn write_temp_rust(content: &str) -> NamedTempFile {
    let mut temp = NamedTempFile::with_suffix(".rs").unwrap();
    write!(temp, "{}", content).unwrap();
    temp
}

#[test]
fn test_parse_rust_code() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("lib.rs");
    let code = r#"
        pub fn run_pipeline() {
            let x = load();
            process(x);
        }

        #[test]
        fn test_helper() {
            save(1);
        }

        impl MyStruct {
            pub fn new() -> Self {
                MyStruct
            }
        }
    "#;
    fs::write(&file_path, code).unwrap();

    let (symbols, call_sites) = parse_rust_file(&file_path).unwrap();

    let run_pipeline = symbols.iter().find(|s| s.name == "run_pipeline").unwrap();
    assert_eq!(run_pipeline.kind, SymbolKind::Function);

    let test_helper = symbols.iter().find(|s| s.name == "test_helper").unwrap();
    assert_eq!(test_helper.kind, SymbolKind::Test);

    let new_method = symbols.iter().find(|s| s.name == "new").unwrap();
    assert_eq!(new_method.kind, SymbolKind::Method);
    assert_eq!(new_method.qualified_name, "MyStruct::new");

    let load_call = call_sites.iter().find(|c| c.raw_text == "load").unwrap();
    assert_eq!(
        load_call.enclosing_temp_index,
        Some(
            symbols
                .iter()
                .position(|s| s.name == "run_pipeline")
                .unwrap()
        )
    );
}

#[test]
fn test_parse_rust_structs() {
    let temp = write_temp_rust(
        r#"
pub struct Point {
    x: i32,
    y: i32,
}

struct Internal;
"#,
    );

    let (symbols, _) = parse_rust_file(temp.path()).unwrap();
    let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Point"));
    assert!(names.contains(&"Internal"));

    let point = symbols.iter().find(|s| s.name == "Point").unwrap();
    assert_eq!(point.kind, SymbolKind::Struct);
    assert!(point.qualified_name.ends_with("::Point"));
}

#[test]
fn test_parse_rust_enums() {
    let temp = write_temp_rust(
        r#"
pub enum Color {
    Red,
    Green,
    Blue,
}
"#,
    );

    let (symbols, _) = parse_rust_file(temp.path()).unwrap();
    let color = symbols.iter().find(|s| s.name == "Color").unwrap();
    assert_eq!(color.kind, SymbolKind::Enum);
}

#[test]
fn test_parse_rust_traits() {
    let temp = write_temp_rust(
        r#"
pub trait Drawable {
    fn draw(&self);
}
"#,
    );

    let (symbols, _) = parse_rust_file(temp.path()).unwrap();
    let drawable = symbols.iter().find(|s| s.name == "Drawable").unwrap();
    assert_eq!(drawable.kind, SymbolKind::Trait);
}

#[test]
fn test_parse_rust_impl_blocks() {
    let temp = write_temp_rust(
        r#"
struct Circle;

impl Circle {
    fn area(&self) -> f64 {
        3.14
    }
}

impl Drawable for Circle {
    fn draw(&self) {}
}
"#,
    );

    let (symbols, _) = parse_rust_file(temp.path()).unwrap();

    let circle_impl = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Impl && s.name == "Circle")
        .unwrap();
    assert_eq!(circle_impl.kind, SymbolKind::Impl);

    let trait_impl = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Impl && s.name.contains("Drawable"))
        .unwrap();
    assert!(trait_impl.name.contains("Circle"));

    let area = symbols.iter().find(|s| s.name == "area").unwrap();
    assert_eq!(area.kind, SymbolKind::Method);
    assert_eq!(area.qualified_name, "Circle::area");

    let draw = symbols.iter().find(|s| s.name == "draw").unwrap();
    assert_eq!(draw.kind, SymbolKind::Method);
}

#[test]
fn test_parse_rust_modules() {
    let temp = write_temp_rust(
        r#"
mod outer {
    mod inner {
        fn nested() {}
    }
}
"#,
    );

    let (symbols, _) = parse_rust_file(temp.path()).unwrap();

    let outer = symbols.iter().find(|s| s.name == "outer").unwrap();
    assert_eq!(outer.kind, SymbolKind::Module);

    let inner = symbols.iter().find(|s| s.name == "inner").unwrap();
    assert_eq!(inner.kind, SymbolKind::Module);
    assert!(inner.qualified_name.contains("outer::inner"));

    let nested = symbols.iter().find(|s| s.name == "nested").unwrap();
    assert_eq!(nested.kind, SymbolKind::Function);
}

#[test]
fn test_parse_rust_empty_file() {
    let temp = write_temp_rust("");
    let (symbols, occurrences) = parse_rust_file(temp.path()).unwrap();
    assert!(symbols.is_empty());
    assert!(occurrences.is_empty());
}

#[test]
fn test_parse_rust_comments_only() {
    let temp = write_temp_rust(
        r#"
// line comment
/* block comment */
/// doc comment
"#,
    );
    let (symbols, occurrences) = parse_rust_file(temp.path()).unwrap();
    assert!(symbols.is_empty());
    assert!(occurrences.is_empty());
}

#[test]
fn test_parse_rust_malformed_graceful_error() {
    let temp = write_temp_rust("fn broken( {");
    let result = parse_rust_file(temp.path());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Syntax error") || err.contains("parse"));
}

#[test]
fn test_rust_parser_backend_trait() {
    let temp = write_temp_rust("fn via_backend() { helper(); }");
    let parser = RustParser;
    assert_eq!(parser.parser_id().0, "tree-sitter-rust");
    assert_eq!(parser.parser_version(), "0.20.0");

    let parsed = parser
        .parse_file(ParseInput { path: temp.path() })
        .unwrap();
    assert_eq!(parsed.symbols.len(), 1);
    assert_eq!(parsed.symbols[0].name, "via_backend");

    let call = parsed
        .occurrences
        .iter()
        .find(|o| o.raw_text == "helper")
        .unwrap();
    assert_eq!(call.kind, OccurrenceKind::Call);
}