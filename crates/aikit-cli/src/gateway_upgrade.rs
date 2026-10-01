//! `aikit gateway upgrade` — a managed upgrade of the running gateway, as one
//! durable transaction that survives the restart it causes.
//!
//! ```text
//! inspect/plan → choose candidate → retain recovery basis → managed install
//!   → drain → restart → verify the RUNNING version → resume → visible receipt
//! ```
//!
//! The gateway is the thing an upgrade restarts, and an upgrade can be asked
//! for *through* the gateway (`/upgrade apply` in a conversation). So the
//! process that performs the upgrade is never the gateway and never a child
//! of it: it is a worker started under the platform's own service manager (a
//! one-shot LaunchAgent, a transient systemd unit), which a gateway restart
//! does not touch. Everything the worker knows lives in the transaction file,
//! written before each step and after it, so any process can finish what the
//! worker started — the worker itself resumed, or the *new* gateway noticing
//! an orphan on its first tick.
//!
//! What this module promises, and how each is tested (see the unit tests at
//! the foot, which drive the whole machine against a scripted environment):
//!
//! * **The installed binary is not the running binary.** `oi update` flips a
//!   symlink; a resident keeps executing its old image. The transaction
//!   reads the running process's own identity before and after, and calls an
//!   upgrade done only when a *different process* runs the *expected image*.
//! * **A failed install changes nothing it does not name.** The installer is
//!   allowed to have flipped some binaries before it failed; the transaction
//!   compares installed identity before and after and rolls back or hands the
//!   exact state to the operator — it never claims "unchanged" unread.
//! * **A gateway that cannot restart itself is not restarted.** A foreground
//!   or application-managed gateway has no supervisor to start the next
//!   build; draining it would just turn it off. The upgrade installs, says so,
//!   and names the command that starts the new build.
//! * **Pending work is retained, exactly, and never replayed blindly.** The
//!   drain names every interrupted turn and every unreceipted operation; the
//!   receipt carries them; the restart re-sends nothing it cannot prove
//!   unsent.
//! * **The receipt reaches the conversation that asked.** A receipt for an
//!   upgrade asked for in a chat is announced into that chat by the gateway
//!   that is then running, once.

use std::path::{Path, PathBuf};
use std::time::Duration;

use aikit_adapters::{DrainReport, GatewayLifecycle, UpgradeOrigin};
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const TRANSACTION_SCHEMA: &str = "aikit.gateway-upgrade/v1";
pub const RECEIPT_SCHEMA: &str = "aikit.gateway-upgrade-receipt/v1";
pub const PLAN_SCHEMA: &str = "aikit.gateway-upgrade-plan/v1";

// ---------------------------------------------------------------------------
// The transaction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// Recorded; nothing changed yet.
    Planned,
    /// The managed installer is running (or was, when the worker stopped).
    Installing,
    /// The installed build is what the plan expects; the running process has
    /// not been touched.
    Installed,
    /// The running gateway is being drained (and asked to exit).
    Draining,
    /// The old process is ending and the supervisor is starting the next.
    Restarting,
    /// Waiting for a different process to answer as the expected build.
    Verifying,
    /// The new build is running; the receipt is being written and returned.
    Resuming,
    /// Terminal: the expected build is running.
    Completed,
    /// Terminal: the installer failed and the installed build is unchanged.
    FailedBeforeChange,
    /// The previous build is being restored.
    RollingBack,
    /// Terminal: the previous build runs again.
    RolledBack,
    /// Terminal: something only the operator can finish; the receipt names it.
    NeedsOperator,
}

impl Phase {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::FailedBeforeChange | Self::RolledBack | Self::NeedsOperator
        )
    }
}

/// What a caller wants done, resolved from what is installed and running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// The installed build is already what should run: drain and restart.
    RestartOnly,
    /// Run the managed installer first, then drain and restart.
    InstallThenRestart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installer {
    /// argv of the install step (`oi update --apply aikit`).
    pub install: Vec<String>,
    /// argv of the supported rollback (`oi update --rollback`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rollback: Vec<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer: Option<Installer>,
    /// The build that must be running when the upgrade is done, when the
    /// caller named one (a revision prefix). Otherwise it is whatever the
    /// installed executable turns out to be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
    pub drain_grace_ms: u64,
    pub exit_wait_ms: u64,
    pub verify_timeout_ms: u64,
    pub auto_rollback: bool,
}

/// What a process (or an installed file) is, for comparison.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Identity {
    pub revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
}

impl Identity {
    /// The same executable image: digests when both are known, else the
    /// revision.
    pub fn same_image(&self, other: &Identity) -> bool {
        match (&self.executable_sha256, &other.executable_sha256) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => {
                !self.revision.is_empty()
                    && self.revision != "unknown"
                    && (other.revision.starts_with(&self.revision)
                        || self.revision.starts_with(&other.revision))
            }
        }
    }
}

/// A running gateway process, read from the process itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Running {
    pub pid: u32,
    pub started_at_unix_ms: u64,
    pub identity: Identity,
    #[serde(default)]
    pub lifecycle: GatewayLifecycle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workcell_ref: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Recovery {
    /// Where the gateway's state was copied before anything changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_files: Vec<String>,
    /// The installed build before the upgrade (what a rollback must restore).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_before: Option<Identity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub at_unix_ms: u64,
    pub phase: Phase,
    pub ok: bool,
    pub detail: String,
}

/// How an upgrade ended, in words and in facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// `completed`, `no-change`, `failed-before-change`, `rolled-back` or
    /// `needs-operator`.
    pub status: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<Running>,
    /// What the operator has to do, exactly, when the upgrade could not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operator_steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub schema: String,
    pub id: String,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub phase: Phase,
    /// Who asked: `cli`, or `conversation:<binding>`.
    pub requested_by: String,
    /// The conversation the receipt returns to, when the upgrade was asked
    /// for in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<UpgradeOrigin>,
    pub plan: Plan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Running>,
    #[serde(default)]
    pub recovery: Recovery,
    /// The drain report: every interrupted turn and unreceipted operation,
    /// named. Uncertain effects live here and in the receipt; nothing is
    /// replayed from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drain: Option<DrainReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Running>,
    /// The build the installed executable turned out to be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed: Option<Identity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drain_requested_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(default)]
    pub receipt_delivered: bool,
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Where transactions live: `<state>/gateway-upgrade/<id>/transaction.json`.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            root: state_dir.join("gateway-upgrade"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    pub fn transaction_path(&self, id: &str) -> PathBuf {
        self.dir(id).join("transaction.json")
    }

    pub fn save(&self, transaction: &Transaction) -> Result<()> {
        let dir = self.dir(&transaction.id);
        std::fs::create_dir_all(&dir).map_err(io("create the upgrade directory", &dir))?;
        write_atomic(
            &self.transaction_path(&transaction.id),
            &serde_json::to_vec_pretty(transaction).map_err(|error| {
                AikitError::new(
                    "gateway_upgrade.encode",
                    format!("encode the upgrade transaction: {error}"),
                )
            })?,
        )
    }

    pub fn load(&self, id: &str) -> Result<Transaction> {
        let path = self.transaction_path(id);
        let bytes = std::fs::read(&path).map_err(io("read the upgrade transaction", &path))?;
        serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "gateway_upgrade.decode",
                format!("decode {}: {error}", path.display()),
            )
        })
    }

    /// Every transaction, oldest first (ids sort by creation).
    pub fn list(&self) -> Vec<Transaction> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut ids: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        ids.sort();
        ids.iter().filter_map(|id| self.load(id).ok()).collect()
    }

    /// The transaction not yet terminal, if any: at most one is in flight.
    pub fn in_flight(&self) -> Option<Transaction> {
        self.list()
            .into_iter()
            .rev()
            .find(|transaction| !transaction.phase.is_terminal())
    }

    pub fn latest(&self) -> Option<Transaction> {
        self.list().into_iter().next_back()
    }
}

