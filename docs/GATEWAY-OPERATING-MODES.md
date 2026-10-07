# Gateway operating modes — a crosswalk

One gateway, one conversation model. What changes between "on this machine",
"across the tailnet" and "through a tunnel" is **how a client reaches the
listener** — never which agent answers, never how a conversation is journaled,
never what a Communique or a connector binding is. This page separates the
things that are easy to fuse, then crosswalks every way of reaching and running
the gateway against them.

Read it alongside `GATEWAY-CONTACT-AND-DAY.md` (contact, relay, service),
`GATEWAY-BOT-TO-BOT.md` (the one communication system) and
`GATEWAY-UPGRADE.md` (the managed upgrade). Evidence standing follows the
suite's classes: **D** deterministic, **C** cross-product, **P** real provider,
**M** physical machine, **H** human. Shipped means a test or a live probe ran;
nothing here is "supported" from intention.

```sh
aikit gateway modes            # this page's table, plus what THIS machine actually runs
aikit gateway doctor           # every finding below, with the command that fixes it
aikit gateway setup --mode private-tailnet --peer workcell:omarchy=100.92.62.101:7788
aikit gateway upgrade plan     # what runs, what is installed, what differs
```

## Five questions, never one

A "mode" answers several independent questions. Keeping them apart is what
stops a convenience flag from becoming a second agent model.

| Question | Decides | Lives in | Changes when you change the connection mode? |
|---|---|---|---|
| **Listener binding** | Where a socket is bound, so who could possibly reach it | `ListenerClass` (`local-ipc`, `loopback`, `tailnet`, `private-network`, `wildcard`, `public`, `named`) | yes — this *is* the mode |
| **Carrier scope** | What a peer that got through may *do*: `owner` (drain, restore, bind) or `peer` (relay, occupancy, contact, reads) | `CarrierScope`; owner token vs peer token | no — same policy on every carrier |
| **Transport** | The bytes: Unix socket, WebSocket over TCP, TCP forwarded by a tailnet service, SSH | `GatewayCarrierTarget` | yes |
| **Workcell placement** | Which machine's Actuation answers "who occupies this Position"; where a session lives | `AIKIT_WORKCELL_REF`, `gateway remote add`, Position occupancy | no |
| **Connector identity** | Which external identity (bot, account) a Surface is, and its per-sender admission | connector bindings | no |
| **Session continuity** | Which Stream, AgentSession and delivery a turn belongs to, across restarts | Stream journals, `encounter_deliveries` | no |
| **Lifecycle** | Who keeps the process running and restarts it | foreground, supervised, application-managed | yes — it decides whether an upgrade can restart it |

The rule the table encodes: **only listener binding, transport and lifecycle
move with the mode.** Workcell placement, connector identity and session
continuity are facts about the world, read from their owners, and a connection
mode reads them exactly as it would on the same host.

## Entry modes

Each row is one way a client reaches a gateway. "Authenticates" is the
application's check; tailnet membership, a loopback source address and an SSH
login are not application identity and are never treated as one.

| Mode | Listener binding | Transport | Who can reach it | Application auth | Transport confidentiality | State here |
|---|---|---|---|---|---|---|
| **local-ipc** | Unix socket `~/.aikit/state/gateway.sock`, mode 0600 | Unix | local processes the file mode admits | file mode = **owner** scope | none needed | shipped (D, M) |
| **loopback-service** | `127.0.0.1:PORT` | WebSocket | any local process of any user, local browsers | bearer token (peer) / owner token | none (local) | shipped (D) |
| **private-tailnet** | the node's tailnet address `100.x` | WebSocket | tailnet peers the tailnet policy admits, plus local processes | bearer token; WireGuard authenticates the *machine* | WireGuard node-to-node; the app sees plaintext `ws://` | shipped (M: Mac⇄Omarchy live) |
| **tailscale-serve** | `127.0.0.1:PORT` behind `tailscale serve --tcp PORT` | WebSocket carried over a tailnet TCP forward | tailnet peers the policy admits on the serve port, plus local processes | bearer token; the node key authenticates the machine | WireGuard; backend hop is loopback | composed (D: plan + guard; M: pending a real peer, below) |
| **ssh-tunnel** | far gateway on `127.0.0.1`; near end `127.0.0.1:LOCAL` | WebSocket inside `ssh -L` | whoever can log in to the far host; every local user at the near end | SSH keys (or Tailscale SSH), plus the bearer token | SSH | composed (D: plan; M: the Flow route uses ssh today) |
| **remote-authenticated-endpoint** | a routable address | WebSocket | anyone who can route to it | bearer token only | **none** unless TLS is terminated in front | refused by default (`--allow-wide-bind`); `wss://` is not spoken by the carrier |
| **tailscale-funnel** | — | — | the whole internet, through Tailscale ingress | none from Tailscale | TLS on the node | **never configured or assumed**; `doctor` fails if a gateway port is funnelled |

