# Upgrading the gateway

The gateway is a long-lived process. Installing a newer `aikit` changes a file;
it does not change the process already running, and it cannot — the process
keeps executing the image it started from. Until something restarts it, the
machine is answering with the build it had before the update. This page is how
an upgrade reaches the *running* gateway, what is kept while it does, and what
to do when something goes wrong.

```sh
aikit gateway doctor                   # is the running gateway the installed build?
aikit gateway upgrade plan             # what runs, what is installed, what each peer runs
aikit gateway upgrade apply            # drain, restart on the installed build, verify
aikit gateway upgrade apply --install  # …and run the managed installer first (oi update)
aikit gateway upgrade apply --install --candidate <rev>   # …building exactly that commit
aikit gateway upgrade apply --restart-only                # restart onto what is installed; no installer
aikit gateway upgrade status           # the transaction, its steps, its receipt
```

Or from a conversation bound to the gateway (a Telegram DM with the owner, the
TUI): `/upgrade` reads the plan, `/upgrade apply` starts it, and the receipt
comes back into the same conversation when the new build is running. A group
conversation refuses `/upgrade apply`: a group admits several senders and a
slash command carries a message's authority, not an owner's.

## The steps

```text
inspect/plan → choose the candidate → retain a recovery basis → managed install
   → drain → restart → verify the RUNNING version → resume → visible receipt
```

| Step | What happens | Where it is recorded |
|---|---|---|
| inspect / plan | the running process is read (`protocol`: revision, pid, start time, executable digest, lifecycle); the installed build is what the service definition's executable resolves to; every declared peer is asked what it runs and which features it lacks | `upgrade plan` |
| choose | `--install` runs the managed installer (`oi update --apply [--channel mainline] [--candidate ai-kit=<rev>] ai-kit`); `--candidate` names the exact commit to build. Without `--install` the upgrade restarts onto the build already installed. `--install` refuses first when less than 3 GiB is free where the install builds (`gateway_upgrade.disk_low`): nothing is changed | the transaction's `plan` |
| recovery basis | the gateway's state files are copied aside; the installed build before the upgrade is recorded; the installer's own rollback (`oi update --rollback`) is named | `<state>/gateway-upgrade/<id>/recovery/` |
| managed install | the installer runs with a time bound and its log kept. The installed build is read before and after: an installer that flipped the build and *then* failed is rolled back, not reported as "unchanged" | `installer.log` |
| drain | see below | the transaction's `drain` |
| restart | the old process exits; the platform supervisor starts the next; if it has not by the end of the wait the manager is asked to | `steps` |
| verify | a **different process** must answer as the **expected image** (digest, else revision). Nothing less is "upgraded" — a new binary on disk is not a new process. A process reads its own executable digest in the background after it starts; until it has, the driver waits rather than take a revision for an image, and only at the bound accepts the revision — recorded as "verified by revision only" | `after` |
| resume | conversations continue on the new process; the receipt is announced into the conversation that asked, once | `receipt.json`, `receipt.md` |

Bounds, all adjustable on `apply`: `--drain-grace-secs` (an in-flight turn's
grace, default 60), `--exit-wait-secs` (how long the old process may take to
exit, and then the service manager to start the next one, before the manager
is asked to; default 90) and `--verify-timeout-secs` (how long the new process
has to answer as the expected build; default 120). `doctor` reports a running
gateway whose digest is not read yet as `gateway.identity_pending` — neither
current nor stale — and says to run it again.

## The drain, and what is never replayed

A drain stops admitting conversation work, gives each in-flight turn a bounded
grace (default 60 s), interrupts what has not finished, journals the
interruption on its stream, persists state, answers, and lets the process exit.
It also runs on `SIGTERM` (`launchctl bootout`, `systemctl stop`), so a stop
that nobody called an upgrade is no longer a turn killed unrecorded.

**A predecessor that predates the drain cannot be drained.** The first upgrade of
such a gateway stops it with its clean shutdown (it persists after every command),
and the receipt says the drain was **not measured**: what the old process had in
flight at that moment is *unknown*, not zero. "0 interrupted" is only ever stated
from a drain that ran and counted. Nothing is replayed either way.

The same holds when a drain *ran* but its reply was lost (the connection closed
with the process): whatever it counted was never seen, so the receipt says
unknown. "Nothing was in flight" is stated only when no gateway was running.

Two kinds of work are *uncertain* after a restart, and the receipt names each:

* **An interrupted turn.** What the model or its tools did before the interrupt
  is not known. It is recorded as an interruption and is **not** re-run.
* **An unreceipted outbound operation.** It stays pending in the persisted
  state. The restart does not re-send something it cannot prove was not sent.

Communiques are in the journal and survive the restart untouched; what a
predecessor already received is never delivered again.

## An upgrade asked for *through* the gateway

The gateway is what an upgrade restarts, so the process doing the upgrade can
never be the gateway or its child:

* On Linux, systemd stops every process in a unit's cgroup when the unit
  restarts (`KillMode=control-group`). A child of the gateway would die with it.