fn io<'a>(what: &'a str, path: &'a Path) -> impl Fn(std::io::Error) -> AikitError + 'a {
    move |error| {
        AikitError::new(
            "gateway_upgrade.io",
            format!("{what} {}: {error}", path.display()),
        )
    }
}

/// Write a file so a reader sees the old content or the new, never a tear,
/// and so a crash right after leaves the new content durable.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&tmp).map_err(io("create", &tmp))?;
        file.write_all(bytes).map_err(io("write", &tmp))?;
        file.sync_all().map_err(io("sync", &tmp))?;
    }
    std::fs::rename(&tmp, path).map_err(io("replace", path))
}

// ---------------------------------------------------------------------------
// The environment the machine acts on
// ---------------------------------------------------------------------------

/// What an installer run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    pub success: bool,
    pub detail: String,
}

/// What asking the service manager to start the gateway did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartAction {
    /// The manager was asked (and said what).
    Requested(String),
    /// No manager stands behind this gateway; the operator starts it.
    OperatorMustStart(String),
}

/// Everything the upgrade machine touches, behind one seam: the real machine
/// implements it against the gateway's carrier, the installer and the service
/// manager; the tests implement it as a script.
pub trait UpgradeEnv {
    fn now_unix_ms(&self) -> u64;
    /// The running gateway, read from the process; `None` when nothing answers.
    fn read_running(&self) -> Result<Option<Running>>;
    /// The build a supervisor would start now: the executable the service
    /// definition names, resolved and identified.
    fn installed_identity(&self) -> Result<Option<Identity>>;
    fn run_installer(
        &self,
        argv: &[String],
        timeout: Duration,
        log: &Path,
    ) -> Result<CommandOutcome>;
    /// Ask the running gateway (named by `expected_pid`) to drain and, with
    /// `exit`, stop. A predecessor that predates the drain is stopped with its
    /// clean shutdown instead, and the report says so.
    fn drain(
        &self,
        expected_pid: u32,
        reason: &str,
        grace: Duration,
        exit: bool,
    ) -> Result<DrainReport>;
    /// Start the gateway when the supervisor has not.
    fn start_service(&self, lifecycle: GatewayLifecycle) -> Result<StartAction>;
    /// Copy the gateway's state somewhere the recovery can find it.
    fn backup_state(&self, into: &Path) -> Result<Vec<String>>;
    /// Say one line into a conversation, through the running gateway.
    fn announce(&self, origin: &UpgradeOrigin, text: &str) -> Result<()>;
    fn sleep(&self, duration: Duration);
}

// ---------------------------------------------------------------------------
// The machine
// ---------------------------------------------------------------------------

pub struct Driver<'a, E: UpgradeEnv> {
    pub env: &'a E,
    pub store: &'a Store,
}

