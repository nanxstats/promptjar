//! Core library for `ptj`, the promptjar CLI.
//!
//! A Git repository of Markdown prompt archives is treated as a queryable
//! database: directory = project, file = thread, YAML frontmatter = thread
//! metadata, top-level `---` thematic break = record separator. See SPEC.md
//! in the repository for the full design.

pub mod cli;
pub mod frontmatter;
pub mod output;
pub mod records;
pub mod scan;
pub mod thread;

pub use thread::{Diagnostic, Parsed, Record, Severity, Thread};

/// Typed library errors; the CLI maps these to messages and exit code 1.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("PROMPTJAR_ROOT is not a directory: {0}")]
    RootMissing(std::path::PathBuf),
    #[error("cannot determine current directory: {0}")]
    CurrentDir(std::io::Error),
}
