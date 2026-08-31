# Changelog

## promptjar 0.1.0

### New features

- The initial `promptjar` CLI `ptj` with six commands over a Git repo of
  Markdown prompt archives:
  - `list` (threads or records as TSV or JSON Lines, with project/model/date
    filters)
  - `show` (a thread body or one record by `project/file.md:N` address)
  - `stats` (thread or record counts by model, project, month, or year)
  - `lint` (line-anchored diagnostics with a `--strict` mode for CI)
  - `new` (the only writing command; creates a thread with fresh frontmatter)
  - `export` (full structured JSON Lines dump).
- CommonMark-correct record splitting via `pulldown-cmark`: only top-level
  dash style thematic breaks separate prompts, so `---` inside code fences,
  block quotes, and list items, and setext heading underlines, stay intact.
- Stateless scanning with `.gitignore` and hidden directory awareness; root
  discovery via `PROMPTJAR_ROOT` or the nearest `.git` ancestor.
