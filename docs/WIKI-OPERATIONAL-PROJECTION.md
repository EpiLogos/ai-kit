# Wiki operational projection and the feedback loop

Standing: owner-directed implementation, 20 September 2026. The O:I Wiki practice (#414), active Wiki implementation (#418) and constructive field (#420) remain their owners. This extends existing source and continuity operations rather than creating another Wiki, runtime or universal user-profile schema.

## The distinction that makes adaptation possible

Human governance states durable intent. A situated operational projection interprets that intent for a person, Project, Agent, task or occasion. Feedback records where that interpretation failed or where the intended practice changed. A correction can revise the projection immediately within its authorised scope; later review can compare accumulated corrections against governance. Neither generated inference nor a count of repeated corrections ratifies human source.

These are responsibilities within the same field, not three storage layers. Separately, the Wiki has ordinary source links/backlinks/tags and deliberately authored QL constellations. Both governance and projection may be ordinary Markdown, discoverable through that base field. A constellation can relate a rule, its intent, an exception, examples, corrections and results. A QL position or visual proximity never confers instruction authority. Expression construction is still the actual creative medium, not this source tool's renderer.

## Executable native slice

`aikit wiki projection read --file PATH` reads an existing Agent Wiki Markdown source and returns body, exact SHA-256 and retained feedback basis. `update` receives a full replacement body on stdin plus `--expected-revision`, `--evidence`, `--actor` and `--reason`. Updates serialize through the existing ContextLock, recheck the basis and replace the source atomically. The ordinary Markdown body is preserved verbatim. A trailing HTML comment (after the unchanged body, so YAML frontmatter stays first) retains an attributed feedback ledger without turning the source into a database. The comment's actor/evidence refs are attribution supplied by the caller, not an authentication or acceptance claim.

The command operates only on existing `.md` files in `Control/agents/wiki` or `ProjectCentral/agents/wiki`. Governance and user apertures are not eligible targets. Symlink paths, traversal, `.no-agent-retrieval` subtrees, invalid history, oversized bodies, missing sources and stale bases are explicit failures. Independent editors do not share this advisory lock: the final revision check detects their prior edits, but this is not an atomic transaction with a writer which ignores the lock. No deletion, governance promotion, source discovery, capability enrollment or model call occurs as a side effect.

The existing continuity engine gains an optional `hook/continuity/wiki-projection` capability. It is active only through the existing composition/trust selection. Configuration names the sources explicitly:

```toml
enable = ["hook/continuity/wiki-projection"]

[config."hook/continuity/wiki-projection"]
sources = [
  "central:Control/agents/wiki/projections/collaboration.md",
  "project:ProjectCentral/agents/wiki/projections/writing.md",
]
```

This is a configuration excerpt, not an instruction to replace an existing profile. Use existing profile/scope authoring and retain its other fields. Selecting a shared/global source is a separate act from authoring a Project correction. Unselected Wiki content, external bkmr links and search hits do not become instructions.

On SessionStart and UserPromptSubmit the source is reread, including when the Service itself remains open. It is not a frozen generation copy. Each block carries its source address and byte revision. Bodies are admitted whole within a bounded context allowance; a rule is never truncated into a different rule. A missing or denied source emits an unavailable status, not old cached text. An empty body explicitly clears the earlier operational projection from that same source. It does not erase a model's historical context or prove changed behaviour.

## Actual harness delivery, not an internal-only test

The inspected command computed `HookDecision.injected`, but plain `aikit hook dispatch` translated only its allowed/denied bit. This dropped the prepared guidance on the actual process boundary even though internal Service tests passed. The repair serializes the complete decision into Claude's event-specific `hookSpecificOutput.additionalContext` for SessionStart, UserPromptSubmit, PreToolUse and PostToolUse. It preserves denial behaviour, keeps permissionDecision limited to PreToolUse and does not send Claude-specific JSON to a strict or unknown client. Other clients and PreCompact disclose unavailable context transport rather than claiming delivery.

Primary protocol: https://code.claude.com/docs/en/hooks (consulted 20 September 2026). Existing Actuation capability descriptors still own which hooks are installed. This change does not invent a descriptor or bypass installation admission. `--json` remains the diagnostic machine envelope; plain mode is the harness-facing protocol. Emitted bytes are not proof that a running provider loaded or followed them.

The focused regression drives the actual binary: update Markdown with evidence, inspect its stored receipt, dispatch the next plain Claude hook and assert the exact new revision/body in additionalContext. Additional tests cover existing-Service refresh, unchanged governance, stale/concurrent writers, clear/history, unselected sources, two Projects, two Central roots, denied/missing sources, symlinks and whole-body budgets. The associated PR records actual execution results; this document alone is not passing evidence.

## Root identity and scope

Central remains the root meta-project. `central:` is a locator under the currently enclosing/configured workcell's Central root; it does not mean a hard-coded main machine. `project:` is under the resolved Project root, not the event's arbitrary working subdirectory. The product repository named Central, the personal Central root, a Workcell placement and ProjectCentral are related identities, not interchangeable names.

The current slice preserves those path/root boundaries. It does not yet implement a cross-machine user/guardian identity federation, replicated source conflict resolution or a shared-field acknowledgement protocol. Those must retain native user/Agent/Agency/World/Workcell refs and selected source authority. Moving or mirroring a source does not create a new user or silently widen its scope. Default/main workcell selection is routing, not authorship.

## UX and remaining joins

The existing Context surface should expose Current guidance with source/revision and compact reasons. A correction from chat or selected text offers its natural scope (this session, this Project or personal) inline, with the actual diff and undo. Explicit user corrections do not need a second participant/authority modal. Agent suggestions remain visibly proposed. Show stored, projected, emitted, harness-acknowledged and observed-use separately.

Document skills consume the same selected operational reading, then retrieve the actual project vision, founding positions, relevant UX/Wayfinder and source material through native knowledge operations. Successful projection does not prove those sources were retrieved. Conversely, unavailable bkmr, ripgrep or rich QL providers do not justify pretending the ordinary corpus is absent. The knowledge-navigation skill now describes the concrete update/refresh path.

Remaining integration work: Context/agent UI and native action discovery; typed session/user/guardian source selection and expiry/conflict policies; source-index invalidation/Return readback through #418; cross-workcell identity binding; other harness transports and live model acknowledgement; installed profile/capability admission. Existing source history owns prior complete versions; the in-file ledger retains reasons/bases, not every old body. Governance review must gather evidence and propose an explicit diff, never auto-rewrite it. General Flow cognition remains owner-read-only; this implementation does not claim to add a Flow writer.
