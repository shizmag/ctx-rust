use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub(crate) fn highlight_line(line: &str, ext: &str) -> Line<'static> {
    let ext = ext.to_lowercase();
    let trimmed = line.trim_start();
    if (ext == "rs"
        || ext == "go"
        || ext == "js"
        || ext == "ts"
        || ext == "tsx"
        || ext == "jsx"
        || ext == "c"
        || ext == "cpp")
        && trimmed.starts_with("//")
    {
        return Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Rgb(86, 95, 137)),
        ));
    }
    if (ext == "py"
        || ext == "sh"
        || ext == "bash"
        || ext == "yaml"
        || ext == "yml"
        || ext == "toml")
        && trimmed.starts_with('#')
    {
        return Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Rgb(86, 95, 137)),
        ));
    }

    let keyword_color = Color::Rgb(187, 154, 247);
    let type_color = Color::Rgb(125, 207, 255);
    let string_color = Color::Rgb(158, 206, 106);
    let comment_color = Color::Rgb(86, 95, 137);
    let text_color = Color::Rgb(192, 202, 245);
    let number_color = Color::Rgb(224, 175, 104);

    let mut spans = Vec::new();
    let mut chars = line.chars().peekable();
    let mut word = String::new();

    while let Some(&c) = chars.peek() {
        if c == '/' {
            chars.next();
            if let Some(&c2) = chars.peek() {
                if c2 == '/'
                    && (ext == "rs"
                        || ext == "go"
                        || ext == "js"
                        || ext == "ts"
                        || ext == "tsx"
                        || ext == "jsx"
                        || ext == "c"
                        || ext == "cpp")
                {
                    let mut comment = "/".to_string();
                    for ch in chars.by_ref() {
                        comment.push(ch);
                    }
                    spans.push(Span::styled(comment, Style::default().fg(comment_color)));
                    break;
                } else {
                    spans.push(Span::styled("/", Style::default().fg(text_color)));
                }
            } else {
                spans.push(Span::styled("/", Style::default().fg(text_color)));
            }
        } else if c == '#'
            && (ext == "py"
                || ext == "sh"
                || ext == "bash"
                || ext == "yaml"
                || ext == "yml"
                || ext == "toml")
        {
            let mut comment = String::new();
            for ch in chars.by_ref() {
                comment.push(ch);
            }
            spans.push(Span::styled(comment, Style::default().fg(comment_color)));
            break;
        } else if c == '"' || c == '\'' {
            let quote = c;
            chars.next();
            let mut s = quote.to_string();
            let mut escaped = false;
            for ch in chars.by_ref() {
                s.push(ch);
                if ch == '\\' && !escaped {
                    escaped = true;
                } else {
                    if ch == quote && !escaped {
                        break;
                    }
                    escaped = false;
                }
            }
            spans.push(Span::styled(s, Style::default().fg(string_color)));
        } else if c.is_alphabetic() || c == '_' {
            word.clear();
            while let Some(&ch) = chars.peek() {
                if ch.is_alphanumeric() || ch == '_' {
                    word.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }
            let style = if is_keyword(&word) {
                Style::default()
                    .fg(keyword_color)
                    .add_modifier(Modifier::BOLD)
            } else if is_type(&word) {
                Style::default().fg(type_color)
            } else {
                Style::default().fg(text_color)
            };
            spans.push(Span::styled(word.clone(), style));
        } else if c.is_numeric() {
            let mut num = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_numeric() || ch == '.' || ch == 'x' || ch == 'f' {
                    num.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }
            spans.push(Span::styled(num, Style::default().fg(number_color)));
        } else {
            let mut punct = String::new();
            punct.push(c);
            chars.next();
            spans.push(Span::styled(punct, Style::default().fg(text_color)));
        }
    }

    Line::from(spans)
}

fn is_keyword(w: &str) -> bool {
    matches!(
        w,
        "fn" | "def"
            | "let"
            | "mut"
            | "pub"
            | "use"
            | "import"
            | "from"
            | "struct"
            | "enum"
            | "impl"
            | "if"
            | "else"
            | "match"
            | "for"
            | "in"
            | "while"
            | "return"
            | "class"
            | "const"
            | "var"
            | "function"
            | "package"
            | "type"
            | "as"
            | "break"
            | "continue"
            | "crate"
            | "extern"
            | "false"
            | "true"
            | "loop"
            | "mod"
            | "static"
            | "trait"
            | "where"
            | "async"
            | "await"
            | "dyn"
    )
}

