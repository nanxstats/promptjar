//! The data model and the file-level parser: one Markdown file in, one
//! [`Parsed`] out (a thread if the frontmatter is valid, plus diagnostics).

use crate::{frontmatter, records};

/// One Markdown file with valid frontmatter.
#[derive(Debug, Clone)]
pub struct Thread {
    /// Path relative to the archive root, `/`-separated.
    pub rel_path: String,
    /// First path component of `rel_path`, or `.` for root-level files.
    pub project: String,
    pub date: jiff::civil::Date,
    pub models: Vec<String>,
    /// Frontmatter keys other than `date` and `model`, converted to JSON.
    pub extra: serde_json::Map<String, serde_json::Value>,
    /// Raw file body (everything after the closing frontmatter delimiter).
    pub body: String,
    pub records: Vec<Record>,
}

impl Thread {
    /// Total whitespace-delimited words over all records.
    pub fn words(&self) -> usize {
        self.records.iter().map(|r| r.words).sum()
    }

    /// The `project/file.md:N` address of one record.
    pub fn address(&self, index: usize) -> String {
        format!("{}:{index}", self.rel_path)
    }
}

/// One prompt block: a chunk of the body between top-level dash-style
/// thematic breaks. Indices are 1-based and stable even for empty chunks.
#[derive(Debug, Clone)]
pub struct Record {
    pub index: usize,
    /// 1-based line where the record's content starts.
    pub line: usize,
    /// Chunk text with surrounding whitespace trimmed.
    pub text: String,
    pub words: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => f.write_str("error"),
            Severity::Warning => f.write_str("warning"),
        }
    }
}

/// One lint finding, anchored to a 1-based line in the source file.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub line: usize,
    pub severity: Severity,
    /// Reported only under `lint --strict` (unknown frontmatter keys).
    pub strict_only: bool,
    pub message: String,
}

impl Diagnostic {
    pub fn error(line: usize, message: String) -> Self {
        Diagnostic {
            line,
            severity: Severity::Error,
            strict_only: false,
            message,
        }
    }

    pub fn warning(line: usize, message: String) -> Self {
        Diagnostic {
            line,
            severity: Severity::Warning,
            strict_only: false,
            message,
        }
    }

    pub fn strict_warning(line: usize, message: String) -> Self {
        Diagnostic {
            line,
            severity: Severity::Warning,
            strict_only: true,
            message,
        }
    }
}

/// The result of parsing one Markdown file.
#[derive(Debug)]
pub struct Parsed {
    /// `Some` only when the file has valid frontmatter.
    pub thread: Option<Thread>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    fn not_thread(diagnostic: Diagnostic) -> Self {
        Parsed {
            thread: None,
            diagnostics: vec![diagnostic],
        }
    }

