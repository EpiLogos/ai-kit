//! Close-out verification (W2/CASE 08): did the close-out actually leave the
//! objects it claims to have left?
//!
//! The star `*end` protocol names routes; this reads the carriers back and
//! answers in clauses. It asks the native owners — Central's NOW field through
//! `ctrl projectcentral.now.inspect`, Factory's development ledger through
//! `factory development observations` — and reports what they hold. It never
//! writes anything, and it never infers a pass from having asked.
//!
//! A clause that cannot apply (no Factory Run bound to this session) is
//! `skipped`, not passed. "I did not check this" and "this is fine" are
//! different answers and are printed differently.

use std::path::Path;

use aikit_adapters::runner::CommandRunner;
use serde::Serialize;
use serde_json::Value;

/// Whether a clause held, did not hold, or could not be asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClauseState {
    Pass,
    Fail,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct Clause {
    pub clause: &'static str,
    pub state: ClauseState,
    /// What the owner actually reported — the evidence, not a restatement of
    /// the verdict.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Verification {
    pub project: String,
    /// True when every clause that could be asked held.
    pub verified: bool,
    pub clauses: Vec<Clause>,
}

impl Verification {
    /// Render the human reading: one line per clause, evidence included.
    pub fn describe(&self) -> String {
        let mut lines = vec![format!(
            "close-out verification — project {} — {}",
            self.project,
            if self.verified {
                "verified"
            } else {
                "NOT verified"
            }
        )];
        for clause in &self.clauses {
            let mark = match clause.state {
                ClauseState::Pass => "ok",
                ClauseState::Fail => "FAIL",
                ClauseState::Skipped => "skipped",
            };
            lines.push(format!("  [{mark}] {}: {}", clause.clause, clause.detail));
        }
        lines.join("\n")
    }
}

/// The Factory ledger binding to check deferred work against, when the session
/// has one.
#[derive(Debug, Clone)]
pub struct FactoryBinding<'a> {
    pub binary: &'a str,
    pub ledger_root: &'a str,
    pub run_ref: &'a str,
}

