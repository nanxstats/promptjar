# promptjar design specification

`promptjar` (binary: `ptj`) treats a Git repository of Markdown prompt
archives as a queryable database. The repository is the database, directories
are tables, files are rows of threads, and YAML frontmatter supplies the
columns. `ptj` is the read/query layer; Git is the write layer and the text
editor is the UI.

## 1. Philosophy

- **Markdown database pattern.** Directory = project, file = thread,
  frontmatter = metadata, `---` thematic break = record separator.
  The archive stays fully usable without `ptj`.
- **Suckless constraints.** Do one thing. No config file, no daemon, no cache,
  no index, no lock-in. Every invocation scans the tree; at hundreds to low
  thousands of files a full scan in Rust is milliseconds. Correctness and
  statelessness beat speed.
- **Composability.** Default output is TSV with a stable column order, one row
  per item, and no decoration, so `awk`, `sort`, `cut`, and `xsv`/`qsv` work
  directly. `--json` switches to JSON Lines. `ptj` never emits ANSI escape
  sequences anywhere; there is nothing to strip in a pipe.
- **Read-mostly.** The only command that writes is `new`, and it only creates
  files. No command ever edits an existing file in v1.

## 2. Data model

### 2.1 Thread

A *thread* is one Markdown file that begins with valid YAML frontmatter.
Fields:

| Field | Type | Rule |
|---|---|---|
| `date` | ISO 8601 `YYYY-MM-DD` | Required. Plain or quoted YAML scalar. Zero-padded, calendar-valid. |
| `model` | string or list of strings | Required, non-empty. Normalized internally to `Vec<String>`. |
| anything else | arbitrary YAML | Preserved as-is and carried through to `export`. |

A Markdown file without frontmatter is not a thread. It is still scanned by
`lint` (which warns about it) but is invisible to `list`, `stats`, `show`,
and `export`.

### 2.2 Record

A *record* is one prompt block: a chunk of the file body delimited by
top-level dash-style thematic breaks (`---`). Records are 1-based and
addressed as `project/file.md:N`. Record text is the chunk with surrounding
whitespace trimmed; a chunk that trims to nothing is an *empty record*. Empty
records keep their index (so addresses are stable) and appear in `list
--records` with zero words; `lint` warns about them.

A thread whose body is empty (frontmatter-only file) has zero records.

### 2.3 Project

A *project* is the first path component of a thread's path relative to the
repository root. Files that live directly in the root belong to the
pseudo-project `.` (see Questions).

### 2.4 Derived values

- `words`: whitespace-delimited token count of a record's text. For a thread,
  the sum over its records (separator lines are never counted).
- `n_records`: the number of records in a thread, including empty ones.
- `models`: in TSV output, the `model` list joined with `, `.

## 3. Repository root

The root is resolved once per invocation:

1. `PROMPTJAR_ROOT`, if set, is used verbatim (error if it does not exist).
2. Otherwise, walk up from the current directory to the nearest ancestor
   containing a `.git` entry (directory or file, so worktrees count).
3. Otherwise, the current directory.

There is no config file and no `--root` flag. The only environment variables
are `PROMPTJAR_ROOT` (root override) and `PROMPTJAR_MODEL` (default model for
`new`).

## 4. Scanning

- The tree walk uses the `ignore` crate: `.gitignore` rules are respected
  (also outside Git repositories), hidden files and directories are skipped.
- Only files with the `.md` extension are considered; everything else is
  skipped silently (stray `.R`, `.pdf`, `.txt`, `.rst` files are attachments,
  not records).
- Non-UTF-8 `.md` files are skipped with a warning on stderr.
- `.md` files with invalid frontmatter (bad YAML, bad `date`, missing
  `model`, ...) are skipped by read commands with a warning on stderr; `lint`
  reports them precisely. Data is never silently missing from a listing.
- Output ordering is deterministic: threads sort by (`date`, path), records
  additionally by index. `stats` rows sort by key (byte order).

## 5. CLI reference

```
ptj list [--records] [--project P] [--model M] [--since DATE] [--until DATE] [--json]
ptj show PROJECT/FILE.md[:N]
ptj stats [--by model|project|month|year] [--records] [--json]
ptj lint [--strict] [PATHS...]
ptj new PROJECT [NAME] [--model M] [--date DATE]
ptj export [--json]
```

### 5.1 `ptj list`

One row per thread. TSV columns: `date`, `project`, `file`, `models`
(comma-joined), `n_records`, `words`.

```console
$ ptj list
2026-08-08	okr	okr/lockfile.md	Claude Fable 5 Extra, GPT-5.6 Sol Pro	2	310
2026-08-11	okr	okr/prompts.md	Claude Fable 5 Extra	3	1289
```

