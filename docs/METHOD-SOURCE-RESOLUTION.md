# Explicit Method source resolution

`aikit --json method resolve --source /absolute/path/method.json` loads the
existing `aikit_core::method::Method` JSON contract and resolves its members
against the current actual navigation resource field. Run it inside a bound
Project. `--focus <resource-ref>` may repeat and selects situated Focus;
without it, the native ContextResolution's Project is used. Neither form
rewrites the Method declaration's `focus` or `project_domain`.

`method list` remains prefix-based discovery. A `METHOD:` description is not a
typed Method body or successful resolution evidence.

The result contains the native Method, PraxisResolution, and
ContextResolutionEvidence (reference, basis, provenance). Missing members or
unknown situated Focus refuse resolution. Members need not be active to be
resolvable: `skill_states` reports current capsule activation separately.
Resolution grants no capability, projection, runtime loading or authority.

`source_read.path` identifies the actual canonical filesystem source.
`source_read.revision` is BLAKE3 of the exact bytes read, also supplied as the
resolved Method revision. Authored `source` and optional `declared_revision`
remain separately disclosed. The loader validates reference syntax but does
not claim a declared Central SourceRef was resolved or endorsed by Central.
It creates no registry and writes neither source nor persistent resolution.
Callers retaining this result own its evidence placement and freshness checks.

Verification: `cargo test --locked -p aikit-cli --test method_source_native`
exercises the actual CLI source catalog, promoted Skill, Project binding,
source byte preservation, changed-revision distinction, and refusal of missing
members and Focus. It makes no model or harness loading claim.

Central AgentProfile and local Actuation instantiation sources also join the
shared canonical Context resource field as ephemeral Agent/Agency descriptors.
Central's returned source path is checked against the actual current profile
bytes; the source locator, authored revision and observed byte digest remain
visible. Actuation receipt identity, path and byte revision remain visible.
These source observations retain undetermined eligibility and no provider offer.
They make source-backed identity resolvable, while runtime admission remains a
separate question. Multiple Central profiles require explicit selection and
refuse this implicit composition path; no profile is guessed.

ACP session opening separately retains the provider's reported current model
and advertised models on the native session binding. This report is not model
catalog availability or independent selection/inference proof. The bounded
Factory acceptance checks its requested model against that report before prompt.

Within one compose or Method operation, owner resources are observed once and
that same resource snapshot supplies Context and Method resolution. The native
Context evidence basis retains `observed_source_resources`, including declared
revision and observed byte-digest annotations. Source bytes changing without a
revision-label change therefore change the Context receipt. Empty observations
are omitted to preserve older basis serialization and references. This is a
consistent read snapshot, not an atomic lock on files during later execution.
