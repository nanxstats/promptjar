//! Frontmatter extraction and the hand-rolled YAML-to-thread mapping.
//!
//! The schema is tiny (`date`, `model`, arbitrary extras), so instead of a
//! serde deserializer this walks `saphyr::Yaml` directly, which keeps the
//! dependency tree small and lets extra keys pass through losslessly.

use saphyr::{LoadableYamlNode, Scalar, Yaml};

use crate::thread::Diagnostic;

/// Result of locating the frontmatter block in a file.
#[derive(Debug)]
pub enum Extract<'a> {
    /// Valid delimiters. `yaml_line` is the 1-based file line of the first
    /// YAML line; `body_offset` is the byte offset just past the closing
    /// delimiter line.
    Found {
        yaml: &'a str,
        yaml_line: usize,
        body_offset: usize,
    },
    /// The file does not begin with frontmatter.
    None,
    /// A frontmatter-shaped block exists but something precedes it.
    Displaced { line: usize },
    /// An opening `---` at byte 0 with no closing `---` line.
    Unclosed,
}

/// A delimiter is a line that is exactly `---`, tolerating trailing
/// whitespace and CRLF. Longer runs of dashes are content, not delimiters.
fn is_delimiter(line: &str) -> bool {
    line.trim_end_matches(['\n', '\r'])
        .trim_end_matches([' ', '\t'])
        == "---"
}

/// Locate the frontmatter block. Frontmatter is a `---` line at byte 0,
/// YAML until the next `---` line (SPEC.md section 6.1).
pub fn extract(content: &str) -> Extract<'_> {
    let mut lines = content.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Extract::None;
    };

    if is_delimiter(first) {
        let yaml_start = first.len();
        let mut pos = yaml_start;
        for line in lines {
            if is_delimiter(line) {
                return Extract::Found {
                    yaml: &content[yaml_start..pos],
                    yaml_line: 2,
                    body_offset: pos + line.len(),
                };
            }
            pos += line.len();
        }
        return Extract::Unclosed;
    }

    // Not at byte 0: if the first non-blank line opens a closed `---` block,
    // report it as displaced frontmatter rather than a missing one.
    let mut rest = content.split_inclusive('\n');
    let mut line_no = 0;
    let mut opener = None;
    for line in rest.by_ref() {
        line_no += 1;
        if line.trim().is_empty() {
            continue;
        }
        if is_delimiter(line) {
            opener = Some(line_no);
        }
        break;
    }
    match opener {
        Some(line) if rest.any(is_delimiter) => Extract::Displaced { line },
        _ => Extract::None,
    }
}

/// Validated frontmatter of one thread.
#[derive(Debug)]
pub struct Frontmatter {
    pub date: jiff::civil::Date,
    pub models: Vec<String>,
    /// Keys other than `date` and `model`, converted to JSON.
    pub extra: serde_json::Map<String, serde_json::Value>,
    /// The extra keys with their 1-based file lines, in document order.
    pub extra_lines: Vec<(String, usize)>,
}

/// Strictly validate a `YYYY-MM-DD` date: zero-padded, calendar-checked.
pub fn parse_iso_date(s: &str) -> Option<jiff::civil::Date> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, &c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return None;
    }
    s.parse().ok()
}

/// Map the raw YAML text to a [`Frontmatter`], collecting every problem as a
/// line-anchored diagnostic. `first_line` is the file line of the first YAML
/// line (always 2 today; kept explicit for the line arithmetic).
pub fn parse_yaml(yaml: &str, first_line: usize) -> Result<Frontmatter, Vec<Diagnostic>> {
    // File line of the opening `---`, the anchor for block-level problems.
    let anchor = first_line - 1;

    let docs = Yaml::load_from_str(yaml).map_err(|e| {
        vec![Diagnostic::error(
            anchor + e.marker().line(),
            format!("unparseable YAML: {}", e.info()),
        )]
    })?;

    let map = match docs.into_iter().next() {
        None | Some(Yaml::Value(Scalar::Null)) => {
            return Err(vec![Diagnostic::error(
                anchor,
                "frontmatter is empty".to_string(),
            )]);
        }
        Some(Yaml::Mapping(map)) => map,
        Some(_) => {
            return Err(vec![Diagnostic::error(
                anchor,
                "frontmatter is not a YAML mapping".to_string(),
            )]);
        }
    };

    let mut diags = Vec::new();
    let mut date = None;
    let mut models = None;
    let mut extra = serde_json::Map::new();
    let mut extra_lines = Vec::new();
    let mut saw_date = false;
    let mut saw_model = false;

    for (k, v) in &map {
        let Some(key) = k.as_str() else {
            diags.push(Diagnostic::error(
                anchor,
                "frontmatter keys must be strings".to_string(),
            ));
            continue;
        };
        let line = key_line(yaml, first_line, key).unwrap_or(anchor);
        match key {
            "date" => {
                saw_date = true;
                match v.as_str().and_then(parse_iso_date) {
                    Some(d) => date = Some(d),
                    None => diags.push(Diagnostic::error(
                        line,
                        format!("invalid `date` (expected YYYY-MM-DD): {}", describe(v)),
                    )),
                }
            }
            "model" => {
                saw_model = true;
                match model_list(v) {
                    Ok(m) => models = Some(m),
                    Err(msg) => diags.push(Diagnostic::error(line, msg)),
                }
            }
            _ => match yaml_to_json(v) {
                Ok(value) => {
                    extra.insert(key.to_string(), value);
                    extra_lines.push((key.to_string(), line));
                }
                Err(msg) => diags.push(Diagnostic::error(
                    line,
                    format!("unsupported YAML in key `{key}`: {msg}"),
                )),
            },
        }
    }

    if !saw_date {
        diags.push(Diagnostic::error(
            anchor,
            "missing `date` in frontmatter".to_string(),
        ));
    }
    if !saw_model {
        diags.push(Diagnostic::error(
            anchor,
            "missing `model` in frontmatter".to_string(),
        ));
    }
    if !diags.is_empty() {
        return Err(diags);
    }
    match (date, models) {
        (Some(date), Some(models)) => Ok(Frontmatter {
            date,
            models,
            extra,
            extra_lines,
        }),
        // Both keys were seen and produced no diagnostics, so both parsed.
        _ => unreachable!("date and model present without diagnostics"),
    }
}

