# Central root context is not a binding-free context

Central is the enclosing root meta-project. Child Project selection narrows the
working situation; a Profile selects composition. Neither creates the enclosing
World. This repair of #294 restores that distinction rather than making
`ContextResolution.project_binding` optional.

## Two existing native relations, not a synthetic child

A configured filesystem Central is recognised at its root, in Control, or at the
Work container, with the existing `control:root` protocol identity. It uses a real
local locator, without creating `.aikit`, a Profile, or ProjectCentral. An existing
child ProjectCentral manifest also participates without an AIKit profile marker.
Authored payloads are not read by location discovery. A redirected or removed root
is not silently converted to a successful binding-free resolution. Alias entry
compares canonical locations on both sides and retains the existing profile
scope chain, Project specification and Skill Sets rather than silently dropping them.

Explicit Actuation admission supplies its own World, WorldBinding, scope and
source basis. For declared root scope with no narrower local Project, the new `native-world`
locator preserves those exact references, declared revision and byte digest in
the required ProjectBinding. It makes no local-directory claim. An arbitrary
non-root World is not silently promoted into a Project; it needs its own native
Project binding. In particular,
`central:root` supplied by an admitted World is not silently renamed to Central's
`control:root` source-graph reference. The respective owners retain their refs.
When a local Central root is resolved, its local ProjectBinding remains the ground
and the independently admitted Agency/WorldBinding remains in `agency_admission`.
Root-scope admission is refused from a discovered child Project rather than
silently inheriting that child's capabilities or source scopes.

Actor bootstrap, SessionSpace evidence and Flow scope checks continue consuming
the complete required binding. Context evidence includes the native relation and
source basis; a changed basis therefore changes the SessionSpace evidence ref.
No NativeWorld locator is treated as a filesystem grant by encounter dispatch.
Native model policy, credentials, owner admission and later-turn checks remain
in the resident execution path; this repair weakens none of them.

## Verification

The existing mandatory native CAW case
`compose_cli_realisation_uses_the_same_existing_resident_target` remains profileless
and child-Project-free. It now asserts the native root identity/binding in both the
composition reading and actor bootstrap, then executes the existing selected-model
resident and verifies its Return. The new ordinary tests cover configured Central
root/Control/Work entry, markerless child identity, non-Central absence, source
privacy, changed root refusal, native binding roundtrip, bootstrap, SessionSpace
basis and Flow scope discrimination. An additional Unix regression protects the
profile chain and Project specification when entering Central through an alias.
Retained native campaign logs distinguish controlled providers from real
commercial-model or installed-personal-world evidence.

The root repair is in #294's existing lane. It does not close the wider CAW
programme, adopt personal Control, merge the stacked work or assert local acceptance.

## Observed repository proof — 11 September 2026

[Repair run 34633999067](https://github.com/EpiLogos/ai-kit/actions/runs/34633999067)
passed all gates and published the tested source as
`dd691fb31e17c66a493e3df021918e50b957579c`. The workflow run itself is keyed to
`2986ff92e12095ef289d5d251a96faca92144539`, the temporary delivery commit: it applied
a checksum-verified patch over reconciled source `43f790b4b878292be64c735e25d6341caeff4901`,
tested that candidate, and only then committed/pushed the tested tree.
`tested-commit.txt` and `tested-source.tar.gz` retain that correlation; the temporary
delivery workflow and patch are absent from the published source tree.

| Gate | Observed result |
| --- | --- |
| New focused root/core/adapter/CLI tests | 10 passed, 0 failed, 0 ignored |
| `scripts/verify` workspace tests | 2,398 passed, 0 failed, 35 ignored |
| Workspace all-target Clippy, warnings denied | Passed |
| Native CAW delivery, explicitly running ignored cases | 13 passed, 0 failed |
| Native placement | 3 passed, 0 failed |
| Native persistent-task dispatch | 7 passed, 0 failed |

The original failing `compose_cli_realisation_uses_the_same_existing_resident_target`
now passed and emitted `MODEL_COMPOSE_NATIVE_CONNECTION_EXECUTED`. Fresh native
Central root/Control/Work-container composition also passed and emitted
`CENTRAL_ROOT_META_PROJECT_COMPOSE_EXECUTED`. Ordinary ignored tests are not
counted as executed; native cases above were explicitly selected with `--ignored`.

Exact dependency basis: Actuation `2d73f957c287ef386b09ac2c55eeab3603b60873`,
Central `5d1b8bf6692f9aca45b2fe39db54cbc378dda70e`, and Workcell
`0187b0d76e8f27632d127d54ef2441310b073108`, under Rust 1.98.0 on Ubuntu 24.04.
Evidence artifact `10277628800` has SHA-256
`44311871858e8e817cbe77799665f759d081da6cb56418268d986bd508ce7fc8`;
its tested-source archive has SHA-256
`330f212a91a39511c1f216c4faff3cd549807c5241a708706e386e3642d595a6`.

The final code including alias-path preservation is
`600321dbbcda3353a28c8f2b2672ef8ac90406e0`, checked through PR test merge
`a11e397402776644538095cfbb2fd756b1d4c542`. Its
[CI 34635160694](https://github.com/EpiLogos/ai-kit/actions/runs/34635160694)
and [native CAW 34635160678](https://github.com/EpiLogos/ai-kit/actions/runs/34635160678)
both completed successfully. All five Linux crate jobs, the macOS real integration
suite, real bkmr and GitNexus conformance passed. The Linux CLI job ran 358 tests,
all passed, with 25 skipped; its log explicitly records the alias-profile regression
as passed. The native CAW run passed delivery, placement, persistent-task and
receipt/admission/ACP/recurrence/duplicate regression gates. Provider conformance
`34635160734` and documentation maintenance `34635160720` also passed.

These results establish the root-composition repair at those source revisions.
They are not blanket P02/CAW closure or evidence of an installed personal World.
