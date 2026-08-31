# promptjar

[![crates.io](https://img.shields.io/crates/v/promptjar.svg)](https://crates.io/crates/promptjar)
[![CI tests](https://github.com/nanxstats/promptjar/actions/workflows/ci.yml/badge.svg)](https://github.com/nanxstats/promptjar/actions/workflows/ci.yml)

promptjar treats a Git repo of Markdown prompt archives as a queryable database.
It follows the Markdown database pattern: a directory is a table (project),
a file is a row (thread), YAML frontmatter holds the columns, and `---`
thematic breaks separate individual prompts (records) within a thread.

In the suckless spirit it does one thing and composes with Unix tools: no
config file, no daemon, no index, no cache, no lock-in. Git is the write
layer, your editor is the UI, and `ptj` is the read/query layer: every
invocation is a stateless scan of the tree, which in Rust takes milliseconds
at the scale of a personal archive.

## Installation

Installing the `promptjar` crate gives you the `ptj` command
(the ripgrep -> `rg` convention):

```sh
cargo install promptjar
```

## Quickstart

An archive is any tree of Markdown files with frontmatter:

```text
prompts/
├── okr/
│   ├── prompts.md
│   └── lockfile.md
└── README.md        # no frontmatter: ignored by queries, noted by lint
```

Each thread starts with frontmatter and separates prompts with `---`:

```markdown
---
date: 2026-08-08
model: [Claude Fable 5 Extra, GPT-5.6 Sol Pro]
---

Explain the lock file drift check.

---

Compare digest strategies for vendored trees.
```

`ptj` resolves the archive root from `PROMPTJAR_ROOT`, or walks up from the
current directory to the nearest `.git`. Then:

```console
$ ptj list
2026-08-08	okr	okr/lockfile.md	Claude Fable 5 Extra, GPT-5.6 Sol Pro	2	12
2026-08-11	okr	okr/prompts.md	Claude Fable 5 Extra	3	20

$ ptj show okr/lockfile.md:2
Compare digest strategies for vendored trees.

$ ptj stats --by month
2026-08	2

$ ptj lint
README.md:1: warning: no YAML frontmatter; not a thread

$ PROMPTJAR_MODEL='Claude Fable 5 Extra' ptj new bisectrunk blog
bisectrunk/blog.md
```

Default output is TSV with a stable column order and no decoration; add
`--json` for JSON Lines. `ptj list` prints one thread per row (`date`,
`project`, `file`, `models`, `n_records`, `words`); `--records` switches to
one prompt per row. `ptj export` dumps every record with its full metadata
as JSON Lines. See `ptj --help` and [SPEC.md](SPEC.md) for the whole
surface.

## Recipes

Everything is a filter or a stream, so the usual tools apply directly.

Count prompts per model:

```sh
ptj list --records --json | jq -r '.models[]' | sort | uniq -c | sort -rn
```

Projects touched this year:

```sh
ptj list --since 2026-01-01 | cut -f2 | sort -u
```

promptjar has no full-text search on purpose; ripgrep already does it.
Lint only the threads that mention a topic:

```sh
rg -l 'changed a result' | xargs ptj lint
```

Words archived per month, as a table:

```sh
ptj list --json | jq -r '[.date[:7], .words] | @tsv' \
  | awk -F'\t' '{w[$1]+=$2} END {for (m in w) print m "\t" w[m]}' | sort
```

Back up the archive into one JSONL file (or feed it to anything that eats
JSON):

```sh
ptj export > archive.jsonl
```

Gate commits on a clean archive in CI:

```sh
ptj lint --strict
```

## License

MIT
