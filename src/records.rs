//! Record splitting: find top-level dash-style thematic breaks with a real
//! Markdown parser, never with line matching (SPEC.md section 6.2).

use std::ops::Range;

use pulldown_cmark::{Event, Parser};

/// A separator's source must be dashes only; `***` and `___` thematic
/// breaks are content (SPEC.md Questions 2).
fn is_dash_rule(source: &str) -> bool {
    let t = source.trim();
    !t.is_empty() && t.chars().all(|c| matches!(c, '-' | ' ' | '\t'))
}

/// Split a thread body into record chunks (byte spans into `body`).
///
/// A boundary is an `Event::Rule` at container depth zero, so `---` inside
/// fenced code blocks, block quotes, and list items never splits, and setext
/// heading underlines never produce a `Rule` at all. An empty body yields no
/// records; separators always yield one chunk more than their count, even
/// when chunks are empty (empty records keep their index).
pub fn split(body: &str) -> Vec<Range<usize>> {
    let mut cuts: Vec<Range<usize>> = Vec::new();
    let mut depth = 0usize;
    for (event, range) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Rule if depth == 0 && is_dash_rule(&body[range.clone()]) => cuts.push(range),
            _ => {}
        }
    }

    if cuts.is_empty() && body.trim().is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::with_capacity(cuts.len() + 1);
    let mut pos = 0;
    for cut in cuts {
        chunks.push(pos..cut.start);
        pos = cut.end;
    }
    chunks.push(pos..body.len());
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(body: &str) -> Vec<&str> {
        split(body).into_iter().map(|r| body[r].trim()).collect()
    }

    #[test]
    fn empty_body_has_no_records() {
        assert!(split("").is_empty());
        assert!(split("\n\n").is_empty());
    }

    #[test]
    fn body_without_separators_is_one_record() {
        assert_eq!(texts("just text\n"), ["just text"]);
    }

    #[test]
    fn separators_split_and_keep_empty_chunks() {
        assert_eq!(texts("a\n\n---\n\nb\n"), ["a", "b"]);
        assert_eq!(texts("a\n\n---\n\n---\n\nb\n"), ["a", "", "b"]);
        // A body that is only a separator: two empty records.
        assert_eq!(texts("---\n"), ["", ""]);
    }

    #[test]
    fn longer_dash_runs_and_spaced_dashes_split() {
        assert_eq!(texts("a\n\n-----\n\nb\n").len(), 2);
        assert_eq!(texts("a\n\n- - -\n\nb\n").len(), 2);
    }

    #[test]
    fn non_dash_rules_do_not_split() {
        assert_eq!(texts("a\n\n***\n\nb\n").len(), 1);
        assert_eq!(texts("a\n\n___\n\nb\n").len(), 1);
    }
}
