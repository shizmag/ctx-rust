use ctx_lang_python::parser::parse_python_file;
use ctx_codegraph_lang::model::{OccurrenceKind, SymbolKind};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_parse_python_pipeline() {
    let content = r#"import os
from mlx_lm import load, generate

class RAGPipeline:
    def __init__(self, model_path: str):
        self.model_path = model_path
        self.model, self.tokenizer = load(model_path)

    def retrieve(self, query: str) -> list[str]:
        return ["Context 1", "Context 2"]

    def run(self, query: str) -> str:
        contexts = self.retrieve(query)
        prompt = f"Context: {contexts}\nQuery: {query}"
        response = generate(self.model, self.tokenizer, prompt=prompt)
        return response

def main():
    pipeline = RAGPipeline("mlx-community/Llama-3-8B-Instruct-4bit")
    res = pipeline.run("What is gravity?")
    print(res)
"#;

    let mut temp = NamedTempFile::new().unwrap();
    write!(temp, "{}", content).unwrap();

    let (symbols, occurrences) = parse_python_file(temp.path()).unwrap();

    assert_eq!(symbols.len(), 5);

    let class_sym = &symbols[0];
    assert_eq!(class_sym.name, "RAGPipeline");
    assert_eq!(class_sym.kind, SymbolKind::Class);
    assert!(class_sym.qualified_name.ends_with("::RAGPipeline"));
    assert_eq!(class_sym.range.start_line, 4);
    assert_eq!(class_sym.range.end_line, 16);
    let class_body = class_sym.body_range.as_ref().unwrap();
    assert_eq!(class_body.start_line, 5);
    assert_eq!(class_body.end_line, 16);

    let init_sym = &symbols[1];
    assert_eq!(init_sym.name, "__init__");
    assert_eq!(init_sym.kind, SymbolKind::Method);
    assert!(init_sym.qualified_name.ends_with("::RAGPipeline::__init__"));
    assert_eq!(init_sym.range.start_line, 5);
    assert_eq!(init_sym.range.end_line, 7);
    let init_body = init_sym.body_range.as_ref().unwrap();
    assert_eq!(init_body.start_line, 6);
    assert_eq!(init_body.end_line, 7);

    let retrieve_sym = &symbols[2];
    assert_eq!(retrieve_sym.name, "retrieve");
    assert_eq!(retrieve_sym.kind, SymbolKind::Method);
    assert!(retrieve_sym.qualified_name.ends_with("::RAGPipeline::retrieve"));
    assert_eq!(retrieve_sym.range.start_line, 9);
    assert_eq!(retrieve_sym.range.end_line, 10);

    let run_sym = &symbols[3];
    assert_eq!(run_sym.name, "run");
    assert_eq!(run_sym.kind, SymbolKind::Method);
    assert!(run_sym.qualified_name.ends_with("::RAGPipeline::run"));

    let main_sym = &symbols[4];
    assert_eq!(main_sym.name, "main");
    assert_eq!(main_sym.kind, SymbolKind::Function);
    assert!(main_sym.qualified_name.ends_with("::main"));

    let imports: Vec<_> = occurrences
        .iter()
        .filter(|o| o.kind == OccurrenceKind::Import)
        .collect();
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].raw_text, "import os");
    assert_eq!(imports[1].raw_text, "from mlx_lm import load, generate");

    let calls: Vec<_> = occurrences
        .iter()
        .filter(|o| o.kind == OccurrenceKind::Call)
        .collect();

    let call_texts: Vec<&str> = calls.iter().map(|c| c.raw_text.as_str()).collect();
    assert!(call_texts.contains(&"load"));
    assert!(call_texts.contains(&"self.retrieve"));
    assert!(call_texts.contains(&"generate"));
    assert!(call_texts.contains(&"RAGPipeline"));
    assert!(call_texts.contains(&"pipeline.run"));
    assert!(call_texts.contains(&"print"));

    let load_call = calls.iter().find(|c| c.raw_text == "load").unwrap();
    assert_eq!(load_call.enclosing_temp_index, Some(1));

    let retrieve_call = calls.iter().find(|c| c.raw_text == "self.retrieve").unwrap();
    assert_eq!(retrieve_call.enclosing_temp_index, Some(3));
}

#[test]
fn test_parse_python_trim_body_range() {
    let content = r#"class TestClass:
    def method(self):
        print("hello")
        # indented comment inside method body
    # unindented comment at class level
# unindented comment at module level
"#;

    let mut temp = NamedTempFile::new().unwrap();
    write!(temp, "{}", content).unwrap();

    let (symbols, _) = parse_python_file(temp.path()).unwrap();

    assert_eq!(symbols.len(), 2);

    let class_sym = &symbols[0];
    assert_eq!(class_sym.name, "TestClass");
    let class_body = class_sym.body_range.as_ref().unwrap();
    assert_eq!(class_body.start_line, 2);
    assert_eq!(class_body.end_line, 5);

    let method_sym = &symbols[1];
    assert_eq!(method_sym.name, "method");
    let method_body = method_sym.body_range.as_ref().unwrap();
    assert_eq!(method_body.start_line, 3);
    assert_eq!(method_body.end_line, 4);
}