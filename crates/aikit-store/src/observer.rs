//! Recurrence: noticing what you keep doing.
//!
//! "You ran this three times; want it as a script?" (Spec II §2). Sources feed
//! this module attributed command runs — an agent's Bash tool, a terminal pane, a
//! Factory frame — and AIKit decides what repeats. The sources never decide: a
//! feed that counted for itself would count differently in every place, and a
//! threshold that lived in each feed would drift.
//!
//! ```text
//! run → empty / too long / failed / noise? → secret? → decided already? → record
//!     → count this command by this actor in the window → third time: offer it
//! ```
//!
//! ## What is kept, and for how long
//!
//! Only commands that passed the secret scan are kept, and only for thirty days;
//! the table is pruned on every write. A command holding a secret is not
//! redacted-and-kept — it is not kept at all, because a redacted command is no
//! longer the command that was run and would be offered as a script that cannot
//! work. A person who wants such a command saved captures it by hand.
//!
//! ## Why a file of its own
//!
//! Observations are high-volume and short-lived, and a new table in the shared
//! database is a schema step that makes every older `aikit` on the machine refuse
//! that database. This store is a separate SQLite file with its own schema, so it
//! can change without touching the one everything else shares.
//!
//! ## Offered once
//!
//! Reaching the threshold files a capture candidate (which announces itself on the
//! inbox channel). It is offered once per command and actor: the fourth run does
//! not offer again, and a command that was turned down or is already a saved
//! capsule is not offered at all.
//!
//! ## Which pane a run ran in
//!
//! A source may say which terminal pane a run ran in (an agent harness's hook
//! names the pane it runs in from the multiplexer's own variables). Such a run is
//! kept for that pane as well, whether or not it counts as a habit: a shell
//! staple or a failed run is still what that actor did there. The pane history
//! is what a working Surface's capture joins to say whose command a line is. A
//! run holding a secret is kept for no pane, like everywhere else, and the pane
//! history is pruned on the same thirty days.

use std::time::Duration;

use rusqlite::{params, Connection, Transaction, TransactionBehavior};

use aikit_core::catalog::Catalog;
use aikit_core::id::{InboxId, ProcedureId};
use aikit_core::{Kind, Result};

use crate::events::Timestamp;
use crate::home::create_dir_all;
use crate::inbox::{normalize, CandidateState, Capture, Inbox};
use crate::index::sql_error;
use crate::AikitHome;

/// How many repeats make a pattern worth offering.
pub const DEFAULT_THRESHOLD: usize = 3;
/// How far back repeats count, by default.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(7 * 24 * 3600);
/// How long an observation is kept at all.
const RETENTION: Duration = Duration::from_secs(30 * 24 * 3600);
/// A repeated one-liner is short; a long body is a document, not a habit.
const MAX_COMMAND_BYTES: usize = 8 * 1024;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS observations (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    key      TEXT NOT NULL,
    command  TEXT NOT NULL,
    cwd      TEXT,
    by       TEXT NOT NULL,
    session  TEXT,
    at_ns    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS observations_by_key ON observations(key, at_ns);
CREATE TABLE IF NOT EXISTS filings (
    key       TEXT PRIMARY KEY,
    candidate TEXT NOT NULL,
    filed_ns  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS pane_runs (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    run       TEXT NOT NULL,
    pane      TEXT NOT NULL,
    command   TEXT NOT NULL,
    cwd       TEXT,
    by        TEXT NOT NULL,
    session   TEXT,
    exit_code INTEGER,
    at_ns     INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS pane_runs_by_pane ON pane_runs(pane, at_ns);
"#;

/// The most panes one run is kept for (a harness inside a multiplexer inside
/// another is in each), and the longest name one pane may carry.
const MAX_PANES: usize = 4;
const MAX_PANE_BYTES: usize = 512;

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// One attributed command run, as a source reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedRun {
    pub command: String,
    pub cwd: Option<String>,
    /// Who ran it: `human`, or an agent ref. A run whose actor is not known is
    /// `unknown`, and is counted as its own actor rather than guessed.
    pub by: String,
    pub at: Timestamp,
    pub session: Option<String>,
    /// When the source knows how the command ended. A failed run is not a habit
    /// worth saving.
    pub exit_code: Option<i32>,
    /// The terminal panes the run ran in, as the source names them
    /// (`tmux:%3@<socket>,<server pid>`, `herdr:w1:p1@<socket>`,
    /// `cmux:surface:20`). Empty when the source does not know.
    pub panes: Vec<String>,
}

/// What counts as a repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub threshold: usize,
    pub window: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            window: DEFAULT_WINDOW,
        }
    }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Why a run was not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    Empty,
    TooLong,
    /// The source said it exited non-zero.
    FailedRun(i32),
    /// Too ordinary to be worth a name; the reason says which kind.
    Noise(&'static str),
    /// The scanner found these rules; nothing was kept.
    Secret(Vec<String>),
    /// The person already decided: turned down, or already saved.
    Decided(String),
}

