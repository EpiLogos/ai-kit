# claude (Claude Code plugin)

- `.claude-plugin/plugin.json`: `name` (kebab-case) is the only required field; aikit writes `version`, `description`, `author`, `license`, `keywords`, `homepage`, `repository`, and `displayName` from presentation.
- Default components: `skills/<name>/SKILL.md`, `.mcp.json` (`{mcpServers}`), `hooks/hooks.json` (`{hooks: {<Event>: [{matcher?, hooks: [{type: "command", command}]}]}}`). Hook commands use `${CLAUDE_PLUGIN_ROOT}`.
- Event mapping: session-start → SessionStart, pre-tool → PreToolUse, post-tool → PostToolUse, prompt-submit → UserPromptSubmit, stop → Stop.
- `commands/<name>.md` only for declared `[[package.commands]]`; `agents/` is never generated.
- Presentation other than `display_name`/`website_url` has no plugin.json field (category/tags belong to a marketplace entry) and is planned `unsupported`.
- `[package.targets.claude]` may add recognised plugin.json fields (e.g. `defaultEnabled`, `userConfig`); unknown fields are refused because `--strict` would fail.
- Validation: `claude plugin validate --strict --json <dir>` (offline; exit 0 and `success: true`).
