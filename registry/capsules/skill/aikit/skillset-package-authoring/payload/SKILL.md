---
name: aikit-skillset-package-authoring
description: Use when turning a selected SkillSet into an idiomatic OpenAI/Codex, Claude or pi package — authoring its neutral [package] metadata and target overlays while the SkillSet stays the source; do not select merely to author or edit a Skill.
---

# SkillSet package authoring

Semantic ref: `aikit:skillset-package-authoring`. Native owner: `EpiLogos/ai-kit` (`crates/aikit-core/src/skillset_package/`, `aikit set package`). Contract: `docs/PRAXIS-ARCHITECTURE.md` §5.

A provider package is a **target projection** of a native SkillSet. It is never a new source SkillSet, and exporting never writes into the set or its member capsules. When the package is wrong, fix the SkillSet, its members or its neutral metadata, then re-export.

## What you author, and where

```text
member Skills            the capsules themselves (skill-authoring owns them)
neutral [package]        beside the set:  <home>/skillsets/<set>/set.toml   [package]
                                          <root>/skillsets/index.toml       [skillset.package]
target-specific material [package.targets.<openai|codex|claude|pi>]  — only here
```

Neutral fields: `name` (kebab-case, defaults to the set name), `version`, `description`, `license`, `[package.author]`, `homepage`, `repository`, `keywords`, `tools` (required host tools), `[[package.mcp]]` (`name`, `command`+`args` or `url`, `env` = variable NAMES), `[[package.hooks]]` (`event` = session-start | pre-tool | post-tool | prompt-submit | stop, `purpose`, `command`, optional `matcher`; `${PACKAGE_ROOT}` is rewritten per target), `[[package.commands]]` (only when the package genuinely contributes a user command), `[[package.environment]]` (`name`, `purpose` — never a value), `[package.presentation]` (`display_name`, `short_description`, `category`, `developer_name`, `brand_color`, `website_url`, `default_prompt`).

Secrets never enter package metadata. A literal token, `KEY=value` for a secret-named key, or credentials in a URL are refused with `skillset.package.secret_value`; reference `${NAME}` and declare the name under `environment`.

## Procedure

1. Recover the selected SkillSet and its members: `aikit set show <set>` then `aikit set package inspect <set>`. Confirm every member resolves at an exact revision; an unresolved member is an `unsupported` entry, never a silent omission.
2. Decide what the package must say beyond its Skills. A documentation or praxis SkillSet usually exports as Skills only — add MCP, hooks or commands only when the Skills actually depend on them.
3. Author the neutral `[package]` table. Keep target vocabulary out of it.
4. Read the plan for each intended target: `aikit set package plan <set> --target <t>`. Every relation is classified `portable`, `translated`, `target-addition` or `unsupported` with a reason. Treat `unsupported` as information for the human reading the receipt, not something to paper over.
5. Only where a target needs something the neutral model cannot say, add `[package.targets.<t>]`. Identity (`name`) is owned by the SkillSet and is never overridden there.
6. Export and validate through the Method `aikit:skillset-package-export`.

## Per-target detail

Open only the reference for the target in hand:

- `references/openai.md` — Agent Plugins v1 root manifest and the Codex compatibility overlay.
- `references/claude.md` — Claude Code plugin layout and strict validation.
- `references/pi.md` — pi package, the no-MCP boundary and generated extensions.

## Boundaries

- Skills are carried byte-for-byte under `skills/<name>/` (the SKILL.md frontmatter name). They never become commands, agents or extensions.
- Claude `commands/` and `agents/`, and pi extensions, appear only when the package explicitly declares commands or hooks.
- Every export carries `aikit-package.json` (source SkillSet ref, source revision, member ids and revisions). Do not hand-edit exported trees; `aikit set package diff` will report the drift.
