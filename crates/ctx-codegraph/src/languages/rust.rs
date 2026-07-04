use crate::error::CodeGraphError;
use crate::model::{CallSite, Language, Symbol, SymbolKind, TextRange};
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};

struct ParserState<'a> {
    source: &'a [u8],
    file_path: PathBuf,
    file_stem: String,
    symbols: Vec<Symbol>,
    call_sites: Vec<CallSite>,
}

fn to_text_range(range: tree_sitter::Range) -> TextRange {
    TextRange {
        start_line: range.start_point.row + 1,
        start_col: range.start_point.column + 1,
        end_line: range.end_point.row + 1,
        end_col: range.end_point.column + 1,
    }
}

fn get_node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.byte_range()]).unwrap_or("")
}

fn clean_type_name(s: &str) -> String {
    let clean = if let Some(idx) = s.find('<') {
        s[..idx].trim()
    } else {
        s.trim()
    };
    // Also clean up any leading/trailing reference symbols or deref, though usually type_name is clean.
    clean.to_string()
}

impl<'a> ParserState<'a> {
    fn visit(
        &mut self,
        node: Node,
        current_impl: Option<String>,
        current_function_idx: Option<usize>,
        current_modules: Option<String>,
    ) {
        let kind = node.kind();
        let mut next_impl = current_impl.clone();
        let mut next_function_idx = current_function_idx;
        let mut next_modules = current_modules.clone();

        match kind {
            "impl_item" => {
                let type_node = node.child_by_field_name("type");
                let trait_node = node.child_by_field_name("trait");

                let impl_name = if let Some(t) = type_node {
                    let type_text = clean_type_name(get_node_text(t, self.source));
                    if let Some(tr) = trait_node {
                        let trait_text = clean_type_name(get_node_text(tr, self.source));
                        format!("{} as {}", type_text, trait_text)
                    } else {
                        type_text
                    }
                } else {
                    "impl".to_string()
                };

                next_impl = Some(impl_name.clone());

                let range = to_text_range(node.range());
                let symbol = Symbol {
                    id: None,
                    file_id: None,
                    name: impl_name.clone(),
                    qualified_name: impl_name.clone(),
                    kind: SymbolKind::Impl,
                    language: Language::Rust,
                    file: self.file_path.clone(),
                    range,
                    body_range: None,
                };
                self.symbols.push(symbol);
            }
            "mod_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let mod_name = get_node_text(name_node, self.source).to_string();
                    let range = to_text_range(node.range());

                    let qname = match current_modules {
                        Some(ref m) => format!("{}::{}::{}", self.file_stem, m, mod_name),
                        None => format!("{}::{}", self.file_stem, mod_name),
                    };

                    next_modules = Some(match current_modules {
                        Some(ref m) => format!("{}::{}", m, mod_name),
                        None => mod_name.clone(),
                    });

                    let symbol = Symbol {
                        id: None,
                        file_id: None,
                        name: mod_name,
                        qualified_name: qname,
                        kind: SymbolKind::Module,
                        language: Language::Rust,
                        file: self.file_path.clone(),
                        range,
                        body_range: None,
                    };
                    self.symbols.push(symbol);
                }
            }
            "struct_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = get_node_text(name_node, self.source).to_string();
                    let range = to_text_range(node.range());
                    let qualified_name = match current_modules {
                        Some(ref m) => format!("{}::{}::{}", self.file_stem, m, name),
                        None => format!("{}::{}", self.file_stem, name),
                    };
                    let symbol = Symbol {
                        id: None,
                        file_id: None,
                        name: name.clone(),
                        qualified_name,
                        kind: SymbolKind::Struct,
                        language: Language::Rust,
                        file: self.file_path.clone(),
                        range,
                        body_range: None,
                    };
                    self.symbols.push(symbol);
                }
            }
            "enum_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = get_node_text(name_node, self.source).to_string();
                    let range = to_text_range(node.range());
                    let qualified_name = match current_modules {
                        Some(ref m) => format!("{}::{}::{}", self.file_stem, m, name),
                        None => format!("{}::{}", self.file_stem, name),
                    };
                    let symbol = Symbol {
                        id: None,
                        file_id: None,
                        name: name.clone(),
                        qualified_name,
                        kind: SymbolKind::Enum,
                        language: Language::Rust,
                        file: self.file_path.clone(),
                        range,
                        body_range: None,
                    };
                    self.symbols.push(symbol);
                }
            }
            "trait_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = get_node_text(name_node, self.source).to_string();
                    let range = to_text_range(node.range());
                    let qualified_name = match current_modules {
                        Some(ref m) => format!("{}::{}::{}", self.file_stem, m, name),
                        None => format!("{}::{}", self.file_stem, name),
                    };
                    let symbol = Symbol {
                        id: None,
                        file_id: None,
                        name: name.clone(),
                        qualified_name,
                        kind: SymbolKind::Trait,
                        language: Language::Rust,
                        file: self.file_path.clone(),
                        range,
                        body_range: None,
                    };
                    self.symbols.push(symbol);
                }
            }
            "function_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = get_node_text(name_node, self.source).to_string();

                    let mut is_test = false;
                    let mut prev = node.prev_sibling();
                    while let Some(p) = prev {
                        let pk = p.kind();
                        if pk == "attribute_item" {
                            let attr_text = get_node_text(p, self.source);
                            if attr_text.contains("test") {
                                is_test = true;
                                break;
                            }
                            prev = p.prev_sibling();
                        } else if pk == "line_comment" || pk == "block_comment" {
                            prev = p.prev_sibling();
                        } else {
                            break;
                        }
                    }

                    let kind = if is_test {
                        SymbolKind::Test
                    } else if current_impl.is_some() {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };

                    let prefix = match current_modules {
                        Some(ref m) => format!("{}::{}", self.file_stem, m),
                        None => self.file_stem.clone(),
                    };

                    let qualified_name = if let Some(ref impl_name) = current_impl {
                        format!("{}::{}", impl_name, name)
                    } else {
                        format!("{}::{}", prefix, name)
                    };

                    let range = to_text_range(node.range());
                    let body_range = node
                        .child_by_field_name("body")
                        .map(|b| to_text_range(b.range()));

                    let symbol = Symbol {
                        id: None,
                        file_id: None,
                        name,
                        qualified_name,
                        kind,
                        language: Language::Rust,
                        file: self.file_path.clone(),
                        range,
                        body_range,
                    };

                    let symbol_idx = self.symbols.len();
                    self.symbols.push(symbol);
                    next_function_idx = Some(symbol_idx);
                }
            }
            "call_expression" => {
                if let Some(func_node) = node.child_by_field_name("function") {
                    let raw_name = get_node_text(func_node, self.source).to_string();
                    let range = to_text_range(func_node.range());

                    let call_site = CallSite {
                        id: None,
                        file_id: None,
                        from: None,
                        from_temp_index: current_function_idx,
                        raw_name,
                        file: self.file_path.clone(),
                        range,
                    };
                    self.call_sites.push(call_site);
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit(
                child,
                next_impl.clone(),
                next_function_idx,
                next_modules.clone(),
            );
        }
    }
}