/// Verify the close-out for `project`.
///
/// `since` bounds "this close-out": with it, learned material and the
/// continuation must have been recorded at or after that instant, so a passing
/// clause cannot be satisfied by a record from last week.
pub fn verify<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project: &str,
    since: Option<i64>,
    factory: Option<FactoryBinding<'_>>,
) -> Result<Verification, String> {
    let field = crate::orientation_packet::inspect_project_now_for(
        runner,
        central_root,
        project,
        "close-out verification",
    )?;
    if field["exists"] != true {
        return Err(format!(
            "close-out verification: project {project} has no NOW field to read"
        ));
    }
    let empty = Vec::new();
    let active = field["active_items"].as_array().unwrap_or(&empty);
    let recorded_at = |item: &Value| item["recorded_at_unix_seconds"].as_i64().unwrap_or(0);
    let within = |item: &Value| {
        since
            .map(|since| recorded_at(item) >= since)
            .unwrap_or(true)
    };

    let handoffs: Vec<&Value> = active
        .iter()
        .filter(|item| item["kind"].as_str() == Some("handoff"))
        .collect();
    let newest = handoffs
        .iter()
        .copied()
        .max_by_key(|item| recorded_at(item));

    let mut clauses = Vec::new();

    // 1. A continuation exists, and it belongs to this close-out.
    clauses.push(match newest {
        Some(item) if within(item) => Clause {
            clause: "continuation-registered",
            state: ClauseState::Pass,
            detail: format!(
                "{} — {}",
                item["id"].as_str().unwrap_or("(no id)"),
                item["subject"].as_str().unwrap_or("(untitled)")
            ),
        },
        Some(item) => Clause {
            clause: "continuation-registered",
            state: ClauseState::Fail,
            detail: format!(
                "the newest open handoff {} was recorded at {}, before this close-out began",
                item["id"].as_str().unwrap_or("(no id)"),
                recorded_at(item)
            ),
        },
        None => Clause {
            clause: "continuation-registered",
            state: ClauseState::Fail,
            detail: "no open handoff return in the field".to_owned(),
        },
    });

    // 2. Exactly one stands open: a new continuation supersedes the previous
    //    one rather than joining it. Two open handoffs is the failure this
    //    clause exists to catch — a fresh session would not know which to
    //    resume.
    clauses.push(match handoffs.len() {
        1 => Clause {
            clause: "previous-handoff-superseded",
            state: ClauseState::Pass,
            detail: "exactly one handoff return stands open".to_owned(),
        },
        0 => Clause {
            clause: "previous-handoff-superseded",
            state: ClauseState::Fail,
            detail: "no handoff return stands open".to_owned(),
        },
        count => Clause {
            clause: "previous-handoff-superseded",
            state: ClauseState::Fail,
            detail: format!(
                "{count} handoff returns stand open ({}); the superseded ones were not closed",
                handoffs
                    .iter()
                    .map(|item| item["id"].as_str().unwrap_or("(no id)"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
    });

    // 3. Durable learned material was captured — as learned material, which is
    //    not authored source and never becomes it without Recognition.
    let learnings: Vec<&Value> = active
        .iter()
        .filter(|item| item["kind"].as_str() == Some("learning"))
        .filter(|item| within(item))
        .collect();
    clauses.push(if learnings.is_empty() {
        Clause {
            clause: "learned-material-recorded",
            state: ClauseState::Fail,
            detail: match since {
                Some(since) => format!("no learning return recorded at or after {since}"),
                None => "no learning return in the field".to_owned(),
            },
        }
    } else {
        Clause {
            clause: "learned-material-recorded",
            state: ClauseState::Pass,
            detail: format!(
                "{} learning return(s): {}",
                learnings.len(),
                learnings
                    .iter()
                    .map(|item| item["id"].as_str().unwrap_or("(no id)"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    });

    // 4. Deferred work, when this session has a Factory Run to defer into.
    clauses.push(match factory {
        None => Clause {
            clause: "deferred-work-registered",
            state: ClauseState::Skipped,
            detail: "no Factory Run ledger bound to this session; not checked".to_owned(),
        },
        Some(binding) => match observations(runner, &binding) {
            Ok(count) if count > 0 => Clause {
                clause: "deferred-work-registered",
                state: ClauseState::Pass,
                detail: format!(
                    "{count} development observation(s) open in {}",
                    binding.run_ref
                ),
            },
            Ok(_) => Clause {
                clause: "deferred-work-registered",
                state: ClauseState::Fail,
                detail: format!("no development observation in {}", binding.run_ref),
            },
            Err(error) => Clause {
                clause: "deferred-work-registered",
                state: ClauseState::Fail,
                detail: error,
            },
        },
    });

    let verified = clauses
        .iter()
        .all(|clause| clause.state != ClauseState::Fail);
    Ok(Verification {
        project: project.to_owned(),
        verified,
        clauses,
    })
}

/// How many observations the Run's ledger holds, read through Factory's own
/// command rather than by reading its files.
fn observations<R: CommandRunner>(runner: &R, binding: &FactoryBinding<'_>) -> Result<u64, String> {
    let argv = vec![
        binding.binary.to_owned(),
        "development".to_owned(),
        "observations".to_owned(),
        binding.ledger_root.to_owned(),
        binding.run_ref.to_owned(),
        "--json".to_owned(),
    ];
    let output = runner
        .run(&argv)
        .map_err(|error| format!("factory unavailable: {error}"))?;
    if output.status != 0 {
        return Err(format!(
            "factory development observations failed ({}): {}",
            output.status,
            output.stderr.trim()
        ));
    }
    let reading: Value = serde_json::from_str(&output.stdout)
        .map_err(|error| format!("unreadable factory reply: {error}"))?;
    reading["observationCount"]
        .as_u64()
        .ok_or_else(|| "factory reply carried no observationCount".to_owned())
}
