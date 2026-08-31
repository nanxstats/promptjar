//! Root discovery and the stateless tree scan. Every invocation walks the
//! tree; there is no index and no cache.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::thread::{Parsed, Thread, parse_file};
use crate::{Error, output};

/// Resolve the archive root: `PROMPTJAR_ROOT`, else the nearest ancestor of
/// the current directory containing a `.git` entry, else the current
/// directory.
pub fn discover_root() -> Result<PathBuf, Error> {
    if let Some(v) = std::env::var_os("PROMPTJAR_ROOT") {
        let p = PathBuf::from(v);
        if p.is_dir() {
            return Ok(p);
        }
        return Err(Error::RootMissing(p));
    }
    let cwd = std::env::current_dir().map_err(Error::CurrentDir)?;
    for ancestor in cwd.ancestors() {
        // `.git` may be a file in worktrees; `exists` covers both.
        if ancestor.join(".git").exists() {
            return Ok(ancestor.to_path_buf());
        }
    }
    Ok(cwd)
}

/// All `.md` files under `root`, in sorted order. Hidden files and
/// directories are skipped and `.gitignore` rules are respected, also
/// outside Git repositories. Everything else is skipped silently.
pub fn walk_md(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut builder = WalkBuilder::new(root);
    builder
        .require_git(false)
        .sort_by_file_path(std::cmp::Ord::cmp);
    for entry in builder.build() {
        match entry {
            Ok(e) => {
                if e.file_type().is_some_and(|t| t.is_file())
                    && e.path().extension().is_some_and(|x| x == "md")
                {
                    out.push(e.into_path());
                }
            }
            Err(err) => eprintln!("warning: {err}"),
        }
    }
    out
}

/// Read a file as UTF-8. Unreadable or non-UTF-8 files are skipped with a
/// warning on stderr.
pub fn read_utf8(path: &Path) -> Option<String> {
    let display = output::display_path(path);
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("warning: skipping {display}: {err}");
            return None;
        }
    };
    match String::from_utf8(bytes) {
        Ok(s) => Some(s),
        Err(_) => {
            eprintln!("warning: skipping non-UTF-8 file: {display}");
            None
        }
    }
}

/// One scanned `.md` file with its parse result.
pub struct ScannedFile {
    pub path: PathBuf,
    /// Path relative to the root, `/`-separated.
    pub rel: String,
    pub parsed: Parsed,
}

/// The root-relative, `/`-separated form of a path.
pub fn rel_string(root: &Path, path: &Path) -> String {
    let p = path.strip_prefix(root).unwrap_or(path);
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Walk and parse every `.md` file under `root`.
pub fn scan_files(root: &Path) -> Vec<ScannedFile> {
    walk_md(root)
        .into_iter()
        .filter_map(|path| {
            let content = read_utf8(&path)?;
            let rel = rel_string(root, &path);
            let parsed = parse_file(&rel, &content);
            Some(ScannedFile { path, rel, parsed })
        })
        .collect()
}

/// All valid threads under `root`, sorted by (date, path). Files with
/// invalid frontmatter are skipped with a warning on stderr so data is never
/// silently missing; files without frontmatter (README and friends) are
/// silently ignored here and reported by `lint`.
pub fn scan_threads(root: &Path) -> Vec<Thread> {
    let mut threads = Vec::new();
    for file in scan_files(root) {
        match file.parsed.thread {
            Some(t) => threads.push(t),
            None if file.parsed.has_errors() => {
                eprintln!(
                    "warning: skipping {}: invalid thread (run `ptj lint` for details)",
                    file.rel
                );
            }
            None => {}
        }
    }
    threads.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });
    threads
}
