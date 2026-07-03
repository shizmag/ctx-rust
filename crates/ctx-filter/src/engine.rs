use ctx_models::{Mode, ScanOptions, Visibility, HiddenReason};

use crate::FilterEntry;
use crate::rule::FilterRule;
use crate::rules;

pub struct FilterContext<'a> {
    pub options: &'a ScanOptions,
}

pub struct FilterEngine {
    pub rules: Vec<Box<dyn FilterRule>>,
}

impl FilterEngine {
    pub fn new(rules: Vec<Box<dyn FilterRule>>) -> Self {
        Self { rules }
    }

    pub fn default_smart() -> Self {
        Self {
            rules: rules::default_rules(),
        }
    }

    pub fn check(&self, entry: &FilterEntry, context: &FilterContext<'_>) -> Visibility {
        if context.options.mode == Mode::All {
            return Visibility::Visible;
        }

        for rule in &self.rules {
            match rule.check(entry, context) {
                crate::RuleDecision::Pass => {}
                crate::RuleDecision::Hide(reason) => {
                    return Visibility::Hidden(reason);
                }
            }
        }

        if entry.is_file() {
            match context.options.mode {
                Mode::Code => {
                    if !is_code_or_config_or_readme(entry) {
                        return Visibility::Hidden(HiddenReason::NonCode);
                    }
                }
                Mode::Docs => {
                    if !is_docs_or_text(entry) {
                        return Visibility::Hidden(HiddenReason::NonDocs);
                    }
                }
                Mode::Llm => {
                    if is_llm_ignored(entry) {
                        return Visibility::Hidden(HiddenReason::Binary);
                    }
                }
                _ => {}
            }
        }

        Visibility::Visible
    }
}

fn is_code_or_config_or_readme(entry: &FilterEntry) -> bool {
    let name_lower = entry.name.to_lowercase();
    
    // Check specific file names
    if name_lower == "readme.md" 
        || name_lower == "readme.txt" 
        || name_lower == "readme"
        || name_lower == "license" 
        || name_lower == "license.md" 
        || name_lower == "license.txt"
        || name_lower == "contributing.md" 
        || name_lower == "changelog.md"
        || name_lower == "architecture.md"
        || name_lower == "security.md"
        || name_lower == "makefile"
        || name_lower == "cmakelists.txt"
        || name_lower == "dockerfile"
        || name_lower == "docker-compose.yml"
        || name_lower == "docker-compose.yaml"
        || name_lower == "package.json"
        || name_lower == "cargo.toml"
        || name_lower == "tsconfig.json"
        || name_lower == "jsconfig.json"
        || name_lower == ".gitignore"
        || name_lower == ".gitattributes"
        || name_lower == ".env"
        || name_lower == ".env.example"
        || name_lower == ".env.local"
        || name_lower == ".dockerignore"
    {
        return true;
    }

    // Check extensions
    if let Some(ext) = entry.extension() {
        let ext_lower = ext.to_lowercase();
        match ext_lower.as_str() {
            // Programming languages
            "rs" | "go" | "py" | "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" | "jsx" | "tsx"
            | "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" | "hh" | "cs" | "java" | "kt" | "kts"
            | "scala" | "rb" | "php" | "swift" | "sh" | "bash" | "zsh" | "fish" | "bat" | "cmd" | "ps1"
            | "html" | "htm" | "css" | "scss" | "sass" | "less" | "pl" | "pm" | "hs" | "lua" | "r"
            | "dart" | "zig" | "ex" | "exs" | "erl" | "clj" | "fs" | "asm" | "s" | "ml" | "mli"
            // Configs / DB
            | "toml" | "yaml" | "yml" | "json" | "json5" | "jsonc" | "ini" | "conf" | "config"
            | "xml" | "properties" | "sql" | "graphql" | "gql" | "proto" | "mk" | "cmake" => {
                return true;
            }
            _ => {}
        }
    }

    false
}

fn is_docs_or_text(entry: &FilterEntry) -> bool {
    let name_lower = entry.name.to_lowercase();
    
    // Check specific file names
    if name_lower == "readme" 
        || name_lower == "license" 
        || name_lower == "changelog"
        || name_lower == "contributing"
        || name_lower == "readme.md" 
        || name_lower == "readme.txt" 
        || name_lower == "license.md" 
        || name_lower == "license.txt"
        || name_lower == "contributing.md" 
        || name_lower == "changelog.md"
        || name_lower == "architecture.md"
        || name_lower == "security.md"
    {
        return true;
    }

    // Check extensions
    if let Some(ext) = entry.extension() {
        let ext_lower = ext.to_lowercase();
        match ext_lower.as_str() {
            "md" | "txt" | "rst" | "adoc" | "asciidoc" | "org" | "tex" | "pdf" | "epub"
            | "docx" | "doc" | "odt" | "html" | "htm" | "rtf" | "xml" | "json" | "csv" | "tsv" => {
                return true;
            }
            _ => {}
        }
    }

    false
}

fn is_llm_ignored(entry: &FilterEntry) -> bool {
    if let Some(ext) = entry.extension() {
        let ext_lower = ext.to_lowercase();
        match ext_lower.as_str() {
            // Images
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "tiff" | "webp"
            // Archives
            | "zip" | "tar" | "gz" | "bz2" | "xz" | "rar" | "7z"
            // Executables & Binaries
            | "exe" | "dll" | "so" | "dylib" | "bin" | "elf" | "msi" | "deb" | "rpm"
            // Compiled / Bytecode
            | "class" | "pyc" | "pyo" | "o" | "obj" | "pdb" | "wasm"
            // Media
            | "mp3" | "wav" | "mp4" | "avi" | "mkv" | "mov" | "flac" => true,
            _ => false,
        }
    } else {
        false
    }
}