### What each mode is not

- **Not a new agent.** A client of `private-tailnet` and a client of `local-ipc`
  ask the *same* gateway; the Communique they send is attributed from the
  sender's own occupancy either way.
- **Not a trust upgrade.** A token over `ws://` inside a WireGuard tunnel is
  confidential because of the tunnel. The same token over a routable `ws://`
  endpoint is cleartext; install refuses that bind unless told otherwise.
- **Not an identity.** `Tailscale-User-Login` headers exist only on HTTP
  served through Tailscale Serve, are absent for tagged nodes and for Funnel,
  and are forgeable by any local process that can reach a loopback backend. The
  gateway does not read them. Its peer check is the bearer token.

## Private service versus public exposure

These are different acts with different risks and are kept apart in code and
commands.

| | Tailscale Serve | Tailscale Funnel |
|---|---|---|
| Audience | the tailnet, as its policy admits | the public internet |
| Proxy ports | any | 443, 8443, 10000 only, TLS only |
| Identity | node key; identity headers on HTTP | none; the application must authenticate everything |
| Safe default here | yes | **never**; requires an owner decision and `tailscale funnel` by hand |
| Shares state with the other | **yes**: Serve and Funnel are two *access levels of one port mapping*; a `funnel` command on a port that already has a private Serve handler makes that handler public |

Two consequences the tooling enforces:

1. `aikit gateway setup --mode tailscale-serve` refuses a port that already has
   a Serve mapping (it would rewrite someone's handler, and a later `funnel`
   would publish it), and verifies the result reads `(tailnet only)`.
2. `aikit gateway doctor` reads `tailscale funnel status` (read-only) and fails
   loudly if any funnelled port points at a gateway port.

Observed on the Mac on 30 September 2026 (read-only): Tailscale 1.102.4
(Standalone), Serve enabled, Funnel not enabled, but the node already carries
the `funnel` attribute (`funnel-ports=443,8443,10000`) — one command from
public. An existing Serve mapping (`/ → http://127.0.0.1:18790`, unowned and
dangling) occupies port 443. Nothing in this work touched either.

### Listener binding: loopback + Serve, or the tailnet address directly

| | `127.0.0.1` + `tailscale serve --tcp` | bind `100.x` directly |
|---|---|---|
| Peer address the gateway sees | always `127.0.0.1` | the real tailnet address (`tailscale whois` maps it) |
| If Tailscale is down at boot or wake | the listener binds and waits; the tailnet entry is simply unreachable (fails closed) | `bind()` fails (`EADDRNOTAVAIL`); the service **waits and retries** — the Unix carrier keeps serving, `status` says `waiting` |
| macOS application firewall | the allowance belongs to the Tailscale process, which is stable | belongs to the `aikit` binary, which is re-signed on every update and loses it |
| What could go wrong | any local process reaches the loopback backend | an address that changes is a bind to a dead address |

The firewall row is why `tailscale-serve` exists as a mode. Every `aikit`
update produces a new ad-hoc-signed binary, and the macOS application firewall
queues inbound connections to an unapproved binary until the owner allows it.
Loopback behind Serve moves the listener off that binary. `doctor` reads the
firewall state (read-only) and names the exact allow command for the owner to
run; it never runs `sudo`.

## Lifecycle modes

| Lifecycle | Started and kept by | Restarts itself? | `upgrade apply` |
|---|---|---|---|
| **foreground** | you, in a terminal (`aikit gateway serve`) | no | installs, **does not stop it**, names the command that starts the new build |
| **supervised-launchd** | a LaunchAgent, `KeepAlive`, `ExitTimeOut 60` | yes | drains, exits, the agent starts the new build, verified |
| **supervised-systemd** | a systemd user unit, `Restart=always`, `TimeoutStopSec=60` | yes | same |
| **application** | O:I or the desktop, which owns the process | the application does | installs and **leaves it running**; the receipt says to restart it from the application |

The lifecycle is reported by the running process itself (`status.build.lifecycle`).
A service definition declares it in `AIKIT_GATEWAY_LIFECYCLE`; launchd and
systemd markers are read as a fallback.

## Carrier scope

| Command class | `peer` (ordinary token) | `owner` (owner token or Unix socket) |
|---|---|---|
| protocol, status, discover, ecology | yes | yes |
| send, inbox, ack, conversation, read, escalate, counts | yes | yes |
| relay: ingest, forward queue, standing, occupancy read/list | yes | yes |
| bind, unbind, ingest (conversation), replay, subscribe, conversation control | no | yes |
| drain, shutdown, snapshot, restore | no | yes |
| anything added later | **no** until named | yes |