impl Skip {
    pub fn code(&self) -> &'static str {
        match self {
            Skip::Empty => "empty",
            Skip::TooLong => "too-long",
            Skip::FailedRun(_) => "failed-run",
            Skip::Noise(_) => "noise",
            Skip::Secret(_) => "secret",
            Skip::Decided(_) => "decided",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Skip::Empty | Skip::TooLong => None,
            Skip::FailedRun(code) => Some(format!("exited {code}")),
            Skip::Noise(why) => Some((*why).to_string()),
            Skip::Secret(rules) => Some(rules.join(", ")),
            Skip::Decided(why) => Some(why.clone()),
        }
    }
}

/// The candidate a repeat produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offered {
    pub candidate: String,
    /// The inbox message that says so.
    pub item: Option<InboxId>,
    pub proposal: Option<ProcedureId>,
    /// False when the same text was already a candidate (captured by hand, or
    /// offered by an earlier run): nothing new was filed.
    pub new: bool,
}

/// What observing one run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    /// The command as it was compared: normalized, never holding a secret.
    pub command: String,
    pub by: String,
    pub recorded: bool,
    pub skipped: Option<Skip>,
    /// How many times this actor has run it inside the window, this run included.
    pub count: usize,
    pub threshold: usize,
    pub window: Duration,
    /// Set on the run that reached the threshold.
    pub offered: Option<Offered>,
    /// The panes this run was kept for, counted or not.
    pub panes: Vec<String>,
}

/// One run as a pane's history keeps it: what ran there, who ran it, when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRun {
    /// The pane name it was read by.
    pub pane: String,
    pub command: String,
    pub cwd: Option<String>,
    pub by: String,
    pub session: Option<String>,
    pub exit_code: Option<i32>,
    pub at: Timestamp,
}

/// One command that is repeating, for `observe status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repeating {
    pub command: String,
    pub by: String,
    pub count: usize,
    pub first: Timestamp,
    pub last: Timestamp,
    /// The candidate it was offered as, once it was.
    pub offered: Option<String>,
}

// ---------------------------------------------------------------------------
// The observer
// ---------------------------------------------------------------------------

pub struct RunObserver {
    conn: Connection,
}

impl RunObserver {
    /// Open the observation store under `home` only when it exists: a reader
    /// that finds no store reads no runs and creates nothing.
    pub fn open_existing(home: &AikitHome) -> Result<Option<Self>> {
        if !store_path(home).is_file() {
            return Ok(None);
        }
        Self::open(home).map(Some)
    }

