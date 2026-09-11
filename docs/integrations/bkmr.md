# bkmr through Central

Central #154 corrects the live file-map integration. **Central maintains persistent
root and Project bkmr maps, native file/source identities and managed links. AIKit
uses those public owner operations for knowledge and skill resolution.** It does
not delete/rebuild the owner's database or create a second durable path policy.

The old integration investigation is retained in
[history/bkmr-pre-central.md](history/bkmr-pre-central.md). Its machine observations
and recommendations describe that earlier arrangement, not the current product.
The Central companion contract is `docs/integrations/BKMR-FILE-MAP.md` in
`EpiLogos/Central`; both implementations belong to Central #154.

## Knowledge

Inside a Central World, `aikit knowledge search/read/open` uses
`CentralFileMapProvider` and Central's `central.file-map.inspect/search/resolve` Actions.
The adapter attaches to persistent state and fetches source payloads on demand;
`rebuild` is an owner-only error. The requesting Project uses its map plus specifically linked foreign
sources, narrowed by native World exclusions; root navigation can query participating Project maps with World provenance.

Current World exclusions and `.no-agent-retrieval` treatment are enforced by Central
before data reaches this consumer. Source reads and Flow cognition return to the
owner rather than reusing a cached corpus body. Filesystem-discovered source shards
are a standalone mechanism and do not become a second source authority inside a
Central World. Independent Wiki and code providers retain their own roles.

A missing map operation is an explicit degraded knowledge lens, not a replacement
local map and not a reason to suppress independent Wiki/code results. A source
read without its owner fails. `CENTRAL_CTRL_BIN` (or `OI_CENTRAL_CTRL_BIN`) selects
the installed native Central executable. No database pathname is accepted by this
consumer.

Full-text/tag search works without embeddings. Hybrid availability comes from
Central's prepared-index reading; preparation is explicit native
`central.file-map.refresh` with `embeddings:true`. Semantic-only bkmr CLI output has no
structured contract in this adapter and is not advertised.

## Skill sources

Register the authoritative skill directory in Central and retain its returned
SourceRef. Bind that native directory, not a remembered location:

```sh
aikit source bind-central root-skills '<returned SourceRef>' --root "$CENTRAL_ROOT"
aikit source sync root-skills
aikit source promote root-skills
aikit apply
```

The same binding command can replace a prior directory source while retaining
snapshot history. Central returns active standing and the complete exact skill
bundle, including scripts, references and binary assets. AIKit creates immutable
candidate snapshots, applies its own contextual guidance and materialises native
harness projections. The snapshot retains the owner's tree revision. Sync,
promotion, registry reuse and generation publication recheck it. Retiring or
changing the owner skill invalidates stale projection; rollback does not bypass
current owner standing.

A containing World can move: set `CENTRAL_ROOT` to the new connection root and the
binding still resolves the same SourceRef. Internal directory moves go through
Central's journalled operations. The connection root is a locating hint, not the
skill's identity.

After a generation is committed, AIKit reports its actual generation directory
and only the source capsules selected into that generation to
`central.file-map.projection-record`. Failures at this post-commit join are explicit
reconciliation warnings, not a false claim that a committed generation was rolled
back. Central records `reported-generation`, not an observed loaded harness. A
running agent may still need a fresh session to reload native skill material.

## Standalone compatibility

Outside a Central World, the native baseline and independent providers still work.
A genuinely disposable standalone bkmr cache must now declare `disposable = true`
in its `tool/search/bkmr` configuration. Without that explicit choice, AIKit refuses
a rebuild and directs the operator to native Central adoption. The Rust disposable
adapter also requires its own durable creation marker before replacing any
existing database and refuses Central's reserved `.central/bkmr` storage, including resolved
symlink paths. No migration erases an existing native database automatically.

## Verification

`crates/aikit-adapters/tests/central_file_map.rs` pins the owner envelope, live
payload lookup, missing/withheld-source behaviour and refusal to rebuild owner
storage. The companion Central repository's `tests/bkmr_joined.py` runs actual
source-built `ctrl`, `aikit` and bkmr in temporary Worlds: live knowledge reads,
owner disconnection, companion snapshots, retirement, stale-shard privacy,
source/root relocation and actual generation reporting. Native bkmr proof uses
7.6.7. Personal installation, real embedding inference and loaded-harness acceptance
remain separately observable work, not inferred from these controlled tests.
