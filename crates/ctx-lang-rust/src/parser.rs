use ctx_codegraph_lang::backend::{BackendId, ParseInput, ParsedFile, ParserBackend, ParserId};
use ctx_codegraph_lang::error::CodeGraphError;
use ctx_codegraph_lang::model::{
    Language, Occurrence, OccurrenceKind, Symbol, SymbolKind, TextRange,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};

pub struct RustParser {
    _dummy: (),
}

thread_local! {
    static RUST_TS_PARSER: RefCell<Parser> = {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("failed to set tree-sitter-rust language");
        RefCell::new(parser)
    };
}

// SAFETY: RustParser holds no state and uses thread-local tree-sitter parsers, which is safe.
unsafe impl Send for RustParser {}
unsafe impl Sync for RustParser {}

impl RustParser {
    pub fn new() -> Self {
        Self { _dummy: () }
    }
}

impl ParserBackend for RustParser {
    fn parser_id(&self) -> ParserId {
        ParserId("tree-sitter-rust".to_string())
    }

    fn parser_version(&self) -> String {
        "0.20.0".to_string()
    }

    fn parse_file(&self, input: ParseInput<'_>) -> Result<ParsedFile, CodeGraphError> {
        let content_str = std::fs::read_to_string(input.path)?;
        let source = content_str.as_bytes();
        let tree = RUST_TS_PARSER.with(|parser_cell| {
            let mut parser = parser_cell.borrow_mut();
            parser
                .parse(source, None)
                .ok_or_else(|| CodeGraphError::Parse(format!("Failed to parse {}", input.path.display())))
        })?;

        if tree.root_node().has_error() {
            return Err(CodeGraphError::Parse(format!(
                "Syntax error in {}",
                input.path.display()
            )));
        }

        let file_stem = input.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("mod")
            .to_string();

        let mut state = ParserState {
            source,
            file_path: input.path.to_path_buf(),
            file_stem,
            symbols: Vec::new(),
            occurrences: Vec::new(),
        };

        state.visit(tree.root_node(), None, None, None, None);

        Ok(ParsedFile {
            symbols: state.symbols,
            occurrences: state.occurrences,
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

fn compute_rust_nesting_depth(node: Node, current_depth: i64) -> i64 {
    let is_nesting_node = match node.kind() {
        "block" | "match_arm" | "if_expression" | "for_expression" | "while_expression" | "loop_expression" => true,
        _ => false,
    };
    let next_depth = if is_nesting_node { current_depth + 1 } else { current_depth };
    let mut max_depth = next_depth;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        max_depth = max_depth.max(compute_rust_nesting_depth(child, next_depth));
    }
    max_depth
}

fn compute_rust_complexity_proxy(node: Node) -> i64 {
    let mut count = 0;
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "if_expression" | "match_arm" | "for_expression" | "while_expression" | "loop_expression" | "question_mark" => {
                count += 1;
            }
            _ => {}
        }
        let mut cursor = n.walk();
        for child in n.children(&mut cursor) {
            stack.push(child);
        }
    }
    count
}

fn compute_rust_param_count(node: Node) -> i64 {
    if let Some(params_node) = node.child_by_field_name("parameters") {
        let mut count = 0;
        let mut cursor = params_node.walk();
        for child in params_node.children(&mut cursor) {
            let k = child.kind();
            if k != "(" && k != ")" && k != "," && k != "|" {
                count += 1;
            }
        }
        count
    } else {
        0
    }
}

impl<'a> ParserState<'a> {
    fn create_symbol(
        &self,
        name: String,
        qualified_name: String,
        kind: SymbolKind,
        range: TextRange,
        body_range: Option<TextRange>,
        node: Node,
        current_parent_idx: Option<usize>,
    ) -> Symbol {
        let lines_of_code = (range.end_line as i64 - range.start_line as i64 + 1).max(1);
        let nesting_depth = compute_rust_nesting_depth(node, 0);
        let complexity_proxy = 1 + compute_rust_complexity_proxy(node);
        let param_count = compute_rust_param_count(node);
        let parent_symbol_id = current_parent_idx.map(|idx| ctx_codegraph_lang::model::SymbolId(idx as i64));

        Symbol {
            id: None,
            file_id: None,
            name,
            qualified_name,
            kind,
            language: Language("rust".to_string()),
            file: self.file_path.clone(),
            range,
            body_range,
            nesting_depth,
            lines_of_code,
            complexity_proxy,
            param_count,
            parent_symbol_id,
            fan_in: 0,
            fan_out: 0,
            coupling: 0.0,
            cohesion: 0.0,
        }
    }

    fn visit(
        &mut self,
        node: Node,
        current_impl: Option<String>,
        current_function_idx: Option<usize>,
        current_modules: Option<String>,
        current_parent_idx: Option<usize>,
    ) {
        let kind = node.kind();
        let mut next_impl = current_impl.clone();
        let mut next_function_idx = current_function_idx;
        let mut next_modules = current_modules.clone();
        let mut next_parent_idx = current_parent_idx;

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
                let symbol = self.create_symbol(
                    impl_name.clone(),
                    impl_name.clone(),
                    SymbolKind::Impl,
                    range,
                    None,
                    node,
                    current_parent_idx,
                );
                let symbol_idx = self.symbols.len();
                self.symbols.push(symbol);
                next_parent_idx = Some(symbol_idx);
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

                    let symbol = self.create_symbol(
                        mod_name,
                        qname,
                        SymbolKind::Module,
                        range,
                        None,
                        node,
                        current_parent_idx,
                    );
                    let symbol_idx = self.symbols.len();
                    self.symbols.push(symbol);
                    next_parent_idx = Some(symbol_idx);
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
                    let symbol = self.create_symbol(
                        name.clone(),
                        qualified_name,
                        SymbolKind::Struct,
                        range,
                        None,
                        node,
                        current_parent_idx,
                    );
                    let symbol_idx = self.symbols.len();
                    self.symbols.push(symbol);
                    next_parent_idx = Some(symbol_idx);
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
                    let symbol = self.create_symbol(
                        name.clone(),
                        qualified_name,
                        SymbolKind::Enum,
                        range,
                        None,
                        node,
                        current_parent_idx,
                    );
                    let symbol_idx = self.symbols.len();
                    self.symbols.push(symbol);
                    next_parent_idx = Some(symbol_idx);
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
                    let symbol = self.create_symbol(
                        name.clone(),
                        qualified_name,
                        SymbolKind::Trait,
                        range,
                        None,
                        node,
                        current_parent_idx,
                    );
                    let symbol_idx = self.symbols.len();
                    self.symbols.push(symbol);
                    next_parent_idx = Some(symbol_idx);
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

                    let symbol = self.create_symbol(
                        name,
                        qualified_name,
                        kind,
                        range,
                        body_range,
                        node,
                        current_parent_idx,
                    );

                    let symbol_idx = self.symbols.len();
                    self.symbols.push(symbol);
                    next_function_idx = Some(symbol_idx);
                    next_parent_idx = Some(symbol_idx);
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
                        kind: OccurrenceKind::Call,
                        raw_text,
                        file: self.file_path.clone(),
                        range,
                        language: ctx_codegraph_lang::model::LanguageId::rust(),
                        backend_id: BackendId::new("rust"),
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
                next_parent_idx,
            );
        }
    }
}

pub fn parse_rust_file(path: &Path) -> Result<(Vec<Symbol>, Vec<Occurrence>), CodeGraphError> {
    let p = RustParser::new();
    let parsed = p.parse_file(ParseInput { path })?;
    Ok((parsed.symbols, parsed.occurrences))
}
