# AGENTS.md

This file is guidance for coding agents working on `promptjar` itself.
`SPEC.md` is the source of truth for behavior, output formats, lint rules,
and exit codes. Read it in full before changing behavior; if this file
disagrees with it, follow `SPEC.md`. Its "Questions" section records how
ambiguous edges were resolved: extend it instead of silently choosing when
you hit a new one.

## Project boundary

`ptj` is the read/query layer over a Git repo of Markdown prompt archives.
Git is the write layer and the editor is the UI. Keep these invariants:

- `new` is the only command that writes, and it only creates files. Never
  add a command that edits an existing file.
- No index, no cache, no config file, no daemon. Every invocation scans the
  tree. Behavior is controlled by flags plus exactly two environment
  variables: `PROMPTJAR_ROOT` and `PROMPTJAR_MODEL`.
- Default output is undecorated TSV; `--json` is JSON Lines. Never emit
  ANSI escape sequences anywhere, so there is nothing to detect or strip.
- No full-text search, TUI, server, embeddings, or SQLite in v1. `export
  --sqlite` behind a cargo feature is the only anticipated extension.
- Read commands must never silently drop data: files with invalid
  frontmatter are skipped with a warning on stderr, and `lint` is the tool
  that explains them.

## Parsing architecture (the part worth understanding first)

Frontmatter and body are handled by two deliberately different mechanisms:

- `frontmatter::extract` hand-scans delimiter lines. This is not laziness:
  lint must distinguish "no frontmatter" (warning) from "frontmatter not at
  byte 0" (error) and "unclosed frontmatter" (error), and a Markdown parser
  cannot report those. A delimiter is exactly `---` (trailing whitespace and
  CRLF tolerated); `----` is content.
- `records::split` then parses only the body with `pulldown-cmark` and cuts
  at `Event::Rule` events that are (a) at container depth zero, tracked with
  a `Start`/`End` counter over the offset iterator, and (b) dash-style in
  the source text. This is what makes `---` inside code fences, block
  quotes, and list items inert, and setext underlines never produce a
  `Rule` at all. Do not enable `ENABLE_YAML_STYLE_METADATA_BLOCKS` when
  parsing the body: frontmatter is already stripped, and a body that begins
  with a `---` line must parse as a rule (an empty first record), not as
  metadata.
- YAML goes through `saphyr` with a hand-rolled mapping, not serde. The
  schema is two known keys plus arbitrary extras preserved into
  `serde_json::Value`; matching on `saphyr::Yaml` directly keeps that
  lossless and the dependency tree small. `serde_yaml` (deprecated) and
  `serde_yml` (RUSTSEC-2025-0068) must never be introduced; `serde-saphyr`
  was rejected for its 1.89 MSRV and the flatten gymnastics the extras map
  would need.
- Line numbers are 1-based file lines. YAML errors map through
  `ScanError::marker().line()` plus the frontmatter offset; `date:`/
  `model:`/unknown-key diagnostics are anchored by scanning the raw YAML for
  the key's line, falling back to line 1.
- Records keep their 1-based index even when empty so `show file.md:N`
  addresses stay stable; empty records are a lint warning, not a skip.

## Module map

| Module | Responsibility |
|---|---|
| `src/main.rs` | One line: exit with `cli::run()`. |
| `src/lib.rs` | Module declarations and the small typed `Error`. |
| `src/thread.rs` | Data model (`Thread`, `Record`, `Diagnostic`, `Parsed`) and `parse_file`, the one entry point from file text to model. The parser test suite lives here. |
| `src/frontmatter.rs` | Delimiter scanning, strict `YYYY-MM-DD` validation, YAML-to-model mapping, YAML-to-JSON conversion for extras. |
| `src/records.rs` | Body splitting on top-level dash rules. |
| `src/scan.rs` | Root discovery, `ignore`-based walk (`require_git(false)`, sorted), UTF-8 reading, thread collection. |
| `src/output.rs` | TSV field sanitization and cwd-relative path display. |
| `src/cli.rs` | Clap surface and all six command implementations. |

## Determinism and composability

- Ordering is part of the contract: the walk is sorted, threads sort by
  (date, path), stats keys by byte order. Tests assert exact TSV output.
- TSV fields pass through `output::sanitize_tsv`; record text is only ever
  emitted as JSON.
- A broken pipe (`ptj list | head`) exits 0; `cli::run` downcasts the
  `io::Error` rather than letting it print as a failure. Use `writeln!` to
  a locked, buffered stdout in commands, never `println!` in loops.
- Exit codes: 0 success, 1 runtime failure or lint findings, 2 clap usage
  errors. `lint --strict` turns warnings into exit 1 and additionally
  reports unknown frontmatter keys (they are legal data everywhere else).

## Tests and fixtures

- Parser unit tests live in `src/thread.rs` (with a few in `records.rs` and
  `frontmatter.rs`). Every parsing edge case fixed gets a test at this
  layer first; the fixture archive only proves end-to-end wiring.
- `tests/cli.rs` runs the binary via `assert_cmd` against
  `tests/fixtures/archive/`, pinned with `PROMPTJAR_ROOT` and with
  `PROMPTJAR_MODEL` removed so the invoking user's environment cannot leak
  in. Scratch archives for `new`, lint-error, and root-discovery tests use
  `tempfile`, never the fixture tree.
- The fixture word counts and stats in the exact-match assertions are real;
  if you change a fixture file, rerun the binary and update every affected
  expected string.
- Do not add a nested `.gitignore` to the fixture archive: files it hides
  would need `git add -f` and `cargo package` would drop them from the
  published crate. The `ignore` crate's gitignore handling is upstream's
  responsibility; our tests cover hidden-directory and non-Markdown
  skipping.

## Working conventions

Run the release gates before handing off:

```console
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo publish --dry-run
```

Use conventional commits. Update `CHANGELOG.md` for user-visible changes.
Dependency versions were pinned from the vendored sources under `deps-src/`
(regenerate with `okr sync`); every dependency needs a one-line
justification in `SPEC.md` section 8, and the tree stays small on purpose.
