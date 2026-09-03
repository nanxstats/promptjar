# Changelog

## promptjar (development version)

### Breaking changes

- Rename the CLI binary from `ptj` to `pj`. Scripts, aliases, and CI steps
  that call `ptj` need to be updated (#5).

## promptjar 0.1.0

### New features

- The initial `promptjar` CLI `ptj` with six commands over a Git repo of
  Markdown prompt archives:
  - `list` - Threads or records as TSV or JSON Lines, with project/model/date filters.
  - `show` - A thread body or one record by `project/file.md:N` address.
  - `stats` - Thread or record counts by model, project, month, or year.
  - `lint` - Line-anchored diagnostics with a `--strict` mode for CI.
  - `new` - Create a thread with fresh frontmatter (the only writing command).
  - `export` - Full structured JSON Lines dump.
- CommonMark-correct record splitting via `pulldown-cmark`: only top-level
  dash style thematic breaks separate prompts, so `---` inside code fences,
  block quotes, and list items, and setext heading underlines, stay intact.
- Stateless scanning with `.gitignore` and hidden directory awareness; root
  discovery via `PROMPTJAR_ROOT` or the nearest `.git` ancestor.
