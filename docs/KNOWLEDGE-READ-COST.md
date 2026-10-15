# What a Knowledge read costs

A Knowledge read should cost what its question costs. Before this change every
`aikit knowledge …` invocation walked its whole project tree, rebuilt and
rewrote the SemanticWiki SQLite projection and attached every source pool and
code index, whatever it was asked. O:I's Technē door asks seventeen relation
views at once; most of them exceeded its twenty-second limit.

Three changes make the answer cheap without changing it.

## The discovery walk is kept while nothing it observed changed

Discovery (`discover_material` in `crates/aikit-cli/src/app/knowledge.rs`)
lists the project tree under its admission policy and reads the JSON
candidates it finds. A walk now records each filesystem observation that
decided its outcome and keeps that record, with the material and disclosures
it produced, under `<AIKIT_HOME>/cache/knowledge/discovery/` (the ledger,
`knowledge/ledger.rs`). The next walk of the same horizon repeats only those
observations; when every one answers exactly as recorded it reuses the kept
outcome, and otherwise it walks again in full.

- Listed directories and the files whose bytes were read are observed in
  full: device, inode, mode, size and both nanosecond timestamps. Adding,
  removing or renaming an entry, a `.no-agent-retrieval` marker included,
  changes the directory that holds it; any write to a file changes its ctime,
  which no process can set.
- Every directory the walk tried to admit has its own marker observed, so a
  refused or never-listed directory is still seen.
- An alias records the link itself, its followed kind (or the error a
  dangling one gave) and the identity and marker of every directory on its
  canonical chain.
- Ancestors the policy or a canonicalisation walks through without listing
  are observed for exactly what those consult: their identity and kind, and
  their own marker. A sibling written beside the project does not discard it.
- A walk is never kept when one of its observed timestamps falls within two
  seconds before the walk began: a write in the same clock tick could read
  back unchanged.
- The key carries the executable's own identity, so a rebuilt or upgraded
  AIKit never reuses an older discovery; an executable that cannot name
  itself keeps nothing. There is one ledger per horizon (root, home, World
  root), so a newer build replaces the ledger an older one kept.

The walk itself checks each directory entry once: the checkpoint that binds an
entry's observation to the unchanged directory is also the one the next
listing follows.

A change anywhere in the observed set discards the whole kept walk; the next
walk is complete. The admission law in
[`ARCHITECTURE-NAVIGATION.md`](ARCHITECTURE-NAVIGATION.md#current-admission-and-origin-questions)
still holds: no stat record stands in for a current refusal, because every
input a refusal could come from is one of the observations, and any change to
it makes the next read walk.

## A relation view materialises what it reads

`aikit knowledge relations` for a Wiki or ProjectMap address reads the
SemanticWiki and the ProjectMap and nothing else, so it no longer attaches
Central's file map, the NOW-field, Work repos, bkmr or GitNexus. It still
composes the whole SemanticWiki the other operations see — Central's root and
Project registers, compiled entities, capability matrices, authored
ProjectCentral links, folder subjects, the Explore seed and the edges compiled
from discovered source material — because a view at depth two reaches the
root and sibling spaces, and the answer must not change. Source and Code
relation views keep the whole runtime.

Central's world reading, this Project's World binding and the discovery walk
run beside each other; their results are applied in the order they always
were.

The SQLite projection (`crates/aikit-store/src/knowledge_wiki.rs`) records a
digest of its stored rows. A rebuild whose objects encode to the same digest
leaves the file untouched; otherwise only changed rows are written and only
vanished rows deleted.

## `aikit knowledge relations-batch`

One process answers many relation views. The request is JSON on stdin:

```json
{"schema": "aikit.knowledge-relations-batch/v1",
 "items": [
   {"cwd": "/Users/me/Central/Work/O-I",
    "address": "{\"kind\":\"wiki\",\"value\":\"central:wiki:project:O-I\"}",
    "depth": 2, "max_nodes": 96, "max_edges": 192}
 ]}
```

Each item is one `aikit --json -C <cwd> knowledge relations --depth … -- <address>`
invocation: `address` is the same ADDRESS text, or the typed address object
itself; `cwd` defaults to the batch's `-C`; the limits default as the single
form's do (2, 256, 512). At most 256 items and 1 MiB of input.

The answer is one success envelope whose `data` is
`{"schema": "aikit.knowledge-relations-batch/v1", "answers": [...]}`. Each
answer, in request order, is exactly the envelope the item's single invocation
prints — its own `context`, `data` and `warnings`, or its own failure
envelope. One item's refusal never sinks the others. Only a malformed batch is
refused whole, with `knowledge.relations_batch_input`. The items share one
reading of each Central root's world, taken when the first of them needs it,
and run on up to four threads.

## Central's World binding

A Project context's World binding asks Central for the Project's effective
sources. Central's `central.world.effective-sources` answers a Project with no
authored World record — the convention-inherited case `central.world.here`
reports as `world_record.state: "absent"` — with `ok: false`, status
`invalid_input`, exit 2 and `central.world_declaration_absent`. AIKit reads
that typed absence as root lineage and asks again for `control:root`, so every
Project-context read spends one refused Central call. The binding itself is
unchanged here.
