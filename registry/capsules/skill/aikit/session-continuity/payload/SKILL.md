---
name: aikit-session-continuity
description: "METHOD: Preserve and recover time, source, subject and session identity across re-entry through the real `aikit now-context`, `aikit-session-space` and `aikit continuity` surfaces plus Central Day/NOW, distinguishing replay from a later reading and a cancelled action from an uncertain one."
---

# Preserve continuity through re-entry

Semantic ref: `aikit:session-continuity`. Native owner: `EpiLogos/ai-kit`, consuming Central Day/NOW as the temporal owner. This Method is QL-MEF practice **XP10** — "Preserve time, source, subject and session through re-entry" — bound at its actual owners: AIKit's NOW-context/SessionSpace/continuity machinery and Central's Day/NOW, not a QL Skill (QL does not own session/time state).

## Contract metadata

- Executable support (verified against the installed `aikit`/`ctrl` binaries' own `--help`): `aikit now-context status|prepare|inspect|publish|append-change|revoke`; `aikit-session-space list|show|discover|explain|history|compare|reconcile|agent-session-read|agent-session-find|working-surface`; `aikit continuity commands|pressure|closeout verify`; `ctrl action run central.now.read|central.now.list|central.day.read` (root) / `ctrl action run projectcentral.now.inspect` (project).
- Governing source: QL-MEF `docs/kernel-rebuild/AGENT-PRACTICE-AND-BOOTSTRAP.md` (practice XP10), `PRE-K8-AGENT-WORLD-LOCK.md` §K10.2 ("Central owns the actual temporal/source system... Preserve long-lived NOW, occurrence versus receipt, late returns, original identity/source revisions"); Central `AGENTS.md` "Source, projection, continuation and Return are different things" and "The day closes".
- Real distinctions this Method must not collapse: source text is not harness loading; a stale generation is not current uptake; a cancelled action is not an action whose effect is merely unconfirmed; replaying an original occasion is not the same act as later reinterpreting it.

## When

Use whenever a session resumes work after interruption, handoff, a new surface, or a changed provider/body — or when a fresh session must recover what an earlier one left rather than reconstruct it from a paraphrase. Use it before treating any prior session's summary as ground truth.

## Inputs

The actual participant/session ref; the real Central root or child Project, current NOW clearing(s) and Day; any persisted SessionSpace ref and its correlation id; the actual prior receipt/history to compare against; the caller's real authority to inspect or mutate that state (publish/append-change/revoke are participant-scoped mutations, not read-only).

## Authority

`status`, `inspect`, `list`, `show`, `discover`, `explain`, `history`, `compare`, `reconcile` (with no supplied observations), `agent-session-read`, `agent-session-find`, `commands`, `pressure` and `closeout verify` are read-only. `prepare`, `publish`, `append-change` and `revoke` are real participant-scoped mutations against Redis-backed NOW context; `apply`/`stage` against a SessionSpace apply exactly a previously reviewed preview, never an unreviewed guess. None of these establishes Central Day/NOW authorship — Central remains the owner of Day/NOW; this Method reads and republishes derived participant views, it does not originate NOW records.

## Procedure

1. Recover before asking: read the actual current state first. `ctrl action run central.now.list '{}'` and `ctrl action run central.day.read '{}'` at root (or `projectcentral.now.inspect` for a project); `aikit-session-space list`/`discover --project <REF>` for persisted spaces; `aikit now-context status` for the material-service health.
2. For a specific participant view: `aikit now-context inspect --config-file <PATH> --participant-ref <REF>` to read the current prepared version, delivery and cursor state before assuming it is stale or missing.
3. For a SessionSpace: `aikit-session-space show <SPACE>` / `explain <SPACE>` to read its canonical state and the receipt that last changed it; `history` and `compare` to distinguish the current state from an earlier receipt rather than guessing at drift.
4. Distinguish replay from reinterpretation: replaying an original occasion re-runs it exactly as it was: reinterpreting it derives a new reading from old material. State explicitly which one is intended before acting, and do not silently substitute one for the other.
5. Distinguish a cancelled action from an uncertain one: a refusal or explicit cancellation changes nothing; a result whose effect is unconfirmed (timeout, late child, interrupted work) must be checked by reading back actual state, never assumed successful or assumed reverted.
6. Only when the caller's authority and the actual review basis are both real: `aikit now-context prepare`/`publish`/`append-change` to advance the participant view, or `aikit-session-space stage`/`apply` (a previously reviewed preview only) to advance a SessionSpace, or `aikit continuity closeout verify` to check that a claimed close-out actually left the objects it claims to have left.
7. Return through the actual register: root cross-project work to `Control/agents/now/` via `central.now.*`; project-scoped work to `Work/<Name>/ProjectCentral/now/` via `projectcentral.now.*`. Close the day (`central.day.lifecycle` / `projectcentral.now.rollover`) only when the session's work is actually finished, never as a formality.

## Outputs

The exact recovered NOW/Day/SessionSpace refs and revisions, what was found live versus stale versus absent, which of replay/reinterpretation was performed, and the exact next-session continuation point (open NOW clearings, unresolved SessionSpace state, unclosed Day). A pending receipt stays reported as pending after restart; it is never silently marked resolved.

## Verification

`aikit continuity closeout verify` against the actual carriers a close-out claims to have left; `aikit-session-space compare` between two receipt-backed states; `ctrl action run central.now.read` against the exact `now_ref` returned by a prior allocation. A passing read confirms the state matches its own record, not that the underlying work was correct.

## Failure and recovery

A missing or unreadable participant/session ref is a named recovery gap, not an invented continuation. A stale generation is reported as stale, not silently treated as current. An uncertain prior effect is read back before any retry; do not reissue a side effect against an unconfirmed prior attempt. Missing local Redis/session-space access blocks only that command, not the Central Day/NOW read path.

## Continuity

This Method is itself the continuity seam: use it first in every re-entry, before `ql-experience-prepare` or any product-specific practice, so the subject/source/session basis those Methods assume is actually current rather than assumed.
