## bkmr: this project's knowledge base

One SQLite database, bound to this project by AIKit. Another project's session
sees a different one. Do not set `BKMR_DB_URL` yourself and do not read
`~/.config/bkmr/projects/.current`; both are stale by construction.

Verbs, cheapest first:

- `bkmr-text QUERY` — full-text and tag search. Offline, tens of milliseconds,
  JSON out. Use it first and use it freely.
- `bkmr-text --tags a,b QUERY` — narrowed. Tags wrapped in underscores are
  system tags: `_md_` file-backed, `_snip_` snippet, `_shell_` script,
  `_mem_` agent memory.
- `bkmr-text --all QUERY` — sweep the sibling databases this project declares.
- `bkmr-semantic QUERY` — conceptual recall. Runs an embedding pass per query,
  and on bkmr 6.x that is a network call plus an API key. Reach for it when
  text search found nothing, or when you want neighbours of an idea rather
  than occurrences of a word.

**Results are pointers, not content.** A hit's `url` is usually a `file://`
path into a vault. Read that file to get the text; the database holds the
index, not the document.

Compose it: widen with `bkmr-text`, pipe through `jq -r '.[].url'`, then Read
or grep the two or three files that look right. Narrow with `--tags` before
raising `--limit`.

Never run `bkmr show`. It bumps the access count and the update timestamp, so
it rewrites the corpus you are reading. `bkmr-text` uses `search --json`,
which does not.
