# SessionSpace provider and connection conformance

Status: PR #81 evidence ledger for #61 / #63 / #66.  
Boundary: provider/connection truth only; broad #62 application/persistence remains out of scope.

## Live integration base

The work was forked from the audited live `main` head:

```text
9ff28ca31c723b314ddf62b7f85b6e2611d53d66
```

While the branch was in flight, `main` advanced independently through Knowledge Navigation #79 to:

```text
43eafa86437b528162a93c09c05399a137f8d6b9
```

PR #81 therefore uses GitHub's merge ref against the current base. No Profile/SkillSet application-composition work from parallel PR #78 is copied, modified or recreated here.

## Current provider pins

| Provider / target | Current pin used by this proof | Evidence mode |
|---|---|---|
| cmux | `manaflow-ai/cmux@v0.64.22` | real macOS control-socket test, provider-gated |
| tmux | `tmux/tmux@3.7c` | exact source build + real private server |
| VS Code | `microsoft/vscode@1.133.0` | real Extension Host via `@vscode/test-electron` |
| ACP protocol canon | `agentclientprotocol/agent-client-protocol@7b160aeda86d123f37f7a9d201e642cd7ee12ef5` | protocol reference |
| ACP real target | `agentclientprotocol/python-sdk@e668ed9e0034f5076749a6d74c5cec40ab610010` | official `examples/echo_agent.py` real stdio Agent |
| DeepSeek/Cordis existing proof | `deepseek-ai/deepseek-harness@47f943859bef60e4160492346772ded9b24f765a` | existing SessionSpace live composition proof |

## Public provider seam

`aikit.working-environment-provider/v1` is deliberately small.

It exposes:

- provider identity and capability observation;
- health (`healthy`, `degraded`, `unavailable`);
- explicit provider-native bindings with optional canonical refs;
- open / observe / focus / detach operations;
- `MuxWorkingEnvironment<A>` over the existing `MuxAdapter` contract;
- `MuxSessionSpaceActivationDriver<A>` over the existing canonical `SessionSpaceActivationDriver`.

It does **not** provide another mux, workspace daemon, layout language, connection stack or SessionSpace store.

The identity rule is structural: a provider-native id has `canonical_ref = None` unless a caller explicitly binds it. cmux workspace/window/pane ids, tmux server/session/window/pane ids, VS Code workspace/editor/terminal/tab identities and ACP native session ids therefore remain provider/protocol facts.

## cmux

The existing `Cmux` adapter remains the sole controller. `working_environment_cmux_real.rs` uses the same adapter through the public SessionSpace provider seam and, on a real reachable current cmux app, proves:

- create/open through the common provider surface;
- two terminal/conversation Surfaces with explicit canonical bindings;
- multiple Project and AgentSession relations without identity collapse;
- real focus/select;
- real Surface detach;
- provider-native ids remain unequal to and independent from canonical refs.

The lane is intentionally gated by `AIKIT_CMUX_REAL`. A host without a real cmux app/control socket is not Closure evidence and is reported as skipped.

A cmux workspace or surface can **host** an explicitly bound AgentSession relation, but cmux does not itself own an AgentSession protocol. The provider seam therefore does not claim native AgentSession resume/continuity from a cmux id.

## tmux

`working_environment_tmux_real.rs` uses a real private tmux server and proves:

- create/open and pane materialisation;
- explicit Surface, Project and AgentSession bindings;
- real Surface focus;
- AIKit adapter-object restart while the tmux server survives;
- relation rediscovery from the surviving provider;
- provider/session disappearance;
- reconstruction through the same canonical Surface bindings;
- terminal pane restoration;
- provider-local detach without canonical Surface deletion;
- a fresh `SessionSpaceRuntime` does not acquire AgentSession continuity merely because tmux survived.

The existing tmux adapter remains authoritative for presentation truth: `true_popup` is a real capability; non-popup providers remain inline rather than being cosmetically simulated.

As with cmux, tmux can host an AgentSession's terminal Surface but tmux persistence is not AgentSession continuity and no native attach/resume protocol is invented.

## VS Code rich IDE specimen

The fixture under `provider-fixtures/vscode-session-space/` launches actual VS Code `1.133.0` with two workspace roots and proves through the stable extension API:

- multi-root / multi-Project working context;
- active editor and selection/focus;
- integrated terminal creation and focus;
- tab-group visibility and diff Surface;
- webview preview Surface;
- test controller Surface;
- current Chat API / `createChatParticipant` conversation capability.