impl<E: UpgradeEnv> Driver<'_, E> {
    /// Create a transaction (the caller then drives it, in this process or a
    /// detached worker).
    pub fn create(
        &self,
        requested_by: impl Into<String>,
        origin: Option<UpgradeOrigin>,
        plan: Plan,
    ) -> Result<Transaction> {
        if let Some(existing) = self.store.in_flight() {
            return Err(AikitError::new(
                "gateway_upgrade.in_flight",
                format!(
                    "upgrade {} is already {} — let it finish (`aikit gateway upgrade status`), \
                     resume it (`aikit gateway upgrade resume {}`), or read its receipt",
                    existing.id,
                    serde_json::to_value(existing.phase)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default(),
                    existing.id
                ),
            )
            .with("upgrade", existing.id));
        }
        let now = self.env.now_unix_ms();
        let transaction = Transaction {
            schema: TRANSACTION_SCHEMA.into(),
            id: format!(
                "upg-{}",
                ulid::Ulid::generate().to_string().to_ascii_lowercase()
            ),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
            phase: Phase::Planned,
            requested_by: requested_by.into(),
            origin,
            plan,
            before: None,
            recovery: Recovery::default(),
            drain: None,
            after: None,
            installed: None,
            drain_requested_at_unix_ms: None,
            steps: Vec::new(),
            outcome: None,
            receipt_delivered: false,
        };
        self.store.save(&transaction)?;
        Ok(transaction)
    }

    fn note(&self, transaction: &mut Transaction, ok: bool, detail: impl Into<String>) {
        transaction.steps.push(Step {
            at_unix_ms: self.env.now_unix_ms(),
            phase: transaction.phase,
            ok,
            detail: detail.into(),
        });
    }

    fn save(&self, transaction: &mut Transaction) -> Result<()> {
        transaction.updated_at_unix_ms = self.env.now_unix_ms();
        self.store.save(transaction)
    }

    fn enter(&self, transaction: &mut Transaction, phase: Phase) -> Result<()> {
        transaction.phase = phase;
        self.save(transaction)
    }

    /// Drive the transaction to a terminal phase, persisting before and after
    /// every step. Safe to call again on a transaction another process
    /// stopped driving: every phase is re-entrant.
    pub fn drive(&self, transaction: &mut Transaction) -> Result<()> {
        loop {
            match transaction.phase {
                Phase::Planned => self.begin(transaction)?,
                Phase::Installing => self.install(transaction)?,
                Phase::Installed => self.decide(transaction)?,
                Phase::Draining => self.drain(transaction)?,
                Phase::Restarting => self.restart(transaction)?,
                Phase::Verifying => self.verify(transaction)?,
                Phase::Resuming => self.finish(transaction)?,
                Phase::RollingBack => self.roll_back(transaction)?,
                Phase::Completed
                | Phase::FailedBeforeChange
                | Phase::RolledBack
                | Phase::NeedsOperator => {
                    self.deliver_receipt(transaction)?;
                    return Ok(());
                }
            }
        }
    }

    fn begin(&self, transaction: &mut Transaction) -> Result<()> {
        transaction.before = self.env.read_running()?;
        transaction.recovery.installed_before = self.env.installed_identity()?;
        let dir = self.store.dir(&transaction.id).join("recovery");
        std::fs::create_dir_all(&dir).map_err(io("create the recovery directory", &dir))?;
        transaction.recovery.state_files = self.env.backup_state(&dir)?;
        let running = transaction
            .before
            .as_ref()
            .map(|running| {
                format!(
                    "running {} (pid {})",
                    running.identity.revision, running.pid
                )
            })
            .unwrap_or_else(|| "no gateway answering".into());
        self.note(
            transaction,
            true,
            format!(
                "recorded {running}; {} state file(s) copied for recovery",
                transaction.recovery.state_files.len()
            ),
        );
        let next = if transaction.plan.installer.is_some()
            && transaction.plan.mode == Mode::InstallThenRestart
        {
            Phase::Installing
        } else {
            Phase::Installed
        };
        self.enter(transaction, next)
    }

    fn install(&self, transaction: &mut Transaction) -> Result<()> {
        let Some(installer) = transaction.plan.installer.clone() else {
            return self.enter(transaction, Phase::Installed);
        };
        let log = self.store.dir(&transaction.id).join("installer.log");
        // Re-running the installer after a worker died mid-install is safe:
        // the managed updater skips a cut whose receipt already names it.
        let outcome = self.env.run_installer(
            &installer.install,
            Duration::from_millis(installer.timeout_ms),
            &log,
        );
        let installed_now = self.env.installed_identity()?;
        let changed = match (&transaction.recovery.installed_before, &installed_now) {
            (Some(before), Some(now)) => !before.same_image(now),
            (None, Some(_)) => true,
            _ => false,
        };
        match outcome {
            Ok(outcome) if outcome.success => {
                self.note(transaction, true, format!("installed: {}", outcome.detail));
                self.enter(transaction, Phase::Installed)
            }
            Ok(outcome) => self.installer_failed(transaction, outcome.detail, changed),
            Err(error) => self.installer_failed(transaction, error.to_string(), changed),
        }
    }

    fn installer_failed(
        &self,
        transaction: &mut Transaction,
        detail: String,
        installed_build_changed: bool,
    ) -> Result<()> {
        self.note(
            transaction,
            false,
            format!("the installer failed: {detail}"),
        );
        if !installed_build_changed {
            return self.finalize(
                transaction,
                Phase::FailedBeforeChange,
                "failed-before-change",
                format!(
                    "the install failed ({detail}); the installed gateway build is unchanged \
                     and the running gateway was not touched"
                ),
                vec![],
            );
        }
        // The installer flipped the build before it failed: that is a change,
        // and it is named, not hidden.
        let can_roll_back = transaction
            .plan
            .installer
            .as_ref()
            .is_some_and(|installer| !installer.rollback.is_empty());
        if transaction.plan.auto_rollback && can_roll_back {
            self.note(
                transaction,
                true,
                "the installed build changed before the install failed; restoring the previous set",
            );
            return self.enter(transaction, Phase::RollingBack);
        }
        let rollback = transaction
            .plan
            .installer
            .as_ref()
            .map(|installer| installer.rollback.join(" "))
            .filter(|command| !command.is_empty());
        self.finalize(
            transaction,
            Phase::NeedsOperator,
            "needs-operator",
            format!(
                "the install failed ({detail}) after changing the installed gateway build; the \
                 running gateway was not touched"
            ),
            vec![rollback
                .map(|command| format!("restore the previous build: {command}"))
                .unwrap_or_else(|| "restore the previous build with your installer".into())],
        )
    }

    fn decide(&self, transaction: &mut Transaction) -> Result<()> {
        let installed = self.env.installed_identity()?;
        transaction.installed = installed.clone();
        let Some(installed) = installed else {
            return self.finalize(
                transaction,
                Phase::NeedsOperator,
                "needs-operator",
                "no gateway executable could be identified from the service definition or PATH"
                    .into(),
                vec!["install AIKit, then `aikit gateway install-service`".into()],
            );
        };
        if let Some(expected) = &transaction.plan.expected_revision {
            if !installed.revision.starts_with(expected.as_str())
                && !expected.starts_with(installed.revision.as_str())
            {
                return self.finalize(
                    transaction,
                    Phase::NeedsOperator,
                    "needs-operator",
                    format!(
                        "the installed build is {} but the plan expects {expected}",
                        installed.revision
                    ),
                    vec![format!(
                        "install revision {expected}, then `aikit gateway upgrade apply --restart-only`"
                    )],
                );
            }
        }
        let Some(before) = transaction.before.clone() else {
            // Nothing answers: there is no process to drain. Starting the
            // installed build is the whole upgrade.
            self.note(
                transaction,
                true,
                "no gateway was running; starting the installed build",
            );
            return self.enter(transaction, Phase::Restarting);
        };
        if before.identity.same_image(&installed) {
            return self.finalize(
                transaction,
                Phase::Completed,
                "no-change",
                format!(
                    "no change: the running gateway (pid {}, revision {}) already is the \
                     installed build",
                    before.pid, before.identity.revision
                ),
                vec![],
            );
        }
        if !before.lifecycle.restarts_itself() {
            let steps = if before.lifecycle == GatewayLifecycle::Application {
                vec!["restart it from the application that owns it (O:I or the desktop)".into()]
            } else {
                vec![
                    "stop it and start the new build with the command you started it with \
                     (`aikit gateway serve …`)"
                        .into(),
                    "or put it under a supervisor: `aikit gateway install-service`".into(),
                ]
            };
            return self.finalize(
                transaction,
                Phase::NeedsOperator,
                "needs-operator",
                format!(
                    "the installed build is {} and the gateway (pid {}, revision {}) runs in the \
                     {} lifecycle: nothing would start the new build if it were stopped, so it \
                     was left running",
                    installed.revision,
                    before.pid,
                    before.identity.revision,
                    before.lifecycle.as_str()
                ),
                steps,
            );
        }
        self.note(
            transaction,
            true,
            format!(
                "the running gateway is {} and the installed build is {}: draining for restart",
                before.identity.revision, installed.revision
            ),
        );
        self.enter(transaction, Phase::Draining)
    }

    fn drain(&self, transaction: &mut Transaction) -> Result<()> {
        let Some(before) = transaction.before.clone() else {
            return self.enter(transaction, Phase::Restarting);
        };
        // A re-entered drain finds the old process already gone: that is the
        // drain having worked.
        match self.env.read_running()? {
            Some(running) if running.pid == before.pid => {}
            _ => {
                self.note(transaction, true, "the previous process has already ended");
                return self.enter(transaction, Phase::Restarting);
            }
        }
        transaction.drain_requested_at_unix_ms = Some(self.env.now_unix_ms());
        self.save(transaction)?;
        match self.env.drain(
            before.pid,
            &format!("upgrade {}", transaction.id),
            Duration::from_millis(transaction.plan.drain_grace_ms),
            true,
        ) {
            Ok(report) => {
                self.note(
                    transaction,
                    true,
                    format!(
                        "drained: {} turn(s) resolved, {} interrupted, {} operation(s) pending, \
                         nothing replayed",
                        report.turns_resolved.len(),
                        report.turns_interrupted.len(),
                        report.pending_operations.len()
                    ),
                );
                transaction.drain = Some(report);
            }
            Err(error) => {
                // The drain may have landed and the connection closed with the
                // process; what matters is whether the process ends.
                self.note(
                    transaction,
                    false,
                    format!("the drain call did not answer cleanly: {error}"),
                );
            }
        }
        self.enter(transaction, Phase::Restarting)
    }

    fn restart(&self, transaction: &mut Transaction) -> Result<()> {
        let before_pid = transaction.before.as_ref().map(|running| running.pid);
        let lifecycle = transaction
            .before
            .as_ref()
            .map(|running| running.lifecycle)
            .unwrap_or(GatewayLifecycle::Foreground);
        // 1. The old process ends.
        let deadline = self.env.now_unix_ms() + transaction.plan.exit_wait_ms;
        loop {
            match (self.env.read_running()?, before_pid) {
                (Some(running), Some(pid)) if running.pid == pid => {
                    if self.env.now_unix_ms() >= deadline {
                        return self.finalize(
                            transaction,
                            Phase::NeedsOperator,
                            "needs-operator",
                            format!(
                                "the previous gateway (pid {pid}) did not stop within {} ms of \
                                 the drain",
                                transaction.plan.exit_wait_ms
                            ),
                            vec![format!(
                                "stop it: kill {pid} (it drains on SIGTERM), then `aikit gateway \
                                 upgrade resume {}`",
                                transaction.id
                            )],
                        );
                    }
                    self.env.sleep(Duration::from_millis(200));
                }
                _ => break,
            }
        }
        // 2. The supervisor starts the next build; when it has not by the end
        //    of the wait, it is asked to.
        let start_deadline = self.env.now_unix_ms() + transaction.plan.exit_wait_ms;
        let mut asked = false;
        loop {
            if let Some(running) = self.env.read_running()? {
                if Some(running.pid) != before_pid {
                    self.note(
                        transaction,
                        true,
                        format!("a new process is answering: pid {}", running.pid),
                    );
                    break;
                }
            }
            if !asked && self.env.now_unix_ms() >= start_deadline {
                asked = true;
                match self.env.start_service(lifecycle)? {
                    StartAction::Requested(what) => self.note(
                        transaction,
                        true,
                        format!("asked the service manager: {what}"),
                    ),
                    StartAction::OperatorMustStart(how) => {
                        return self.finalize(
                            transaction,
                            Phase::NeedsOperator,
                            "needs-operator",
                            "the previous gateway stopped and nothing started the new build".into(),
                            vec![how],
                        );
                    }
                }
            }
            if self.env.now_unix_ms() >= start_deadline + transaction.plan.verify_timeout_ms {
                break;
            }
            self.env.sleep(Duration::from_millis(200));
        }
        self.enter(transaction, Phase::Verifying)
    }

    fn verify(&self, transaction: &mut Transaction) -> Result<()> {
        let before_pid = transaction.before.as_ref().map(|running| running.pid);
        let expected = transaction.installed.clone().unwrap_or_default();
        let deadline = self.env.now_unix_ms() + transaction.plan.verify_timeout_ms;
        let mut last_seen: Option<Running> = None;
        // A different process whose revision matches but whose executable
        // digest it has not finished reading: the revision alone cannot show
        // that it is the installed image. The process reads its own digest in
        // the background after it starts, so the wait is for that — and only
        // at the deadline is the revision accepted, said so.
        let mut by_revision_only: Option<Running> = None;
        loop {
            if let Some(running) = self.env.read_running()? {
                let different_process = Some(running.pid) != before_pid;
                let expected_image = expected.revision.is_empty()
                    || expected.executable_sha256.is_none() && expected.revision == "unknown"
                    || running.identity.same_image(&expected);
                let digest_pending = expected.executable_sha256.is_some()
                    && running.identity.executable_sha256.is_none();
                if different_process && expected_image && !digest_pending {
                    transaction.after = Some(running);
                    self.note(
                        transaction,
                        true,
                        "verified: a different process is running the expected build",
                    );
                    return self.enter(transaction, Phase::Resuming);
                }
                // Each reading replaces the last: a digest that arrives and
                // differs must not leave an earlier revision-only reading to
                // be accepted at the deadline.
                by_revision_only = (different_process && expected_image && digest_pending)
                    .then(|| running.clone());
                last_seen = Some(running);
            }
            if self.env.now_unix_ms() >= deadline {
                break;
            }
            self.env.sleep(Duration::from_millis(250));
        }
        if let Some(running) = by_revision_only {
            transaction.after = Some(running);
            self.note(
                transaction,
                true,
                format!(
                    "verified by revision only: a different process runs revision {} but had not \
                     finished reading its executable digest within {} ms",
                    expected.revision, transaction.plan.verify_timeout_ms
                ),
            );
            return self.enter(transaction, Phase::Resuming);
        }
        let saw = match &last_seen {
            Some(running) => format!(
                "a gateway answers as pid {} revision {}",
                running.pid, running.identity.revision
            ),
            None => "nothing answers".into(),
        };
        self.note(
            transaction,
            false,
            format!(
                "not verified within {} ms: {saw}; expected revision {}",
                transaction.plan.verify_timeout_ms, expected.revision
            ),
        );
        let installed_changed = transaction
            .recovery
            .installed_before
            .as_ref()
            .zip(transaction.installed.as_ref())
            .is_some_and(|(before, now)| !before.same_image(now));
        let can_roll_back = transaction
            .plan
            .installer
            .as_ref()
            .is_some_and(|installer| !installer.rollback.is_empty());
        if transaction.plan.auto_rollback && installed_changed && can_roll_back {
            self.note(transaction, true, "restoring the previous build");
            return self.enter(transaction, Phase::RollingBack);
        }
        self.finalize(
            transaction,
            Phase::NeedsOperator,
            "needs-operator",
            format!("the new build was installed but the expected gateway did not come up: {saw}"),
            vec![
                "read the service log (`aikit gateway doctor`)".into(),
                format!(
                    "restore and restart the previous build: `aikit gateway upgrade rollback {}`",
                    transaction.id
                ),
            ],
        )
    }

    fn roll_back(&self, transaction: &mut Transaction) -> Result<()> {
        let Some(installer) = transaction.plan.installer.clone() else {
            return self.finalize(
                transaction,
                Phase::NeedsOperator,
                "needs-operator",
                "a rollback was needed and no installer rollback is declared".into(),
                vec!["restore the previous build with your installer".into()],
            );
        };
        let log = self.store.dir(&transaction.id).join("rollback.log");
        let outcome = self.env.run_installer(
            &installer.rollback,
            Duration::from_millis(installer.timeout_ms),
            &log,
        );
        match outcome {
            Ok(outcome) if outcome.success => self.note(
                transaction,
                true,
                format!("rolled back: {}", outcome.detail),
            ),
            Ok(outcome) => {
                return self.finalize(
                    transaction,
                    Phase::NeedsOperator,
                    "needs-operator",
                    format!("the rollback itself failed: {}", outcome.detail),
                    vec![format!("run it by hand: {}", installer.rollback.join(" "))],
                )
            }
            Err(error) => {
                return self.finalize(
                    transaction,
                    Phase::NeedsOperator,
                    "needs-operator",
                    format!("the rollback itself failed: {error}"),
                    vec![format!("run it by hand: {}", installer.rollback.join(" "))],
                )
            }
        }
        // Whatever is running now must be the previous build.
        let previous = transaction
            .recovery
            .installed_before
            .clone()
            .unwrap_or_default();
        let running = self.env.read_running()?;
        let lifecycle = transaction
            .before
            .as_ref()
            .map(|before| before.lifecycle)
            .unwrap_or(GatewayLifecycle::Foreground);
        if let Some(running) = &running {
            if !running.identity.same_image(&previous) && lifecycle.restarts_itself() {
                let _ = self.env.drain(
                    running.pid,
                    &format!("rollback of upgrade {}", transaction.id),
                    Duration::from_millis(transaction.plan.drain_grace_ms),
                    true,
                );
            }
        }
        let deadline = self.env.now_unix_ms() + transaction.plan.verify_timeout_ms;
        let mut asked = false;
        loop {
            if let Some(running) = self.env.read_running()? {
                if running.identity.same_image(&previous) {
                    transaction.after = Some(running);
                    return self.finalize(
                        transaction,
                        Phase::RolledBack,
                        "rolled-back",
                        format!(
                            "the new build did not come up; the previous build ({}) is \
                             running again",
                            previous.revision
                        ),
                        vec![],
                    );
                }
            }
            if !asked && self.env.now_unix_ms() >= deadline - transaction.plan.verify_timeout_ms / 2
            {
                asked = true;
                let _ = self.env.start_service(lifecycle);
            }
            if self.env.now_unix_ms() >= deadline {
                break;
            }
            self.env.sleep(Duration::from_millis(250));
        }
        self.finalize(
            transaction,
            Phase::NeedsOperator,
            "needs-operator",
            format!(
                "the previous build ({}) was restored on disk but is not running",
                previous.revision
            ),
            vec!["start it: `aikit gateway upgrade resume` or restart the service".into()],
        )
    }

    fn finish(&self, transaction: &mut Transaction) -> Result<()> {
        let after = transaction.after.clone();
        let (resolved, interrupted, pending) = transaction
            .drain
            .as_ref()
            .map(|report| {
                (
                    report.turns_resolved.len(),
                    report.turns_interrupted.len(),
                    report.pending_operations.len(),
                )
            })
            .unwrap_or((0, 0, 0));
        let summary = match (&transaction.before, &after) {
            (Some(before), Some(after)) => format!(
                "now running {} (pid {}); was {} (pid {}). {resolved} turn(s) finished, \
                 {interrupted} interrupted and recorded, {pending} unreceipted operation(s) \
                 retained; nothing was replayed",
                after.identity.revision, after.pid, before.identity.revision, before.pid
            ),
            (None, Some(after)) => format!(
                "started {} (pid {}); no gateway was running before",
                after.identity.revision, after.pid
            ),
            _ => "completed".to_owned(),
        };
        self.finalize(transaction, Phase::Completed, "completed", summary, vec![])
    }

    /// End the transaction: outcome, receipt files, terminal phase.
    fn finalize(
        &self,
        transaction: &mut Transaction,
        phase: Phase,
        status: &str,
        summary: String,
        operator_steps: Vec<String>,
    ) -> Result<()> {
        transaction.outcome = Some(Outcome {
            status: status.to_owned(),
            summary: summary.clone(),
            running: transaction.after.clone().or_else(|| {
                if status == "completed" && transaction.drain.is_none() {
                    transaction.before.clone()
                } else {
                    None
                }
            }),
            operator_steps,
        });
        self.note(transaction, status != "needs-operator", summary);
        transaction.phase = phase;
        self.save(transaction)?;
        self.write_receipt(transaction)
    }

    fn write_receipt(&self, transaction: &Transaction) -> Result<()> {
        let dir = self.store.dir(&transaction.id);
        write_atomic(
            &dir.join("receipt.json"),
            &serde_json::to_vec_pretty(&receipt_json(transaction)).map_err(|error| {
                AikitError::new(
                    "gateway_upgrade.encode",
                    format!("encode the upgrade receipt: {error}"),
                )
            })?,
        )?;
        write_atomic(
            &dir.join("receipt.md"),
            receipt_markdown(transaction).as_bytes(),
        )
    }

    /// Return the receipt to the conversation that asked, once. A gateway that
    /// is not answering yet leaves the receipt undelivered and this returns
    /// without error: the next driver (or `status`) tries again, and the file
    /// is the visible receipt meanwhile.
    fn deliver_receipt(&self, transaction: &mut Transaction) -> Result<()> {
        if transaction.receipt_delivered {
            return Ok(());
        }
        let Some(origin) = transaction.origin.clone() else {
            transaction.receipt_delivered = true;
            return self.save(transaction);
        };
        let text = receipt_line(transaction);
        match self.env.announce(&origin, &text) {
            Ok(()) => {
                transaction.receipt_delivered = true;
                self.note(
                    transaction,
                    true,
                    "the receipt was announced to the conversation",
                );
            }
            Err(error) => {
                self.note(
                    transaction,
                    false,
                    format!("the receipt could not be announced yet: {error}"),
                );
            }
        }
        self.save(transaction)
    }
}

