---
name: aikit-skillset-package-export
description: "METHOD: Export a SkillSet to a native agent package — resolve exact member revisions, read the target plan, render, validate natively, prove Skill discovery and return the receipt, leaving the SkillSet untouched."
---

# Export a SkillSet to a native agent package

Semantic ref: `aikit:skillset-package-export`. Native owner: `EpiLogos/ai-kit`. Contract: `docs/PRAXIS-ARCHITECTURE.md` §5. Authoring the package metadata itself is `aikit:skillset-package-authoring`.

The operation, in order. Each step's evidence is the command's JSON, not a paraphrase of it.

1. **Select the SkillSet.** A home set name or a registry semantic ref (`central:documentation`). `aikit set show <set>`.
2. **Resolve exact member and source revisions.** `aikit set package inspect <set>` → `aikit.portable-skill-package/v1`: every member's id, form, SKILL.md name, capsule revision and payload file hashes, plus `source_revision`. An `unresolved` list that is not empty stops the export unless the human accepts a partial package (`--allow-partial`).
3. **Inspect target capabilities.** `aikit set package inspect <set> --target <t>` adds `capabilities`: identity, where Skills live, MCP, hooks/extensions, UI, install/discovery, validation, and what has no analogue.
4. **Classify and plan.** `aikit set package plan <set> --target <t>`: every relation is `portable`, `translated`, `target-addition` or `unsupported{reason}`. Nothing is dropped silently; read the unsupported entries before going on.
5. **Render.** `aikit set package export <set> --target <t> --out <dir> [--with-codex-overlay] [--receipt <file>]`. The tree carries `aikit-package.json`; the reply is the receipt. `source_unchanged: true` is the proof the SkillSet and capsules were not written.
6. **Native validation.** Add `--native` (or run `aikit set package verify <set> --target <t> --out <dir> --native`): claude → `claude plugin validate --strict --json`; pi → `pi --mode rpc … get_commands`; openai/codex → structural Agent Plugins check plus a disposable Codex marketplace load. A missing tool is reported `unavailable`, which is **not** a pass.
7. **Disposable discovery.** Where the target can load the package without touching the user's configuration (pi with a disposable HOME, Codex with a temporary CODEX_HOME), the receipt's `discovery` lists the Skills the host actually found and any it missed.
8. **Verify Skill discovery and loading.** Every exported member must appear as a discovered Skill. A validator passing while a Skill is missing is a failed export.
9. **Return the receipt.** `aikit.skillset-package-receipt/v1`: source ref and revision, member ids and revisions, target and format version, exported files with sha256, translated relations, target additions, unsupported relations, validation (command, exit, summary) and discovery evidence. Later, `aikit set package diff <set> --target <t> --out <dir>` reports which members moved since the export.

Completion is claimed only at the scope the receipt establishes: "exported and validated for claude" is not "works in every target", and a structural pass is not a native one.