    /// Open (creating if need be) the observation store under `home`.
    pub fn open(home: &AikitHome) -> Result<Self> {
        let path = home.state().join("observations.sqlite3");
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        let conn = Connection::open(&path).map_err(|e| sql_error("observer.open_failed", &e))?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|e| sql_error("observer.open_failed", &e))?;
        // WAL so several agents' hooks can write while one reads status.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| sql_error("observer.open_failed", &e))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| sql_error("observer.open_failed", &e))?;
        Ok(Self { conn })
    }

    /// Observe one run. May file a capture candidate through `inbox`.
    pub fn observe(
        &self,
        run: ObservedRun,
        policy: Policy,
        inbox: &Inbox<'_>,
        catalog: &dyn Catalog,
    ) -> Result<Observed> {
        let command = normalize(&run.command);
        let by = if run.by.trim().is_empty() {
            "unknown".to_string()
        } else {
            run.by.trim().to_string()
        };
        let skipped = |skip: Skip, command: String, panes: &[String]| Observed {
            command,
            by: by.clone(),
            recorded: false,
            skipped: Some(skip),
            count: 0,
            threshold: policy.threshold,
            window: policy.window,
            offered: None,
            panes: panes.to_vec(),
        };

        if command.is_empty() {
            return Ok(skipped(Skip::Empty, command, &[]));
        }
        if command.len() > MAX_COMMAND_BYTES {
            return Ok(skipped(Skip::TooLong, String::new(), &[]));
        }
        // The scan comes before everything that stores or even repeats the text:
        // a command with a secret in it is reported by rule name and nothing else.
        let findings = inbox.scan(&command);
        if !findings.is_empty() {
            let rules = findings.iter().map(|f| f.rule.to_string()).collect();
            return Ok(skipped(Skip::Secret(rules), String::new(), &[]));
        }
        // What ran in a pane is that pane's history whether or not it counts.
        let panes = self.keep_for_panes(&run, &command, &by)?;
        if let Some(code) = run.exit_code.filter(|code| *code != 0) {
            return Ok(skipped(Skip::FailedRun(code), command, &panes));
        }
        if let Some(why) = noise(&command) {
            return Ok(skipped(Skip::Noise(why), command, &panes));
        }
        if inbox.was_rejected(&command)? {
            return Ok(skipped(
                Skip::Decided("turned down before".to_string()),
                command,
                &panes,
            ));
        }
        if let Some(saved) = inbox.already_saved(&command, catalog) {
            return Ok(skipped(
                Skip::Decided(format!("already saved as {saved}")),
                command,
                &panes,
            ));
        }

        let key = key_of(&command, &by);
        let now = Timestamp::now().as_nanos();
        let at = run.at.as_nanos();
        let window = policy.window.as_nanos() as i64;

        let (count, reserved) = {
            let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
                .map_err(|e| sql_error("observer.write_failed", &e))?;
            let retention = RETENTION.as_nanos() as i64;
            tx.execute(
                "DELETE FROM observations WHERE at_ns < ?1",
                params![now - retention],
            )
            .and_then(|_| {
                tx.execute(
                    "DELETE FROM filings WHERE filed_ns < ?1",
                    params![now - retention],
                )
            })
            .and_then(|_| {
                tx.execute(
                    "INSERT INTO observations (key, command, cwd, by, session, at_ns) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![key, command, run.cwd, by, run.session, at],
                )
            })
            .map_err(|e| sql_error("observer.write_failed", &e))?;

            // The window is anchored at the newest observation of this command, so
            // a source that reports a batch out of order still counts the same.
            let newest: i64 = tx
                .query_row(
                    "SELECT MAX(at_ns) FROM observations WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .map_err(|e| sql_error("observer.query_failed", &e))?;
            let count: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM observations WHERE key = ?1 AND at_ns >= ?2",
                    params![key, newest - window],
                    |row| row.get(0),
                )
                .map_err(|e| sql_error("observer.query_failed", &e))?;
            let count = count as usize;

            let due = count >= policy.threshold && {
                let filed: i64 = tx
                    .query_row(
                        "SELECT COUNT(*) FROM filings WHERE key = ?1",
                        params![key],
                        |row| row.get(0),
                    )
                    .map_err(|e| sql_error("observer.query_failed", &e))?;
                filed == 0
            };
            if due {
                // Reserve the offer inside the transaction, so two hooks reaching
                // the threshold together cannot both file it.
                tx.execute(
                    "INSERT INTO filings (key, candidate, filed_ns) VALUES (?1, '', ?2)",
                    params![key, now],
                )
                .map_err(|e| sql_error("observer.write_failed", &e))?;
            }
            tx.commit()
                .map_err(|e| sql_error("observer.write_failed", &e))?;
            (count, due)
        };

        let mut observed = Observed {
            command: command.clone(),
            by: by.clone(),
            recorded: true,
            skipped: None,
            count,
            threshold: policy.threshold,
            window: policy.window,
            offered: None,
            panes,
        };
        if !reserved {
            return Ok(observed);
        }

        let mut capture = Capture::new(title_of(&command), command.clone())
            .of_kind(Kind::Script)
            .from_source(source_of(count, policy.window, &run))
            .by(by.clone());
        capture.project_root = None;
        match inbox.capture_against(capture, catalog) {
            Ok(outcome) => {
                self.conn
                    .execute(
                        "UPDATE filings SET candidate = ?1 WHERE key = ?2",
                        params![outcome.candidate.id, key],
                    )
                    .map_err(|e| sql_error("observer.write_failed", &e))?;
                observed.offered = Some(Offered {
                    new: outcome.duplicate_of.is_none(),
                    candidate: outcome.candidate.id,
                    item: outcome.announcement.as_ref().map(|a| a.item.clone()),
                    proposal: outcome.announcement.and_then(|a| a.proposal),
                });
                Ok(observed)
            }
            Err(error) => {
                // Release the reservation: the next repeat should be able to try.
                let _ = self
                    .conn
                    .execute("DELETE FROM filings WHERE key = ?1", params![key]);
                Err(error)
            }
        }
    }

    /// Keep one secret-free run for each pane it names (deduplicated, bounded),
    /// pruning the pane history to the retention window in the same write.
    /// Returns the panes it was kept for.
    fn keep_for_panes(&self, run: &ObservedRun, command: &str, by: &str) -> Result<Vec<String>> {
        let mut panes: Vec<String> = Vec::new();
        for pane in &run.panes {
            let pane = pane.trim();
            if pane.is_empty() || pane.len() > MAX_PANE_BYTES || panes.iter().any(|p| p == pane) {
                continue;
            }
            panes.push(pane.to_string());
            if panes.len() == MAX_PANES {
                break;
            }
        }
        if panes.is_empty() {
            return Ok(panes);
        }
        // One run kept for several panes reads back once, whichever it is read by.
        let id = aikit_core::id::EventId::generate().to_string();
        let now = Timestamp::now().as_nanos();
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
            .map_err(|e| sql_error("observer.write_failed", &e))?;
        tx.execute(
            "DELETE FROM pane_runs WHERE at_ns < ?1",
            params![now - RETENTION.as_nanos() as i64],
        )
        .map_err(|e| sql_error("observer.write_failed", &e))?;
        for pane in &panes {
            tx.execute(
                "INSERT INTO pane_runs (run, pane, command, cwd, by, session, exit_code, at_ns) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    pane,
                    command,
                    run.cwd,
                    by,
                    run.session,
                    run.exit_code,
                    run.at.as_nanos()
                ],
            )
            .map_err(|e| sql_error("observer.write_failed", &e))?;
        }
        tx.commit()
            .map_err(|e| sql_error("observer.write_failed", &e))?;
        Ok(panes)
    }

    /// The newest `limit` runs kept for any of `panes`, oldest first: what ran
    /// in a terminal pane, by whom. Several names of one pane read as one
    /// history, each run once.
    pub fn in_panes(&self, panes: &[String], limit: usize) -> Result<Vec<PaneRun>> {
        if panes.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; panes.len()].join(", ");
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT MIN(pane), command, cwd, by, session, exit_code, at_ns, MIN(id) \
                 FROM pane_runs WHERE pane IN ({marks}) GROUP BY run \
                 ORDER BY at_ns DESC, MIN(id) DESC LIMIT {limit}"
            ))
            .map_err(|e| sql_error("observer.query_failed", &e))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(panes.iter()), |row| {
                Ok(PaneRun {
                    pane: row.get(0)?,
                    command: row.get(1)?,
                    cwd: row.get(2)?,
                    by: row.get(3)?,
                    session: row.get(4)?,
                    exit_code: row.get(5)?,
                    at: Timestamp::from_nanos(row.get(6)?),
                })
            })
            .map_err(|e| sql_error("observer.query_failed", &e))?;
        let mut runs = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| sql_error("observer.query_failed", &e))?;
        runs.reverse();
        Ok(runs)
    }

    /// Commands repeating inside `window`, most repeated first. Only those seen
    /// at least twice — one run is not a pattern, and listing every command
    /// would make the list a shell history.
    pub fn repeating(&self, window: Duration) -> Result<Vec<Repeating>> {
        let since = Timestamp::now().as_nanos() - window.as_nanos() as i64;
        let mut stmt = self
            .conn
            .prepare(
                "SELECT o.command, o.by, COUNT(*), MIN(o.at_ns), MAX(o.at_ns), \
                        (SELECT f.candidate FROM filings f WHERE f.key = o.key) \
                 FROM observations o WHERE o.at_ns >= ?1 \
                 GROUP BY o.key HAVING COUNT(*) >= 2 \
                 ORDER BY COUNT(*) DESC, MAX(o.at_ns) DESC LIMIT 50",
            )
            .map_err(|e| sql_error("observer.query_failed", &e))?;
        let rows = stmt
            .query_map(params![since], |row| {
                Ok(Repeating {
                    command: row.get(0)?,
                    by: row.get(1)?,
                    count: row.get::<_, i64>(2)? as usize,
                    first: Timestamp::from_nanos(row.get(3)?),
                    last: Timestamp::from_nanos(row.get(4)?),
                    offered: row.get::<_, Option<String>>(5)?.filter(|c| !c.is_empty()),
                })
            })
            .map_err(|e| sql_error("observer.query_failed", &e))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| sql_error("observer.query_failed", &e))
    }
}

