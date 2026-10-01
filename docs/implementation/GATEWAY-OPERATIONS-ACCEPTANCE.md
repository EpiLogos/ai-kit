# Gateway operations — parent acceptance record

Commission: *Gateway operational architecture and upgrade lane*, under
EpiLogos/O-I#154 / #220 / #65, EpiLogos/ai-kit#275 and EpiLogos/Workcell#72.
Source basis: ai-kit `64d4e1d12fd9` (main when the lane began), O:I
`17c22891e6c9`, Hermes installed v0.21.1 (`20f7ef4df5e1`) against upstream
`7239625ae1b7`. This record holds the acceptance object for the whole lane. A
slice that lands advances it; it does not replace it, and a requirement is
closed only at the evidence level it names.

Evidence levels: **D** deterministic test, **R** real-binary test (the real
`aikit`, real gateway processes, a scripted supervisor and installer), **M**
the real service manager on a real machine (controlled instance), **I** the
installed service of this machine, **V** independent verification by a session
that did not build the slice. A row says what was *executed*; "pending" is
not a level.

## Requirements

| # | Required behaviour | Native owner | Implementation | Test | Evidence now | Open |
|---|---|---|---|---|---|---|
| 1 | The commissioned sources are read, not recalled | — | the research record (Hermes, Tailscale, route, update lifecycle) | `GATEWAY-OPERATING-MODES.md` (Hermes table, crosswalk), this file | read; classified shipped / upstream / claim | — |
| 2 | Installed Hermes compared with upstream at a pinned revision | ai-kit docs | `GATEWAY-OPERATING-MODES.md`, `GATEWAY-CONNECTOR-MATRIX.md` § operational parity | doc-parity test (`gateway_modes`) | D (parity), source-read | upstream read through a summarising fetch; named as such |
| 3 | An ordinary, discoverable way to configure, run, inspect, upgrade and recover the gateway on one machine and across two | ai-kit | `gateway doctor / modes / setup / upgrade / recover`, `remote add` probe | `gateway_doctor`, `gateway_modes`, `gateway_recover` unit tests; `gateway_upgrade_native` | D, R | I and M pending (rows 9, 11); desktop System/Gateway consumer not built — the native `--json` readings are the Actions it will call |
| 4 | Operating-mode crosswalk: local IPC; loopback service; private tailnet; supported SSH/tunnel; explicit remote authenticated endpoint; foreground / supervised / application-managed; Serve and Funnel distinct | ai-kit | `gateway_modes.rs` (`ENTRY_MODES`, `LIFECYCLES`, `AXES`, `classify_endpoint`, `plan_setup`/`apply_setup`), `gateway_posture.rs` | doc-parity test (every mode, lifecycle and axis named in the doc is in the table and the reverse); setup refusals (mapped/funnelled port, remote endpoint, Funnel) | D | Serve front verified on a controlled port only with the owner's consent to a Serve change; WebSocket-through-Serve unverified |
| 5 | Listener binding, transport, Workcell placement, connector identity and session continuity kept separate; a connection mode creates no second agent/conversation model | ai-kit | `ListenerClass`, `CarrierScope`, the five axes in `GATEWAY-OPERATING-MODES.md` | `gateway_posture` unit tests; carrier-scope test `a_peer_token_relays_and_reads_but_only_the_owner_token_can_stop_the_gateway` | D | — |
| 6 | The one native route traced (admitted message → queue → exact recipient/occurrence → turn → response → Flow/connector Return) and its defects repaired | ai-kit | Communique A→B→A fix; process-reported identity; configured-ref precedence; scope-checked carrier; one Flow carrier (`EncounterRelay`); hardened legacy ssh route | engine/contact suites; `gateway_encounter_relay` tests; flap regression | D | **three journals are still three** (Communique, connector conversation, Flow Encounter) — named in `GATEWAY-BOT-TO-BOT.md`; four routing deciders in `gateway_contact.rs` (`route_to_occupancy`, `place_instance`, `forward_pass_via`, `relay_attempt`) not merged; connector plane has no durable outbound queue or attempt marker; the sender copy never learns remote delivery |
| 7 | Agent-identity addressability: an unembodied registered agent remains addressable; durable-address succession proved separately from exact generation and required-Workcell targeting | ai-kit | existing (`a_registered_profile_without_a_position_is_addressable…`, `a_durable_route_follows_succession_and_an_exact_route_refuses_the_successor`, `a_required_workcell_mismatch_is_held…`) re-run on the lane tree | `gateway_contact` suite | D (19 tests, this tree) | no new behaviour; not re-proved across the remote Flow route (row 11) |
| 8 | Setup, status and recovery reachable by CLI, TUI/desktop and connector commands, keeping streaming, stop/restart, group bounds, attachments, return-to-origin | ai-kit | `/upgrade`, `/upgrade apply` (group refused), `Announce`, TUI operation names | `upgrade_is_planned_on_request_started_only_by_apply_and_never_from_a_group`; tui suite; real-binary `…reported_back_into_it` | D, R | the desktop surface itself is not built; streaming, attachments and return-to-origin are the existing engine paths, re-run green, not re-proved here |
| 9 | Upgrade is a complete native lifecycle: inspect/plan → choose candidate → recovery basis → managed install → drain/restart/rebind → running-version verification → resumed conversation → visible receipt; including one requested through the gateway itself; exact pending work and uncertain effects retained | ai-kit (driver), O:I (`oi update`) | `gateway_upgrade.rs`, `gateway_upgrade_system.rs`; O:I `update_flow.rs` two-phase apply, direction, resident readings | scripted driver suite (14); real-binary suite: stale→upgrade→verify, install fails, flip-then-fail rollback, broken new build rolled back, foreground left running, SIGTERM drain; O:I 23 tests; **real launchd on this Mac, controlled instance** (see below) | D, R, M (launchd on the Mac, systemd on Omarchy), I (both machines) | — |
| 10 | Terminal loss, failed build, service restart, mixed peer versions, one machine offline, retry, supported rollback exercised; no blind replay of uncertain model/tool effects; no silent downgrade of exact routing | ai-kit | worker detached from the gateway; adoption of orphans; drain records uncertain effects; per-feature refusal | worker-death and resume (D); rollback (R); mixed versions (`doctor` peer reading, D); exact route refuses a successor (D) | D, R | one-machine-offline and mixed-version across the two real machines pending (row 11) |
| 11 | Usable operating modes; one easy managed upgrade route; **actual new-running-version proof on both machines**; **native remote Flow delivery** | ai-kit, O:I, Workcell | all of the above | the managed install and `gateway upgrade apply` on both real services; the Flow driver's gateway route (below) | running version: **I on both machines**. Remote Flow delivery: **R/I with a controlled body** — two requests, replies included once | a real model body; the real Omarchy gateway as relay target; the reverse direction; independent verification |
| 12 | Parity map, native tests and operator guidance updated | ai-kit docs | `GATEWAY-CONNECTOR-MATRIX.md` (operational parity), `GATEWAY-OPERATING-MODES.md`, `GATEWAY-UPGRADE.md`, `GATEWAY-CONTACT-AND-DAY.md`, `GATEWAY-BOT-TO-BOT.md`; O:I `docs/INSTALL-UPDATE-FLOW.md` | doc-parity test | D | O:I `.wayfinder/maps/plural-flow-now.md` section for the remote route after row 11 |

