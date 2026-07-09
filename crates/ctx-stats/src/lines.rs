pub fn count_lines(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        content.lines().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_content_has_zero_lines() {
        assert_eq!(count_lines(""), 0);
    }

    #[test]
    fn single_line_without_newline_counts_as_one() {
        assert_eq!(count_lines("hello"), 1);
    }

    #[test]
    fn crlf_line_endings_are_counted() {
        assert_eq!(count_lines("one\r\ntwo\r\nthree"), 3);
    }

    #[test]
    fn trailing_newline_does_not_add_extra_line() {
        assert_eq!(count_lines("one\ntwo\n"), 2);
        assert_eq!(count_lines("only\n"), 1);
    }

    #[test]
    fn unicode_content_is_counted_correctly() {
        assert_eq!(count_lines("你好\n世界"), 2);
        assert_eq!(count_lines("🦀\n🦀\n"), 2);
    }
}