fn is_type(w: &str) -> bool {
    matches!(
        w,
        "i32"
            | "u32"
            | "i64"
            | "u64"
            | "usize"
            | "f64"
            | "String"
            | "str"
            | "Option"
            | "Result"
            | "bool"
            | "Self"
            | "self"
            | "Vec"
            | "Box"
            | "HashMap"
            | "HashSet"
            | "Path"
            | "PathBuf"
            | "std"
            | "io"
            | "fs"
    )
}

pub(crate) fn highlight_search_matches<'a>(
    text: &'a str,
    query: &str,
    base_style: Style,
    highlight_style: Style,
) -> Vec<Span<'a>> {
    if query.is_empty() {
        return vec![Span::styled(text, base_style)];
    }

    let mut spans = Vec::new();
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    let mut last_idx = 0;
    while let Some(start_idx) = text_lower[last_idx..]
        .find(&query_lower)
        .map(|i| last_idx + i)
    {
        if start_idx > last_idx {
            spans.push(Span::styled(&text[last_idx..start_idx], base_style));
        }
        let end_idx = start_idx + query_lower.len();
        spans.push(Span::styled(&text[start_idx..end_idx], highlight_style));
        last_idx = end_idx;
    }

    if last_idx < text.len() {
        spans.push(Span::styled(&text[last_idx..], base_style));
    }

    spans
}

pub(crate) fn highlight_line_matches(line: Line<'static>, query: &str) -> Line<'static> {
    if query.is_empty() {
        return line;
    }

    let query_lower = query.to_lowercase();
    let mut new_spans = Vec::new();

    for span in line.spans {
        let text = span.content.to_string();
        let text_lower = text.to_lowercase();

        if text_lower.contains(&query_lower) {
            let mut last_idx = 0;
            while let Some(start_idx) = text_lower[last_idx..]
                .find(&query_lower)
                .map(|i| last_idx + i)
            {
                if start_idx > last_idx {
                    new_spans.push(Span::styled(
                        text[last_idx..start_idx].to_string(),
                        span.style,
                    ));
                }
                let end_idx = start_idx + query_lower.len();
                let highlight_style = span
                    .style
                    .bg(Color::Rgb(224, 175, 104))
                    .fg(Color::Rgb(36, 40, 59))
                    .add_modifier(Modifier::BOLD);
                new_spans.push(Span::styled(
                    text[start_idx..end_idx].to_string(),
                    highlight_style,
                ));
                last_idx = end_idx;
            }
            if last_idx < text.len() {
                new_spans.push(Span::styled(text[last_idx..].to_string(), span.style));
            }
        } else {
            new_spans.push(span);
        }
    }

    Line::from(new_spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Style};
    use ratatui::text::Line;

    #[test]
    fn test_highlighting() {
        let base_style = Style::default().fg(Color::White);
        let highlight_style = Style::default().fg(Color::Black).bg(Color::Yellow);

        // Test highlight_search_matches
        let spans = highlight_search_matches("hello world", "world", base_style, highlight_style);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "hello ");
        assert_eq!(spans[0].style, base_style);
        assert_eq!(spans[1].content, "world");
        assert_eq!(spans[1].style, highlight_style);

        // Case insensitivity of highlight_search_matches
        let spans = highlight_search_matches("Hello World", "world", base_style, highlight_style);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "Hello ");
        assert_eq!(spans[1].content, "World");

        // Test highlight_line_matches
        let line = Line::from(vec![Span::styled("fn main()", base_style)]);
        let line_hl = highlight_line_matches(line, "main");
        assert_eq!(line_hl.spans.len(), 3);
        assert_eq!(line_hl.spans[0].content, "fn ");
        assert_eq!(line_hl.spans[1].content, "main");
        assert_eq!(line_hl.spans[2].content, "()");
    }
}
