# Native Pi RPC connection

`PiRpcConnectionAdapter` implements the installed Pi 0.84 JSONL RPC contract
through `AgentSessionHost`. It does not present Pi as ACP or add another process
host. Launch Pi in its actual project directory, initialize the host, then attach
an explicitly supplied canonical `agent-session/*` reference. The adapter re-reads
the native session identity before binding and refuses a busy session, another
resident binding, mismatched identity, or an attempt to change launch context.

One Pi process carries one resident session. Another concurrent encounter needs
another native connection. Native Pi session identity remains routing evidence;
it never becomes canonical AgentSession identity. Pi retains its model selection
and history. This increment advertises attach, ordered text streaming and cancel;
it does not advertise create/load/resume, reconnect, native session closure,
MCP/additional-directory mutation, or tool-permission parity. Pi extension UI
requests are explicitly degraded until an owner-authorized bridge implements
their meaning.

Prompt acceptance and low-level `agent_end` are not completion. Only
`agent_settled` ends the host turn, using the native assistant's observed stop
reason. Provider failures end that turn as failed while retaining the session.
Cancellation clears Pi's queued continuations and issues `abort`; its commands
and responses remain observable and cancellation is recorded after the native
stop. Dropping a turn/view handle does not stop the provider. Deliberate process
shutdown remains the host operation and does not claim semantic continuity.

The explicit live acceptance uses the real installed Pi and its configured
provider, with tools, extensions and persistent history disabled in temporary
ground:

```sh
OI_PI_BIN=/absolute/path/to/pi cargo test -p aikit-adapters --test pi_rpc_native -- --ignored
```

It proves native binding, one-session limits, streamed output, continuation after
dropping a view handle, mid-stream cancellation, and a later turn on the same
native/canonical binding. It makes three bounded provider calls. It is ignored
by the ordinary test invocation because an absent installed harness or provider
credential is not simulated. A full native acceptance receipt must include the
explicit live run; ordinary owner tests alone do not prove provider integration.