With `--records`, one row per record. TSV columns: `date`, `project`, `file`,
`record` (1-based index), `models`, `words`.

Filters (combined with AND):

- `--project P`: exact match on the project name.
- `--model M`: case-insensitive substring match against any model of the
  thread (model names carry version suffixes; `--model 'fable'` matches
  `Claude Fable 5 Extra`).
- `--since DATE` / `--until DATE`: inclusive ISO `YYYY-MM-DD` bounds on the
  thread `date`.

`--json` emits JSON Lines, one object per row:

```json
{"date":"2026-08-11","project":"okr","file":"okr/prompts.md","models":["Claude Fable 5 Extra"],"n_records":3,"words":1289}
{"date":"2026-08-11","project":"okr","file":"okr/prompts.md","record":2,"address":"okr/prompts.md:2","models":["Claude Fable 5 Extra"],"words":210}
```

### 5.2 `ptj show`

`ptj show okr/prompts.md` prints the whole file body (frontmatter stripped,
separators kept). `ptj show okr/prompts.md:2` prints only record 2. The path
is relative to the repository root. Errors: file missing, file is not a
thread, index out of range. Output always ends with exactly one newline.

### 5.3 `ptj stats`

Counts grouped by one dimension: `--by model` (default), `project`, `month`
(`YYYY-MM` from `date`), or `year`. TSV columns: `key`, `count`.

The counted unit is threads by default; `--records` switches it to records,
mirroring `list --records`. When a thread lists multiple models, it counts
**once per model** (its records likewise); totals across models can therefore
exceed the number of threads.

```console
$ ptj stats
Claude Fable 5 Extra	12
GPT-5.6 Sol Pro	3
$ ptj stats --by month --records
2026-07	21
2026-08	48
```

Rows sort by key in byte order; pipe through `sort -t'	' -k2 -rn` for
count order. `--json` emits `{"key":"...","count":N}` lines.

### 5.4 `ptj lint`

Validates the archive (or only `PATHS...`, which may be files or
directories; non-Markdown files passed explicitly are ignored, so `rg -l PAT
| xargs ptj lint` is safe). Diagnostics are `path:line: severity: message`,
one per line on stdout, suitable for editors and CI. No summary line.

Errors (exit 1):

| Condition | Message anchor |
|---|---|
| Unparseable YAML frontmatter | line reported by the YAML parser |
| Unclosed frontmatter (no closing `---`) | line 1 |
| Frontmatter is not a YAML mapping | line 1 |
| Missing or invalid `date` (not a `YYYY-MM-DD` string) | the `date:` line, else line 1 |
| Missing or empty `model` (or non-string entries) | the `model:` line, else line 1 |
| Frontmatter not at byte 0 (e.g. leading blank line) | the displaced `---` line |

Warnings (exit 0 unless `--strict`):

| Condition | Notes |
|---|---|
| Markdown file with no frontmatter at all | expected for `README.md` and attachment copies |
| Empty record (separators with only whitespace between them) | anchored at the empty chunk |
| Unknown frontmatter key | reported **only** under `--strict`; extra keys are legal data |

Exit codes: `0` clean (or warnings without `--strict`), `1` any error, or any
warning under `--strict`, `2` usage error.

### 5.5 `ptj new`

`ptj new PROJECT [NAME]` creates `ROOT/PROJECT/NAME.md` (default `NAME` is
`prompts`; a missing `.md` extension is appended) with frontmatter:

```yaml
---
date: 2026-08-30
model: Claude Fable 5 Extra
---
```

- `--date DATE` overrides today's date (validated `YYYY-MM-DD`).
- `--model M` may be repeated; two or more models are written as a flow
  sequence `model: [A, B]`. Without `--model`, `$PROMPTJAR_MODEL` is used;
  if neither is set, `new` fails.
- `PROJECT` and `NAME` must be plain names (no path separators). The project
  directory is created if missing.
- Refuses to overwrite an existing file. Prints the created path (relative to
  the current directory when possible) on stdout.

### 5.6 `ptj export`

Full structured dump: JSON Lines, one record per line, always (the `--json`
flag is accepted for symmetry and changes nothing). Each line:

```json
{"project":"okr","file":"okr/lockfile.md","record":1,"address":"okr/lockfile.md:1","date":"2026-08-08","models":["Claude Fable 5 Extra","GPT-5.6 Sol Pro"],"extra":{"tags":["lockfile"]},"text":"..."}
```

`extra` holds all frontmatter keys other than `date` and `model`, converted
to JSON with keys in sorted order (see Questions).

## 6. Parsing rules

### 6.1 Frontmatter

- Frontmatter is a `---` line at byte 0 of the file, YAML until the next
  `---` line. Delimiter lines tolerate trailing whitespace and CRLF. A UTF-8
  BOM is stripped before the byte-0 check (see Questions).
