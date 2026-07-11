use ctx_codegraph_lang::backend::{BackendId, ParseInput, ParsedFile, ParserBackend, ParserId};
use ctx_codegraph_lang::error::CodeGraphError;
use ctx_codegraph_lang::model::{
    Language, Occurrence, OccurrenceKind, Symbol, SymbolKind, TextRange,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};

pub struct PythonParser {
    _dummy: (),
}

thread_local! {
    static PYTHON_TS_PARSER: RefCell<Parser> = {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("failed to set tree-sitter-python language");
        RefCell::new(parser)
    };
}

// SAFETY: PythonParser holds no state and uses thread-local tree-sitter parsers, which is safe.
unsafe impl Send for PythonParser {}
unsafe impl Sync for PythonParser {}

impl PythonParser {
    pub fn new() -> Self {
        Self { _dummy: () }
    }
}

impl ParserBackend for PythonParser {
    fn parser_id(&self) -> ParserId {
        ParserId("tree-sitter-python".to_string())
    }

    fn parser_version(&self) -> String {
        "0.23.0".to_string()
    }

    fn parse_file(&self, input: ParseInput<'_>) -> Result<ParsedFile, CodeGraphError> {
        let content_str = std::fs::read_to_string(input.path)?;
        let source = content_str.as_bytes();
        let tree = PYTHON_TS_PARSER.with(|parser_cell| {
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

        state.visit(tree.root_node(), None, None, None);

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

fn trim_body_range(
    source: &[u8],
    def_range: TextRange,
    body_range: TextRange,
) -> TextRange {
    let source_str = std::str::from_utf8(source).unwrap_or("");
    let lines: Vec<&str> = source_str.lines().collect();

    let mut end_line = body_range.end_line;
    let mut end_col = body_range.end_col;

    // Find the indentation of the definition line
    let def_indent = if def_range.start_line <= lines.len() {
        let def_line = lines[def_range.start_line - 1];
        def_line.len() - def_line.trim_start().len()
    } else {
        0
    };

    while end_line > body_range.start_line {
        if end_line > lines.len() {
            end_line -= 1;
            if end_line <= lines.len() {
                end_col = lines[end_line - 1].len() + 1;
            }
            continue;
        }

        let line = lines[end_line - 1];
        let trimmed = line.trim_start();
        
        // Check if line is empty/whitespace
        if trimmed.is_empty() {
            end_line -= 1;
            if end_line <= lines.len() {
                end_col = lines[end_line - 1].len() + 1;
            }
            continue;
        }

        // Check if line is a comment
        if trimmed.starts_with('#') {
            let comment_indent = line.len() - trimmed.len();
            if comment_indent <= def_indent {
                // Unindented trailing comment!
                end_line -= 1;
                if end_line <= lines.len() {
                    end_col = lines[end_line - 1].len() + 1;
                }
                continue;
            }
        }

        break;
    }

    TextRange {
        start_line: body_range.start_line,
        start_col: body_range.start_col,
        end_line,
        end_col,
    }
}

fn compute_python_nesting_depth(node: Node, current_depth: i64) -> i64 {
    let is_nesting_node = match node.kind() {
        "if_statement" | "elif_clause" | "else_clause" | "for_statement" | "while_statement" | "except_clause" | "with_statement" => true,
        _ => false,
    };
    let next_depth = if is_nesting_node { current_depth + 1 } else { current_depth };
    let mut max_depth = next_depth;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        max_depth = max_depth.max(compute_python_nesting_depth(child, next_depth));
    }
    max_depth
}

fn compute_python_complexity_proxy(node: Node) -> i64 {
    let mut count = 0;
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "if_statement" | "elif_clause" | "for_statement" | "while_statement" | "except_clause" | "conditional_expression" => {
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

fn compute_python_param_count(node: Node) -> i64 {
    if let Some(params_node) = node.child_by_field_name("parameters") {
        let mut count = 0;
        let mut cursor = params_node.walk();
        for child in params_node.children(&mut cursor) {
            let k = child.kind();
            if k != "(" && k != ")" && k != "," {
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
        let nesting_depth = compute_python_nesting_depth(node, 0);
        let complexity_proxy = 1 + compute_python_complexity_proxy(node);
        let param_count = compute_python_param_count(node);
        let parent_symbol_id = current_parent_idx.map(|idx| ctx_codegraph_lang::model::SymbolId(idx as i64));

        Symbol {
            id: None,
            file_id: None,
            name,
            qualified_name,
            kind,
            language: Language("python".to_string()),
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
        current_class: Option<String>,
        current_function_idx: Option<usize>,
        current_parent_idx: Option<usize>,
    ) {
        let kind = node.kind();
        let mut next_class = current_class.clone();
        let mut next_function_idx = current_function_idx;
        let mut next_parent_idx = current_parent_idx;

        match kind {
            "class_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = get_node_text(name_node, self.source).to_string();
                    let range = to_text_range(node.range());
                    
                    let qualified_name = match &current_class {
                        Some(p) => format!("{}::{}::{}", self.file_stem, p, name),
                        None => format!("{}::{}", self.file_stem, name),
                    };

                    let body_range = node
                        .child_by_field_name("body")
                        .map(|b| trim_body_range(self.source, range.clone(), to_text_range(b.range())));

                    let symbol = self.create_symbol(
                        name.clone(),
                        qualified_name,
                        SymbolKind::Class,
                        range,
                        body_range,
                        node,
                        current_parent_idx,
                    );

                    let symbol_idx = self.symbols.len();
                    self.symbols.push(symbol);

                    next_class = Some(match &current_class {
                        Some(p) => format!("{}::{}", p, name),
                        None => name,
                    });
                    next_parent_idx = Some(symbol_idx);
                }
            }
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = get_node_text(name_node, self.source).to_string();
                    let range = to_text_range(node.range());
                    
                    let body_range = node
                        .child_by_field_name("body")
                        .map(|b| trim_body_range(self.source, range.clone(), to_text_range(b.range())));

                    let kind = if current_class.is_some() {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };

                    let qualified_name = match &current_class {
                        Some(p) => format!("{}::{}::{}", self.file_stem, p, name),
                        None => format!("{}::{}", self.file_stem, name),
                    };

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
            "call" => {
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
                        language: ctx_codegraph_lang::model::LanguageId::new("python"),
                        backend_id: BackendId::new("python-backend"),
                    };
                    self.occurrences.push(occurrence);
                }
            }
            "import_statement" | "import_from_statement" => {
                let raw_text = get_node_text(node, self.source).to_string();
                let range = to_text_range(node.range());

                let occurrence = Occurrence {
                    id: None,
                    file_id: None,
                    enclosing_symbol: None,
                    enclosing_temp_index: current_function_idx,
                    kind: OccurrenceKind::Import,
                    raw_text,
                    file: self.file_path.clone(),
                    range,
                    language: ctx_codegraph_lang::model::LanguageId::new("python"),
                    backend_id: BackendId::new("python-backend"),
                };
                self.occurrences.push(occurrence);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit(child, next_class.clone(), next_function_idx, next_parent_idx);
        }
    }
}

pub fn parse_python_file(path: &Path) -> Result<(Vec<Symbol>, Vec<Occurrence>), CodeGraphError> {
    // Delegate to PythonParser (which holds the reusable instance) to keep
    // public API and output identical. Each direct call creates a fresh
    // instance (init+set once), matching prior behavior.
    let p = PythonParser::new();
    let parsed = p.parse_file(ParseInput { path })?;
    Ok((parsed.symbols, parsed.occurrences))
}
