/// Estimates the number of tokens in a string.
/// A standard approximation is ~4 characters per token.
pub fn estimate_tokens(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        (content.chars().count() + 3) / 4
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
}
