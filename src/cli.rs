//! The `ptj` command surface and command implementations. `main.rs` stays
//! thin: it calls [`run`] and exits with the returned code.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::thread::{Severity, parse_file};
use crate::{Thread, frontmatter, output, scan};

#[derive(Parser)]
#[command(
    name = "ptj",
    version,
    about = "Query a Git repo of Markdown prompt archives like a database"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List threads (default) or records as TSV
    List {
        /// One row per record instead of one per thread
        #[arg(long)]
        records: bool,
        /// Only this project (exact directory name)
        #[arg(long, value_name = "P")]
        project: Option<String>,
        /// Only threads whose model matches (case-insensitive substring)
        #[arg(long, value_name = "M")]
        model: Option<String>,
        /// Only threads dated on or after DATE (inclusive)
        #[arg(long, value_name = "DATE", value_parser = cli_date)]
        since: Option<jiff::civil::Date>,
        /// Only threads dated on or before DATE (inclusive)
        #[arg(long, value_name = "DATE", value_parser = cli_date)]
        until: Option<jiff::civil::Date>,
        /// JSON Lines instead of TSV
        #[arg(long)]
        json: bool,
    },
    /// Print a thread body, or one record with `:N`
    Show {
        /// PROJECT/FILE.md[:N], relative to the archive root
        address: String,
    },
    /// Count threads (or records with --records) by one dimension
    Stats {
        /// Grouping dimension
        #[arg(long, value_enum, default_value_t = Dimension::Model)]
        by: Dimension,
        /// Count records instead of threads
        #[arg(long)]
        records: bool,
        /// JSON Lines instead of TSV
        #[arg(long)]
        json: bool,
    },
    /// Validate the archive; diagnostics as `path:line: severity: message`
    Lint {
        /// Exit 1 on warnings too, and report unknown frontmatter keys
        #[arg(long)]
        strict: bool,
        /// Files or directories to lint (default: the whole archive)
        paths: Vec<PathBuf>,
    },
    /// Create PROJECT/NAME.md with fresh frontmatter
    New {
        /// Project directory (created if missing)
        project: String,
        /// File name, `.md` appended if missing [default: prompts]
        name: Option<String>,
        /// Model name; repeat for multiple [env fallback: PROMPTJAR_MODEL]
        #[arg(long, value_name = "M")]
        model: Vec<String>,
        /// Date to record [default: today]
        #[arg(long, value_name = "DATE", value_parser = cli_date)]
        date: Option<jiff::civil::Date>,
    },
    /// Dump every record with full metadata as JSON Lines
    Export {
        /// Accepted for symmetry; export is always JSON Lines
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Dimension {
    Model,
    Project,
    Month,
    Year,
}

fn cli_date(s: &str) -> Result<jiff::civil::Date, String> {
    frontmatter::parse_iso_date(s)
        .ok_or_else(|| format!("invalid date `{s}` (expected YYYY-MM-DD)"))
}

/// Parse the CLI, dispatch, and return the process exit code.
pub fn run() -> i32 {
    match dispatch(Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            // A closed pipe (e.g. `ptj list | head`) is a normal way for a
            // consumer to stop reading, not an error.
            if err
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
            {
                return 0;
            }
            eprintln!("ptj: {err:#}");
            1
        }
    }
}

fn dispatch(cli: Cli) -> Result<i32> {
    match cli.command {
        Command::List {
            records,
            project,
            model,
            since,
            until,
            json,
        } => {
            cmd_list(
                records,
                project.as_deref(),
                model.as_deref(),
                since,
                until,
                json,
            )?;
            Ok(0)
        }
        Command::Show { address } => {
            cmd_show(&address)?;
            Ok(0)
        }
        Command::Stats { by, records, json } => {
            cmd_stats(by, records, json)?;
            Ok(0)
        }
        Command::Lint { strict, paths } => cmd_lint(strict, &paths),
        Command::New {
            project,
            name,
            model,
            date,
        } => {
            cmd_new(&project, name.as_deref(), &model, date)?;
            Ok(0)
        }
        Command::Export { json: _ } => {
            cmd_export()?;
            Ok(0)
        }
    }
}

#[derive(Serialize)]
struct ThreadRow<'a> {
    date: String,
    project: &'a str,
    file: &'a str,
    models: &'a [String],
    n_records: usize,
    words: usize,
}

#[derive(Serialize)]
struct RecordRow<'a> {
    date: String,
    project: &'a str,
    file: &'a str,
    record: usize,
    address: String,
    models: &'a [String],
    words: usize,
}

