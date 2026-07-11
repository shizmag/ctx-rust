use ctx_codegraph_lang::backend::{ResolveInput, ResolverBackend};
use ctx_codegraph_lang::model::{
    LanguageId, Occurrence, OccurrenceKind, ResolutionConfidence, Symbol, SymbolKind, TextRange,
};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;
use tempfile::tempdir;

use crate::LspDefinitionResolver;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_python_resolver_mock_lsp() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let script_path = bin_dir.join("pyright-langserver");
    let script_content = r#"#!/usr/bin/env python3
import sys
import json
import traceback

log_file = open("/tmp/mock_pyright.log", "w")

def log(msg):
    log_file.write(f"{msg}\n")
    log_file.flush()

def write_response(id, result):
    response = {
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    }
    msg = json.dumps(response)
    log(f"Writing response: {msg}")
    sys.stdout.write(f"Content-Length: {len(msg)}\r\n\r\n{msg}")
    sys.stdout.flush()

log("Script started")
try:
    while True:
        line = sys.stdin.readline()
        if not line:
            log("No more input (EOF)")
            break
        log(f"Read line: {repr(line)}")
        if line.startswith("Content-Length:"):
            length = int(line.split(":")[1].strip())
            empty = sys.stdin.readline()
            log(f"Read empty line: {repr(empty)}")
            content = sys.stdin.read(length)
            log(f"Read content: {content}")
            req = json.loads(content)
            method = req.get("method")
            req_id = req.get("id")
            log(f"Method: {method}, id: {req_id}")
            
            if method == "initialize":
                write_response(req_id, {"capabilities": {"textDocumentSync": 1}})
            elif method == "textDocument/definition":
                uri = req.get("params", {}).get("textDocument", {}).get("uri")
                if uri:
                    write_response(req_id, [
                        {
                            "uri": uri,
                            "range": {
                                "start": {"line": 4, "character": 4},
                                "end": {"line": 4, "character": 8}
                            }
                        }
                    ])
                else:
                    write_response(req_id, [])
except Exception as e:
    log(f"Exception: {e}")
    log(traceback.format_exc())
"#;

    {
        let mut file = File::create(&script_path).unwrap();
        file.write_all(script_content.as_bytes()).unwrap();
    }

    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), old_path);
    unsafe {
        std::env::set_var("PATH", &new_path);
    }

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# Dummy python file\n").unwrap();

    let occurrence = Occurrence {
        id: None,
        file_id: None,
        enclosing_symbol: None,
        enclosing_temp_index: None,
        kind: OccurrenceKind::Call,
        raw_text: "pipeline.run".to_string(),
        file: test_file.clone(),
        range: TextRange {
            start_line: 14,
            start_col: 18,
            end_line: 14,
            end_col: 30,
        },
        language: LanguageId::new("python"),
        backend_id: "python-backend".to_string(),
    };

    let symbols = vec![Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
        id: None,
        file_id: None,
        name: "run".to_string(),
        qualified_name: "RAGPipeline::run".to_string(),
        kind: SymbolKind::Method,
        language: LanguageId::new("python"),
        file: test_file.clone(),
        range: TextRange {
            start_line: 5,
            start_col: 5,
            end_line: 5,
            end_col: 10,
        },
        body_range: None,
    }];

    let resolver = LspDefinitionResolver::python();
    let input = ResolveInput {
        workspace_root,
        occurrence: &occurrence,
        symbols: &symbols,
    };

    let output = resolver.resolve(input).unwrap();

    unsafe {
        std::env::set_var("PATH", old_path);
    }

    assert_eq!(output.resolved_symbol_index, Some(0));
    assert_eq!(output.confidence, ResolutionConfidence::LspExact);
}

#[test]
fn test_python_resolver_fallback_noop() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let old_path = std::env::var("PATH").unwrap_or_default();
    unsafe {
        std::env::set_var("PATH", "");
    }

    let temp_dir = tempdir().unwrap();
    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# Dummy python file\n").unwrap();

    let occurrence = Occurrence {
        id: None,
        file_id: None,
        enclosing_symbol: None,
        enclosing_temp_index: None,
        kind: OccurrenceKind::Call,
        raw_text: "run".to_string(),
        file: test_file.clone(),
        range: TextRange {
            start_line: 14,
            start_col: 18,
            end_line: 14,
            end_col: 21,
        },
        language: LanguageId::new("python"),
        backend_id: "python-backend".to_string(),
    };

    let symbols = vec![Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
        id: None,
        file_id: None,
        name: "run".to_string(),
        qualified_name: "RAGPipeline::run".to_string(),
        kind: SymbolKind::Method,
        language: LanguageId::new("python"),
        file: test_file.clone(),
        range: TextRange {
            start_line: 5,
            start_col: 5,
            end_line: 5,
            end_col: 10,
        },
        body_range: None,
    }];

    let resolver = LspDefinitionResolver::python();
    let input = ResolveInput {
        workspace_root,
        occurrence: &occurrence,
        symbols: &symbols,
    };

    let output = resolver.resolve(input).unwrap();

    unsafe {
        std::env::set_var("PATH", old_path);
    }

    assert_eq!(output.resolved_symbol_index, Some(0));
    assert_eq!(output.confidence, ResolutionConfidence::Syntax);
}