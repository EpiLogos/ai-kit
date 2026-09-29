//! Probe discipline for every spawn a read, status or probe surface performs.
//!
//! The seam: [`SystemRunner`] already knows how to bound a child
//! ([`with_timeout`]); the actuation/central/workcell intake functions already
//! disclose a failed run. What was missing was the *vocabulary* and the
//! classification seam: a surface that spawns a helper binary could not name
//! what happened to the spawn in shared words, so a hang looked like silence
//! and a missing binary looked like "unavailable". This module closes that:
//!
//! * [`probe_runner`] — the bounded runner read/status surfaces construct;
//! * [`ProbeTracker`] — a [`CommandRunner`] wrapper that classifies every call
//!   it delegates into the shared [`ProbeOutcome`] vocabulary, so a caller can
//!   hand a tracker to an intake function and afterwards report *what the
//!   spawns it performed actually did*;
//! * [`run_probe`] — one bounded spawn, classified;
//! * [`which`] — PATH presence for a program, without spawning it.
//!
//! Long-lived children — a foreground `aikit harness run`, an encounter
//! resident, a background job — are the product, not probes, and stay
//! unbounded by design. Probe discipline governs the surfaces that *ask a
//! question* of an external binary.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use aikit_adapters::runner::{CommandRunner, Output, SystemRunner};
use aikit_core::probe::{probe_budget, ProbeOutcome};
use aikit_core::Result;

/// The bounded runner every read/status/probe surface constructs: the shared
/// budget (default 10s, `AIKIT_PROBE_BUDGET_SECS` overrides) instead of an
/// unbounded child.
pub fn probe_runner() -> SystemRunner {
    SystemRunner::probe()
}

/// Classify one finished runner call into the shared vocabulary. A timeout is
/// the runner's `mux.command_timeout` error; a spawn failure (binary missing,
/// unusable) is `mux.command_spawn_failed`; a non-zero exit is the target's
/// refusal.
pub(crate) fn classify(run: &Result<Output>, budget: Duration) -> ProbeOutcome {
    match run {
        Ok(output) if output.status == 0 => ProbeOutcome::Ok,
        Ok(output) => {
            let detail = if output.stderr.trim().is_empty() {
                output.stdout.trim().to_string()
            } else {
                output.stderr.trim().to_string()
            };
            ProbeOutcome::Refused {
                detail: truncate(&detail),
            }
        }
        Err(error) if error.code() == "mux.command_timeout" => ProbeOutcome::TimedOut {
            bound_secs: budget.as_secs(),
        },
        Err(error) => ProbeOutcome::Unreachable {
            reason: truncate(error.message()),
        },
    }
}

/// Refusal and spawn-failure text is disclosure, not a transcript: 240 code
/// points is enough to name the refusal and never enough to flood a status row.
fn truncate(text: &str) -> String {
    text.chars().take(240).collect()
}

/// One bounded spawn, classified into the vocabulary. The test-facing and
/// one-shot form of [`ProbeTracker`].
pub fn run_probe(budget: Duration, argv: &[String]) -> ProbeOutcome {
    let start = Instant::now();
    let outcome = classify(&SystemRunner::new().with_timeout(budget).run(argv), budget);
    debug_assert!(
        outcome.bound_secs().is_none_or(|bound| {
            start.elapsed() <= Duration::from_secs(bound) + Duration::from_secs(5)
        }),
        "a timed-out probe must return within its bound"
    );
    outcome
}

/// A [`CommandRunner`] that remembers what its spawns did, in the shared
/// vocabulary. Hand `&tracker` to an intake function exactly where a bare
/// runner would go; afterwards the surface reports the outcomes alongside the
/// intake's own reading. Delegation is unchanged: the intake sees a bounded
/// [`SystemRunner`].
pub struct ProbeTracker {
    runner: SystemRunner,
    budget: Duration,
    outcomes: Mutex<Vec<ProbeOutcome>>,
}

impl ProbeTracker {
    pub fn bounded(budget: Duration) -> Self {
        Self {
            runner: SystemRunner::new().with_timeout(budget),
            budget,
            outcomes: Mutex::new(Vec::new()),
        }
    }

    /// A tracker bounded by the shared probe budget.
    pub fn shared() -> Self {
        Self::bounded(probe_budget())
    }

