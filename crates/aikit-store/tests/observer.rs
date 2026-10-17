//! The recurrence observer: "you ran this three times; want it as a script?"
//!
//! Sources feed attributed command runs; the observer decides what repeats. These
//! tests drive it the way a feed does — one run at a time, with real timestamps,
//! against a real inbox on a temporary home — and read back what a person would
//! see: the candidate, whose name it carries, and the message on the inbox.

mod common;

use common::*;

use std::time::Duration;

use aikit_core::catalog::Catalog;
use aikit_core::{Kind, RegistrySource};
use aikit_store::channel::{InboxChannel, InboxKind};
use aikit_store::inbox::{CandidateState, Inbox};
use aikit_store::index::Index;
use aikit_store::observer::{ObservedRun, Policy, RunObserver, Skip};
use aikit_store::registry::load_registry;
use aikit_store::{AikitHome, Timestamp};

const DAY: Duration = Duration::from_secs(24 * 3600);

struct Rig {
    _tmp: tempfile::TempDir,
    home: AikitHome,
    index: Index,
    observer: RunObserver,
    catalog: aikit_store::registry::Snapshot,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path().join("home"));
    home.ensure_layout().unwrap();
    let index = Index::open(&home.database()).unwrap();
    let observer = RunObserver::open(&home).unwrap();
    let empty = RegistryFixture::at(tmp.path().join("registry"));
    let catalog = load_registry(empty.root(), RegistrySource::personal())
        .unwrap()
        .catalog;
    Rig {
        _tmp: tmp,
        home,
        index,
        observer,
        catalog,
    }
}

impl Rig {
    fn observe(&self, command: &str, by: &str, at: Timestamp) -> aikit_store::observer::Observed {
        self.observer
            .observe(
                run(command, by, at),
                Policy::default(),
                &Inbox::new(&self.home, &self.index),
                &self.catalog,
            )
            .unwrap()
    }
}

fn run(command: &str, by: &str, at: Timestamp) -> ObservedRun {
    ObservedRun {
        command: command.to_string(),
        cwd: Some("/work/app".to_string()),
        by: by.to_string(),
        at,
        session: Some("ses_1".to_string()),
        exit_code: Some(0),
    }
}

fn ago(days: u64) -> Timestamp {
    Timestamp::from_nanos(Timestamp::now().as_nanos() - (DAY.as_nanos() as i64) * days as i64)
}

const COMMAND: &str = "cargo nextest run --workspace --no-fail-fast";

#[test]
fn three_identical_agent_commands_file_one_candidate_carrying_the_agents_name() {
    let rig = rig();

    let first = rig.observe(COMMAND, "claude-code:saved-ops", ago(2));
    let second = rig.observe(COMMAND, "claude-code:saved-ops", ago(1));
    assert_eq!((first.count, second.count), (1, 2));
    assert!(first.offered.is_none() && second.offered.is_none());

    let third = rig.observe(COMMAND, "claude-code:saved-ops", ago(0));
    let offered = third.offered.expect("the third run reaches the threshold");
    assert_eq!(third.count, 3);
    assert!(offered.new);

    let inbox = Inbox::new(&rig.home, &rig.index);
    let candidate = inbox.candidate(&offered.candidate).unwrap().unwrap();
    assert_eq!(candidate.by.as_deref(), Some("claude-code:saved-ops"));
    assert_eq!(candidate.kind, Kind::Script);
    assert_eq!(candidate.state, CandidateState::Ready);
    assert!(candidate
        .source_ref
        .as_deref()
        .unwrap()
        .starts_with("observed 3x in 7d"));
    assert_eq!(
        std::fs::read_to_string(candidate.body_path()).unwrap(),
        COMMAND
    );
    assert_eq!(inbox.candidates().unwrap().len(), 1);

    // It is on the inbox, where a person or an agent will see it.
    let pending = InboxChannel::new(&rig.index)
        .pending(Timestamp::now())
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].kind, InboxKind::CaptureCandidate);
    assert_eq!(Some(pending[0].id.clone()), offered.item);
    assert!(pending[0].body.contains("claude-code:saved-ops"));

    // Offered once: the fourth and fifth runs offer nothing more.
    let fourth = rig.observe(COMMAND, "claude-code:saved-ops", Timestamp::now());
    let fifth = rig.observe(COMMAND, "claude-code:saved-ops", Timestamp::now());
    assert!(fourth.offered.is_none() && fifth.offered.is_none());
    assert_eq!((fourth.count, fifth.count), (4, 5));
    assert_eq!(inbox.candidates().unwrap().len(), 1);
}

