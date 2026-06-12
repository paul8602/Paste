use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use serde::{Deserialize, Serialize};

pub fn score_clip(query: &str, preview: &str) -> Option<i64> {
    let query = query.trim();
    if query.is_empty() {
        return Some(0);
    }

    let matcher = SkimMatcherV2::default().smart_case();
    matcher.fuzzy_match(preview, query)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilters {
    pub tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub types: Vec<String>,
    pub exclude_types: Vec<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub pinned: Option<bool>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub free_text: String,
    pub has_tag: Option<bool>,
}

/// Parse a search query string into structured filters and a free-text component.
///
/// Supported syntax:
///   tag:name        - filter by tag
///   -tag:name       - exclude tag
///   tag:*           - has at least one tag
///   type:text       - filter by kind
///   -type:image     - exclude kind
///   date:today      - relative date (today/week/month/year)
///   date:2026-06-01 - absolute date
///   date:2026-06-01..2026-06-15 - date range
///   date:<2026-06-01 - before date
///   date:>2026-06-01 - after date
///   pinned:true     - filter by pinned status
///   size:>100KB     - filter by size
///   size:<1MB       - filter by size
///
/// Everything else is treated as free-text for fuzzy matching.
pub fn parse_search_query(query: &str) -> SearchFilters {
    let mut filters = SearchFilters::default();
    let mut text_parts: Vec<String> = Vec::new();

    for token in tokenize(query) {
        if let Some((key, value)) = token.split_once(':') {
            let key_lower = key.to_lowercase();
            match key_lower.as_str() {
                "tag" => {
                    if value == "*" {
                        filters.has_tag = Some(true);
                    } else {
                        filters.tags.push(value.to_string());
                    }
                }
                "-tag" => {
                    filters.exclude_tags.push(value.to_string());
                }
                "type" => {
                    filters.types.push(value.to_lowercase());
                }
                "-type" => {
                    filters.exclude_types.push(value.to_lowercase());
                }
                "date" => {
                    parse_date_filter(value, &mut filters);
                }
                "pinned" => {
                    filters.pinned = match value.to_lowercase().as_str() {
                        "true" | "yes" | "1" => Some(true),
                        "false" | "no" | "0" => Some(false),
                        _ => None,
                    };
                }
                "size" => {
                    parse_size_filter(value, &mut filters);
                }
                _ => {
                    text_parts.push(token);
                }
            }
        } else {
            text_parts.push(token);
        }
    }

    filters.free_text = text_parts.join(" ");
    filters
}

fn tokenize(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in query.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == ' ' && !in_quotes {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn parse_date_filter(value: &str, filters: &mut SearchFilters) {
    let now = time::OffsetDateTime::now_utc();
    let fmt = time::format_description::well_known::Rfc3339;

    match value.to_lowercase().as_str() {
        "today" => {
            let start = now
                .replace_time(time::Time::MIDNIGHT)
                .format(&fmt)
                .unwrap_or_default();
            filters.date_from = Some(start);
        }
        "week" => {
            let start = now
                .saturating_sub(time::Duration::days(7))
                .format(&fmt)
                .unwrap_or_default();
            filters.date_from = Some(start);
        }
        "month" => {
            let start = now
                .saturating_sub(time::Duration::days(30))
                .format(&fmt)
                .unwrap_or_default();
            filters.date_from = Some(start);
        }
        "year" => {
            let start = now
                .saturating_sub(time::Duration::days(365))
                .format(&fmt)
                .unwrap_or_default();
            filters.date_from = Some(start);
        }
        _ => {
            if let Some(rest) = value.strip_prefix('<') {
                filters.date_to = Some(format!("{}T23:59:59Z", rest));
            } else if let Some(rest) = value.strip_prefix('>') {
                filters.date_from = Some(format!("{}T00:00:00Z", rest));
            } else if let Some((from, to)) = value.split_once("..") {
                filters.date_from = Some(format!("{}T00:00:00Z", from));
                filters.date_to = Some(format!("{}T23:59:59Z", to));
            } else {
                filters.date_from = Some(format!("{}T00:00:00Z", value));
                filters.date_to = Some(format!("{}T23:59:59Z", value));
            }
        }
    }
}

fn parse_size_filter(value: &str, filters: &mut SearchFilters) {
    let (op, num_str) = if let Some(rest) = value.strip_prefix('>') {
        (">", rest)
    } else if let Some(rest) = value.strip_prefix('<') {
        ("<", rest)
    } else {
        ("=", value)
    };

    let bytes = parse_size_bytes(num_str);
    let Some(bytes) = bytes else { return };

    match op {
        ">" => filters.min_size = Some(bytes),
        "<" => filters.max_size = Some(bytes),
        _ => {
            filters.min_size = Some(bytes);
            filters.max_size = Some(bytes);
        }
    }
}

fn parse_size_bytes(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();
    let (num_str, multiplier) = if let Some(rest) = s.strip_suffix("GB") {
        (rest, 1024 * 1024 * 1024)
    } else if let Some(rest) = s.strip_suffix("MB") {
        (rest, 1024 * 1024)
    } else if let Some(rest) = s.strip_suffix("KB") {
        (rest, 1024)
    } else if let Some(rest) = s.strip_suffix('B') {
        (rest, 1)
    } else {
        (s.as_str(), 1)
    };
    let num: u64 = num_str.trim().parse().ok()?;
    Some(num * multiplier)
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

    #[test]
    fn parse_tag_filter() {
        let f = parse_search_query("tag:work hello");
        assert_eq!(f.tags, vec!["work"]);
        assert_eq!(f.free_text, "hello");
    }

    #[test]
    fn parse_exclude_tag() {
        let f = parse_search_query("-tag:spam important");
        assert_eq!(f.exclude_tags, vec!["spam"]);
        assert_eq!(f.free_text, "important");
    }

    #[test]
    fn parse_type_filter() {
        let f = parse_search_query("type:image");
        assert_eq!(f.types, vec!["image"]);
        assert!(f.free_text.is_empty());
    }

    #[test]
    fn parse_date_today() {
        let f = parse_search_query("date:today");
        assert!(f.date_from.is_some());
    }

    #[test]
    fn parse_date_range() {
        let f = parse_search_query("date:2026-01-01..2026-06-01");
        assert!(f.date_from.is_some());
        assert!(f.date_to.is_some());
    }

    #[test]
    fn parse_pinned_filter() {
        let f = parse_search_query("pinned:true");
        assert_eq!(f.pinned, Some(true));
    }

    #[test]
    fn parse_size_filter() {
        let f = parse_search_query("size:>1MB");
        assert_eq!(f.min_size, Some(1024 * 1024));
    }

    #[test]
    fn parse_has_tag() {
        let f = parse_search_query("tag:*");
        assert_eq!(f.has_tag, Some(true));
    }

    #[test]
    fn parse_combined_filters() {
        let f = parse_search_query("tag:work type:text date:week pinned:true hello");
        assert_eq!(f.tags, vec!["work"]);
        assert_eq!(f.types, vec!["text"]);
        assert!(f.date_from.is_some());
        assert_eq!(f.pinned, Some(true));
        assert_eq!(f.free_text, "hello");
    }
}
