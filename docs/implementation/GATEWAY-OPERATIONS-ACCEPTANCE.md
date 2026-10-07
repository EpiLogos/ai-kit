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
| A real `oi update --rollback` and a real (not scripted) `oi` install-then-restart in one transaction were never exercised | **Exercised on the real Omarchy service** (section below). It also showed that a rollback the operator asked for was reported as "the new build did not come up"; the receipt now says "rolled back at the operator's request" (`rollback_requested`; unit test) |

## Second independent verification (a second session that did not build it)

A second fresh verifier took the tree after #482/#485, re-derived the real
services' state, ran probes and two deliberate **mutations** (one in `verify()`, one
in the serve wiring — both made the named real-binary tests fail, so those tests do
guard the connection), and read the code adversarially. **Verdict: still not a
usable end-to-end feature as the commission is worded**, for the reasons in the
last rows. Its findings, and where each is now:

| Finding | State |
|---|---|
| **N1** The doctor's firewall reading was wrong, and my #485 fix was vacuous: `socketfilterfw --getappblocked` answers "is permitted" for *any* path (`/bin/ls`, a path that does not exist) | **Repaired** (merged): the finding is an Info that says only what the list can show, and names the real test — a peer running `aikit gateway --at workcell:<this> protocol` |
| **N2** A drain whose reply was lost (the turn interrupted, the connection gone with the process) was reported as "nothing was in flight to drain; nothing was replayed", `uncertain_effects` null | **Repaired**: the lost reply is stored as an *unmeasured* report with the reason; the receipt says "unknown, not zero"; an old process that ended before a drain with no report says the same; "nothing in flight" is stated only when no gateway was running. Tests: `a_drain_whose_reply_was_lost_is_unknown_not_nothing_in_flight`, `a_process_that_ended_before_the_drain_was_reached_leaves_no_report_and_the_receipt_says_unknown` |
| **N3** Lenient reads took a slow gateway for an ended one in `drain()` and `restart()` | **Repaired**: both use the strict reading (`read_running_strict`: "nothing listening" is absent; "held the socket and did not answer in time" is `gateway_upgrade.gateway_unresponsive`). A drain that finds one ends `needs-operator`; a restart treats it as still there and never starts a second gateway beside it. Tests: `a_gateway_that_does_not_answer_at_the_drain_is_not_taken_for_gone`, `the_old_process_still_answering_is_never_verified_as_the_new_one` |
| **N4** A step that could not run (an unreadable state file) left the transaction `planned`, blocking every later `apply`, with no recorded cause and nothing to clear it | **Repaired**: the failure is **recorded** as a step and a transaction that had changed nothing ends `failed-before-change`; `aikit gateway upgrade abandon [ID] --reason …` ends one whose worker is gone (refused while a worker holds it; changes nothing on disk or in the gateway). Tests: `a_step_that_cannot_run_ends_a_transaction_that_changed_nothing_and_a_new_apply_can_start`, `an_operator_can_abandon_a_transaction_nothing_is_driving_and_it_changes_nothing` |
| **N5** `receipt_delivered` means queued, not delivered; the crash window between announce and save gives at-least-once | **Carried** (ai-kit#481: the durable outbound queue is the closing condition) |
| **N6** Messages admitted while a drain runs are unserved and not named in the `DrainReport` | **Carried** (ai-kit#481, with N5) |
| **N7** The "named" ssh downgrade was not named: `Choice::Legacy(_) => {}` discarded the reason | **Repaired**: the owner's log says `encounter route: <why>; using the ssh route` |
| **N8** `rollback <id>` was documented as restoring *any* recorded upgrade; `oi update --rollback` restores only the installer's latest previous build, while verification expects that transaction's own `installed_before` | **Repaired**: rollback is for the **latest** upgrade only and refuses an older id naming the latest (`gateway_upgrade.rollback_not_latest`; test `only_the_latest_upgrade_can_be_rolled_back`); the documentation says so |
| **N9** `adopt_orphans` had no test | **Repaired**: `adopt_orphans_with(home, spawn)` is a seam; the adoption rule (a dead worker's nonterminal transaction is adopted once it has been quiet, never while fresh or held) is pinned by `a_dead_workers_transaction_is_adopted_once_it_has_been_quiet_and_never_while_it_is_fresh_or_held` |
| **N10** The two machines run different `oi` builds and neither the plan nor the doctor shows it | **Carried** (ai-kit#481) |
| **N11** No new ssh-injection, peer-escalation or replay hole found | none needed |
| "Asked through the gateway itself" proven only with a process-mode worker and a scripted supervisor | **Exercised under both real service managers**: `scripts/gateway-upgrade-rehearse.py` scenario 6 (`asked-through-the-gateway`) runs a real connector specimen, types `/upgrade apply` into the bound conversation, and the worker the gateway starts is its **own** transient unit (systemd) / one-shot job (launchd), not a child; the restart replaces the process and the receipt is announced into the same conversation **once**. Run on this tree (`origin/main` `12ccb272f46a` + the repair files, release build on the Mac, debug on Omarchy): **all six scenarios ok on launchd (Mac) and on systemd (Omarchy)**. The Mac run's scenario-6 transaction: `upg-01m3ya5r7enb…` pid 5813 → 8202, drain measured (0/0/0), `verified: a different process is running the expected build`, `completed`, `receipt_announced_into_the_conversation: 1`, worker `ai.aikit.gateway-upgrade.01m3ya5r7enb` |
| **Found by the first launchd run of scenario 6 (it failed twice before this was repaired)**: a controlled instance's service definition never carried its own instance name (`AIKIT_GATEWAY_SERVICE_INSTANCE` was read only from the process environment). A gateway started by launchd therefore resolved "the installed build" from the **default** service definition — on that machine, the real service's — and the asked-for upgrade ended `needs-operator` ("expected revision 8c6c48f03996", the real service's executable) while the instance had in fact restarted onto the right image. The real service was never touched | **Repaired**: `ServiceOptions::environment()` writes the instance into the definition (the default service writes none), so a gateway and the workers it starts act on their own definition; test `a_named_instances_definition_carries_its_own_name_and_the_default_service_does_not`. Instances installed before this repair carry no name and are rehearsal instances only |
| The Mac had no real-`oi` `apply --install` transaction (only `--restart-only`) | **Open**: the Mac's installed cut has since been replaced by another lane's, and moving it would disturb that lane; see "Not shown" |

## Third independent verification (a third session that did not build it)

A third fresh verifier took the **published commit** `417d6d72` of #488 (not the
builder's tree), re-ran the suites, mutation-tested each repair on Omarchy, ran the
rehearsal under real systemd with the instance-name fix reverted, and read both real
services without acting on them. **Verdict: #488 is safe to land as a repair.** It
did not call the lane a usable end-to-end feature, and neither does this record.

| Finding | State |
|---|---|
| Suites on the published commit: fmt clean; adapters `gateway_` 83; cli lib 81; engine 29; the nine real-binary upgrade tests each as its own process, all exit 0; clippy `-D warnings` clean; CI 17 pass, 2 skipped (`proof`, `register`) | reproduced |
| **Mutation, instance-name fix** (instance push removed): rehearsal scenario 6 **fails under real systemd** ("installed build is 3ee08a9081c9", the real Omarchy unit's executable) — the same failure as on the Mac. Linux therefore shows the bug; the earlier green Linux scenario-6 run (before the fix) could not be reproduced and is **unexplained** | the guard is the rehearsal; carried: not in CI (#481 item 15) |
| N2, N4: mutation makes the named tests fail | **VERIFIED** |
| N3: only the `drain()` half was pinned — the lenient reading in `restart()` failed no test | **Repaired**: `an_old_process_that_does_not_answer_while_restarting_is_waited_for_and_never_replaced_around`; the test env's lenient reading now maps "did not answer" to "nothing there", as the real one does |
| N8: removing the guard from `rollback_command` failed no test | the guard now runs after the installer-recorded check inside `rollback_command`; `require_latest` is tested for every kind of transaction (below) |
| N9: adopting a fresh finished transaction, or one whose lock is held, failed no test | **Repaired**: both branches pinned in the adoption test |
| N7: removing the log line fails no test | the wording is a function (`legacy_route_notice`) with a test; **the call itself is not pinned** — stated, not hidden |
| **Receipts said "UNKNOWN" for a predecessor that was never stopped** (`no-change`, an install that failed, a drain that found it busy): the unmeasured wording was keyed on "no drain report", not on "the predecessor may have stopped" | **Repaired**: `predecessor_may_have_stopped` (a drain was attempted, a new process answered, or a different pid runs at the end); otherwise the receipt says the previous gateway was never asked to stop. Test `a_gateway_that_was_never_asked_to_stop_is_not_reported_as_unknown_work` |
| **Rollback guard compared transactions, not installs**: after a later restart-only or `no-change` run, `rollback <older install>` was refused naming a run that itself cannot be rolled back | **Repaired**: "latest" is the latest upgrade that left a newly installed build in place (installer ran, install took effect, not undone, not `no-change`); none → `gateway_upgrade.nothing_to_roll_back`. Test `only_the_latest_upgrade_that_changed_the_installed_build_can_be_rolled_back` |
| Disk-preflight wording said "where the install builds"; the code measures the `$HOME` volume (a shared `target-dir` can be elsewhere) | wording corrected; behaviour carried (#481 item 12) |
| `docs/GATEWAY-UPGRADE.md` said "five scenarios" | corrected: six |
| Scenario 6's worker is the machine's installed `aikit` (found by PATH), not the instance's build, so scenario 6 exercises this code on the **gateway side** only | carried (#481 item 13) |
| `apply --wait` can wait up to its bound on a transaction nothing is driving | reasoned, not exercised; carried (#481 item 14) |
| The verifier's own rehearsal ended with doctor `warn` | **Explained** (re-run with the rehearsal now recording the doctor's non-info findings): `disk.low`, 1390 MiB free on the small tmpfs the rehearsal's home lives in — a property of the test machine, not of the gateway |
| The Mac doctor says `fail` `gateway.stale` and the old `firewall.running_not_allowed`: the Mac's installed `aikit` (`8c6c48f03996`) is another lane's cut and predates #485 | an honest reading; the doctor cannot say the installed cut belongs to another lane (not repaired); the Mac is not moved without that lane's cohort |

After the follow-up (Omarchy, real systemd): fmt clean; `aikit-cli` lib 83 passed; adapters 83; engine 29; clippy `-D warnings` clean; the nine real-binary tests exit 0; rehearsal **all six scenarios ok**. Each new test was mutation-checked: the lenient reading in `restart()`, an always-"unknown" receipt, counting a `rolled-back` and counting a `no-change` as the latest install each make the named test fail.

What the verifier could not prove, and neither can this record: the Mac launchd
scenario 6 on the verifier's own hands (no Mac compiler by rule), a release build, a
live chat, a real in-flight turn, a real-`oi` install on the Mac. The Mac launchd run
in this record is the builder's, on the code of `417d6d72`; the follow-up changes
receipt wording, the rollback guard, tests and docs, none of which scenario 6 exercises.

## The real `oi`: install, restart and rollback in one lane (Omarchy, real systemd service)

The commission's lifecycle through the **real managed installer** on a **real
service**, on the repaired build, not a scripted `oi`:

| Step | Command | Result |
|---|---|---|
| install a chosen commit, drain, restart, verify — **one transaction** | `aikit gateway upgrade apply --install --candidate 4e07023f2ed5… --wait` (the CLI plans `oi update --apply --candidate ai-kit=<rev> ai-kit`; `plan` shows that argv first) | `upg-01m3wrp9enx13r685yersaev59` **completed**: the worker ran as its own transient systemd unit; the real `oi update --apply` built the cut (release build, about ten minutes, with the service answering throughout); the running gateway was drained (**measured**: 0 turns finished, 0 interrupted, 0 unreceipted operations; nothing replayed) and the supervisor restarted it: **pid 2544323 `0bfe6e4c70de` → pid 2557622 `4e07023f2ed5`, digest `e0b5de577a6e…`, `supervised-systemd`** |
| the supported rollback | `aikit gateway upgrade rollback upg-01m3wrp9enx13r685yersaev59` (runs the real `oi update --rollback`) | `rolled-back`: the previous cut `0bfe6e4c70de` is the installed one again **and running** (pid 2557953), verified by the process answering as that build |
| before these, the first repaired-build upgrade | `oi update --apply --candidate ai-kit=0bfe6e4c…` then `aikit gateway upgrade apply --restart-only --wait` from `6e452a60` (which has the drain) | `completed` (`6e452a60` pid 2348420 → `0bfe6e4c` pid 2544323); its receipt understated what was known ("predates the drain") — the defect row above |

Other lanes were building on this machine at the same time (`oi update` holds a
lock; my install waited for it rather than racing it). The machine ends on
`0bfe6e4c`, the rolled-back cut.

## Final state: both machines on the merged cut (I)

After #482 merged (`3ee08a90`), each real service was moved onto it — Omarchy
through the real `oi` in one transaction, the Mac by `oi update --apply
--candidate ai-kit=3ee08a90…` and then `aikit gateway upgrade apply --restart-only
--wait` (the Mac's installed CLI at the time had no `--candidate` flag):

| | Mac (launchd) | Omarchy (systemd) |
|---|---|---|
| transaction | `upg-01m3y163c4zn…` `completed` | `upg-01m3xzjaqy7d…` `completed` (`apply --install --candidate 3ee08a90… --wait`, real `oi`) |
| before → after | pid 47288 `6e452a600a4c` → **pid 62095 `3ee08a9081c9`**, digest `462a40671c73…` | pid 2557953 `0bfe6e4c70de` → **pid 2747406 `3ee08a9081c9`**, digest `4e819d5943d4…` |
| drain | **measured**: 0 turns finished, 0 interrupted, 0 unreceipted operations (an idle gateway; a predecessor whose report predates the `measured` flag still reads as measured) | same |
| doctor after | `gateway.current`, `peer.ok` — "peer workcell:omarchy answers and runs the same build" | `gateway.current`, `peer.ok` — "peer workcell:mac answers and runs the same build" |

The two machines run the same build, each says so about the other, and each
reaches the other over the gateway carrier. One doctor reading was misleading:
`firewall.running_not_allowed` (a Warn, with a `sudo` remedy) stayed on the Mac
while Omarchy reached the Mac gateway — macOS admits signed binaries it never
lists, and `socketfilterfw --listapps` cannot say which. A first attempt to ask
`--getappblocked <path>` instead was **wrong**, found by the second independent
verifier: that call answers "is permitted" for *any* path (`/bin/ls`,
`/usr/bin/true`, a path that does not exist), which would have made the check
unable to fire. The doctor now says only what the list can show (an Info: "the
list does not name this binary; macOS may still admit it"), names the real test —
a peer running `aikit gateway --at workcell:<this> protocol` — and gives the
owner's `sudo` command only for the case where that queues or times out.

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

* `/upgrade apply` in a live Telegram/Slack conversation on a real service; a
  real in-flight turn drained through the carrier (ai-kit#481).
* A Flow request delivered to another Workcell with a real model body, with the
  real Omarchy gateway as relay target, or in the reverse direction.
* A Tailscale Serve front on a controlled port (needs the owner's consent to a
  Serve change); Funnel is never configured.
* An owner-scope administrator token on a *remote* gateway (`--at` uses the
  peer token: relay and read, never stop, drain or restore).
* A fourth independent verification of the tree repaired after the third; the Mac launchd rehearsal re-run on the follow-up commit (it needs a Mac release build, which waits for a quiet slot and a memory budget).
* A real-`oi` `apply --install --candidate` transaction on the Mac (only the restart half has been run there).
* The carried N5, N6 and N10 (ai-kit#481).

## The everyday-conversation slice (6 October 2026, branch `feat/gateway-everyday-conversation-20261006`)

Commission: make the existing Omarchy Gateway a complete everyday
agent-conversation service (TUI + Mac + Telegram), closing the applicable
ai-kit#481 items on current main (`1bf1f02a7a20e1892629504c38123a18606b12d5`).
Machine ground truth when the slice began: the real `aikit-gateway.service`
(systemd, supervised-systemd) ran `3ee08a9081c9` (pid 2747406) while the
installed client was another lane's `ea4ff9c63f9f` cut — a live instance of
item 11, reproduced and now named by `plan`/`doctor`.

| #481 item | What closed it | Evidence now |
|---|---|---|
| 2 + 9 (durable outbound attempts; receipt_delivered ≠ delivered) | `OutboundOperation.attempts` / `last_attempt_at_unix_ms` written BEFORE the connector is invoked; a connector (re)connect re-arms its idempotent pending operations and HOLDS outcome-unknown sends; `aikit gateway recover --deliveries` and `--resolve … --state delivered\|abandoned --evidence …` resolve one by evidence (owner carrier) | D: `a_pending_send_is_held_on_reconnect_an_idempotent_operation_is_rearmed_and_attempts_are_durable` (pump) — restored kernel keeps both ops, the send is held and named, the typing pulse is re-queued, the attempt marker survives the restore, and an evidence receipt retires the send |
| 10/N6 (drain admits unserved, unnamed) | `appended()` records admitted-during-drain messages; `DrainReport.admitted_unserved` names each (stream, sequence, conversation, preview); the restart line and the upgrade receipt name them; retained, never replayed | D: `a_message_admitted_during_a_drain_is_named_in_the_report_retained_and_never_served` (engine): the message is in its stream, no agent-message follows, `prompted_turns == 0` |
| 3 (sender copy never learns remote delivery) | relay-pass readback: `CommuniqueFate` over the carrier to the forwarding target; a learned delivery re-stoods the sender copy `delivered` with the remote named in the basis; unknown stays put, named unresolved | D: `the_sender_copy_learns_remote_delivery_on_the_relay_pass_and_unknown_stays_unresolved` (contact): exactly one `RecordRemoteDelivery` on the sender journal, one unresolved entry, two fate asks |
| TUI remote-target failure path | `AIKIT_GATEWAY_AT` set is a commitment: undeclared remote or unusable token resolves to `Absent { reason }` naming the remote — never the local socket; the aperture renders the reason | D: four `conversation_surface` unit tests (commitment, local fallback, token failure, rendered note) |
| 12 (disk preflight reads $HOME's volume) | preflight reads the O:I data root (`OI_DATA_HOME`, XDG, platform default) — the volume the managed installer builds on and installs into | D: `the_managed_install_root_follows_the_data_root_not_the_home`; the live machine builds on `/mnt/hdd` under a `/home` volume — the exact mixed-volume case |
| 13 (worker runs PATH's aikit, not the instance build) | `spawn_worker` uses `service_executable()` (the service definition's named executable); the worker records its own executable as a transaction step; rehearsal scenario 6 reads the step back and fails unless it equals the definition's image | D: step recorded (`worker_command`); rehearsal script extended (`worker_ran_the_definitions_executable` gate). M re-run pending a quiet machine |
| 14 (`--wait` waits out a dead worker) | the wait loop ends early on a quiet, non-terminal transaction whose driver lock is free, with a `nothing-is-driving` finding naming `resume`/`abandon`; a held lock (live worker) keeps the wait honest. `AIKIT_UPGRADE_WAIT_QUIET_SECS` makes it testable | D: `a_quiet_transaction_nothing_is_driving_ends_the_wait_and_a_live_worker_holds_it` |
| 15 (instance-name guard only in the rehearsal) | `environment()` itself is pinned through BOTH rendered definitions: a named instance's plist and systemd unit carry `AIKIT_GATEWAY_SERVICE_INSTANCE`; without the env var neither does the default | D: `environment_writes_the_instance_into_both_rendered_definitions` |
| 11 (installed/running/peer oi versions invisible) | `GatewayBuildIdentity.oi_revision` (read once at process start from `oi --version`); `upgrade plan` carries `installed_oi`, probes carry the peer's; `plan` notes and `doctor` (`peer.oi_revision_differs`) name a mixed-oi fleet | D: unit-level derivation; the live split (installed `ea4ff9c63f9f` vs running `3ee08a9081c9` vs oi `17c22891e6c9`) is the reproduced case the reading names. Peer-side M pending both machines on the new build |

Executed evidence (this slice, Omarchy, debug builds under load): `cargo check`
`-p aikit-adapters -p aikit-cli -p aikit-tui` clean; adapters lib `gateway_` 82
passed / 2 failed (below); TUI `conversation_surface` 5 passed. The Telegram
lane was operated live against the real service:

- **Telegram ingress → real harness turn → delegation → reply (P).** In the
  bound private conversation (chat 6381957258, binding
  `gateway-binding/telegram-frank-private/generation-2`, bot `@Ohisysbot`):
  the owner's message (stream seq 4) was admitted by the running service, the
  pi/GLM-5.3-flash harness turned, delegated to a child pi agent
  non-interactively (setup 0's shared pi capability), which wrote
  `T/findings/telegram-child-finding.md` carrying this lane's exact HEAD
  `1bf1f02a…` and the crate count; the agent's substantive reply (seq 9)
  landed in the same conversation. Exactly one ingress poller throughout (the
  service's connector pump; no getUpdates conflicts).
- **Local TUI conversation (the aperture)**: history, streaming, tool lines,
  permissions/failure states, composition, stop and reconnect hold their
  deterministic proofs (`gateway_conversation_v2`); the remote-commitment law
  is new unit-tested behaviour. A live TUI session against the real gateway is
  recorded in the slice Return when the machine was quiet enough to build the
  binary.
- **Access**: local IPC and the private tailnet were exercised with real
  agents through the existing services (see the slice Return for the exact
  commands); loopback remains deterministic-tests-only; Serve/Funnel stayed
  untouched (no owner consent sought for a Serve change; Funnel never).

Failed/honest rows, resolved: (1) a first defective assertion in the new pump
test, repaired with the test kept; (2) the posture digest test's fixed 60 s
deadline — diagnosed with an in-thread probe (the thread started and hashed
past the deadline on a just-linked 310 MB image, CPU-starved by other lanes)
and repaired by scaling the deadline with the image size, after which the
test passes on the same machine. Items still open with ai-kit#481: the
item-1 convergence (one resolver, one attempt record across all three
journals), item 4 (`/upgrade apply` through a live Telegram/Slack chat on the
REAL service — needs the owner's coordinated upgrade of the running gateway,
which this lane did not disturb), and the receiver-side pin-enforcement
plumbing named under item 8. Items 5, 7 and 8 were closed by this slice
(below).

### Real service manager rehearsal on this slice (controlled instance, real systemd) — final run

`scripts/gateway-upgrade-rehearse.py` with the stamped slice binary
(`27f5c0879ae7…`, stripped, 100 MB, with the specimen connector present),
controlled instance `aikit-gateway-rh*.service`, real transient worker units,
instance and workers removed afterwards (`leftover_worker_definitions: []`,
root cleaned on success). **All six scenarios ended as expected:**

| Scenario | Outcome |
|---|---|
| install-drain-restart-verify | `completed` — receipt carries the new drain naming; pid and image changed; the pre-upgrade Communique survived |
| already-current | `no-change` |
| installer-fails-unchanged | `failed-before-change` |
| installer-flips-then-fails | `rolled-back` |
| broken-new-build | `rolled-back` — the previous image verified running |
| asked-through-the-gateway | accepted → worker under its own transient unit → restart → **receipt announced into the conversation once**; `worker_executable == definition_names == managed/bin-a/aikit`, `worker_ran_the_definitions_executable: true` — the #481-13 gate GREEN from the transaction's own recorded step |

Each transaction's own recorded `worker executable:` step named the instance
definition's build (`managed/bin-a|b`) in every scenario — never a
PATH-resolved `aikit` (#481-13). Doctor ended `warn` on `disk.low` (the
small /tmp rehearsal volume — a machine property, named). Earlier runs of
the same script failed on time budgets (a 562 MB debug binary's own digest
read under machine load 10–15; the lane's documented 285 s case) and on two
script defects (a raw-string regex excluding the letter `n`; a definition
path reconstructed from the bare instance name) — all fixed with the gates
made self-diagnosing, and the run above is the whole rehearsal on the fixed
script.

### The carried #481 items closed later in this slice (same session, second day)

| #481 item | What closed it | Evidence now |
|---|---|---|
| 5 (a real in-flight turn drained through the carrier) | real-binary service test: the serve process runs a real turn (the deterministic ACP fixture as the connector's agent backing); a `Drain` command on the socket interrupts it under a bounded grace and the receipt names the interrupted turn AND the message admitted mid-drain; the restart preserves binding/stream identity with the journal as a superset, re-runs neither, and a fresh message is served on the same conversation | R: `a_real_in_flight_turn_is_drained_through_the_carrier_named_and_never_replayed` |
| 7 (owner-scope administration of a remote gateway) | `aikit gateway --at <workcell> --owner <token-location>` — explicit consent per invocation (owner-only file or secret ref), the carrier presents the owner token, the answer warns it ran with owner scope; without the flag the declared peer token stands | D: `at_carrier_owner_presents_the_named_owner_token_and_refuses_an_unusable_one`, `at_carrier_keeps_presenting_the_declared_peer_token` |
| 8 (a relayed send carries a verified sender attestation) | Ed25519 sender attestation: the relaying gateway signs position/generation/attribution/body-digest/sent-time with a key that lives only in its own home (0600, stable across restarts); `IngestCommunique` carries the proof; the receiver verifies freshness (10 min — stale is a replay), body binding and signature, refuses a bad one, names the attestation in the stored basis; the sender's protocol answer advertises the public key (feature `sender-attestation`) and the verifier honours an operator pin | D: verify/tamper/stale/pin; key stability + owner-only; `the_ingest_upgrades_an_attested_sender_claim_and_refuses_a_bad_one`. Carried honestly: receiver-side pin ENFORCEMENT needs carrier→remote identity plumbing; an unpinned receiver learns the key from the sender's protocol answer (TOFU), named in the docs |
| (test) digest deadline vs machine reality | the posture digest test's fixed 60 s wall-clock deadline failed on a 310 MB just-linked debug binary whose read-and-hash was CPU-starved by other lanes (probe-proven: the thread started and simply had not finished; the same file hashes in 5 s warm). The page cache is warmed before the clock and the deadline scales with the image at the OBSERVED ~2 MB/s floor — and even that lost a suite-parallel run (209 s), so the test is `ignored` by default with its reason and the exact `--include-ignored` command; it passes standalone on this machine (70–88 s) | D: standalone run green (70.76 s); suite runs exclude it rather than fail flakily |

Everyday quality found live and fixed: a message sent while a turn runs used
to fail "the session already has a turn in flight"; one ordered lock per
binding now queues it, names the wait for drains, and answers in order
(`124eb3a7`).

## Owner-only steps this lane will not take

* macOS application firewall allowance for a newly installed `aikit` binary
  needs `sudo`; `gateway doctor` names the exact command.
* Any Tailscale Funnel; any Serve mapping change (the existing `:443` mapping is
  not this lane's).
