# pi (pi package)

- `package.json`: `name`, `version`, `description`, `keywords` (first is `pi-package`), `license`, `author`, and `pi: {skills: ["./skills"]}`.
- pi has **no MCP host**: every MCP dependency is planned `unsupported` with that reason and stays in the receipt.
- Declared hooks or commands produce one TypeScript extension, `extensions/<package>-aikit.ts` (`export default function (pi: ExtensionAPI)`, `pi.on(...)`, `pi.registerCommand(...)`), shelling to the declared command with `PACKAGE_ROOT` set. It is a `target-addition`; `package.json` then lists it under `pi.extensions` and declares `@earendil-works/pi-coding-agent` as a `"*"` peer dependency.
- Event mapping: session-start → `session_start`, pre-tool → `tool_call` (exit 2 blocks), post-tool → `tool_result`, prompt-submit → `input`, stop → `agent_end`.
- Skills never become extensions.
- Validation: `pi --mode rpc --no-session --offline --no-approve -e <dir>` with a disposable HOME, request `get_commands`, and require `skill:<name>` sourced from the package for every exported member.