/// `model` accepts one string scalar or a flow/block sequence of strings.
fn model_list(v: &Yaml) -> Result<Vec<String>, String> {
    match v {
        Yaml::Value(Scalar::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                Err("empty `model`".to_string())
            } else {
                Ok(vec![t.to_string()])
            }
        }
        Yaml::Value(Scalar::Null) => Err("empty `model`".to_string()),
        Yaml::Sequence(seq) if seq.is_empty() => Err("empty `model`".to_string()),
        Yaml::Sequence(seq) => seq
            .iter()
            .map(|item| match item.as_str().map(str::trim) {
                Some(t) if !t.is_empty() => Ok(t.to_string()),
                _ => Err("`model` entries must be non-empty strings".to_string()),
            })
            .collect(),
        _ => Err("`model` must be a string or a list of strings".to_string()),
    }
}

/// Best-effort file line of a top-level key, for diagnostics.
fn key_line(yaml: &str, first_line: usize, key: &str) -> Option<usize> {
    yaml.lines()
        .position(|l| {
            l.strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with(':'))
        })
        .map(|i| first_line + i)
}

/// Short value description for error messages.
fn describe(v: &Yaml) -> String {
    match v {
        Yaml::Value(Scalar::String(s)) => format!("`{s}`"),
        Yaml::Value(Scalar::Integer(i)) => format!("`{i}`"),
        Yaml::Value(Scalar::FloatingPoint(f)) => format!("`{f}`"),
        Yaml::Value(Scalar::Boolean(b)) => format!("`{b}`"),
        Yaml::Value(Scalar::Null) => "null".to_string(),
        Yaml::Sequence(_) => "a sequence".to_string(),
        Yaml::Mapping(_) => "a mapping".to_string(),
        _ => "an unsupported value".to_string(),
    }
}

/// Convert an extra frontmatter value to JSON, preserving it as-is.
fn yaml_to_json(v: &Yaml) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    Ok(match v {
        Yaml::Value(Scalar::Null) => Value::Null,
        Yaml::Value(Scalar::Boolean(b)) => Value::Bool(*b),
        Yaml::Value(Scalar::Integer(i)) => Value::from(*i),
        Yaml::Value(Scalar::FloatingPoint(f)) => serde_json::Number::from_f64(**f)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(f.to_string())),
        Yaml::Value(Scalar::String(s)) => Value::String(s.to_string()),
        Yaml::Sequence(seq) => {
            Value::Array(seq.iter().map(yaml_to_json).collect::<Result<_, _>>()?)
        }
        Yaml::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, val) in map {
                let key = k
                    .as_str()
                    .ok_or_else(|| "non-string mapping key".to_string())?;
                obj.insert(key.to_string(), yaml_to_json(val)?);
            }
            Value::Object(obj)
        }
        Yaml::Tagged(_, inner) => yaml_to_json(inner)?,
        _ => return Err("unsupported YAML construct".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delimiter_shapes() {
        assert!(is_delimiter("---"));
        assert!(is_delimiter("---\n"));
        assert!(is_delimiter("---\r\n"));
        assert!(is_delimiter("---  \n"));
        assert!(!is_delimiter("----\n"));
        assert!(!is_delimiter(" ---\n"));
        assert!(!is_delimiter("--- x\n"));
    }

    #[test]
    fn iso_dates_are_strict() {
        assert!(parse_iso_date("2026-08-11").is_some());
        assert!(parse_iso_date("2026-8-11").is_none());
        assert!(parse_iso_date("2026-08-11 ").is_none());
        assert!(parse_iso_date("2026-02-30").is_none());
        assert!(parse_iso_date("20260811").is_none());
    }

    #[test]
    fn extract_finds_yaml_and_body() {
        let Extract::Found {
            yaml,
            yaml_line,
            body_offset,
        } = extract("---\ndate: d\n---\nbody\n")
        else {
            panic!("expected Found");
        };
        assert_eq!(yaml, "date: d\n");
        assert_eq!(yaml_line, 2);
        assert_eq!(body_offset, 16);
    }

    #[test]
    fn extract_classifies_edge_cases() {
        assert!(matches!(extract("# no frontmatter\n"), Extract::None));
        assert!(matches!(extract(""), Extract::None));
        assert!(matches!(extract("---\ndate: d\n"), Extract::Unclosed));
        assert!(matches!(
            extract("\n---\ndate: d\n---\n"),
            Extract::Displaced { line: 2 }
        ));
        // Text before a `---` pair is content, not displaced frontmatter.
        assert!(matches!(extract("text\n\n---\n\n---\n"), Extract::None));
    }
}
