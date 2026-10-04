---
role: architecture
standing: agent-inference
scope: AIKit native operations and composed O:I consumer boundaries
updated: 2026-10-03
---
# AIKit architecture navigation

## Main documentation publication — 4 October 2026

This publication changes documentation only. Its Main basis is `12ccb272f46a655fd95a7201d9468410a96990c7`; the retained operation walks and test results below belong to their stated Source revisions. They do not qualify a new Main implementation, installed runtime or human acceptance.

At committed Source `9de95bfc3e33410a366823a7f2390757f1cc5619`, knowledge context retains source descriptors and fetches payloads through the live owner ([app/knowledge.rs:2137–2141](https://github.com/EpiLogos/ai-kit/blob/9de95bfc3e33410a366823a7f2390757f1cc5619/crates/aikit-cli/src/app/knowledge.rs#L2137)). This Source changes the knowledge owner route; earlier successful cuts remain dated evidence. The actual [product-ground CLI build](https://github.com/EpiLogos/ai-kit/actions/runs/37238209056/job/111541475952) at that same Source fails with `E0433`: unresolved `aikit_adapters::wiki_publication` references in `wiki.rs:1502`, `1729` and `1999`. The documentation check was not reached. This failure remains open at that cut; it is not an intentional architecture boundary, and old baselines do not verify 9de. The active code repair owns its successor and replay.

This is an implementation-facing navigation companion for O:I #65/#220 and the
existing documentation programme. It recovers native owners and successors;
it does not adopt a new design or claim the whole running experience complete.
Inspected native checkout revision: `377bfe674191df5ab721be54c4ebb78a391a2905`. Active repair source may advance
that cut; identify the file revision before relying on its returned result.

## Governing source and successors

- [ARCHITECTURE](ARCHITECTURE.md) — native implementation and material boundaries; named cuts below.
- [PRAXIS-ARCHITECTURE](PRAXIS-ARCHITECTURE.md)
- [WIKI-OPERATIONAL-PROJECTION](WIKI-OPERATIONAL-PROJECTION.md)
- [encounter-runtime](encounter-runtime.md)
- [README](v2/README.md)

Directory names are routes, not authority. Target design, amendment, historical
baseline, current implementation and observed result keep their own standing.

| Concern | Public operation / entry | Native source | Boundary and lifecycle |
| --- | --- | --- | --- |
| Source/context resolution | `aikit knowledge search/resolve/read/route/explain/history`; context resolution | `crates/aikit-core/src/context_resolution.rs`; `context_source.rs` | Provider authorities stay distinct; a source ref is not automatically delivered body. |
| Praxis | `aikit praxis read` and actor praxis disclosure | `crates/aikit-core/src/praxis.rs`; `agent_praxis.rs` | Skill/Method/Methodology share identity; SkillSet selects/packages without granting authority. |
| Wiki correction / hooks | `aikit wiki projection read/update` | `crates/aikit-cli/src/wiki_projection.rs`; `crates/aikit-adapters/src/hook_sources.rs` | Saved, selected, emitted and observed use are distinct. |
| Interrupted plural Flow | `aikit session-space encounter --request-json <JSON>`; conversation actions in JSON | `crates/aikit-cli/src/encounter_conversation.rs`; `encounter_conversation_tests.rs` | Canonical requests/delivery cursors in encounters.sqlite3; uncertain delivery is not replayed; current membership checked before dispatch and inclusion. |
| Upgrade / resident lifecycle | PID-bound owner shutdown, gateway native service install | `crates/aikit-cli/src/encounter_service.rs`; `gateway_install.rs` | A new installed binary does not replace an already running provider body; canonical session/journal persists separately. |

## Diagram and consumer relation

The maintained suite companion is
`source:project:O-I:docs/architecture/agent-context.md`; its editable diagram is
`source:project:O-I:docs/architecture/agent-context.mmd`. The O:I architecture entry
contains six question-specific companions, full-size rendered SVGs, an indexed
basis for every arrow, exact source hashes and independent navigation evidence.
Resolve that source in the current O:I checkout before substituting a cached or
historical copy. Its solid arrows are inspected relations, not installed
acceptance; proposed joins remain explicitly proposed.

Existing capability records link this companion through the optional
`extensions.documentation` protocol. These links change discoverability, not
capability IDs, coordinate placements or source authority. The moved account's
links now resolve from `ProjectCentral/user/telos/`. The harness admission row
retains its historical Grok-bot basis but names the corrected Grok Build source
and its documentation-level, brokered census; installed admission and verified
ACP connection arguments remain unclaimed.

## Verification and open joins

Read each native test and its actual runner conditions, then the corresponding
dated Return. A test definition is not an executed result; a process/receipt is
not human Recognition. The suite's architecture verification records real
Mermaid rendering, source/link checks and the fresh-agent navigation task.
The four repair lanes continue to own their code, installed replay and open
architectural decisions. Preserve a missing join as missing until that proof
or decision is returned.

## Wiki publication candidate and verification boundary — 2 October 2026

AIKit owns semantic Wiki mutation and SourcePool material callers; the native
physical publisher is shared by those callers, with distinct effect scopes.
At `9536f3f4bd196435c3b3181df26cfc287d35d84d`,
[Wiki commands](https://github.com/EpiLogos/ai-kit/blob/9536f3f4bd196435c3b3181df26cfc287d35d84d/crates/aikit-cli/src/wiki.rs),
the [SourcePool caller](https://github.com/EpiLogos/ai-kit/blob/9536f3f4bd196435c3b3181df26cfc287d35d84d/crates/aikit-adapters/src/projectcentral.rs)
and the [physical publisher](https://github.com/EpiLogos/ai-kit/blob/9536f3f4bd196435c3b3181df26cfc287d35d84d/crates/aikit-adapters/src/wiki_publication.rs)
retain the admitted no-follow source basis and distinguish unchanged, published
and uncertain effects. An unchanged Wiki revalidates its exact same-inode byte
basis; it is not a new publication. Participating-writer locking does not claim
protection against every external mutation. See [ARCHITECTURE](ARCHITECTURE.md)
for the native semantic/storage contract; that document has a separate writer.

The 9536 cut passes
[CAW native delivery](https://github.com/EpiLogos/ai-kit/actions/runs/36968068617)
and [Linux / Mac pre-local packaging](https://github.com/EpiLogos/ai-kit/actions/runs/36968070773),
including actual Task cache/version/redirection and positive/negative own-login
scrub controls. Its [full CI](https://github.com/EpiLogos/ai-kit/actions/runs/36968066200)
fails in Linux maintenance and three Mac Wiki targets. That dated failure is
retained; it does not describe every later cut.

### Later qualification — f05a626b, 2 October 2026

At `f05a626b0cdd9dcf766219ab3b881a214ec51d07`,
[CAW delivery](https://github.com/EpiLogos/ai-kit/actions/runs/36970490606) and
[Linux / Mac packaging](https://github.com/EpiLogos/ai-kit/actions/runs/36970493112)
pass again. Linux CLI reports 1051 passed and 56 skipped. The three previous Mac
cases now pass: `maintenance_replay_with_no_upserts_keeps_the_document_whole`,
`real_refresh_prunes_only_canonical_native_shards_and_preserves_foreign_names`
and `symlink_file_and_symlink_lock_are_refused_without_touching_target`.
The [complete CI](https://github.com/EpiLogos/ai-kit/actions/runs/36970488105)
still fails: `bash scripts/verify 2>&1 | tee /tmp/aikit.log` reports five
`clippy::type_complexity` errors promoted by `-D warnings` at
`crates/aikit-adapters/src/wiki_publication.rs:741,743,745,747,749` in lib-test
callback declarations. This is a named later receipt, not a rewrite of the
earlier 9536 or frozen v6 reading. A later lint-source successor inherits no
executed grade from it.

### Later qualification — b64b8055, 2 October 2026

At `b64b80551bbfa6c3f7e2890bcb6874d70107f363`, the
[full CI](https://github.com/EpiLogos/ai-kit/actions/runs/36971732163) fails in
Mac repository Verify. The actual command
`bash scripts/verify 2>&1 | tee /tmp/aikit.log` reaches
`projectcentral::publication::native::tests::concurrent_exact_basis_writers_have_one_acknowledged_result_then_compose`.
The one-acknowledgement assertion at `wiki_publication.rs:1390` has passed.
The next assertion, starting at `crates/aikit-adapters/src/wiki_publication.rs:1391`,
counts `knowledge.wiki_concurrent_write` errors: it finds 0 where 1 is required.
The other writer's error is not captured in that run. The adapter suite reports
565 passed, 1 failed and 2 ignored; the Linux adapter job passes the named case.

Correction: an earlier Agent receipt misread the failed counter as zero
acknowledgements. That receipt is retained as history and superseded by this
source-checked reading. The one-success/one-stale-basis-refusal requirement is
unchanged. The later diagnostic below supplies the other error at its own cut;
its result is not backdated to b64.

### Diagnostic qualification — ebaaedb, 2 October 2026

The diagnostic-only `ebaaedb77e25ac09b0f036348b98440b51ccbf8a` successor
[fails full CI again](https://github.com/EpiLogos/ai-kit/actions/runs/36974391884)
in the Mac concurrency case. Its actual result records the second writer as
acknowledged, with changed bytes; the first writer returns
`knowledge.wiki_publication_identity`, `NotFound`, OS error 2 while opening
the canonical `.wiki.json.publication.lock`. The ordinary lock exists afterward
and the source contains the acknowledged second writer's bytes. The expected
stale-basis refusal is still absent.

The native owner is investigating lock acquisition. The diagnostic exposes the
error; it does not establish the narrow race mechanism, a semantic repair or a
passing complete native CI result. Separate typed Source material-read and
current-owner admission/cache amendments receive no grade from this diagnostic.

### Current admission and origin questions

Current owner eligibility must precede body materialisation and selected
disclosure. Structural validity, a copied body, a result-cache hit, TTL/stat
metadata or process memo cannot override a current owner refusal. Workcell's
optional Redis material does not own source permission. These are source and
cache authority boundaries, separate from prepared session context and
transport state.

The parent accepted the inspected marker-admission and conditional result-cache
source faults; the R4 native repair has its own custody and evidence. The f05
root-marker/no-op corrections do not close generic admission, cache retirement
or live-origin withdrawal. A copied Source projection is not an explicitly
independently accepted retained Source. Origin-withdrawal semantics and legacy
compatibility remain under native contract recovery; retained bytes do not
infer permission, deletion or promotion.

The exact unresolved joins are: current admission before cache/body disclosure;
qualification of the native cache/private-runtime repair; and the contract for
withdrawal of a live origin versus independently accepted retained source.
No real-model worker Return, personal installed composition, human Recognition
or QL/domain mapping decision is established by the hosted receipts. The
original diagram inspection remains at its declared source cut.

## Native command qualification — 3 October 2026

Published Source `88eb816c9af51c9b9ae875ec8cbc97ffa8e4b464` does not inherit a
passing whole-workflow result from earlier native command controls. Its
[original full CI](https://github.com/EpiLogos/ai-kit/actions/runs/37108561300)
failed at two distinct prerequisites. Linux rejected the current executable's
compiler-artifact admission. Mac's old-image control exited 101 because its
derived Capsule manifest omitted the required `description`, before the
intended native deadline behaviour. Equal old/new image digests with different
Source roots and a fresh artifact flag do not prove an old/new semantic comparison.

The three-path qualification repair supplies a valid derived fixture, native
workspace-only clean in the same target before selecting old/new/current builds,
and held actual compiler-artifact basis/alias admission. Independent review of
V3 found a further missing relation: a safely admitted original image could be
rejected for its semantic role or freshness without retaining its actual byte
digest. V4 now records the held regular image digest, bounded by the observed
size plus one change probe, before those guards. Unsafe, unaffiliated, changed
or unreadable originals instead retain explicit unavailability and cause;
they are not credited with fabricated byte evidence. Successful private image
copies must match that earlier digest and the driver joins its exact count,
read bound and physical bases to the actual compiler artifact.

The frozen V4 author packet is SHA-256
`78cf263cd823731ad71a0528168ac748903db6613700362fbcf45e2aaec1510e`.
It preserves all seventeen original command definitions, four complete target
selections, eight lifecycle readings and thirty-four required records. The
complete historical input and its three-case prefix retain separate identities.
V3's original packet and Parent custody record remain historical; Parent's
custody review missed the digest omission found by the independent review.
V4 passed two independent Source reviews and is adopted in this Source cut
through the existing owner route. Fresh full native qualification remains
pending; no successor native pass is claimed here.
These changes qualify command execution; they do not supply new production API,
live provider, Flow/orchestration, capture privacy, installed or human acceptance.
Earlier original-17 observations, platform failures and Wiki publication results
above retain their named Source cuts. Assertions, repeated host observations,
image metadata and a test definition are not interchangeable evidence.

The current matrix has no exact native-capture capability identity to promote
from this failed gate. The existing knowledge-command records link this scope
as a pressure on qualification. Native owner/capture transport, source permission,
prepared context, subsequent learning and public disclosure require their own
actual consumer results. No new registry, domain mapping or completed learning
cycle is inferred from these command controls.