#[allow(clippy::fn_params_excessive_bools)]
fn cmd_list(
    records: bool,
    project: Option<&str>,
    model: Option<&str>,
    since: Option<jiff::civil::Date>,
    until: Option<jiff::civil::Date>,
    json: bool,
) -> Result<()> {
    let root = scan::discover_root()?;
    let model_query = model.map(str::to_lowercase);
    let selected = scan::scan_threads(&root).into_iter().filter(|t| {
        project.is_none_or(|p| t.project == p)
            && since.is_none_or(|d| t.date >= d)
            && until.is_none_or(|d| t.date <= d)
            && model_query
                .as_ref()
                .is_none_or(|q| t.models.iter().any(|m| m.to_lowercase().contains(q)))
    });

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for t in selected {
        let date = t.date.to_string();
        if records {
            for r in &t.records {
                if json {
                    let row = RecordRow {
                        date: date.clone(),
                        project: &t.project,
                        file: &t.rel_path,
                        record: r.index,
                        address: t.address(r.index),
                        models: &t.models,
                        words: r.words,
                    };
                    writeln!(out, "{}", serde_json::to_string(&row)?)?;
                } else {
                    writeln!(
                        out,
                        "{date}\t{}\t{}\t{}\t{}\t{}",
                        output::sanitize_tsv(&t.project),
                        output::sanitize_tsv(&t.rel_path),
                        r.index,
                        output::sanitize_tsv(&t.models.join(", ")),
                        r.words,
                    )?;
                }
            }
        } else if json {
            let row = ThreadRow {
                date,
                project: &t.project,
                file: &t.rel_path,
                models: &t.models,
                n_records: t.records.len(),
                words: t.words(),
            };
            writeln!(out, "{}", serde_json::to_string(&row)?)?;
        } else {
            writeln!(
                out,
                "{date}\t{}\t{}\t{}\t{}\t{}",
                output::sanitize_tsv(&t.project),
                output::sanitize_tsv(&t.rel_path),
                output::sanitize_tsv(&t.models.join(", ")),
                t.records.len(),
                t.words(),
            )?;
        }
    }
    out.flush()?;
    Ok(())
}

/// Split `path[:N]`; the suffix must be all digits to count as an index.
fn split_address(address: &str) -> (&str, Option<usize>) {
    if let Some((path, n)) = address.rsplit_once(':')
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
        && let Ok(index) = n.parse()
    {
        return (path, Some(index));
    }
    (address, None)
}

fn cmd_show(address: &str) -> Result<()> {
    let (rel, index) = split_address(address);
    let root = scan::discover_root()?;
    let path = root.join(rel);
    let bytes = std::fs::read(&path).with_context(|| format!("no such thread: {rel}"))?;
    let content = String::from_utf8(bytes).with_context(|| format!("not UTF-8: {rel}"))?;
    let parsed = parse_file(rel, &content);
    let Some(thread) = parsed.thread else {
        bail!("not a thread (no valid frontmatter): {rel}");
    };

    let text = match index {
        None => thread.body.trim(),
        Some(n) => match thread.records.iter().find(|r| r.index == n) {
            Some(r) => &r.text,
            None => bail!("no record {n} in {rel} ({} records)", thread.records.len()),
        },
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "{text}")?;
    Ok(())
}

#[derive(Serialize)]
struct StatsRow<'a> {
    key: &'a str,
    count: u64,
}

fn cmd_stats(by: Dimension, records: bool, json: bool) -> Result<()> {
    let root = scan::discover_root()?;
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for t in scan::scan_threads(&root) {
        // The counted unit mirrors `list`: threads, or records with the flag.
        let unit = if records { t.records.len() as u64 } else { 1 };
        match by {
            // A thread with several models counts once per model.
            Dimension::Model => {
                for m in &t.models {
                    *counts.entry(m.clone()).or_default() += unit;
                }
            }
            Dimension::Project => *counts.entry(t.project.clone()).or_default() += unit,
            Dimension::Month => {
                *counts
                    .entry(format!("{:04}-{:02}", t.date.year(), t.date.month()))
                    .or_default() += unit;
            }
            Dimension::Year => {
                *counts.entry(format!("{:04}", t.date.year())).or_default() += unit;
            }
        }
    }

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for (key, count) in &counts {
        if json {
            writeln!(
                out,
                "{}",
                serde_json::to_string(&StatsRow { key, count: *count })?
            )?;
        } else {
            writeln!(out, "{}\t{count}", output::sanitize_tsv(key))?;
        }
    }
    out.flush()?;
    Ok(())
}

fn cmd_lint(strict: bool, paths: &[PathBuf]) -> Result<i32> {
    let files: Vec<PathBuf> = if paths.is_empty() {
        scan::walk_md(&scan::discover_root()?)
    } else {
        let mut files = Vec::new();
        for p in paths {
            if p.is_dir() {
                files.extend(scan::walk_md(p));
            } else if p.extension().is_some_and(|x| x == "md") {
                // Non-Markdown paths passed explicitly are attachments;
                // ignore them so `rg -l PAT | xargs ptj lint` is safe.
                files.push(p.clone());
            }
        }
        files
    };

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let mut any_error = false;
    let mut any_finding = false;
    for path in files {
        let Some(content) = scan::read_utf8(&path) else {
            continue;
        };
        let display = output::display_path(&path);
        let mut parsed = parse_file(&display, &content);
        parsed.diagnostics.sort_by_key(|d| d.line);
        for d in &parsed.diagnostics {
            if d.strict_only && !strict {
                continue;
            }
            any_finding = true;
            any_error |= d.severity == Severity::Error;
            writeln!(out, "{display}:{}: {}: {}", d.line, d.severity, d.message)?;
        }
    }
    out.flush()?;
    Ok(i32::from(any_error || (strict && any_finding)))
}