// ---------------------------------------------------------------------------
// Receipts
// ---------------------------------------------------------------------------

/// One plain line for a chat.
pub fn receipt_line(transaction: &Transaction) -> String {
    let outcome = transaction.outcome.as_ref();
    let status = outcome.map(|o| o.status.as_str()).unwrap_or("unfinished");
    let summary = outcome.map(|o| o.summary.as_str()).unwrap_or("");
    let mut line = format!("gateway upgrade {} — {status}: {summary}", transaction.id);
    if let Some(outcome) = outcome {
        for step in &outcome.operator_steps {
            line.push_str(&format!("\n  to finish: {step}"));
        }
    }
    line
}

/// The receipt as data: everything an operator or an agent needs to see what
/// happened, what was retained and what is uncertain.
pub fn receipt_json(transaction: &Transaction) -> Value {
    json!({
        "schema": RECEIPT_SCHEMA,
        "upgrade": transaction.id,
        "phase": transaction.phase,
        "requested_by": transaction.requested_by,
        "outcome": transaction.outcome,
        "before": transaction.before,
        "installed": transaction.installed,
        "after": transaction.after,
        "drain": transaction.drain,
        "uncertain_effects": transaction.drain.as_ref().map(|report| json!({
            "interrupted_turns": report.turns_interrupted,
            "unreceipted_operations": report.pending_operations,
            "law": "recorded, never replayed: what a turn or tool did before it was \
                    interrupted is not known and is not repeated",
        })),
        "recovery": transaction.recovery,
        "steps": transaction.steps,
        "receipt_delivered": transaction.receipt_delivered,
    })
}