/// The file path of the observation store, for tests and `doctor`.
pub fn store_path(home: &AikitHome) -> std::path::PathBuf {
    home.state().join("observations.sqlite3")
}

/// Read the state of a candidate the observer offered: whether it is still
/// waiting. Used by status to say "offered, waiting" or "offered, decided".
pub fn candidate_waiting(inbox: &Inbox<'_>, candidate: &str) -> Result<bool> {
    Ok(inbox
        .candidate(candidate)?
        .is_some_and(|c| matches!(c.state, CandidateState::Ready | CandidateState::Quarantined)))
}

// ---------------------------------------------------------------------------
// Judgement
// ---------------------------------------------------------------------------

fn key_of(command: &str, by: &str) -> String {
    blake3::hash(format!("{by}\0{command}").as_bytes())
        .to_hex()
        .to_string()
}

fn title_of(command: &str) -> String {
    let one_line = command.lines().next().unwrap_or(command);
    let mut title: String = one_line.chars().take(80).collect();
    if one_line.chars().count() > 80 || command.lines().count() > 1 {
        title.push('…');
    }
    title
}

fn source_of(count: usize, window: Duration, run: &ObservedRun) -> String {
    let days = window.as_secs() / 86_400;
    let span = if days >= 1 {
        format!("{days}d")
    } else {
        format!("{}h", window.as_secs() / 3600)
    };
    match &run.session {
        Some(session) => format!("observed {count}x in {span} (session {session})"),
        None => format!("observed {count}x in {span}"),
    }
}