/// A name usable as a single path component.
fn plain_name(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\'])
}

/// Render a model name as a YAML scalar, quoting only when needed.
fn yaml_scalar(s: &str) -> String {
    let starts_safe = s
        .chars()
        .next()
        .is_some_and(|c| !"-?:,[]{}#&*!|>'\"%@` \t".contains(c));
    let plain_ok = starts_safe
        && !s.ends_with([' ', '\t'])
        && !s.contains(|c: char| ":#,[]{}\"\\\n\t".contains(c));
    if plain_ok {
        s.to_string()
    } else {
        // A JSON string is a valid YAML double-quoted scalar.
        serde_json::to_string(s).expect("string serialization")
    }
}

fn cmd_new(
    project: &str,
    name: Option<&str>,
    models: &[String],
    date: Option<jiff::civil::Date>,
) -> Result<()> {
    if !plain_name(project) {
        bail!("PROJECT must be a plain directory name, got `{project}`");
    }
    let name = name.unwrap_or("prompts");
    if !plain_name(name) {
        bail!("NAME must be a plain file name, got `{name}`");
    }

    let env_model = std::env::var("PROMPTJAR_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty());
    let models: Vec<&str> = if models.is_empty() {
        match &env_model {
            Some(m) => vec![m.trim()],
            None => bail!("no model given: pass --model or set PROMPTJAR_MODEL"),
        }
    } else {
        models.iter().map(|m| m.trim()).collect()
    };
    if models.iter().any(|m| m.is_empty()) {
        bail!("model names must be non-empty");
    }

    let date = date.unwrap_or_else(|| jiff::Zoned::now().date());
    let model_value = match models.as_slice() {
        [one] => yaml_scalar(one),
        many => {
            format!(
                "[{}]",
                many.iter()
                    .map(|m| yaml_scalar(m))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    };

    let root = scan::discover_root()?;
    let file = format!("{}{}", name.trim_end_matches(".md"), ".md");
    let dir = root.join(project);
    let path = dir.join(&file);
    if path.exists() {
        bail!(
            "refusing to overwrite existing {}",
            output::display_path(&path)
        );
    }
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("cannot create {}", output::display_path(&dir)))?;
    std::fs::write(
        &path,
        format!("---\ndate: {date}\nmodel: {model_value}\n---\n\n"),
    )
    .with_context(|| format!("cannot write {}", output::display_path(&path)))?;
    println!("{}", output::display_path(&path));
    Ok(())
}

#[derive(Serialize)]
struct ExportRow<'a> {
    project: &'a str,
    file: &'a str,
    record: usize,
    address: String,
    date: String,
    models: &'a [String],
    extra: &'a serde_json::Map<String, serde_json::Value>,
    text: &'a str,
}

fn cmd_export() -> Result<()> {
    let root = scan::discover_root()?;
    let threads: Vec<Thread> = scan::scan_threads(&root);
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for t in &threads {
        let date = t.date.to_string();
        for r in &t.records {
            let row = ExportRow {
                project: &t.project,
                file: &t.rel_path,
                record: r.index,
                address: t.address(r.index),
                date: date.clone(),
                models: &t.models,
                extra: &t.extra,
                text: &r.text,
            };
            writeln!(out, "{}", serde_json::to_string(&row)?)?;
        }
    }
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_split_on_a_trailing_numeric_suffix() {
        assert_eq!(
            split_address("okr/prompts.md:2"),
            ("okr/prompts.md", Some(2))
        );
        assert_eq!(split_address("okr/prompts.md"), ("okr/prompts.md", None));
        assert_eq!(split_address("odd:name.md"), ("odd:name.md", None));
    }

    #[test]
    fn yaml_scalars_quote_only_when_needed() {
        assert_eq!(yaml_scalar("Claude Fable 5 Extra"), "Claude Fable 5 Extra");
        assert_eq!(yaml_scalar("GPT-5.6 Sol Pro"), "GPT-5.6 Sol Pro");
        assert_eq!(yaml_scalar("odd: name"), "\"odd: name\"");
        assert_eq!(yaml_scalar("[weird]"), "\"[weird]\"");
    }

    #[test]
    fn plain_names_reject_path_components() {
        assert!(plain_name("okr"));
        assert!(!plain_name("a/b"));
        assert!(!plain_name(".."));
        assert!(!plain_name(""));
    }
}