pub fn receipt_markdown(transaction: &Transaction) -> String {
    let mut text = format!("# Gateway upgrade {}\n\n", transaction.id);
    if let Some(outcome) = &transaction.outcome {
        text.push_str(&format!("**{}** — {}\n\n", outcome.status, outcome.summary));
        if !outcome.operator_steps.is_empty() {
            text.push_str("To finish:\n");
            for step in &outcome.operator_steps {
                text.push_str(&format!("- {step}\n"));
            }
            text.push('\n');
        }
    }
    let describe = |running: &Option<Running>| match running {
        Some(running) => format!(
            "revision {} (pid {}, {})",
            running.identity.revision,
            running.pid,
            running.lifecycle.as_str()
        ),
        None => "none".into(),
    };
    text.push_str(&format!(
        "- before: {}\n- after: {}\n",
        describe(&transaction.before),
        describe(&transaction.after)
    ));
    if let Some(drain) = &transaction.drain {
        text.push_str(&format!(
            "- drain: {} turn(s) resolved, {} interrupted, {} operation(s) unreceipted\n",
            drain.turns_resolved.len(),
            drain.turns_interrupted.len(),
            drain.pending_operations.len()
        ));
        for turn in &drain.turns_interrupted {
            text.push_str(&format!(
                "  - interrupted turn on {} (reply to #{}): effects before the interrupt are \
                 uncertain and were not replayed\n",
                turn.binding_ref, turn.in_reply_to_sequence
            ));
        }
        for operation in &drain.pending_operations {
            text.push_str(&format!(
                "  - operation {operation} stays pending; it is not re-sent blindly\n"
            ));
        }
    }
    text.push_str("\n## Steps\n");
    for step in &transaction.steps {
        text.push_str(&format!(
            "- [{}] {:?}: {}\n",
            if step.ok { "ok" } else { "!!" },
            step.phase,
            step.detail
        ));
    }
    text
}

// ---------------------------------------------------------------------------
// The plan (read-only)
// ---------------------------------------------------------------------------

/// What `upgrade plan` reads, as data: what runs, what is installed, whether
/// they differ, what an apply would do, and the declared peers' versions.
pub fn plan_reading<E: UpgradeEnv>(
    env: &E,
    installer: Option<&Installer>,
    peers: Vec<Value>,
) -> Result<Value> {
    let running = env.read_running()?;
    let installed = env.installed_identity()?;
    let stale = match (&running, &installed) {
        (Some(running), Some(installed)) => Some(!running.identity.same_image(installed)),
        _ => None,
    };
    let action = match (&running, &installed, stale) {
        (None, Some(_), _) => "start",
        (Some(_), Some(_), Some(true)) => "restart",
        (Some(_), Some(_), Some(false)) => "none",
        _ => "unknown",
    };
    let restartable = running
        .as_ref()
        .map(|running| running.lifecycle.restarts_itself());
    let mut notes: Vec<String> = Vec::new();
    if let (Some(running), Some(true)) = (&running, restartable.map(|r| !r)) {
        notes.push(format!(
            "the gateway runs in the {} lifecycle: an upgrade installs the new build but \
             cannot restart it; supervise it with `aikit gateway install-service`",
            running.lifecycle.as_str()
        ));
    }
    if running.as_ref().is_some_and(|running| {
        running.identity.revision.is_empty() || running.identity.revision == "unknown"
    }) {
        notes.push(
            "the running gateway does not report its build (it predates build identity): it \
             will be stopped with its clean shutdown rather than drained"
                .into(),
        );
    }
    Ok(json!({
        "schema": PLAN_SCHEMA,
        "running": running,
        "installed": installed,
        "stale": stale,
        "action": action,
        "installer": installer,
        "peers": peers,
        "notes": notes,
        "next": match action {
            "restart" | "start" => "aikit gateway upgrade apply --restart-only",
            _ => "nothing to do; to fetch a newer build: aikit gateway upgrade apply --install",
        },
    }))
}

/// A one-line reading of a plan, for a chat.
pub fn plan_line(plan: &Value) -> String {
    let revision = |field: &str| {
        plan[field]["identity"]["revision"]
            .as_str()
            .or_else(|| plan[field]["revision"].as_str())
            .unwrap_or("none")
            .to_owned()
    };
    match plan["action"].as_str() {
        Some("restart") => format!(
            "the gateway runs {} and {} is installed: `/upgrade apply` drains it, restarts it on \
             the installed build and reports back here",
            revision("running"),
            revision("installed")
        ),
        Some("none") => format!(
            "the running gateway ({}) already is the installed build; nothing to do",
            revision("running")
        ),
        Some("start") => "no gateway is running; the installed build would be started".to_owned(),
        _ => "the plan could not be read; run `aikit gateway upgrade plan`".to_owned(),
    }
}

