# promptjar

[![crates.io](https://img.shields.io/crates/v/promptjar.svg)](https://crates.io/crates/promptjar)
[![CI tests](https://github.com/nanxstats/promptjar/actions/workflows/ci.yml/badge.svg)](https://github.com/nanxstats/promptjar/actions/workflows/ci.yml)

promptjar treats a Git repo of Markdown prompt archives as a queryable database.
It follows the Markdown database pattern: a directory is a table (project),
a file is a row (thread), YAML frontmatter holds the columns, and `---`
thematic breaks separate individual prompts (records) within a thread.

In the suckless spirit it does one thing and composes with Unix tools: no
config file, no daemon, no index, no cache, no lock-in. Git is the write
layer, your editor is the UI, and `pj` is the read/query layer: every
invocation is a stateless scan of the tree, which in Rust takes milliseconds
at the scale of a personal archive.

## Installation

Install with Homebrew:

```console
brew install nanxstats/tap/promptjar
```

Or with Cargo:

```console
cargo install promptjar
```

Installing the `promptjar` crate gives you the `pj` command
(the ripgrep -> `rg` convention).

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

`pj` resolves the archive root from `PROMPTJAR_ROOT`, or walks up from the
current directory to the nearest `.git`. Then:

```console
$ pj list
2026-08-08	okr	okr/lockfile.md	Claude Fable 5 Extra, GPT-5.6 Sol Pro	2	12
2026-08-11	okr	okr/prompts.md	Claude Fable 5 Extra	3	20

$ pj show okr/lockfile.md:2
Compare digest strategies for vendored trees.

$ pj stats --by month
2026-08	2

$ pj lint
README.md:1: warning: no YAML frontmatter; not a thread

$ PROMPTJAR_MODEL='Claude Fable 5 Extra' pj new bisectrunk blog
bisectrunk/blog.md
```

Default output is TSV with a stable column order and no decoration; add
`--json` for JSON Lines. `pj list` prints one thread per row (`date`,
`project`, `file`, `models`, `n_records`, `words`); `--records` switches to
one prompt per row. `pj export` dumps every record with its full metadata
as JSON Lines. See `pj --help` and [SPEC.md](SPEC.md) for the whole
surface.

## Recipes

Everything is a filter or a stream, so the usual tools apply directly.

Count prompts per model:

```sh
pj list --records --json | jq -r '.models[]' | sort | uniq -c | sort -rn
```

Projects touched this year:

```sh
pj list --since 2026-01-01 | cut -f2 | sort -u
```

promptjar has no full-text search on purpose; ripgrep already does it.
Lint only the threads that mention a topic:

```sh
rg -l 'changed a result' | xargs pj lint
```

Words archived per month, as a table:

```sh
pj list --json | jq -r '[.date[:7], .words] | @tsv' \
  | awk -F'\t' '{w[$1]+=$2} END {for (m in w) print m "\t" w[m]}' | sort
```

Back up the archive into one JSONL file (or feed it to anything that eats
JSON):

```sh
pj export > archive.jsonl
```

Gate commits on a clean archive in CI:

```sh
pj lint --strict
```

## License

MIT
