use crate::model::{Chunk, ChunkId, ChunkKind};
use crate::text::{extract_lines_from_file, truncate_large_body};
use ctx_codegraph_lang::model::{FileId, Occurrence, Symbol, SymbolId};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub struct ChunkBuilder {
    file_id: FileId,
    file_path: PathBuf,
    context_lines: usize,
    include_text: bool,
    next_id: i64,
}

impl ChunkBuilder {
    pub fn new(file_id: FileId, file_path: impl Into<PathBuf>) -> Self {
        Self {
            file_id,
            file_path: file_path.into(),
            context_lines: 2,
            include_text: false,
            next_id: 0,
        }
    }

    pub fn context_lines(mut self, lines: usize) -> Self {
        self.context_lines = lines;
        self
    }

    pub fn include_text(mut self, yes: bool) -> Self {
        self.include_text = yes;
        self
    }

    pub fn build(
        &mut self,
        symbols: &[Symbol],
        contains_parent: &HashMap<SymbolId, SymbolId>,
        occurrences: &[Occurrence],
    ) -> Result<Vec<Chunk>, std::io::Error> {
        let file_content = if self.include_text {
            Some(std::fs::read_to_string(&self.file_path)?)
        } else {
            None
        };
        let file_lines: Vec<&str> = file_content
            .as_ref()
            .map(|content| content.lines().collect())
            .unwrap_or_default();

        let mut parents_with_children = HashSet::new();
        for parent_id in contains_parent.values() {
            parents_with_children.insert(*parent_id);
        }

        let mut symbol_parent_chunk = HashMap::<SymbolId, ChunkId>::new();
        let mut chunks = Vec::new();

        let mut ordered: Vec<&Symbol> = symbols.iter().collect();
        ordered.sort_by_key(|sym| sym.range.start_line);

        for sym in ordered {
            let Some(symbol_id) = sym.id else {
                continue;
            };

            let parent_chunk_id = contains_parent
                .get(&symbol_id)
                .and_then(|pid| symbol_parent_chunk.get(pid).copied());

            if parents_with_children.contains(&symbol_id) {
                let (start, end) = (sym.range.start_line, sym.range.end_line);
                let text = if self.include_text {
                    Some(truncate_large_body(
                        &file_lines,
                        start,
                        end,
                        start,
                        end,
                        self.context_lines,
                    ))
                } else {
                    None
                };
                let chunk = self.make_chunk(
                    Some(symbol_id),
                    parent_chunk_id,
                    ChunkKind::ParentSummary,
                    start,
                    end,
                    sym.qualified_name.clone(),
                    text,
                );
                symbol_parent_chunk.insert(symbol_id, chunk.id.unwrap());
                chunks.push(chunk);
            }

            let decl_end = sym
                .body_range
                .as_ref()
                .map(|br| br.start_line.saturating_sub(1))
                .filter(|&line| line >= sym.range.start_line)
                .unwrap_or(sym.range.start_line);
            let decl_text = if self.include_text {
                Some(extract_lines_from_file(
                    &self.file_path,
                    sym.range.start_line,
                    decl_end,
                    self.context_lines,
                )?)
            } else {
                None
            };
            chunks.push(self.make_chunk(
                Some(symbol_id),
                parent_chunk_id,
                ChunkKind::SymbolDecl,
                sym.range.start_line,
                decl_end,
                sym.qualified_name.clone(),
                decl_text,
            ));

            let (body_start, body_end) = match &sym.body_range {
                Some(br) => (br.start_line, br.end_line),
                None => (sym.range.start_line, sym.range.end_line),
            };
            let body_text = if self.include_text {
                Some(if sym.body_range.is_some() {
                    truncate_large_body(
                        &file_lines,
                        sym.range.start_line,
                        sym.range.end_line,
                        body_start,
                        body_end,
                        self.context_lines,
                    )
                } else {
                    extract_lines_from_file(
                        &self.file_path,
                        body_start,
                        body_end,
                        self.context_lines,
                    )?
                })
            } else {
                None
            };
            chunks.push(self.make_chunk(
                Some(symbol_id),
                parent_chunk_id,
                ChunkKind::SymbolBody,
                body_start,
                body_end,
                sym.qualified_name.clone(),
                body_text,
            ));
        }

        for occ in occurrences {
            let enclosing = occ.enclosing_symbol;
            let parent_chunk_id = enclosing.and_then(|sid| symbol_parent_chunk.get(&sid).copied());
            let qname = enclosing
                .and_then(|sid| symbols.iter().find(|s| s.id == Some(sid)))
                .map(|s| s.qualified_name.clone())
                .unwrap_or_else(|| occ.raw_text.clone());
            let text = if self.include_text {
                Some(extract_lines_from_file(
                    &self.file_path,
                    occ.range.start_line,
                    occ.range.end_line,
                    self.context_lines,
                )?)
            } else {
                None
            };
            chunks.push(self.make_chunk(
                enclosing,
                parent_chunk_id,
                ChunkKind::Occurrence,
                occ.range.start_line,
                occ.range.end_line,
                qname,
                text,
            ));
        }

        Ok(chunks)
    }

    fn make_chunk(
        &mut self,
        symbol_id: Option<SymbolId>,
        parent_chunk_id: Option<ChunkId>,
        kind: ChunkKind,
        start_line: usize,
        end_line: usize,
        qualified_name: String,
        text: Option<String>,
    ) -> Chunk {
        let text_hash = text
            .as_deref()
            .map(hash_text)
            .unwrap_or_else(|| hash_text(""));
        let token_count = text
            .as_ref()
            .map(|t| estimate_tokens(t))
            .unwrap_or(0);
        let id = ChunkId(self.next_id);
        self.next_id += 1;
        Chunk {
            id: Some(id),
            symbol_id,
            parent_chunk_id,
            file_id: self.file_id,
            kind,
            text_hash,
            token_count,
            start_line,
            end_line,
            qualified_name,
            text,
        }
    }
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / 4
}