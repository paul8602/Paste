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
