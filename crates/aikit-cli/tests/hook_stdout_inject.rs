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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn native_capture_carries_exact_event_and_drains_verbose_hook_output() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("exact-input");
    fs::create_dir_all(&root).unwrap();
    let script = root.join("run");
    fs::write(&script,"#!/bin/sh\ncat > captured.json\nprintf '%s\n' \"$PWD\"\nhead -c 262144 /dev/zero | tr '\\000' 'x'\n").unwrap();
    let mut mode = fs::metadata(&script).unwrap().permissions();
    mode.set_mode(0o755);
    fs::set_permissions(&script, mode).unwrap();
    let event = HookEvent::new(
        "pi",
        HookEventKind::PreToolUse,
        serde_json::json!({"context":"source-bound","body":"retained".repeat(65536)}),
    );
    let id = CapsuleId::parse("hook/native/exact-input").unwrap();
    let mut step = HookStep::new(id, "run", HookPhase::Inject);
    step.timeout = Some(serde_json::from_value(serde_json::json!("3s")).unwrap());
    let result = hook::run_hook_step(&step, &event, &root);
    match result.verdict {
        aikit_core::hooks::StepVerdict::Inject { text } => {
            assert_eq!(
                std::path::Path::new(text.lines().next().unwrap())
                    .canonicalize()
                    .unwrap(),
                root.canonicalize().unwrap()
            );
            assert!(
                text.ends_with(&"x".repeat(262144)),
                "verbose child stdout must be drained before exit"
            );
        }
        verdict => panic!("actual hook did not complete: {verdict:?}"),
    }
    let retained: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("captured.json")).unwrap()).unwrap();
    assert_eq!(
        retained, event.payload,
        "the complete event reached native stdin, not null stdin or a truncated pipe write"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn native_hook_bounds_nonreading_input_and_retires_inherited_or_closed_pipe_children() {
    use aikit_adapters::runner::{CommandRunner, SystemRunner};
    use std::time::{Duration, Instant};
    for (name, body, timeout, failure) in [
        (
            "nonreading",
            "sleep 30 & echo $! > child.pid; wait",
            "1s",
            true,
        ),
        (
            "inherited",
            "cat >/dev/null; sleep 30 & echo $! > child.pid; printf 'partial'; exit 0",
            "1s",
            true,
        ),
        (
            "closed",
            "cat >/dev/null; sleep 30 >/dev/null 2>&1 & echo $! > child.pid; printf 'complete'; exit 0",
            "2s",
            false,
        ),
    ] {
        let tmp = TempDir::new().unwrap();
        let script = tmp.path().join("run");
        fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut mode = fs::metadata(&script).unwrap().permissions();
        mode.set_mode(0o755);
        fs::set_permissions(&script, mode).unwrap();
        let mut step = HookStep::new(
            CapsuleId::parse(&format!("hook/native/{name}")).unwrap(),
            "run",
            HookPhase::Inject,
        );
        step.timeout = Some(serde_json::from_value(serde_json::json!(timeout)).unwrap());
        let event = HookEvent::new(
            "pi",
            HookEventKind::PreToolUse,
            serde_json::json!({"body":"bounded".repeat(65536)}),
        );
        let start = Instant::now();
        let result = hook::run_hook_step(&step, &event, tmp.path());
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{name} must not wait for the natural 30s child completion"
        );
        if failure {
            assert!(
                matches!(
                    result.verdict,
                    aikit_core::hooks::StepVerdict::SystemFailure { .. }
                ),
                "{name}: {result:?}"
            );
        } else {
            assert_eq!(
                result.verdict,
                aikit_core::hooks::StepVerdict::Inject {
                    text: "complete".into()
                }
            );
        }
        let pid = fs::read_to_string(tmp.path().join("child.pid")).unwrap();
        assert!(
            pid.trim().parse::<u32>().unwrap() > 0,
            "an actual child PID is required"
        );
        // Read only this test's actual owned PID; a zombie is retired material,
        // not a live retained worker. No foreign process or session is inspected.
        let actual = SystemRunner::probe()
            .run(&[
                "/bin/ps".into(),
                "-o".into(),
                "stat=".into(),
                "-p".into(),
                pid.trim().into(),
            ])
            .unwrap();
        assert!(
            actual.stdout.trim().is_empty() || actual.stdout.trim().starts_with('Z'),
            "{name} left unexplained live owned process {pid}: {}",
            actual.stdout
        );
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn native_gate_keeps_status_only_binary_stdout_and_inject_refuses_it() {
    let tmp = TempDir::new().unwrap();
    let script = tmp.path().join("run");
    fs::write(
        &script,
        "#!/bin/sh\ncat >/dev/null\nprintf '\\377'\nexit 0\n",
    )
    .unwrap();
    let mut mode = fs::metadata(&script).unwrap().permissions();
    mode.set_mode(0o755);
    fs::set_permissions(&script, mode).unwrap();
    let id = CapsuleId::parse("hook/native/binary-output").unwrap();
    let gate = HookStep::new(id.clone(), "run", HookPhase::Gate);
    assert_eq!(
        hook::run_hook_step(&gate, &session_start(), tmp.path()).verdict,
        aikit_core::hooks::StepVerdict::Allow
    );
    let inject = HookStep::new(id, "run", HookPhase::Inject);
    assert!(matches!(
        hook::run_hook_step(&inject, &session_start(), tmp.path()).verdict,
        aikit_core::hooks::StepVerdict::SystemFailure { .. }
    ));
}