/// Commands too ordinary to be worth a name, however often they repeat. Said as
/// a reason, so a source (and a person) can see why a run was not counted.
fn noise(command: &str) -> Option<&'static str> {
    const TRIVIAL: [&str; 26] = [
        "cd", "ls", "pwd", "clear", "exit", "echo", "cat", "head", "tail", "true", "false", ":",
        "sleep", "which", "type", "history", "wc", "printf", "test", "[", "read", "export",
        "unset", "set", "source", ".",
    ];
    const GIT_READS: [&str; 8] = [
        "status",
        "diff",
        "log",
        "show",
        "branch",
        "remote",
        "rev-parse",
        "blame",
    ];
    let first_line = command.lines().next().unwrap_or(command);
    // `FOO=bar make` is `make`.
    let mut words = first_line.split_whitespace().skip_while(|word| {
        word.split_once('=').is_some_and(|(name, _)| {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    });
    let head = words.next()?;
    let tail: Vec<&str> = words.collect();
    let head = head.rsplit('/').next().unwrap_or(head);
    if head == "aikit" {
        return Some("an aikit command");
    }
    if TRIVIAL.contains(&head) {
        return Some("a shell staple");
    }
    if head == "git" && tail.first().is_some_and(|sub| GIT_READS.contains(sub)) {
        return Some("reading git state");
    }
    if tail.is_empty() && head.chars().count() <= 3 {
        return Some("a bare short command");
    }
    None
}
