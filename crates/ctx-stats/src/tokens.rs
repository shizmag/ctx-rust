/// Estimates the number of tokens in a string.
/// A standard approximation is ~4 characters per token.
pub fn estimate_tokens(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        content.chars().count().div_ceil(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("hello"), 2); // (5 + 3) / 4 = 2
        assert_eq!(estimate_tokens("hello world"), 3); // (11 + 3) / 4 = 3
    }

    #[test]
    fn unicode_characters_count_as_single_chars() {
        assert_eq!(estimate_tokens("你好"), 1); // 2 chars -> (2 + 3) / 4 = 1
        assert_eq!(estimate_tokens("🦀🦀🦀🦀"), 1); // 4 emoji -> (4 + 3) / 4 = 1
    }

    #[test]
    fn multibyte_utf8_uses_char_count_not_byte_length() {
        // "äöü" is 3 chars but 6 bytes in UTF-8
        assert_eq!(estimate_tokens("äöü"), 1); // (3 + 3) / 4 = 1
        assert_eq!(estimate_tokens("äöüx"), 1); // (4 + 3) / 4 = 1
        assert_eq!(estimate_tokens("äöüxy"), 2); // (5 + 3) / 4 = 2
    }

    #[test]
    fn boundary_at_four_chars() {
        assert_eq!(estimate_tokens("abcd"), 1); // (4 + 3) / 4 = 1
        assert_eq!(estimate_tokens("abc"), 1); // (3 + 3) / 4 = 1
        assert_eq!(estimate_tokens("abcde"), 2); // (5 + 3) / 4 = 2
    }
}
