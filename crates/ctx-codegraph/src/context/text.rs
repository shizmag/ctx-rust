use crate::model::SourceRange;
use std::path::Path;

pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c == ':' || c == '.' || c == '-' || c == '_' || c == '/' || c == '\\' {
            if !current.is_empty() {
                tokens.push(current.to_lowercase());
                current.clear();
            }
        } else if c.is_uppercase() {
            let prev_is_lower = i > 0 && chars[i - 1].is_lowercase();
            let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
            if prev_is_lower || next_is_lower {
                if !current.is_empty() {
                    tokens.push(current.to_lowercase());
                    current.clear();
                }
            }
            current.push(c);
        } else if c.is_whitespace() {
            if !current.is_empty() {
                tokens.push(current.to_lowercase());
                current.clear();
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }

    let lower = text.to_lowercase();
    if !tokens.contains(&lower) {
        tokens.push(lower);
    }

    tokens
}

pub fn is_subsequence(sub: &str, full: &str) -> bool {
    let mut sub_chars = sub.chars();
    let mut current_sub = sub_chars.next();
    if current_sub.is_none() {
        return true;
    }
    for c in full.chars() {
        if Some(c) == current_sub {
            current_sub = sub_chars.next();
            if current_sub.is_none() {
                return true;
            }
        }
    }
    false
}

pub fn extract_snippet(
    file_path: &Path,
    range: SourceRange,
    body_range: Option<SourceRange>,
    is_root: bool,
    context_lines: usize,
) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(file_path)?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Ok("".to_string());
    }

    let mut start_line = range.start_line;
    let mut end_line = range.end_line;

    let limit = if is_root { 160 } else { 80 };

    if let Some(br) = body_range {
        let body_len = br.end_line.saturating_sub(br.start_line) + 1;
        if body_len <= limit {
            start_line = br.start_line.saturating_sub(context_lines).max(1);
            end_line = (br.end_line + context_lines).min(lines.len());
        } else {
            let top_limit = 15;
            let bottom_limit = 15;

            let top_end = br.start_line + top_limit;
            let bottom_start = br.end_line.saturating_sub(bottom_limit);

            let mut snippet = String::new();
            let start = range.start_line.saturating_sub(context_lines).max(1);
            let end_top = top_end.min(lines.len());
            for i in (start - 1)..end_top {
                snippet.push_str(lines[i]);
                snippet.push('\n');
            }

            let omitted = bottom_start.saturating_sub(end_top);
            if omitted > 0 {
                snippet.push_str(&format!("// ... {} lines omitted ...\n", omitted));
            }

            let start_bot = bottom_start.max(end_top + 1);
            let end_bot = (br.end_line + context_lines).min(lines.len());
            for i in (start_bot - 1)..end_bot {
                snippet.push_str(lines[i]);
                snippet.push('\n');
            }
            return Ok(snippet);
        }
    } else {
        start_line = start_line.saturating_sub(context_lines).max(1);
        end_line = (end_line + context_lines).min(lines.len());
    }

    if start_line > lines.len() {
        return Ok("".to_string());
    }
    let end = std::cmp::min(end_line, lines.len());
    if start_line > end {
        return Ok("".to_string());
    }

    let mut snippet = String::new();
    for i in (start_line - 1)..end {
        snippet.push_str(lines[i]);
        snippet.push('\n');
    }

    Ok(snippet)
}