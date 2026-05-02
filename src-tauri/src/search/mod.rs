use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

pub fn score_clip(query: &str, preview: &str) -> Option<i64> {
    let query = query.trim();
    if query.is_empty() {
        return Some(0);
    }

    let matcher = SkimMatcherV2::default().smart_case();
    matcher.fuzzy_match(preview, query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        assert!(score_clip("", "anything").is_some());
        assert!(score_clip("  ", "anything").is_some());
    }

    #[test]
    fn exact_substring_matches() {
        assert!(score_clip("hello", "say hello world").is_some());
    }

    #[test]
    fn fuzzy_match_respects_order() {
        assert!(score_clip("hw", "hello world").is_some());
        assert!(score_clip("wh", "hello world").is_none());
    }

    #[test]
    fn no_match_returns_none() {
        assert!(score_clip("xyz", "hello world").is_none());
    }

    #[test]
    fn smart_case_matches_insensitively() {
        assert!(score_clip("hello", "HELLO world").is_some());
        assert!(score_clip("hello", "Hello world").is_some());
    }
}