#[test]
fn a_human_typed_run_is_attributed_to_the_human_and_counted_apart_from_an_agents() {
    let rig = rig();
    let other = "make -j8 check && make doc";

    // Two agent runs and two human runs of the same line are not a third run of anyone.
    for _ in 0..2 {
        assert!(rig
            .observe(other, "claude-code:a", ago(1))
            .offered
            .is_none());
        assert!(rig.observe(other, "human", ago(1)).offered.is_none());
    }
    let third = rig.observe(other, "human", ago(0));

    let offered = third.offered.expect("the human's third run");
    let candidate = Inbox::new(&rig.home, &rig.index)
        .candidate(&offered.candidate)
        .unwrap()
        .unwrap();
    assert_eq!(candidate.by.as_deref(), Some("human"));
}

#[test]
fn runs_spread_wider_than_the_window_are_not_a_pattern() {
    let rig = rig();

    rig.observe(COMMAND, "human", ago(20));
    rig.observe(COMMAND, "human", ago(10));
    let third = rig.observe(COMMAND, "human", ago(0));

    assert_eq!(
        third.count, 1,
        "only the newest is inside seven days of itself"
    );
    assert!(third.offered.is_none());
}

#[test]
fn a_source_that_reports_a_batch_newest_first_counts_the_same() {
    let rig = rig();

    let a = rig.observe(COMMAND, "human", ago(0));
    let b = rig.observe(COMMAND, "human", ago(3));
    let c = rig.observe(COMMAND, "human", ago(5));

    assert!(a.offered.is_none() && b.offered.is_none());
    assert!(
        c.offered.is_some(),
        "three inside the window, whatever the order"
    );
}

#[test]
fn ordinary_commands_and_failed_runs_are_not_counted() {
    let rig = rig();
    for noise in [
        "ls -la",
        "cd /tmp",
        "git status --short",
        "git diff HEAD",
        "aikit capture x --body y",
        "pwd",
        "FOO=bar ls",
        "ls",
    ] {
        let seen = rig.observe(noise, "human", Timestamp::now());
        assert!(
            matches!(seen.skipped, Some(Skip::Noise(_))),
            "{noise} should be noise, got {:?}",
            seen.skipped
        );
        assert!(!seen.recorded);
    }

    let mut failed = run("cargo build --release", "human", Timestamp::now());
    failed.exit_code = Some(101);
    let seen = rig
        .observer
        .observe(
            failed,
            Policy::default(),
            &Inbox::new(&rig.home, &rig.index),
            &rig.catalog,
        )
        .unwrap();
    assert_eq!(seen.skipped, Some(Skip::FailedRun(101)));

    let empty = rig.observe("   \n  ", "human", Timestamp::now());
    assert_eq!(empty.skipped, Some(Skip::Empty));
    let long = rig.observe(&"x ".repeat(10_000), "human", Timestamp::now());
    assert_eq!(long.skipped, Some(Skip::TooLong));
    assert!(rig.observer.repeating(DAY * 7).unwrap().is_empty());
}

#[test]
fn a_command_holding_a_secret_is_never_kept_even_in_part() {
    let rig = rig();
    let secret = "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8";
    let command = format!("curl -H 'Authorization: token {secret}' https://api.github.com/user");

    for _ in 0..4 {
        let seen = rig.observe(&command, "claude-code:x", Timestamp::now());
        assert!(
            matches!(seen.skipped, Some(Skip::Secret(_))),
            "{:?}",
            seen.skipped
        );
        assert!(seen.offered.is_none());
        assert!(
            !seen.command.contains(secret),
            "the reply never repeats the secret: {}",
            seen.command
        );
    }

    assert!(rig.observer.repeating(DAY * 7).unwrap().is_empty());
    assert!(Inbox::new(&rig.home, &rig.index)
        .candidates()
        .unwrap()
        .is_empty());
    // Not in the observation store's files either - the database or its journal.
    let state = rig.home.state();
    for entry in std::fs::read_dir(&state).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("observations.sqlite3") {
            let bytes = std::fs::read(entry.path()).unwrap();
            assert!(
                !String::from_utf8_lossy(&bytes).contains(secret),
                "{name} holds the secret"
            );
        }
    }
}