pub fn parse_rust_file(path: &Path) -> Result<(Vec<Symbol>, Vec<CallSite>), CodeGraphError> {
    let content_str = std::fs::read_to_string(path)?;
    let source = content_str.as_bytes();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|e| CodeGraphError::Parse(e.to_string()))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| CodeGraphError::Parse(format!("Failed to parse {}", path.display())))?;

    if tree.root_node().has_error() {
        return Err(CodeGraphError::Parse(format!("Syntax error in {}", path.display())));
    }

    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mod")
        .to_string();

    let mut state = ParserState {
        source,
        file_path: path.to_path_buf(),
        file_stem,
        symbols: Vec::new(),
        call_sites: Vec::new(),
    };

    state.visit(tree.root_node(), None, None, None);

    Ok((state.symbols, state.call_sites))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_free_function() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("lib.rs");
        let code = r#"
            pub fn run_pipeline() {
                load();
            }

            fn load() {}
        "#;
        fs::write(&file_path, code).unwrap();

        let (symbols, _call_sites) = parse_rust_file(&file_path).unwrap();

        let run_pipeline = symbols.iter().find(|s| s.name == "run_pipeline").unwrap();
        let load = symbols.iter().find(|s| s.name == "load").unwrap();

        assert_eq!(run_pipeline.kind, SymbolKind::Function);
        assert_eq!(load.kind, SymbolKind::Function);
        assert!(run_pipeline.range.start_line > 0);
        assert!(run_pipeline.body_range.is_some());
    }

    #[test]
    fn test_impl_methods() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("lib.rs");
        let code = r#"
            pub struct Pipeline;

            impl Pipeline {
                pub fn new() -> Self {
                    Self
                }

                pub fn run(&self) {
                    self.load();
                }

                fn load(&self) {}
            }
        "#;
        fs::write(&file_path, code).unwrap();

        let (symbols, _call_sites) = parse_rust_file(&file_path).unwrap();

        let pipeline = symbols
            .iter()
            .find(|s| s.name == "Pipeline" && s.kind == SymbolKind::Struct)
            .unwrap();
        let new_method = symbols.iter().find(|s| s.name == "new").unwrap();
        let run_method = symbols.iter().find(|s| s.name == "run").unwrap();
        let load_method = symbols.iter().find(|s| s.name == "load").unwrap();

        assert_eq!(pipeline.kind, SymbolKind::Struct);
        assert_eq!(new_method.kind, SymbolKind::Method);
        assert_eq!(run_method.kind, SymbolKind::Method);
        assert_eq!(load_method.kind, SymbolKind::Method);

        assert!(new_method.qualified_name.contains("Pipeline"));
        assert!(run_method.qualified_name.contains("Pipeline"));
        assert!(load_method.qualified_name.contains("Pipeline"));
    }

    #[test]
    fn test_trait_methods_and_declaration() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("lib.rs");
        let code = r#"
            pub trait Runner {
                fn run(&self);
            }

            pub struct Job;

            impl Runner for Job {
                fn run(&self) {}
            }
        "#;
        fs::write(&file_path, code).unwrap();

        let (symbols, _call_sites) = parse_rust_file(&file_path).unwrap();

        let runner = symbols
            .iter()
            .find(|s| s.name == "Runner" && s.kind == SymbolKind::Trait)
            .unwrap();
        let run_impl = symbols
            .iter()
            .find(|s| s.name == "run" && s.kind == SymbolKind::Method)
            .unwrap();

        assert_eq!(runner.kind, SymbolKind::Trait);
        assert!(run_impl.qualified_name.contains("Job"));
        assert!(run_impl.qualified_name.contains("Runner"));
    }

    #[test]
    fn test_test_functions() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("lib.rs");
        let code = r#"
            #[test]
            fn test_run_pipeline() {
                run_pipeline();
            }

            fn helper() {}
        "#;
        fs::write(&file_path, code).unwrap();

        let (symbols, _call_sites) = parse_rust_file(&file_path).unwrap();

        let test_run = symbols
            .iter()
            .find(|s| s.name == "test_run_pipeline")
            .unwrap();
        let helper = symbols.iter().find(|s| s.name == "helper").unwrap();

        assert_eq!(test_run.kind, SymbolKind::Test);
        assert_eq!(helper.kind, SymbolKind::Function);
    }

    #[test]
    fn test_simple_call() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("lib.rs");
        let code = r#"
            fn run_pipeline() {
                load();
            }
            fn load() {}
        "#;
        fs::write(&file_path, code).unwrap();

        let (symbols, call_sites) = parse_rust_file(&file_path).unwrap();

        let run_pipeline_idx = symbols
            .iter()
            .position(|s| s.name == "run_pipeline")
            .unwrap();
        let call = call_sites.iter().find(|c| c.raw_name == "load").unwrap();

        assert_eq!(call.from_temp_index, Some(run_pipeline_idx));
    }

    #[test]
    fn test_qualified_path_call() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("lib.rs");
        let code = r#"
            fn run_pipeline() {
                crate::pipeline::load();
            }
        "#;
        fs::write(&file_path, code).unwrap();

        let (_symbols, call_sites) = parse_rust_file(&file_path).unwrap();

        let call = call_sites
            .iter()
            .find(|c| c.raw_name == "crate::pipeline::load")
            .unwrap();
        assert_eq!(call.raw_name, "crate::pipeline::load");
    }

    #[test]
    fn test_method_call() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("lib.rs");
        let code = r#"
            struct Pipeline;

            impl Pipeline {
                fn run(&self) {
                    self.load();
                }

                fn load(&self) {}
            }
        "#;
        fs::write(&file_path, code).unwrap();

        let (symbols, call_sites) = parse_rust_file(&file_path).unwrap();

        let run_idx = symbols.iter().position(|s| s.name == "run").unwrap();
        let call = call_sites
            .iter()
            .find(|c| c.raw_name == "self.load")
            .unwrap();

        assert_eq!(call.from_temp_index, Some(run_idx));
    }

    #[test]
    fn test_associated_function_call() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("lib.rs");
        let code = r#"
            struct Pipeline;

            impl Pipeline {
                fn new() -> Self {
                    Self
                }
            }

            fn build() {
                Pipeline::new();
            }
        "#;
        fs::write(&file_path, code).unwrap();

        let (_symbols, call_sites) = parse_rust_file(&file_path).unwrap();

        let call = call_sites
            .iter()
            .find(|c| c.raw_name == "Pipeline::new")
            .unwrap();
        assert_eq!(call.raw_name, "Pipeline::new");
    }
}