## Executed evidence

Linux (Omarchy, x86_64, debug builds) unless stated; the Mac's disk was too full to rebuild.

| Command | Result |
|---|---|
| `cargo test -p aikit-adapters --lib -- gateway_` | 82 passed |
| `cargo test -p aikit-cli --lib -- gateway_ encounter_conversation` | 72 passed |
| `cargo test -p aikit-cli --test gateway_upgrade_native` — **each test as its own process, two at a time** (what nextest does) | 9 of 9 exit 0: upgrade + verify, install fails, flip-then-fail rollback, broken new build rolled back, **a restart that brings up the old image is not reported as the upgrade**, foreground left running, SIGTERM drain, **an install that cannot fit is refused first**, **an upgrade asked for in a conversation survives the restart and the receipt returns to it** |
| `cargo test -p aikit-cli --test gateway_conversation_engine` | 29 passed, 1 ignored |
| `cargo test -p aikit-cli --test gateway_contact --test gateway_command --test gateway_connector_runtime` | 8 + 8 + 19 passed |
| `cargo test -p aikit-tui --test gateway_conversation_v2` | 15 passed |
| `cargo clippy --locked -p aikit-adapters -p aikit-cli -p aikit-tui --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| CI on the merged PR (#477) | all 22 checks passed (Linux and macOS) |

Defects the executed tests found in *this lane's own work* before and after
the first merge, all repaired with the test that found them kept (none weakened):
the upgrade fixture invoked the gateway through a link named `current` (the
binary dispatches on its own name — nothing started); the doctor judged a
gateway "stale" in the seconds before the new process had read its own
executable digest (comparison is now three-valued and verification waits for the
digest); the scripted installer overwrote its previous-build pointer with the
binary; an application-managed gateway was drained although nothing would
restart it; plist argument parsing in the doctor; under nextest, shared binary
copies rewritten while another process executed them (`ETXTBSY`); a
`stage_and_link` left unused outside tests (O:I clippy).

## Real service manager: launchd, this Mac (controlled instance)

`scripts/rehearse.py` (scratchpad of this lane) installs a **controlled
instance** `ai.aikit.gateway.rh34315` with its own `AIKIT_HOME`, socket and
definition — the real `ai.aikit.gateway` (pid 30455) is untouched — with the
debug build of this tree, a scripted managed installer and two byte-distinct
builds, and drives `aikit gateway upgrade apply` against launchd itself. Every
scenario ended as expected; the instance and its worker jobs were removed
(`launchctl list` afterwards shows only the real service).

| Scenario | Outcome (from the receipt) | Process |
|---|---|---|
| install, drain, restart, verify | `completed`: "now running fffffffff0ff… (pid 72664); was 64d4e1d12fd9… (pid 38952). 0 turns finished, 0 interrupted, 0 unreceipted operations; nothing was replayed" | launchd restarted the service onto a different pid and image; the Communique journalled before the upgrade was read back afterwards |
| already current | `no-change`: nothing drained | same pid |
| installer fails, build unchanged | `failed-before-change`: "the installed gateway build is unchanged and the running gateway was not touched" | same pid |
| installer flips the build, then fails | `rolled-back` through the installer's own rollback | same pid (restored before any drain) |
| new build exits at once | `rolled-back`: "the previous build is running again" | new pid on the previous image |

The worker ran as its own one-shot launchd job (`ai.aikit.gateway-upgrade.<id>`)
and left no definition behind (`leftover_worker_definitions: []`); `gateway
doctor` ended at `info`, not `fail`. The first scenario took 285 s because a
debug binary on a machine at load ~50 reads its own digest slowly and the driver
waits for the digest (release builds read it in well under a second).

## Real service managers: systemd, Omarchy (controlled instance)

The same rehearsal against `systemd --user` on Omarchy (Linux x86_64), with the
**installed release build** `6e452a600a4c`: controlled instance
`aikit-gateway-rh2291351.service` (the real `aikit-gateway.service` untouched),
workers as transient units (`systemd-run`). All five scenarios ended as
expected — `completed` (9.6 s, new pid, image changed, Communique read back),
`no-change`, `failed-before-change`, `rolled-back` (installer flipped then
failed), `rolled-back` (new build exits at once: new pid on the previous image,
144 s with the default 90 s wait for the manager) — `leftover_worker_definitions:
[]`, doctor verdict `info`.

## The installed services, both machines (I)

`oi update --apply --candidate ai-kit=6e452a600a4c…` (the managed release build
of this branch's commit) on each machine, then `aikit gateway upgrade apply
--wait` against the **real** service, each from the previous build (`aikit
gateway upgrade plan` first, read as data):

| | Mac (`workcell:mac`, launchd) | Omarchy (`workcell:omarchy`, systemd) |
|---|---|---|
| running before | pid 30455, build unreported (predates identity): stopped through its clean shutdown | pid 1697929, same |
| transaction | `upg-01m3vnczax54…` `completed` | `upg-01m3vz7w916g…` `completed` |
| running after | **pid 47288, revision `6e452a600a4c`, sha256 `7b62a599a736…`, `supervised-launchd`** | **pid 2348420, revision `6e452a600a4c`, sha256 `367d0a2a66b5…`, `supervised-systemd`** |
| what was in flight | **unknown, not zero**: both predecessors predate the drain and were stopped through their clean shutdown, so no drain counted anything. The receipts (written before this was caught) say "0 interrupted"; that was a default report, not a measurement (repaired below). Nothing was replayed: nothing in the code replays | same |
| gateway ref | `agency-gateway/mac` (was answering as `agency-gateway/local`: the identity drift the research found, repaired by the configured ref winning) | `agency-gateway/omarchy` |
| doctor after | `gateway.current`, `listener.private`; remaining: peer features (until Omarchy was upgraded), lifecycle undeclared (the service definition predates it) | `gateway.current`, `peer.ok` — "peer workcell:mac answers and runs the same build" |

While the Mac ran the new build and Omarchy still the old one, the plan and the
doctor named the gap exactly (the peer's six missing features) instead of
failing. After the Mac's restart the application firewall had no allowance for
the new binary and the doctor said so with the owner command; within minutes
macOS had listed the binary and Omarchy reached the Mac gateway (`aikit gateway
--at workcell:mac protocol` answered with the new identity). No `sudo` was run.

## Native remote Flow delivery (Mac → Omarchy over the gateway relay)

`desktop/cradle/tests/plural-flow-acceptance.mjs` (O:I) with `PF_ROUTE=gateway`,
against the **installed** `6e452a600a4c` on both machines, real owners, Central's
real `ctrl` with `central.flow.*`, and the real Actuation owner minting each
agent's Agency. The agent body is the **controlled ACP fixture**
(`crates/aikit-cli/tests/fixtures/conversation_provider.py`): its replies are
protocol fixtures derived from the asked entry, never model output — this run
proves the route and the owners, not a model.

Setup, as the driver performs it: a disposable world on each machine; on
Omarchy a **controlled gateway instance** (`aikit-gateway-pflab.service`, port
7790, own `AIKIT_HOME`, distinct peer and owner tokens; the real gateway there
untouched) serving that world's owner; on the Mac a declared remote
`workcell:omarchy` for it (probed: an endpoint that answers as another Workcell
is refused). The participant on the other Workcell routes as
`{kind:"gateway", workcell:"workcell:omarchy"}`.

**ssh is broken before each send and stays broken** (the owner's `ssh` is a shim
that fails while a flag file exists; the flag existed before the first request
and after the last). So a delivery that lands did not use ssh. Then, while the
remote agent works, the Omarchy gateway is **stopped for 30 s and started again**
(real systemd, `SIGTERM`): the remote recipient stays `unknown`/`returned` until
the gateway is back, then `included`.

| Request | Ada (this machine) | Ash (Omarchy) |
|---|---|---|
| `conversation/pf-remote-1790883626688` | included, 1 attempt | dispatch `sent`, `included`, 1 attempt; entry `verified from agent-session/pf-ash-o`, `reply → 1@r7` |
| `conversation/pf-remote-1790883721564` | included | `included`; `verified`, `reply → 4@r10` |

On Omarchy: the body received **2 `session/prompt`** in total — one per request,
never replayed across the outage; the lab gateway's journal shows the two
outages exactly (`19:40:29 gateway drained for stop: 0 turns, 0 operations …
stopped cleanly` → started `19:41:00`; `19:42:04` → `19:42:34`). The Flow has 6
entries: each recipient's reply appears once per request, attributed from its
own session. The scratch worlds, the lab gateway instance and both owners were
shut down and removed afterwards (`aikit-gateway.service`, the real one, was
never stopped).

What this does not show: a model body; the real Omarchy gateway as the relay
target (a controlled instance served, to leave the real one undisturbed); the
reverse direction (Omarchy asking a Mac agent); a person using the desktop.

## Independent verification (a session that did not build it)

A fresh verifier read the commission and this record, exercised the usable path
read-only on both real services, re-ran two real-binary failure/recovery tests,
and read the code adversarially. **Verdict: not yet a usable end-to-end feature
as the commission is worded.** What it reproduced: both real services' pid,
revision and digest-against-the-file match this record; a peer token is refused
`snapshot` on the real Omarchy gateway (`carrier_scope_denied`); the broken-build
and flip-then-fail rollbacks pass; throwaway-home plans refuse
`remote-authenticated-endpoint` and `tailscale-funnel`. What it found, and where
each is now:

| Finding | State |
|---|---|
| Both real receipts said "0 interrupted … nothing was replayed" from a default drain report: the predecessor predated the drain, so the count was **unknown** | **Repaired**: `DrainReport.measured`; the step note, summary, receipt JSON and markdown say "not measured … unknown, not zero" when no drain ran; unit test `a_predecessor_without_a_drain_is_reported_unmeasured_never_as_zero_interrupted`. (The two real receipts above were written before this and are corrected in the table.) |
| `/upgrade apply` was tested only with a stub launcher | **A real defect found by writing the real test**: the serve path left `upgrade_launcher: None`, so on a real service `/upgrade apply` could never start an upgrade. Wired. New real-binary test: a real out-of-process connector, a real detached worker, the restart, and the receipt announced into the same conversation once (`an_upgrade_asked_for_in_a_conversation_survives_the_restart_and_is_reported_back_into_it`) |
| The same-pid check in `verify` and the "expected image" check could not be failed by any real-binary test | **Repaired**: unit test `the_old_process_still_answering_is_never_verified_as_the_new_one`; real-binary test `a_restart_that_brings_up_the_old_image_is_never_reported_as_the_upgrade` (a supervisor pinned to the old file: a new pid on the old image ends `rolled-back`, never `completed`) |
| Nothing pinned that a peer cannot `drain` | **Repaired**: the scope test now sends `drain` over the peer carrier and expects `carrier_scope_denied` |
| macOS reads the executable digest by path in the background (an in-place overwrite could make a stale resident look current) | **Repaired**: the executable is opened at start and the digest read from that handle (`sha256_of_open_file`); test: the digest of an open handle survives a rename-swap of the path |
| `AIKIT_HOME` does not isolate the service: `setup` from a throwaway home planned to replace the real service | **Repaired**: setup refuses (`gateway.setup_other_homes_service`) when the installed definition serves another home; the doctor says `service.serves_other_home` instead of "not answering" |
| No free-disk preflight (the Mac was at 100% while the verifier ran) | **Repaired**: `doctor` reports `disk.low` (fail under 512 MiB, warn under 5 GiB); `upgrade apply --install` refuses before anything changes under 3 GiB (`gateway_upgrade.disk_low`; `AIKIT_INSTALL_MIN_FREE_MIB` adjusts it). The refusal fired by itself on a 785 MiB tmpfs during testing; test `an_install_that_cannot_fit_is_refused_before_anything_is_changed` |
| Recovery preferred the older copy over a newer decodable one | **Repaired**: newest by modification time; test |
| `choose()` swallowed an unreadable endpoint registry and fell back to ssh | **Repaired**: an unreadable registry holds the request with the reason; test |
| `rehearse.py` lived only in a scratchpad | **Repaired**: `scripts/gateway-upgrade-rehearse.py` |
| Dangling `--restart-only` hint | **Repaired**: the flag exists; and `--candidate <rev>` makes "choose the candidate" a first-class step of `plan`/`apply` |
| Found while testing (not by the verifier): a receipt announced while the connector was not ready was dropped with a note on stderr and the transaction said "delivered" | **Repaired**: an announcement that cannot be queued is an error the driver sees and the tick retries (`an_announcement_that_cannot_be_queued_is_an_error_the_caller_sees…`) |
| Found while testing: a gateway merely slow to answer at the start (loaded machine) was read as "not running", which would skip the drain and start a second gateway | **Repaired**: the first reading tells "nothing listening" from "did not answer in time" (retried); the latter ends `failed-before-change` with nothing touched; test |
| Found by the first real use of the repaired build (Omarchy, `6e452a60` → `0bfe6e4c`, transaction `upg-01m3wqzc…`): the receipt said "the predecessor predates the drain … unknown", but `6e452a60` *has* the drain and it ran. Its report carries no `measured` field (the flag is newer), so it deserialised as `false` | **Repaired**: `DrainReport::was_measured()` — the flag, *or* the times of a drain that ran (a never-run default report has none); unit test `a_drain_report_from_a_gateway_that_predates_the_measured_flag_still_reads_as_measured`. That transaction's receipt keeps its (understated) wording; the upgrade itself was correct (`completed`, new pid, new image) |
| Pending outbound operations stay pending forever; the Communique planes are still three journals; `/upgrade` through a live Telegram/Slack chat; in-flight turn drained through the carrier with a live harness; Serve/ssh-tunnel exercised; owner-scope admin of a remote gateway | **Carried, with owners and closing conditions, in EpiLogos/ai-kit#481** |
| A real `oi update --rollback` and a real (not scripted) `oi` install-then-restart in one transaction were never exercised | **Open in this record** — run on Omarchy against the merged cut (below) |

## Defects found by the lane's own gates after the first push

* **Linux CI (`V2 crate — aikit-cli`)**: `a_foreground_gateway_is_installed_…`
  failed with `ETXTBSY` — under nextest every test is its own process, and the
  fixture's shared copies of the binary were being rewritten by one process while
  another executed them. The fixture is now per-machine (build A is a link to the
  binary under test; build B is made only by the three tests that install it,
  inside that machine's own directory). Verified by running all six tests at once
  as six processes on the Mac (all exit 0).
* **O:I CI (`rust`, `native`)**: the two-phase apply left `stage_and_link` unused
  outside tests, which `clippy -D warnings` rejects; it is test-only now.

## Not shown

* A real `oi update --rollback` and a real `oi` install-then-restart in one
  transaction (the tests and rehearsals use a scripted installer).
* `/upgrade apply` in a live Telegram/Slack conversation on a real service; a
  real in-flight turn drained through the carrier (ai-kit#481).
* A Flow request delivered to another Workcell with a real model body, with the
  real Omarchy gateway as relay target, or in the reverse direction.
* A Tailscale Serve front on a controlled port (needs the owner's consent to a
  Serve change); Funnel is never configured.
* An owner-scope administrator token on a *remote* gateway (`--at` uses the
  peer token: relay and read, never stop, drain or restore).
* A second independent verification of the repaired tree.

## Owner-only steps this lane will not take

* macOS application firewall allowance for a newly installed `aikit` binary
  needs `sudo`; `gateway doctor` names the exact command.
* Any Tailscale Funnel; any Serve mapping change (the existing `:443` mapping is
  not this lane's).