* On macOS, launchd stops a job's remaining processes.

So `upgrade apply` starts a **worker under the service manager as its own
one-shot job** — a LaunchAgent with no `KeepAlive`, or a transient systemd unit
(`aikit-gateway-upgrade-<id>`). Closing the terminal, restarting the gateway or
killing the requesting chat does not touch it. The worker holds no state the
file does not: the whole transaction is written before each step and after it,
so any process can finish it.

If the worker itself dies, the *new* gateway notices on its next tick — a
non-terminal transaction whose driver lock is free and that has been quiet for
30 s — and starts a resume worker. `aikit gateway upgrade resume [ID]` does the
same by hand. A receipt that could not be announced because the gateway was not
up yet stays pending and is delivered by the next driver; the `receipt.md` file
is the visible receipt meanwhile.

## What the lifecycle decides

| The running gateway is | `upgrade apply` |
|---|---|
| supervised (launchd, systemd) | drains, restarts, verifies |
| foreground | installs, **leaves it running**, and says why: stopping it would just turn it off. The receipt names the command that starts the new build. |
| application-managed (O:I, the desktop) | installs, **leaves it running**: the application holds the process, and the receipt says to restart it from the application |
| not running | starts the installed build |
| already the installed build | `no-change`; nothing is drained |

## A transaction that cannot run, and one that cannot be resumed

A step that cannot run (an unreadable state file, say) is **recorded** in the
transaction, and one that had changed nothing yet ends `failed-before-change`, so it
cannot sit in `planned` blocking every later `apply`. A later phase stays
resumable. A transaction whose worker is gone and which keeps failing is ended by
`aikit gateway upgrade abandon [ID] --reason "…"`: refused while a worker holds it,
and it changes nothing on disk or in the running gateway — the receipt says what was
known. A gateway that holds the socket but does not answer in time is **never
taken for gone**: it is not drained and not restarted around, and the upgrade says
so (`needs-operator`), rather than starting a second gateway beside a slow one.

## Failure and recovery

| What went wrong | What the upgrade does | What is left |
|---|---|---|
| the installer fails, build unchanged | `failed-before-change`; the gateway was never touched | the running gateway, unchanged |
| the installer flips the build, then fails | rolls back through `oi update --rollback`, verifies the old build runs (`rolled-back`) | the previous build |
| the new build does not come up | rolls back, restarts, verifies the old build (`rolled-back`) | the previous build, and the failure in the receipt |
| the old process will not stop | `needs-operator`: names the pid and the command | the old process |
| a supervisor that never restarts | the manager is asked once; if nothing helps, `needs-operator` with the command | the installed build, not running |
| the worker dies mid-upgrade | another driver resumes at the durable phase; nothing already done repeats | — |
| a terminal is lost | nothing: the worker is not attached to it | — |
| a peer is offline | the plan shows it unreachable; relayed Communiques queue and are re-resolved when it returns | — |

`aikit gateway upgrade rollback <id>` restores the previous build of the **latest**
upgrade and verifies it (the installer's rollback restores the previous set of the
latest update; an older id is refused, naming the latest).

**A rollback does not undo effects.** It restores a build. Turns that were
interrupted, messages that were sent, and state written while the new build ran
stay what they were. The snapshot carries no new required fields in this
change, so an older build restoring a newer build's state file drops nothing
load-bearing; a future schema change must add a guard before it relies on that.

## Mixed versions

`upgrade plan` and `doctor` list each declared peer's build and which protocol
features it lacks. A peer that lacks a feature is refused *for that one thing*,
by name: the Flow relay falls back to the ssh route only for a peer that
answers and is known not to advertise `encounter-request-relay`, and holds for
a peer that does not answer. The first upgrade of a peer that predates the
drain is stopped with its clean shutdown rather than drained; the receipt says
so.

## The firewall, on macOS

An update installs a new ad-hoc-signed binary, and the application firewall
queues inbound connections to a binary the owner has not allowed. `doctor`
reads the firewall's allow list (read-only) and warns when the installed binary
is not on it while the running one is. Allowing it needs `sudo`, which aikit
never runs; the warning prints the exact command. The alternative that does not
need it: bind `127.0.0.1` and put `tailscale serve` in front, so the allowance
belongs to Tailscale, not to a binary that changes on every update (see
`GATEWAY-OPERATING-MODES.md`).

## Controlled instances

`scripts/gateway-upgrade-rehearse.py` runs the five scenarios (upgrade, already
current, installer fails, installer flips then fails, new build exits at once)
against such an instance under the platform's real service manager and prints an
evidence document. A rehearsal should never touch the real service. `AIKIT_GATEWAY_SERVICE_INSTANCE=<name>`
makes `install-service`, `uninstall-service`, the upgrade worker and the doctor
manage `ai.aikit.gateway.<name>` (launchd) or `aikit-gateway-<name>.service`
(systemd) instead, with its own `AIKIT_HOME`, socket and port. The upgrade
worker normally runs under the service manager; `AIKIT_UPGRADE_WORKER_MODE=process`
runs it as a detached process for environments that have none.