#[test]
fn a_command_that_was_turned_down_or_is_already_saved_is_not_offered_again() {
    let rig = rig();
    let inbox = Inbox::new(&rig.home, &rig.index);

    // Turned down: offered at the third run, rejected, and then never again.
    for _ in 0..3 {
        rig.observe(COMMAND, "human", Timestamp::now());
    }
    let offered = inbox.candidates().unwrap();
    assert_eq!(offered.len(), 1);
    inbox.reject(&offered[0].id, "not for me").unwrap();
    let again = rig.observe(COMMAND, "human", Timestamp::now());
    assert!(matches!(&again.skipped, Some(Skip::Decided(why)) if why.contains("turned down")));

    // Already saved: a capsule with this very text is in the catalogue.
    let fixture = RegistryFixture::at(rig._tmp.path().join("saved"));
    fixture.capsule(
        "script/personal/nt",
        "script",
        "entry = \"payload/run.sh\"",
        "",
        &[("payload/run.sh", "make -j8 check")],
    );
    let saved = load_registry(fixture.root(), RegistrySource::personal())
        .unwrap()
        .catalog;
    assert!(saved.get(&cid("script/personal/nt")).is_some());
    let seen = rig
        .observer
        .observe(
            run("make -j8 check", "human", Timestamp::now()),
            Policy::default(),
            &inbox,
            &saved,
        )
        .unwrap();
    assert!(
        matches!(&seen.skipped, Some(Skip::Decided(why)) if why.contains("script/personal/nt"))
    );
}

#[test]
fn a_capture_made_by_hand_first_is_not_filed_twice() {
    let rig = rig();
    let inbox = Inbox::new(&rig.home, &rig.index);
    let by_hand = inbox
        .capture(aikit_store::inbox::Capture::new("mine", COMMAND).of_kind(Kind::Script))
        .unwrap()
        .candidate;

    for _ in 0..3 {
        rig.observe(COMMAND, "human", Timestamp::now());
    }

    assert_eq!(inbox.candidates().unwrap().len(), 1);
    let seen = rig.observe(COMMAND, "human", Timestamp::now());
    assert!(seen.offered.is_none());
    let status = rig.observer.repeating(DAY * 7).unwrap();
    assert_eq!(status[0].offered.as_deref(), Some(by_hand.id.as_str()));
}

#[test]
fn the_threshold_and_window_are_the_callers_to_set() {
    let rig = rig();
    let policy = Policy {
        threshold: 2,
        window: Duration::from_secs(3600),
    };
    let inbox = Inbox::new(&rig.home, &rig.index);
    let observe = |at| {
        rig.observer
            .observe(
                run("npm run build:all", "human", at),
                policy,
                &inbox,
                &rig.catalog,
            )
            .unwrap()
    };

    let first = observe(Timestamp::now());
    let second = observe(Timestamp::now());

    assert!(first.offered.is_none());
    assert!(second.offered.is_some(), "two is enough when asked for two");
}

#[test]
fn many_hooks_reaching_the_threshold_together_file_exactly_one_candidate() {
    let rig = rig();
    let home_root = rig.home.root().to_path_buf();

    let threads: Vec<_> = (0..8)
        .map(|n| {
            let root = home_root.clone();
            std::thread::spawn(move || {
                let home = AikitHome::at(root);
                let index = Index::open(&home.database()).unwrap();
                let observer = RunObserver::open(&home).unwrap();
                let catalog = aikit_store::registry::RegistryLoad::default().catalog;
                observer
                    .observe(
                        run(
                            "docker compose up -d --build",
                            "claude-code:x",
                            Timestamp::now(),
                        ),
                        Policy::default(),
                        &Inbox::new(&home, &index),
                        &catalog,
                    )
                    .map(|seen| (n, seen.offered.is_some()))
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let failures: Vec<String> = results
        .iter()
        .filter_map(|r| r.as_ref().err().map(|e| e.to_string()))
        .collect();
    assert!(failures.is_empty(), "{failures:?}");
    let offers = results
        .iter()
        .filter(|r| r.as_ref().is_ok_and(|(_, offered)| *offered))
        .count();

    assert_eq!(offers, 1);
    assert_eq!(
        Inbox::new(&rig.home, &rig.index)
            .candidates()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn status_lists_what_is_repeating_and_whether_it_was_offered() {
    let rig = rig();
    rig.observe("terraform plan -out=tf.plan", "human", ago(1));
    rig.observe("terraform plan -out=tf.plan", "human", ago(0));
    rig.observe(COMMAND, "claude-code:x", Timestamp::now());

    let repeating = rig.observer.repeating(DAY * 7).unwrap();

    assert_eq!(repeating.len(), 1, "one run is not a pattern");
    assert_eq!(repeating[0].command, "terraform plan -out=tf.plan");
    assert_eq!(repeating[0].count, 2);
    assert_eq!(repeating[0].by, "human");
    assert!(repeating[0].offered.is_none());
}
