use crate::backend::{ParseInput, ParsedFile, ParserBackend, ParserId};
use crate::error::CodeGraphError;
use crate::model::{Language, Occurrence, Symbol, SymbolKind, TextRange};
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};

pub struct RustParser;

impl ParserBackend for RustParser {
    fn parser_id(&self) -> ParserId {
        ParserId("tree-sitter-rust".to_string())
    }

    fn parser_version(&self) -> String {
        "0.20.0".to_string()
    }

    fn parse_file(&self, input: ParseInput<'_>) -> Result<ParsedFile, CodeGraphError> {
        let (symbols, occurrences) = parse_rust_file(input.path)?;
        Ok(ParsedFile {
            symbols,
            occurrences,
        })
    }
}

struct ParserState<'a> {
    source: &'a [u8],
    file_path: PathBuf,
    file_stem: String,
    symbols: Vec<Symbol>,
    occurrences: Vec<Occurrence>,
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
                    language: Language("rust".to_string()),
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
                        language: Language("rust".to_string()),
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
                        language: Language("rust".to_string()),
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
                        language: Language("rust".to_string()),
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
                        language: Language("rust".to_string()),
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

                    // Light extraction of signature using tree-sitter fields (Rust parser).
                    // Currently surfaced via post-load extract_signature in model for MCP outputs
                    // (disambig, candidates, context). Future: store in Symbol/LanguageObject + DB.
                    let _sig = {
                        let params = node.child_by_field_name("parameters")
                            .map(|p| get_node_text(p, self.source).to_string())
                            .unwrap_or_else(|| "()".to_string());
                        let ret = node.child_by_field_name("return_type")
                            .map(|r| format!(" {}", get_node_text(r, self.source).trim()))
                            .unwrap_or_default();
                        format!("fn {}{}{}", name, params, ret)
                    };

                    let symbol = Symbol {
                        id: None,
                        file_id: None,
                        name,
                        qualified_name,
                        kind,
                        language: Language("rust".to_string()),
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
                    let raw_text = get_node_text(func_node, self.source).to_string();
                    let range = to_text_range(func_node.range());

                    let occurrence = Occurrence {
                        id: None,
                        file_id: None,
                        enclosing_symbol: None,
                        enclosing_temp_index: current_function_idx,
                        kind: crate::model::OccurrenceKind::Call,
                        raw_text,
                        file: self.file_path.clone(),
                        range,
                        language: crate::model::LanguageId::rust(),
                        backend_id: "rust".to_string(),
                    };
                    self.occurrences.push(occurrence);
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

pub fn parse_rust_file(path: &Path) -> Result<(Vec<Symbol>, Vec<Occurrence>), CodeGraphError> {
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
        return Err(CodeGraphError::Parse(format!(
            "Syntax error in {}",
            path.display()
        )));
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
        occurrences: Vec::new(),
    };

    state.visit(tree.root_node(), None, None, None);

    Ok((state.symbols, state.occurrences))
}