/// Default bounds. Explicit, never ambient.
pub fn default_plan(mode: Mode, installer: Option<Installer>, expected: Option<String>) -> Plan {
    Plan {
        mode,
        installer,
        expected_revision: expected,
        drain_grace_ms: 60_000,
        exit_wait_ms: 90_000,
        verify_timeout_ms: 120_000,
        auto_rollback: true,
    }
}

// ---------------------------------------------------------------------------
// Tests: the whole machine against a scripted environment
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::{DrainedTurn, UpgradeOrigin};
    use std::cell::{Cell, RefCell};

    fn identity(revision: &str) -> Identity {
        Identity {
            revision: revision.into(),
            executable_sha256: Some(format!("{revision:0>64}")),
            executable_path: Some(format!("/managed/{revision}/aikit")),
        }
    }

    fn running(pid: u32, revision: &str, lifecycle: GatewayLifecycle) -> Running {
        Running {
            pid,
            started_at_unix_ms: 1,
            identity: identity(revision),
            lifecycle,
            workcell_ref: Some("workcell:test".into()),
            features: vec![],
        }
    }

    /// A machine that is a script: what runs, what is installed, how an
    /// install or a drain behaves, and when the supervisor brings the next
    /// build up. Time only moves when something sleeps.
    struct Script {
        clock: Cell<u64>,
        /// The gateway process table: `None` while nothing answers.
        gateway: RefCell<Option<Running>>,
        installed: RefCell<Option<Identity>>,
        /// What the installer flips the installed build to on success.
        install_to: RefCell<Option<Identity>>,
        install_fails: Cell<bool>,
        /// An installer that flips the build and then fails.
        install_changes_then_fails: Cell<bool>,
        rollback_to: RefCell<Option<Identity>>,
        /// The process the supervisor starts when the old one ends, and how
        /// many clock ticks later (None = the supervisor never does).
        next: RefCell<Option<(Running, u64)>>,
        drained_at: Cell<Option<u64>>,
        drain_report: RefCell<DrainReport>,
        drain_calls: Cell<u32>,
        start_calls: Cell<u32>,
        announced: RefCell<Vec<String>>,
        announce_fails: Cell<bool>,
        /// Stop driving (simulating a dead worker) after this many `sleep`s.
        die_after_sleeps: Cell<Option<u32>>,
        sleeps: Cell<u32>,
        installer_runs: RefCell<Vec<Vec<String>>>,
    }

    impl Script {
        fn new(gateway: Option<Running>, installed: Identity) -> Self {
            Self {
                clock: Cell::new(1_000),
                gateway: RefCell::new(gateway),
                installed: RefCell::new(Some(installed)),
                install_to: RefCell::new(None),
                install_fails: Cell::new(false),
                install_changes_then_fails: Cell::new(false),
                rollback_to: RefCell::new(None),
                next: RefCell::new(None),
                drained_at: Cell::new(None),
                drain_report: RefCell::new(DrainReport::default()),
                drain_calls: Cell::new(0),
                start_calls: Cell::new(0),
                announced: RefCell::new(Vec::new()),
                announce_fails: Cell::new(false),
                die_after_sleeps: Cell::new(None),
                sleeps: Cell::new(0),
                installer_runs: RefCell::new(Vec::new()),
            }
        }

        /// The supervisor brings the next process up `delay` ms after the drain.
        fn settle(&self) {
            if let (Some(at), Some((process, delay))) =
                (self.drained_at.get(), self.next.borrow().clone())
            {
                if self.clock.get() >= at + delay {
                    *self.gateway.borrow_mut() = Some(process);
                    self.drained_at.set(None);
                }
            }
        }
    }

    impl UpgradeEnv for Script {
        fn now_unix_ms(&self) -> u64 {
            self.clock.get()
        }
        fn read_running(&self) -> Result<Option<Running>> {
            if self
                .die_after_sleeps
                .get()
                .is_some_and(|limit| self.sleeps.get() >= limit)
            {
                return Err(AikitError::new("test.worker_died", "the worker was killed"));
            }
            self.settle();
            Ok(self.gateway.borrow().clone())
        }
        fn installed_identity(&self) -> Result<Option<Identity>> {
            Ok(self.installed.borrow().clone())
        }
        fn run_installer(
            &self,
            argv: &[String],
            _timeout: Duration,
            _log: &Path,
        ) -> Result<CommandOutcome> {
            self.installer_runs.borrow_mut().push(argv.to_vec());
            if argv.iter().any(|a| a == "--rollback") {
                if let Some(previous) = self.rollback_to.borrow().clone() {
                    *self.installed.borrow_mut() = Some(previous);
                }
                return Ok(CommandOutcome {
                    success: true,
                    detail: "previous set restored".into(),
                });
            }
            if self.install_changes_then_fails.get() {
                *self.installed.borrow_mut() = self.install_to.borrow().clone();
                return Ok(CommandOutcome {
                    success: false,
                    detail: "a later product failed to build".into(),
                });
            }
            if self.install_fails.get() {
                return Ok(CommandOutcome {
                    success: false,
                    detail: "cargo build failed".into(),
                });
            }
            if let Some(next) = self.install_to.borrow().clone() {
                *self.installed.borrow_mut() = Some(next);
            }
            Ok(CommandOutcome {
                success: true,
                detail: "installed".into(),
            })
        }
        fn drain(
            &self,
            expected_pid: u32,
            reason: &str,
            _grace: Duration,
            exit: bool,
        ) -> Result<DrainReport> {
            self.drain_calls.set(self.drain_calls.get() + 1);
            let current = self.gateway.borrow().clone();
            match current {
                Some(running) if running.pid == expected_pid => {
                    if exit {
                        *self.gateway.borrow_mut() = None;
                        self.drained_at.set(Some(self.clock.get()));
                    }
                    let mut report = self.drain_report.borrow().clone();
                    report.reason = reason.into();
                    Ok(report)
                }
                _ => Err(AikitError::new(
                    "agency_gateway.drain_wrong_process",
                    "not this process",
                )),
            }
        }
        fn start_service(&self, _lifecycle: GatewayLifecycle) -> Result<StartAction> {
            self.start_calls.set(self.start_calls.get() + 1);
            // Asking the manager brings the next build up at once.
            if let Some((process, _)) = self.next.borrow().clone() {
                *self.gateway.borrow_mut() = Some(process);
            }
            Ok(StartAction::Requested("kickstart".into()))
        }
        fn backup_state(&self, into: &Path) -> Result<Vec<String>> {
            let copy = into.join("gateway.json");
            std::fs::write(&copy, b"{}").unwrap();
            Ok(vec![copy.display().to_string()])
        }
        fn announce(&self, _origin: &UpgradeOrigin, text: &str) -> Result<()> {
            if self.announce_fails.get() {
                return Err(AikitError::new("test.no_gateway", "the gateway is not up"));
            }
            self.announced.borrow_mut().push(text.to_owned());
            Ok(())
        }
        fn sleep(&self, duration: Duration) {
            self.sleeps.set(self.sleeps.get() + 1);
            self.clock
                .set(self.clock.get() + duration.as_millis() as u64);
        }
    }

    fn installer() -> Installer {
        Installer {
            install: vec![
                "oi".into(),
                "update".into(),
                "--apply".into(),
                "aikit".into(),
            ],
            rollback: vec!["oi".into(), "update".into(), "--rollback".into()],
            timeout_ms: 1_000,
        }
    }

    fn plan(mode: Mode) -> Plan {
        Plan {
            mode,
            installer: (mode == Mode::InstallThenRestart).then(installer),
            expected_revision: None,
            drain_grace_ms: 1_000,
            exit_wait_ms: 5_000,
            verify_timeout_ms: 5_000,
            auto_rollback: true,
        }
    }

    fn origin() -> UpgradeOrigin {
        UpgradeOrigin {
            binding_ref: "gateway-binding/telegram-x".into(),
            connector_ref: Some("connector/telegram".into()),
            in_reply_to_sequence: None,
        }
    }

    fn run(script: &Script, mode: Mode, origin: Option<UpgradeOrigin>) -> Transaction {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let driver = Driver {
            env: script,
            store: &store,
        };
        let mut transaction = driver.create("test", origin, plan(mode)).unwrap();
        driver.drive(&mut transaction).unwrap();
        // The receipt files exist and say the same thing the outcome does.
        let receipt: Value = serde_json::from_slice(
            &std::fs::read(store.dir(&transaction.id).join("receipt.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["schema"], RECEIPT_SCHEMA);
        assert_eq!(
            receipt["outcome"]["status"],
            transaction.outcome.as_ref().unwrap().status
        );
        assert!(store.dir(&transaction.id).join("receipt.md").exists());
        assert!(
            store
                .dir(&transaction.id)
                .join("recovery/gateway.json")
                .exists(),
            "the gateway's state was copied before anything changed"
        );
        transaction
    }

    #[test]
    fn a_stale_gateway_is_drained_restarted_and_verified_as_a_different_process_on_the_new_build() {
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedLaunchd)),
            identity("bbbb"),
        );
        *script.next.borrow_mut() = Some((
            running(11, "bbbb", GatewayLifecycle::SupervisedLaunchd),
            2_000,
        ));
        *script.drain_report.borrow_mut() = DrainReport {
            turns_interrupted: vec![DrainedTurn {
                binding_ref: "gateway-binding/x".into(),
                in_reply_to_sequence: 7,
                detail: Some("interrupted".into()),
            }],
            pending_operations: vec!["operation/3".into()],
            ..DrainReport::default()
        };
        let transaction = run(&script, Mode::RestartOnly, Some(origin()));
        assert_eq!(
            transaction.phase,
            Phase::Completed,
            "{:?}",
            transaction.steps
        );
        let outcome = transaction.outcome.as_ref().unwrap();
        assert_eq!(outcome.status, "completed");
        assert_eq!(transaction.after.as_ref().unwrap().pid, 11);
        assert_eq!(
            transaction.after.as_ref().unwrap().identity.revision,
            "bbbb"
        );
        assert!(
            outcome.summary.contains("now running bbbb"),
            "{}",
            outcome.summary
        );
        assert!(
            outcome.summary.contains("1 interrupted"),
            "{}",
            outcome.summary
        );
        assert!(outcome.summary.contains("nothing was replayed"));
        // The uncertain effect and the pending operation are in the receipt.
        let receipt = receipt_json(&transaction);
        assert_eq!(
            receipt["uncertain_effects"]["interrupted_turns"][0]["in_reply_to_sequence"],
            7
        );
        assert_eq!(
            receipt["uncertain_effects"]["unreceipted_operations"][0],
            "operation/3"
        );
        // The supervisor brought the process up by itself: nobody kicked it.
        assert_eq!(script.start_calls.get(), 0);
        // The conversation that asked hears about it, once.
        assert_eq!(script.announced.borrow().len(), 1);
        assert!(script.announced.borrow()[0].contains("completed"));
        assert!(transaction.receipt_delivered);
    }

    #[test]
    fn an_upgrade_that_finds_the_running_gateway_already_on_the_installed_build_restarts_nothing() {
        let script = Script::new(
            Some(running(10, "bbbb", GatewayLifecycle::SupervisedSystemd)),
            identity("bbbb"),
        );
        let transaction = run(&script, Mode::RestartOnly, None);
        assert_eq!(transaction.phase, Phase::Completed);
        assert_eq!(transaction.outcome.as_ref().unwrap().status, "no-change");
        assert_eq!(
            script.drain_calls.get(),
            0,
            "a current gateway is not drained"
        );
    }

    #[test]
    fn a_gateway_that_cannot_restart_itself_is_installed_for_but_left_running() {
        for lifecycle in [GatewayLifecycle::Foreground, GatewayLifecycle::Application] {
            let script = Script::new(Some(running(10, "aaaa", lifecycle)), identity("bbbb"));
            let transaction = run(&script, Mode::RestartOnly, None);
            assert_eq!(transaction.phase, Phase::NeedsOperator);
            assert_eq!(script.drain_calls.get(), 0, "{lifecycle:?} was drained");
            assert!(script.gateway.borrow().is_some(), "it is still running");
            let outcome = transaction.outcome.unwrap();
            assert!(outcome
                .summary
                .contains("nothing would start the new build"));
            let expected = if lifecycle == GatewayLifecycle::Application {
                "application that owns it"
            } else {
                "install-service"
            };
            assert!(
                outcome
                    .operator_steps
                    .iter()
                    .any(|step| step.contains(expected)),
                "{lifecycle:?}: {:?}",
                outcome.operator_steps
            );
        }
    }

    #[test]
    fn a_failed_install_with_an_unchanged_build_leaves_everything_as_it_was() {
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedLaunchd)),
            identity("aaaa"),
        );
        *script.install_to.borrow_mut() = Some(identity("bbbb"));
        script.install_fails.set(true);
        let transaction = run(&script, Mode::InstallThenRestart, Some(origin()));
        assert_eq!(transaction.phase, Phase::FailedBeforeChange);
        assert_eq!(script.drain_calls.get(), 0, "no drain after a failed build");
        assert_eq!(script.gateway.borrow().as_ref().unwrap().pid, 10);
        assert!(transaction
            .outcome
            .as_ref()
            .unwrap()
            .summary
            .contains("unchanged"));
        // The running gateway is what tells the chat the build failed.
        assert!(script.announced.borrow()[0].contains("failed-before-change"));
    }

    #[test]
    fn an_installer_that_flips_the_build_and_then_fails_is_rolled_back_not_called_unchanged() {
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedLaunchd)),
            identity("aaaa"),
        );
        *script.install_to.borrow_mut() = Some(identity("bbbb"));
        *script.rollback_to.borrow_mut() = Some(identity("aaaa"));
        script.install_changes_then_fails.set(true);
        // The old gateway is still the previous build, so the rollback finds
        // it running and needs no restart.
        let transaction = run(&script, Mode::InstallThenRestart, None);
        assert_eq!(
            transaction.phase,
            Phase::RolledBack,
            "{:?}",
            transaction.steps
        );
        assert!(script
            .installer_runs
            .borrow()
            .iter()
            .any(|argv| argv.contains(&"--rollback".to_owned())));
        assert_eq!(script.installed.borrow().as_ref().unwrap().revision, "aaaa");
        assert_eq!(script.drain_calls.get(), 0);
    }

    #[test]
    fn a_new_build_that_does_not_come_up_is_rolled_back_and_the_old_build_verified_running() {
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedLaunchd)),
            identity("aaaa"),
        );
        *script.install_to.borrow_mut() = Some(identity("bbbb"));
        *script.rollback_to.borrow_mut() = Some(identity("aaaa"));
        // The supervisor starts the OLD build again (the new one is broken and
        // exits at once in the real world; here the next process is "aaaa").
        *script.next.borrow_mut() = Some((
            running(12, "aaaa", GatewayLifecycle::SupervisedLaunchd),
            1_000,
        ));
        let transaction = run(&script, Mode::InstallThenRestart, None);
        assert_eq!(
            transaction.phase,
            Phase::RolledBack,
            "{:?}",
            transaction.steps
        );
        let outcome = transaction.outcome.unwrap();
        assert_eq!(outcome.status, "rolled-back");
        assert!(outcome
            .summary
            .contains("previous build (aaaa) is running again"));
        assert_eq!(transaction.after.unwrap().identity.revision, "aaaa");
    }

    #[test]
    fn a_supervisor_that_does_not_restart_is_asked_to_and_when_nothing_helps_the_operator_is_told()
    {
        // The supervisor never starts the next process by itself; asking it
        // works.
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedSystemd)),
            identity("bbbb"),
        );
        *script.next.borrow_mut() = Some((
            running(11, "bbbb", GatewayLifecycle::SupervisedSystemd),
            10_000_000,
        ));
        let transaction = run(&script, Mode::RestartOnly, None);
        assert_eq!(
            transaction.phase,
            Phase::Completed,
            "{:?}",
            transaction.steps
        );
        assert_eq!(
            script.start_calls.get(),
            1,
            "the manager was asked exactly once"
        );
    }

    #[test]
    fn a_worker_that_dies_mid_upgrade_is_finished_by_another_driver_without_redoing_the_drain() {
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedLaunchd)),
            identity("bbbb"),
        );
        *script.next.borrow_mut() = Some((
            running(11, "bbbb", GatewayLifecycle::SupervisedLaunchd),
            2_000,
        ));
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let driver = Driver {
            env: &script,
            store: &store,
        };
        let mut transaction = driver
            .create("test", Some(origin()), plan(Mode::RestartOnly))
            .unwrap();
        // The first worker dies after the drain, while the old process ends.
        script.die_after_sleeps.set(Some(2));
        let crashed = driver.drive(&mut transaction).unwrap_err();
        assert_eq!(crashed.code(), "test.worker_died");
        let on_disk = store.load(&transaction.id).unwrap();
        assert!(!on_disk.phase.is_terminal(), "{:?}", on_disk.phase);
        assert_eq!(script.drain_calls.get(), 1);

        // Another driver (a resumed worker, or the new gateway) reads the
        // durable transaction and finishes it: no second drain, one receipt.
        script.die_after_sleeps.set(None);
        let mut resumed = store.load(&transaction.id).unwrap();
        driver.drive(&mut resumed).unwrap();
        assert_eq!(resumed.phase, Phase::Completed, "{:?}", resumed.steps);
        assert_eq!(script.drain_calls.get(), 1, "the drain is not repeated");
        assert_eq!(script.announced.borrow().len(), 1);
        assert_eq!(resumed.after.as_ref().unwrap().pid, 11);
        // A third driver on a finished transaction does nothing more.
        let mut again = store.load(&transaction.id).unwrap();
        driver.drive(&mut again).unwrap();
        assert_eq!(
            script.announced.borrow().len(),
            1,
            "the receipt goes out once"
        );
    }

    #[test]
    fn a_receipt_that_cannot_be_announced_yet_stays_pending_and_the_file_is_the_receipt() {
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedLaunchd)),
            identity("bbbb"),
        );
        *script.next.borrow_mut() = Some((
            running(11, "bbbb", GatewayLifecycle::SupervisedLaunchd),
            1_000,
        ));
        script.announce_fails.set(true);
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let driver = Driver {
            env: &script,
            store: &store,
        };
        let mut transaction = driver
            .create("test", Some(origin()), plan(Mode::RestartOnly))
            .unwrap();
        driver.drive(&mut transaction).unwrap();
        assert_eq!(transaction.phase, Phase::Completed);
        assert!(!transaction.receipt_delivered);
        assert!(store.dir(&transaction.id).join("receipt.md").exists());
        // The gateway is reachable now; the next driver delivers it.
        script.announce_fails.set(false);
        let mut next = store.load(&transaction.id).unwrap();
        driver.drive(&mut next).unwrap();
        assert!(next.receipt_delivered);
        assert_eq!(script.announced.borrow().len(), 1);
    }

    #[test]
    fn only_one_upgrade_is_in_flight_and_the_second_request_names_the_first() {
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedLaunchd)),
            identity("bbbb"),
        );
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let driver = Driver {
            env: &script,
            store: &store,
        };
        let first = driver
            .create("test", None, plan(Mode::RestartOnly))
            .unwrap();
        let refused = driver
            .create("test", None, plan(Mode::RestartOnly))
            .unwrap_err();
        assert_eq!(refused.code(), "gateway_upgrade.in_flight");
        assert!(refused.to_string().contains(&first.id));
    }

    #[test]
    fn a_new_process_that_has_not_read_its_digest_is_waited_for_and_never_taken_for_another_image()
    {
        // The new process answers with the expected revision but no digest yet
        // (it reads its own executable in the background): the revision cannot
        // show it is the installed image, so the driver waits out the bound and
        // then accepts the revision — and says that is all it had.
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedLaunchd)),
            identity("bbbb"),
        );
        let mut undigested = running(11, "bbbb", GatewayLifecycle::SupervisedLaunchd);
        undigested.identity.executable_sha256 = None;
        *script.next.borrow_mut() = Some((undigested, 500));
        let transaction = run(&script, Mode::RestartOnly, None);
        assert_eq!(
            transaction.phase,
            Phase::Completed,
            "{:?}",
            transaction.steps
        );
        assert!(
            transaction
                .steps
                .iter()
                .any(|step| step.detail.contains("verified by revision only")),
            "{:?}",
            transaction.steps
        );
        assert!(
            !transaction.steps.iter().any(|step| step
                .detail
                .contains("a different process is running the expected build")),
            "a revision alone is not the strong verification"
        );
        // A new process whose digest disagrees is another image whatever its revision says.
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedLaunchd)),
            identity("bbbb"),
        );
        let mut other = running(11, "bbbb", GatewayLifecycle::SupervisedLaunchd);
        other.identity.executable_sha256 = Some("f".repeat(64));
        *script.next.borrow_mut() = Some((other, 500));
        let transaction = run(&script, Mode::RestartOnly, None);
        assert_ne!(
            transaction.phase,
            Phase::Completed,
            "{:?}",
            transaction.steps
        );
    }

    #[test]
    fn a_gateway_that_is_not_running_is_started_on_the_installed_build() {
        let script = Script::new(None, identity("bbbb"));
        *script.next.borrow_mut() =
            Some((running(11, "bbbb", GatewayLifecycle::SupervisedLaunchd), 0));
        let transaction = run(&script, Mode::RestartOnly, None);
        assert_eq!(
            transaction.phase,
            Phase::Completed,
            "{:?}",
            transaction.steps
        );
        assert_eq!(script.drain_calls.get(), 0, "there was nothing to drain");
        assert_eq!(transaction.after.unwrap().identity.revision, "bbbb");
    }

    #[test]
    fn identity_compares_images_by_digest_and_by_revision_when_a_digest_is_missing() {
        let a = identity("aaaa");
        let mut b = identity("aaaa");
        assert!(a.same_image(&b));
        b.executable_sha256 = Some("0".repeat(64));
        assert!(!a.same_image(&b), "a different digest is a different image");
        let no_digest = Identity {
            revision: "aaaa".into(),
            ..Identity::default()
        };
        assert!(no_digest.same_image(&identity("aaaa0000")));
        assert!(!no_digest.same_image(&identity("bbbb")));
        assert!(!Identity::default().same_image(&Identity::default()));
    }

    #[test]
    fn the_plan_reading_says_whether_the_running_gateway_is_stale_and_what_apply_would_do() {
        let script = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::SupervisedLaunchd)),
            identity("bbbb"),
        );
        let plan = plan_reading(&script, None, vec![]).unwrap();
        assert_eq!(plan["stale"], true);
        assert_eq!(plan["action"], "restart");
        assert!(plan_line(&plan).contains("aaaa"));
        assert!(plan_line(&plan).contains("bbbb"));
        let current = Script::new(
            Some(running(10, "bbbb", GatewayLifecycle::SupervisedLaunchd)),
            identity("bbbb"),
        );
        let plan = plan_reading(&current, None, vec![]).unwrap();
        assert_eq!(plan["action"], "none");
        let foreground = Script::new(
            Some(running(10, "aaaa", GatewayLifecycle::Foreground)),
            identity("bbbb"),
        );
        let plan = plan_reading(&foreground, None, vec![]).unwrap();
        assert!(plan["notes"][0].as_str().unwrap().contains("foreground"));
    }
}
