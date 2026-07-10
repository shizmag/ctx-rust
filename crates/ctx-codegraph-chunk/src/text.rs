use std::path::Path;

pub fn extract_lines_from_file(
    path: &Path,
    start_line: usize,
    end_line: usize,
    context_lines: usize,
) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Ok(String::new());
    }

    let start = start_line.saturating_sub(context_lines).max(1);
    let end = (end_line + context_lines).min(lines.len());
    if start > lines.len() || start > end {
        return Ok(String::new());
    }

    let mut out = String::new();
    for line in &lines[(start - 1)..end] {
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

pub fn truncate_large_body(
    lines: &[&str],
    range_start: usize,
    range_end: usize,
    body_start: usize,
    body_end: usize,
    context_lines: usize,
) -> String {
    let limit = 80;
    let body_len = body_end.saturating_sub(body_start) + 1;
    if body_len <= limit {
        let start = range_start.saturating_sub(context_lines).max(1);
        let end = (range_end + context_lines).min(lines.len());
        if start > lines.len() || start > end {
            return String::new();
        }
        let mut snippet = String::new();
        for line in &lines[(start - 1)..end] {
            snippet.push_str(line);
            snippet.push('\n');
        }
        return snippet;
    }

    let top_limit = 15;
    let bottom_limit = 15;
    let top_end = (body_start + top_limit).min(lines.len());
    let bottom_start = body_end.saturating_sub(bottom_limit).max(body_start);

    let mut snippet = String::new();
    let start = range_start.saturating_sub(context_lines).max(1);
    for line in &lines[(start - 1)..top_end] {
        snippet.push_str(line);
        snippet.push('\n');
    }

    let omitted = bottom_start.saturating_sub(top_end);
    if omitted > 0 {
        snippet.push_str(&format!("// ... {} lines omitted ...\n", omitted));
    }

    let start_bot = bottom_start.max(top_end + 1);
    let end_bot = (range_end + context_lines).min(lines.len());
    if start_bot <= end_bot && start_bot <= lines.len() {
        for line in &lines[(start_bot - 1)..end_bot] {
            snippet.push_str(line);
            snippet.push('\n');
        }
    }

    snippet
}