`fixture.code-workspace` is only the launch input to VS Code. It is not an AIKit SessionSpace store and contains no canonical SessionSpace identity.

VS Code provider identity is likewise not Project, Surface or AgentSession identity. An external integration participates through the public working-environment seam and explicit bindings.

### Stable AgentSession limitation

The current stable VS Code extension API does **not** expose a general API for enumerating, attaching, detaching or rebinding arbitrary existing chat/agent sessions. Current VS Code documentation describes `chatSessionsProvider` as a **proposed** API, and the public API does not provide general observation/history injection for sessions owned by VS Code/Copilot.

Therefore this specimen proves a real agent/conversation **Surface**, but it does not claim stable public AgentSession attach/detach/rebind. #63 must remain open unless that acceptance is satisfied by a genuinely supported current provider/API or the ticket is explicitly narrowed. Private workspace storage and proposed APIs are not accepted as a substitute.

Current Zed documentation was also rechecked because its product-level Threads Sidebar, ACP External Agents and Terminal Threads provide richer native agent-session UX. That is useful candidate pressure, but this pass did not prove a stable external control/API seam that would let AIKit bind/focus those objects without private Zed internals, so it is not silently substituted for the actual VS Code proof.

## ACP

`ConnectionProcess` is the missing real transport beneath the existing pure `aikit.connection-adapter/v1` protocol adapter. It owns only the child process and stdin/stdout bytes.

The provider-gated ACP lane installs the pinned official Python SDK and runs its real `echo_agent.py` target. The test proves:

- stable ACP initialization through the existing adapter;
- real `session/new` native session creation;
- explicit canonical AgentSession binding distinct from the native session id;
- real `session/update` streaming;
- prompt completion;
- real cancel notification delivery without inventing a permission phase the target does not expose;
- process disconnect/restart;
- explicit lack of session-resume continuity when the target does not advertise it;
- a restarted target receives a new native session id even when the caller deliberately reuses the canonical AgentSession ref.

Permission request/response remains covered by the existing protocol conformance tests. The selected real echo target does not exercise permissions, so this receipt does not claim a live permission request from it.

## classic / non-ACP

The classic lane launches a real stdio Python process through the same transport owner and existing `ClassicProcessConnectionAdapter` semantics. It proves:

- real process launch;
- real streamed stdout update ingestion;
- real SIGINT interruption from the adapter's `interrupt` command;
- cancelled update ingestion;
- process disconnect;
- explicit lack of resume/reconnect capability;
- replacement process creation is not session continuity.

No ACP permission, session-resume or JSON-RPC semantics are projected onto the classic target.

## Mixed and degraded world

The existing `session_space_live_v2` suite already proves one degraded ACP connection and one connected classic connection can coexist independently in the same SessionSpace read model. `SessionSpaceRuntime::observe_provider_unavailable` degrades only readings owned by the disappeared provider; desired HarnessComposition, canonical Surface refs and AgentSession identity are not rewritten.

The real tmux disappearance/reconstruction test adds the provider-side half of that law: observed provider truth can disappear and return while canonical relations remain explicit and stable.

## #62 frontier after provider proof

This work intentionally does not implement the shared application/persistence continuation. #62 still owns, at minimum:

1. persisted authored SessionSpace definitions and reconstruction inputs beyond the current in-memory runtime;
2. shared application operations for SessionSpace create/open/read/bind/unbind/focus/reconcile/history;
3. Project/Context binding and resolution through the canonical application architecture;
4. provider / host / material binding read models across restarts;
5. durable focus/current-region and history semantics;
6. application-level orchestration of reconstructed provider observations;
7. TUI/desktop/agent consumers over those shared operations.

Provider tests here may instantiate existing core runtime values narrowly; they do not claim ownership of those operations.

## Effect on #67

This branch removes two lower-level uncertainties for the unified terminal working field:

- #67 can consume one provider-neutral working-environment seam for tmux/cmux/IDE focus and provider health instead of owning a mux;
- #67 can consume one real process transport under `aikit.connection-adapter/v1` for ACP/classic conversation lifecycles instead of implementing a protocol adapter.

#67 still depends on #62's application/persistence operations for the enclosing durable SessionSpace read/write experience. #63 also remains open until both the real current cmux gate and the rich-IDE AgentSession attach/detach/rebind acceptance have genuine provider evidence.