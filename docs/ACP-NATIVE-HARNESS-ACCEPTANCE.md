# Real harness acceptance through AIKit ACP

Desktop consumers use `AcpStableConnectionAdapter` and `AgentSessionHost` for
ACP harnesses. The host owns native process routing, ordered event lanes and
observed cancellation. The caller supplies an owner-issued canonical
AgentSession reference; a transport session ID is never that identity.

`acp_harness_native` is an explicit live test, ignored during ordinary tests
because it invokes a configured real provider. Set `AIKIT_ACP_NATIVE_ARGV` to a
JSON array naming the actual ACP process and arguments. Configure its launcher
for isolated temporary working ground with destructive tools disabled, then run:

```
cargo test -p aikit-adapters --test acp_harness_native -- --ignored --nocapture
```

The test observes startup messages separately from prompted output, checks a
real streamed response after dropping a view handle, interrupts after the first
content or native thinking update, requires an observed cancelled terminal result, and prompts again
on the same canonical/native binding. It does not prove tool authorization,
permission policy, transcript persistence or desktop wiring.

A native acceptance on 2026-09-06 used published `pi-acp` 0.0.33 with installed
Pi 0.84.4 and its configured provider. The bridge was installed in an isolated
npm prefix with its normal lockfile. Its Pi command used `--no-tools`,
`--no-extensions` and `--no-session` in temporary working ground. This uses the
existing generic ACP implementation, without the Pi RPC adapter.

The current pi-acp bridge closes other native sessions when creating a new one.
Consumers must therefore give each concurrent canonical encounter its own
connection process for this harness. Dropping or moving a presentation is not
closing that process. Other ACP harnesses retain their native negotiated
capabilities; the Pi limitation must not become a universal ACP assumption.

The integrated resident host preserves ordered events with disk-backed delivery;
normal turns have no total signal-count ceiling. A caller can select a nonzero
operational limit, whose terminal result is distinct from provider cancellation.
The native test crosses the former 512-event ceiling with real thinking updates,
cancels, and continues on the same binding. Thinking bytes are now preserved as
`AgentThoughtChunk` and retained by the resident encounter journal.

See `encounter-runtime.md` for the desktop owner route and
`ACP-REQUIRED-CONTEXT-ADMISSION.md` for opt-in source prerequisite checks. Those
checks do not establish that every provider tool is governed or that a model
understood the supplied context. The desktop uses the generic ACP connection;
Pi RPC remains internal to the pinned bridge, not a competing desktop route.
