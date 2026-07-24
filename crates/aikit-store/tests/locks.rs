//! Per-context advisory locks.
//!
//! Coordination between AIKit processes is "SQLite transactions plus per-context
//! file locks" — there is no daemon to arbitrate. So these tests contend for a
//! **real** lock file from two **real** threads (`flock` is per open file
//! description, so two opens in one process contend exactly as two processes
//! would) and assert the thing a user actually needs: when you lose the race you
//! are told who won, not just that something went wrong.

mod common;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use aikit_store::locks::{ContextLock, LockOptions};
use aikit_store::AikitHome;

fn home(dir: &std::path::Path) -> AikitHome {
    let home = AikitHome::at(dir);
    home.ensure_layout().unwrap();
    home
}

#[test]
fn acquiring_creates_the_lock_file_and_releasing_leaves_it_unlocked() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());

    let guard = ContextLock::acquire(&home, "ctx_one", LockOptions::default()).unwrap();
    let path = guard.path().to_path_buf();
    assert!(path.is_file());
    drop(guard);

    // A second acquisition proves the first really released rather than merely
    // going out of scope with the descriptor still open.
    let again = ContextLock::acquire(&home, "ctx_one", LockOptions::default()).unwrap();
    assert_eq!(again.path(), path);
}

#[test]
fn two_threads_contending_for_one_context_produce_lock_busy_naming_the_holder() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let holder_home = home.clone();
    let holder = std::thread::spawn(move || {
        let guard = ContextLock::acquire(
            &holder_home,
            "ctx_contended",
            LockOptions::default().with_purpose("apply"),
        )
        .unwrap();
        acquired_tx.send(()).unwrap();
        // Hold it until the contender has had its turn.
        release_rx.recv().unwrap();
        drop(guard);
    });

    acquired_rx.recv().unwrap();

    let error = ContextLock::acquire(
        &home,
        "ctx_contended",
        LockOptions::default().with_timeout(Duration::from_millis(80)),
    )
    .expect_err("the second contender must not get the lock");

    assert_eq!(error.code(), "lock.busy");
    let details = error.details();
    assert_eq!(
        details.get("pid").map(String::as_str),
        Some(std::process::id().to_string().as_str()),
        "the message has to name the holder, not just say `busy`"
    );
    assert_eq!(details.get("purpose").map(String::as_str), Some("apply"));
    assert!(details.contains_key("path"));
    assert!(error.message().contains("apply"));

    release_tx.send(()).unwrap();
    holder.join().unwrap();
}

#[test]
fn a_contender_waits_up_to_the_timeout_and_then_succeeds_when_the_holder_leaves() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let (acquired_tx, acquired_rx) = mpsc::channel();

    let holder_home = home.clone();
    let holder = std::thread::spawn(move || {
        let guard = ContextLock::acquire(&holder_home, "ctx_slow", LockOptions::default()).unwrap();
        acquired_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        drop(guard);
    });
    acquired_rx.recv().unwrap();

    let started = Instant::now();
    let guard = ContextLock::acquire(
        &home,
        "ctx_slow",
        LockOptions::default().with_timeout(Duration::from_secs(5)),
    )
    .expect("the contender should get the lock once the holder releases it");

    assert!(
        started.elapsed() >= Duration::from_millis(80),
        "it should actually have waited"
    );
    drop(guard);
    holder.join().unwrap();
}

#[test]
fn locks_for_different_contexts_do_not_contend() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());

    let a = ContextLock::acquire(&home, "ctx_a", LockOptions::default()).unwrap();
    let b = ContextLock::acquire(
        &home,
        "ctx_b",
        LockOptions::default().with_timeout(Duration::from_millis(50)),
    )
    .expect("a different context is a different lock");

    assert_ne!(a.path(), b.path());
}

#[test]
fn a_zero_timeout_fails_immediately_rather_than_blocking_forever() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let holder_home = home.clone();
    let holder = std::thread::spawn(move || {
        let guard = ContextLock::acquire(&holder_home, "ctx_zero", LockOptions::default()).unwrap();
        acquired_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        drop(guard);
    });
    acquired_rx.recv().unwrap();

    let started = Instant::now();
    let error = ContextLock::acquire(
        &home,
        "ctx_zero",
        LockOptions::default().with_timeout(Duration::ZERO),
    )
    .unwrap_err();
    assert_eq!(error.code(), "lock.busy");
    assert!(started.elapsed() < Duration::from_millis(500));

    release_tx.send(()).unwrap();
    holder.join().unwrap();
}

#[test]
fn the_holder_record_is_readable_while_the_lock_is_held() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());

    let guard = ContextLock::acquire(
        &home,
        "ctx_readable",
        LockOptions::default().with_purpose("gc"),
    )
    .unwrap();

    let holder = aikit_store::locks::read_holder(guard.path()).unwrap();
    assert_eq!(holder.pid, std::process::id());
    assert_eq!(holder.purpose, "gc");
    assert!(!holder.describe().is_empty());
}
