//! Stdout as content, phase-gated: a step in the inject phase turns non-empty
//! stdout into injected context; every other phase treats stdout as a gate
//! channel whose exit status is the whole verdict. The scripts here are real
//! subprocesses, not stubs — the property under test lives exactly at the
//! boundary between the capsule's bytes and the dispatcher's verdict.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use aikit_cli::hook;
use aikit_core::capsule::HookPhase;
use aikit_core::hooks::{HookChain, HookEvent, HookEventKind, HookStep, StepOutcome};
use aikit_core::id::{CapsuleId, ContextId};
use aikit_store::index::Index;
use tempfile::TempDir;

/// A capsule whose payload prints `text` on stdout and exits `status`.
fn script_capsule(
    dir: &std::path::Path,
    name: &str,
    text: &str,
    status: u8,
) -> (CapsuleId, BTreeMap<CapsuleId, std::path::PathBuf>) {
    let root = dir.join(name);
    fs::create_dir_all(&root).unwrap();
    let script = root.join("run");
    fs::write(
        &script,
        format!("#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' '{text}'\nexit {status}\n"),
    )
    .unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();

    let id = CapsuleId::parse(&format!("hook/steer/{name}")).unwrap();
    let mut roots = BTreeMap::new();
    roots.insert(id.clone(), root);
    (id, roots)
}

fn chain(id: &CapsuleId, phase: HookPhase) -> HookChain {
    HookChain::plan(
        HookEventKind::SessionStart,
        vec![HookStep::new(id.clone(), "run", phase)],
        &BTreeMap::new(),
    )
    .unwrap()
}

fn session_start() -> HookEvent {
    HookEvent::new(
        "claude",
        HookEventKind::SessionStart,
        serde_json::json!({"cwd": "/tmp"}),
    )
}

fn index(tmp: &TempDir) -> Index {
    Index::open(&tmp.path().join("aikit.sqlite3")).unwrap()
}

#[test]
fn inject_phase_stdout_rides_the_decision() {
    let tmp = TempDir::new().unwrap();
    let index = index(&tmp);
    let context = ContextId::generate();
    let (id, roots) = script_capsule(tmp.path(), "injector", "steer toward the native route", 0);
    let decision = hook::dispatch(
        &index,
        &context,
        &chain(&id, HookPhase::Inject),
        &session_start(),
        &roots,
    )
    .unwrap();

    assert!(decision.allowed);
    assert_eq!(decision.injected, vec!["steer toward the native route"]);
    let step = decision.step(&id.to_string()).unwrap();
    assert_eq!(step.outcome, StepOutcome::Injected);
}

#[test]
fn gate_phase_stdout_is_a_gate_channel_and_stays_ignored() {
    let tmp = TempDir::new().unwrap();
    let index = index(&tmp);
    let context = ContextId::generate();
    let (id, roots) = script_capsule(tmp.path(), "loud-gate", "this must not inject", 0);
    let decision = hook::dispatch(
        &index,
        &context,
        &chain(&id, HookPhase::Gate),
        &session_start(),
        &roots,
    )
    .unwrap();

    assert!(decision.allowed);
    assert!(
        decision.injected.is_empty(),
        "a gate must not smuggle content into the session: {:?}",
        decision.injected
    );
    let step = decision.step(&id.to_string()).unwrap();
    assert_eq!(step.outcome, StepOutcome::Allowed);
}

#[test]
fn empty_stdout_from_an_inject_step_injects_nothing() {
    let tmp = TempDir::new().unwrap();
    let index = index(&tmp);
    let context = ContextId::generate();
    let root = tmp.path().join("silent");
    fs::create_dir_all(&root).unwrap();
    let script = root.join("run");
    fs::write(&script, "#!/bin/sh\ncat > /dev/null\nexit 0\n").unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();
    let id = CapsuleId::parse("hook/steer/silent").unwrap();
    let mut roots = BTreeMap::new();
    roots.insert(id.clone(), root);

    let decision = hook::dispatch(
        &index,
        &context,
        &chain(&id, HookPhase::Inject),
        &session_start(),
        &roots,
    )
    .unwrap();

    assert!(decision.allowed);
    assert!(decision.injected.is_empty());
    assert_eq!(
        decision.step(&id.to_string()).unwrap().outcome,
        StepOutcome::Allowed
    );
}

#[test]
fn an_inject_step_cannot_deny_its_nonzero_exit_is_recorded_not_enforced() {
    let tmp = TempDir::new().unwrap();
    let index = index(&tmp);
    let context = ContextId::generate();
    let (id, roots) = script_capsule(tmp.path(), "failing-injector", "partial output", 1);
    let decision = hook::dispatch(
        &index,
        &context,
        &chain(&id, HookPhase::Inject),
        &session_start(),
        &roots,
    )
    .unwrap();

    // The inject phase cannot deny, so the event continues; the refusal is a
    // warning and the step is recorded as denied, never as a gate decision.
    assert!(decision.allowed);
    assert!(decision.denial.is_none());
    assert!(
        decision.warnings.iter().any(|w| w.contains("cannot deny")),
        "the recorded warning must name the phase's missing teeth: {:?}",
        decision.warnings
    );
    let step = decision.step(&id.to_string()).unwrap();
    assert_eq!(step.outcome, StepOutcome::Denied);
}