| Command class | `peer` (ordinary token) | `owner` (owner token or Unix socket) |
|---|---|---|
| protocol, status, discover, ecology | yes | yes |
| send, inbox, ack, conversation, read, escalate, counts | yes | yes |
| relay: ingest, forward queue, standing, occupancy read/list | yes | yes |
| bind, unbind, ingest (conversation), replay, subscribe, conversation control | no | yes |
| drain, shutdown, snapshot, restore | no | yes |
| anything added later | **no** until named | yes |

One shared token used to grant the whole second column to every machine that
could relay. A peer that answers `agency_gateway.carrier_scope_denied` is told
which token to use.

The administrator path at a distance (`#481-7`): `--at <workcell> --owner
<token-location>` presents the OWNER token for that one invocation — consent
is the flag, the location is resolved through the same owner-only law as any
credential, and the answer warns that it ran with owner scope. Without the
flag, `--at` keeps presenting the declared peer token.

## Feature negotiation

A gateway advertises what it supports in `protocol.features`, and — since
`gateway-build-identity` — which build and process answered. A peer asks for
the feature it needs *before* it uses the command behind it:

| Feature | Means |
|---|---|
| `communique-exact-instance` | keeps a Communique's `to_instance` binding |
| `gateway-build-identity` | `protocol`/`status` carry `build` (revision, pid, start time, executable digest, lifecycle) |
| `gateway-drain` | understands `drain` |
| `gateway-carrier-scope` | distinguishes owner and peer carriers |
| `gateway-unsupported-command` | answers an unknown command as `unsupported_command`, naming what it supports |
| `gateway-configured-identity` | the configured gateway ref outranks the one a snapshot was saved under |

A mixed-version pair degrades on the one thing that is missing, named, instead
of failing on an unknown command. `aikit gateway upgrade plan` lists each
declared peer's build and missing features.

## Hermes: what the specimen actually ships

Compared 30 September 2026. Installed: Hermes Agent v0.21.1 (2026.9.7),
checkout `20f7ef4df5e1` (2026-09-11, 880 commits past the `v2026.9.7` tag,
106 before `v2026.9.11`). Upstream `main` `7239625ae1b7` (2026-09-30), 12,928
commits ahead; latest release v0.21.5 (`v2026.9.24`). Live documentation read
through a summarising fetch on the same date; its content matched the upstream
docs source at the pinned revision for every item compared.

| Behaviour | Shipped in the installed version | Upstream only (newer) | Proposal / undocumented | O:I |
|---|---|---|---|---|
| Restart requested *from inside* the gateway | `/restart` and `/update` spawn a detached worker; the result is recorded **before** the disruptive step; the next gateway process claims the pending marker and reports | `--no-gateway-restart` (updater splits from restart) | the marker protocol is undocumented and version-volatile | a durable transaction + a worker under the service manager; the new gateway re-adopts an orphan |
| Exit-code contract | 75 = restart me, 78 = fatal, 0 = planned | launchd `KeepAlive.SuccessfulExit=false` | — | supervisor restarts on any exit; drain answers first |
| Drain | refuse new turns at once, wait up to 1800 s, then interrupt with `resume_pending` | live progress lines | — | bounded grace (default 60 s), then interrupt and **record**; no auto-resume of interrupted turns |
| Session continuity | `resume_pending` + auto-resume of fresh sessions; queued text flushed to disk and appended to the transcript | — | — | interrupted turns are named in the receipt and **not** re-run; pending operations are retained |
| Egress | delivery ledger, at-least-once, 3 attempts, 24 h | — | — | unreceipted operations are retained, never blindly re-sent |
| Client auth / remote | API server key mandatory, loopback default; Desktop SSH tunnels | host-wide singleton | Tailscale is docs-only guidance, no code | owner/peer token scopes; tailnet bind; Serve and SSH are composed modes |
| Topology | one gateway per profile | multiplexer is the default upstream | `gateway.standalone` is "a temporary compatibility shim" | one gateway per Workcell; placement is a separate axis |
| Doc/code discrepancies | live docs say launchd uses `SuccessfulExit=false`; the installed plist uses `KeepAlive true`. Docs describe `--no-gateway-restart`; the installed parser has none. Docs promise a 5–15 s outage; installed restarts are drain-first | | | not carried |

What O:I does **not** copy: Hermes's unconditional restart of every gateway
after an update, and at-least-once redelivery of anything a model may have
acted on. An O:I upgrade restarts one named process and never re-executes
uncertain work.

## Not yet demonstrated

- A WebSocket upgrade through `tailscale serve --tcp` has not been exercised
  against a real peer; the source suggests a raw TCP forward carries it
  unchanged, but it is not claimed until a probe has run.
- `tailscale serve` on Omarchy: it has no Serve configuration; a kernel
  `tailscale0` interface exists, so a direct tailnet bind works there today.
- The tailnet policy (ACLs/grants, `nodeAttrs`) could not be read from either
  machine; "restrictive" is not assumed.
