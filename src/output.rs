//! Output helpers. TSV is the default surface: stable column order, one row
//! per item, no decoration, and never any ANSI escape sequences.

use std::path::Path;

/// TSV fields must not contain field or row separators; embedded tabs,
/// newlines, and carriage returns are replaced with spaces. Record text is
/// only ever emitted as JSON, so in practice this touches nothing.
pub fn sanitize_tsv(s: &str) -> String {
    if s.contains(['\t', '\n', '\r']) {
        s.replace(['\t', '\n', '\r'], " ")
    } else {
        s.to_string()
    }
}

/// Editor-friendly path form: relative to the current directory when
/// possible, otherwise as given.
pub fn display_path(path: &Path) -> String {
    let p = if path.is_absolute() {
        std::env::current_dir()
            .ok()
            .and_then(|cwd| path.strip_prefix(&cwd).ok())
            .unwrap_or(path)
    } else {
        path
    };
    p.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_fields_lose_separators() {
        assert_eq!(sanitize_tsv("a\tb\nc"), "a b c");
        assert_eq!(sanitize_tsv("plain"), "plain");
    }
}