    /// True when any diagnostic is an error (the file is not a valid thread
    /// for a reason other than simply having no frontmatter).
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// The project a root-relative path belongs to.
pub fn project_of(rel_path: &str) -> String {
    match rel_path.split_once('/') {
        Some((project, _)) => project.to_string(),
        None => ".".to_string(),
    }
}

/// Parse one Markdown file. `rel_path` is the `/`-separated path relative to
/// the archive root, used for the project name and record addresses.
pub fn parse_file(rel_path: &str, raw: &str) -> Parsed {
    // Editors add UTF-8 BOMs invisibly; tolerate one (SPEC.md Questions 4).
    let content = raw.strip_prefix('\u{feff}').unwrap_or(raw);

    let (yaml, yaml_line, body_offset) = match frontmatter::extract(content) {
        frontmatter::Extract::None => {
            return Parsed::not_thread(Diagnostic::warning(
                1,
                "no YAML frontmatter; not a thread".to_string(),
            ));
        }
        frontmatter::Extract::Displaced { line } => {
            return Parsed::not_thread(Diagnostic::error(
                line,
                "frontmatter must start at byte 0 of the file".to_string(),
            ));
        }
        frontmatter::Extract::Unclosed => {
            return Parsed::not_thread(Diagnostic::error(
                1,
                "unclosed frontmatter: missing closing `---` line".to_string(),
            ));
        }
        frontmatter::Extract::Found {
            yaml,
            yaml_line,
            body_offset,
        } => (yaml, yaml_line, body_offset),
    };

    let fm = match frontmatter::parse_yaml(yaml, yaml_line) {
        Ok(fm) => fm,
        Err(diagnostics) => {
            return Parsed {
                thread: None,
                diagnostics,
            };
        }
    };

    let mut diagnostics: Vec<Diagnostic> = fm
        .extra_lines
        .iter()
        .map(|(key, line)| {
            Diagnostic::strict_warning(*line, format!("unknown frontmatter key `{key}`"))
        })
        .collect();

    let body = &content[body_offset..];
    let mut recs = Vec::new();
    for (i, span) in records::split(body).into_iter().enumerate() {
        let chunk = &body[span.clone()];
        let text = chunk.trim();
        let leading = chunk.len() - chunk.trim_start().len();
        let anchor = body_offset + span.start + if text.is_empty() { 0 } else { leading };
        let line = line_of(content, anchor);
        if text.is_empty() {
            diagnostics.push(Diagnostic::warning(line, format!("empty record {}", i + 1)));
        }
        recs.push(Record {
            index: i + 1,
            line,
            words: text.split_whitespace().count(),
            text: text.to_string(),
        });
    }

    Parsed {
        thread: Some(Thread {
            rel_path: rel_path.to_string(),
            project: project_of(rel_path),
            date: fm.date,
            models: fm.models,
            extra: fm.extra,
            body: body.to_string(),
            records: recs,
        }),
        diagnostics,
    }
}

/// 1-based line number of a byte offset.
fn line_of(content: &str, offset: usize) -> usize {
    1 + content.as_bytes()[..offset]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FM: &str = "---\ndate: 2026-08-11\nmodel: Claude Fable 5 Extra\n---\n";

    fn parse(content: &str) -> Parsed {
        parse_file("proj/file.md", content)
    }

    fn thread(content: &str) -> Thread {
        let parsed = parse(content);
        parsed
            .thread
            .unwrap_or_else(|| panic!("expected a thread: {:?}", parsed.diagnostics))
    }

    fn errors(content: &str) -> Vec<Diagnostic> {
        let parsed = parse(content);
        assert!(parsed.thread.is_none(), "expected no thread");
        assert!(
            parsed.has_errors(),
            "expected errors: {:?}",
            parsed.diagnostics
        );
        parsed.diagnostics
    }

    #[test]
    fn separator_splits_records() {
        let t = thread(&format!("{FM}\nOne.\n\n---\n\nTwo.\n"));
        assert_eq!(t.records.len(), 2);
        assert_eq!(t.records[0].text, "One.");
        assert_eq!(t.records[1].text, "Two.");
        assert_eq!(t.address(2), "proj/file.md:2");
    }

    #[test]
    fn dashes_in_fenced_code_do_not_split() {
        let t = thread(&format!(
            "{FM}\nBefore.\n\n```yaml\n---\ndate: 2026-08-11\n---\n```\n\nAfter.\n"
        ));
        assert_eq!(t.records.len(), 1);
        assert!(t.records[0].text.contains("```yaml"));
    }

    #[test]
    fn setext_heading_underline_does_not_split() {
        let t = thread(&format!(
            "{FM}\nA heading\n---\n\nBody under the heading.\n"
        ));
        assert_eq!(t.records.len(), 1);
    }

    #[test]
    fn dashes_in_block_quote_and_list_do_not_split() {
        let t = thread(&format!(
            "{FM}\n> quoted\n> ---\n\n1. item\n\n   ---\n\n   more\n"
        ));
        assert_eq!(t.records.len(), 1);
    }

    #[test]
    fn asterisk_and_underscore_rules_are_content() {
        let t = thread(&format!("{FM}\nOne.\n\n***\n\n___\n\nStill one.\n"));
        assert_eq!(t.records.len(), 1);
    }

    #[test]
    fn extra_keys_preserved() {
        let t = thread("---\ndate: 2026-08-11\nmodel: M\ntags: [a, b]\nnote: hi\n---\n\nx\n");
        assert_eq!(t.extra["tags"], serde_json::json!(["a", "b"]));
        assert_eq!(t.extra["note"], serde_json::json!("hi"));
    }

    #[test]
    fn unknown_keys_reported_strict_only() {
        let parsed = parse("---\ndate: 2026-08-11\nmodel: M\ntags: [a]\n---\n\nx\n");
        let diag = parsed
            .diagnostics
            .iter()
            .find(|d| d.message.contains("unknown frontmatter key `tags`"))
            .expect("unknown-key diagnostic");
        assert!(diag.strict_only);
        assert_eq!(diag.severity, Severity::Warning);
        assert_eq!(diag.line, 4);
    }

    #[test]
    fn model_scalar_and_sequence_forms() {
        let scalar = thread(FM);
        assert_eq!(scalar.models, ["Claude Fable 5 Extra"]);
        let flow =
            thread("---\ndate: 2026-08-08\nmodel: [Claude Fable 5 Extra, GPT-5.6 Sol Pro]\n---\n");
        assert_eq!(flow.models, ["Claude Fable 5 Extra", "GPT-5.6 Sol Pro"]);
        let block = thread("---\ndate: 2026-08-08\nmodel:\n  - A\n  - B\n---\n");
        assert_eq!(block.models, ["A", "B"]);
    }

    #[test]
    fn quoted_and_unquoted_dates_agree() {
        let plain = thread("---\ndate: 2026-08-11\nmodel: M\n---\n");
        let quoted = thread("---\ndate: \"2026-08-11\"\nmodel: M\n---\n");
        assert_eq!(plain.date, quoted.date);
        assert_eq!(plain.date.to_string(), "2026-08-11");
    }

    #[test]
    fn invalid_dates_error() {
        for bad in [
            "2026-8-1",
            "2026-13-01",
            "2026-02-30",
            "20260811",
            "not a date",
        ] {
            let diags = errors(&format!("---\ndate: {bad}\nmodel: M\n---\n"));
            assert!(
                diags.iter().any(|d| d.message.contains("date")),
                "{bad}: {diags:?}"
            );
        }
    }

    #[test]
    fn missing_or_empty_model_errors() {
        for fm in [
            "---\ndate: 2026-08-11\n---\n",
            "---\ndate: 2026-08-11\nmodel:\n---\n",
            "---\ndate: 2026-08-11\nmodel: []\n---\n",
        ] {
            let diags = errors(fm);
            assert!(
                diags.iter().any(|d| d.message.contains("model")),
                "{fm}: {diags:?}"
            );
        }
    }

    #[test]
    fn empty_record_between_separators_warns_but_counts() {
        let parsed = parse(&format!("{FM}\nOne.\n\n---\n\n---\n\nThree.\n"));
        let t = parsed.thread.expect("thread");
        assert_eq!(t.records.len(), 3);
        assert_eq!(t.records[1].text, "");
        assert_eq!(t.records[1].words, 0);
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|d| { d.severity == Severity::Warning && d.message == "empty record 2" })
        );
    }

    #[test]
    fn frontmatter_only_file_has_zero_records() {
        let parsed = parse(FM);
        let t = parsed.thread.expect("thread");
        assert!(t.records.is_empty());
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn crlf_line_endings_tolerated() {
        let t =
            thread("---\r\ndate: 2026-08-11\r\nmodel: M\r\n---\r\nOne.\r\n\r\n---\r\n\r\nTwo.\r\n");
        assert_eq!(t.date.to_string(), "2026-08-11");
        assert_eq!(t.records.len(), 2);
        assert_eq!(t.records[1].text, "Two.");
    }

    #[test]
    fn missing_trailing_newline_tolerated() {
        let t = thread(&format!("{FM}\nOne.\n\n---\n\nTwo"));
        assert_eq!(t.records.len(), 2);
        assert_eq!(t.records[1].text, "Two");
    }

    #[test]
    fn bom_is_stripped() {
        let t = thread(&format!("\u{feff}{FM}\nx\n"));
        assert_eq!(t.records.len(), 1);
    }

    #[test]
    fn no_frontmatter_is_a_warning() {
        let parsed = parse("# README\n\nJust a file.\n");
        assert!(parsed.thread.is_none());
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].severity, Severity::Warning);
        assert_eq!(parsed.diagnostics[0].line, 1);
    }

    #[test]
    fn leading_blank_line_before_frontmatter_is_an_error() {
        let diags = errors(&format!("\n{FM}"));
        assert!(diags[0].message.contains("byte 0"), "{diags:?}");
        assert_eq!(diags[0].line, 2);
    }

    #[test]
    fn unclosed_frontmatter_is_an_error() {
        let diags = errors("---\ndate: 2026-08-11\nmodel: M\n");
        assert!(diags[0].message.contains("unclosed"), "{diags:?}");
    }

    #[test]
    fn unparseable_yaml_is_an_error() {
        let diags = errors("---\ndate: [\nmodel: M\n---\n");
        assert!(
            diags.iter().any(|d| d.message.contains("unparseable YAML")),
            "{diags:?}"
        );
    }

    #[test]
    fn words_are_counted_per_record_and_summed() {
        let t = thread(&format!("{FM}\nOne two three.\n\n---\n\nFour five.\n"));
        assert_eq!(t.records[0].words, 3);
        assert_eq!(t.records[1].words, 2);
        assert_eq!(t.words(), 5);
    }

    #[test]
    fn projects_derive_from_the_first_path_component() {
        assert_eq!(project_of("okr/prompts.md"), "okr");
        assert_eq!(project_of("okr/sub/deep.md"), "okr");
        assert_eq!(project_of("rootfile.md"), ".");
    }
}