- A file whose first non-blank line is `---` with a later closing `---`, but
  with anything before it, is *displaced frontmatter*: a lint error, and not
  a thread.
- `date` accepts a plain or quoted YAML scalar and must validate as
  `YYYY-MM-DD` (zero-padded, calendar-checked). `model` accepts one string
  scalar or a flow/block sequence of strings.
- YAML parsing uses `saphyr` (pure Rust, maintained; the deprecated
  `serde_yaml` and the unsound, unmaintained `serde_yml` (RUSTSEC-2025-0068)
  are deliberately avoided). The schema is tiny, so the mapping from parsed
  YAML to the thread model is hand-rolled over `saphyr::Yaml`, which also
  lets extra keys pass through losslessly.

### 6.2 Record splitting

The body (everything after the closing delimiter) is parsed with
`pulldown-cmark` (CommonMark, no extensions). A record boundary is an
`Event::Rule` that is (a) at the top level, i.e. not nested inside any
container such as a block quote or list item, and (b) *dash-style*: its
source text consists only of `-`, spaces, and tabs. Consequences, matching
CommonMark semantics:

- `---` inside fenced code blocks never splits (frontmatter examples in
  fences are common in the archive).
- A setext heading underline (`Some text` directly followed by `---`) is a
  heading, not a separator. A separator therefore needs a blank line before
  it.
- `---` inside block quotes or list items never splits.
- `***` and `___` thematic breaks are content, not separators (see
  Questions).

CRLF line endings and a missing trailing newline are tolerated everywhere.

## 7. Errors and exit codes

- `0`: success.
- `1`: runtime failure (I/O, bad address, `new` refusing to overwrite) or
  lint findings as defined in §5.4.
- `2`: CLI usage error (from clap).

Warnings and skip notices go to stderr; data and diagnostics go to stdout.

## 8. Dependencies

Each dependency, with its one-line justification:

| Crate | Why |
|---|---|
| `clap` (derive) | The standard CLI surface: subcommands, flags, help, exit code 2 on usage errors. |
| `pulldown-cmark` | Spec-compliant CommonMark event stream with source offsets; the only correct way to find top-level thematic breaks. |
| `saphyr` | Maintained pure-Rust YAML parser (yaml-rust2 lineage); replaces deprecated `serde_yaml`/unsound `serde_yml`. |
| `ignore` | `.gitignore`-aware, hidden-skipping tree walk, shared with ripgrep. |
| `jiff` | Strict civil-date parsing and today's date for `new`; no time-zone database gymnastics in our code. |
| `serde` (derive) | Serialization contract for the JSON output rows. |
| `serde_json` | JSON Lines encoder; serde itself ships no codec. |
| `anyhow` | Error context at the CLI boundary. |
| `thiserror` | Typed library errors that the CLI maps to messages and exit codes. |

Dev-dependencies: `assert_cmd` + `predicates` (binary integration tests),
`tempfile` (scratch archives for `new` and root discovery tests).

## 9. Non-goals (v1)

- No full-text search; the README documents `rg` recipes instead.
- No TUI, no server, no daemon.
- No embeddings, no API capture/import, no sync.
- No autofix in `lint`.
- No SQLite; room is left for a later `export --sqlite` behind a cargo
  feature.
- No index or cache of any kind; no config file.

## 10. Questions

Ambiguous edges, with the resolution implemented. Flag any of these to
change them.

1. **`stats --records`.** "Counts of threads and records" is read as: the
   flag switches the counted unit from threads (default) to records, exactly
   like `list --records` switches row granularity. The alternative (always
   printing both counts in two columns) would leave the locked `--records`
   flag meaningless.
2. **`***` / `___` thematic breaks.** CommonMark treats them as identical to
   `---`, but the archive convention says "`---` separates prompts", so only
   dash-style breaks split records; asterisk/underscore breaks remain
   content.
3. **Root-level Markdown files.** A thread directly in the root has no
   directory; it is assigned the pseudo-project `.` and addressed as
   `file.md:N`. In practice only `README.md` (not a thread) lives there.
4. **UTF-8 BOM.** Editors add BOMs invisibly, so a BOM is stripped before
   the frontmatter-at-byte-0 check instead of failing the file.
5. **Empty records keep their index.** `show file.md:N` must be stable
   whether or not a chunk is empty, so empty records are counted, listed,
   and warned about, not skipped.
6. **`extra` key order.** `serde_json`'s default map sorts keys; order of
   extra frontmatter keys is not preserved in `export` output (values are).
   Preserving order would pull in `indexmap` via a feature flag; not worth it
   at this scale.
7. **Frontmatter-only files.** Valid threads with zero records; not warned
   about. An explicitly empty archive entry is presumed intentional.