    /// Every recorded outcome, in call order. Empty when nothing spawned.
    pub fn outcomes(&self) -> Vec<ProbeOutcome> {
        self.outcomes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The first recorded outcome — the leg a single-spawn surface reports.
    pub fn first(&self) -> Option<ProbeOutcome> {
        self.outcomes().into_iter().next()
    }
}

impl CommandRunner for ProbeTracker {
    fn run(&self, argv: &[String]) -> Result<Output> {
        // Pass the real answer through — intake functions parse stdout — and
        // record what it classified to, so a surface can report the spawn's
        // outcome in the shared vocabulary without changing the intake's data.
        let result = self.runner.run(argv);
        let outcome = classify(&result, self.budget);
        self.outcomes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(outcome);
        result
    }
}

/// PATH presence for one program, without spawning it: the same lookup a
/// spawn would perform, so a missing harness is `unreachable` before any
/// launch is attempted.
pub fn which(program: &str) -> Option<std::path::PathBuf> {
    let candidate = std::path::Path::new(program);
    if candidate.components().count() > 1 {
        // A path, not a bare name: presence is a filesystem question.
        return is_executable(candidate).then(|| candidate.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|full| is_executable(full))
}

fn is_executable(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(path)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(program: &str, args: &[&str]) -> Vec<String> {
        std::iter::once(program.to_string())
            .chain(args.iter().map(|arg| arg.to_string()))
            .collect()
    }

    #[test]
    fn a_probe_against_a_sleeping_fake_is_timed_out_within_the_bound() {
        let budget = Duration::from_secs(1);
        let start = Instant::now();
        let outcome = run_probe(budget, &argv("sleep", &["60"]));
        let elapsed = start.elapsed();
        assert_eq!(
            outcome,
            ProbeOutcome::TimedOut { bound_secs: 1 },
            "a sleep-60 fake must be named timed-out at its bound: {outcome:?}"
        );
        assert!(
            // Generous against suite load (the kill itself fires at 1s), while
            // still proving the probe returned inside its bound and not at
            // the fake's own 60s.
            elapsed < Duration::from_secs(10),
            "the probe must return within the bound, took {elapsed:?}"
        );
    }

    #[test]
    fn a_missing_binary_is_unreachable() {
        let outcome = run_probe(
            Duration::from_secs(5),
            &argv("aikit-probe-definitely-not-a-binary-xyz", &[]),
        );
        assert_eq!(
            outcome.as_str(),
            "unreachable",
            "a missing binary is unreachable, got {outcome:?}"
        );
        assert!(outcome.detail().is_some());
    }

    #[test]
    fn a_non_zero_exit_is_the_target_refusing() {
        let outcome = run_probe(
            Duration::from_secs(10),
            &argv("sh", &["-c", "echo denied-credential >&2; exit 2"]),
        );
        match &outcome {
            ProbeOutcome::Refused { detail } => {
                assert!(
                    detail.contains("denied-credential"),
                    "the refusal detail carries the target's own words: {detail}"
                );
            }
            other => panic!("a non-zero exit is refused, got {other:?}"),
        }
    }

    #[test]
    fn a_fast_successful_child_is_ok() {
        let outcome = run_probe(Duration::from_secs(10), &argv("sh", &["-c", "echo fine"]));
        assert_eq!(outcome, ProbeOutcome::Ok);
    }

    #[test]
    fn the_tracker_records_what_the_intake_spawns_did() {
        // Classification, not timing: the budget is generous (the shared
        // default) because this test asserts what each spawn classified to,
        // and a fork/exec under a loaded parallel suite must never be
        // misclassified as a timeout. The sleep-60 test owns the bound.
        let tracker = ProbeTracker::shared();
        let first = tracker.run(&argv("sh", &["-c", "exit 0"]));
        assert!(first.is_ok());
        let second = tracker.run(&argv("sh", &["-c", "exit 3"]));
        assert_eq!(second.unwrap().status, 3);
        assert_eq!(
            tracker.outcomes(),
            vec![
                ProbeOutcome::Ok,
                ProbeOutcome::Refused {
                    detail: String::new()
                }
            ],
            "the tracker classifies each delegated call in order"
        );
    }

    #[test]
    fn which_finds_a_real_binary_and_declines_an_invented_one() {
        // `sh` exists on every machine this suite runs on.
        assert!(which("sh").is_some(), "sh must be found on PATH");
        assert!(which("aikit-which-definitely-not-a-binary-xyz").is_none());
    }
}
