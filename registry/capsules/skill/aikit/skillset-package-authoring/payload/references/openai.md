# openai / codex (Agent Plugins v1)

- Root `plugin.json` with `$schema` `https://agent-plugins.org/schemas/1.0.0/plugin.schema.json`. Only `$schema, name, version, description, author{name,email,url}, homepage, repository, license, keywords, extensions` are allowed (`additionalProperties: false`; nulls are errors).
- Name rule: 1–64 of `[a-z0-9.-]`, alphanumeric at both ends, no `--` or `..`.
- Components are fixed: `./skills/<name>/`, `./mcp.json` (no leading dot, `$schema` mcp 1.0.0, each server typed `stdio` | `streamable-http` | `sse`).
- Hooks and UI presentation go under `extensions["com.openai"]` (`hooks: "./hooks/hooks.json"`, `interface{displayName, shortDescription, category, …}`). Hook commands use `${PLUGIN_ROOT}`.
- `codex` (or `openai --with-codex-overlay`) adds `.codex-plugin/plugin.json` for older Codex builds: it reuses `./skills/`, `./mcp.json`, `./hooks/hooks.json` and carries an `interface` block. Codex ignores the overlay when `extensions["com.openai"]` is present, so both carry the same interface.
- No analogue: user commands, required host tools, package-level environment names (recorded in `aikit-package.json`).
- There is no `codex validate`. `--native` performs a disposable load: temp `CODEX_HOME`, temp marketplace, `codex plugin marketplace add` → `plugin add` → `plugin list --json`, and checks `~/.codex/config.toml` is untouched.
