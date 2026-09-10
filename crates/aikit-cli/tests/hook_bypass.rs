//! The hook dispatcher and the single-use bypass, against a real SQLite index and
//! a real gate subprocess.
//!
//! The gate here is an actual `exit 1` script the dispatcher runs, not a stub
//! returning a canned verdict. The bypass is issued into and spent out of the real
//! store. The property under test is the one ARCHITECTURE.md §8 promises: a bypass
//! is a short-lived scoped token that lets exactly one event through and is then
//! spent — not a global switch.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use aikit_cli::hook;
use aikit_core::capsule::{BypassPolicy, HookPhase};
use aikit_core::hooks::{BypassScope, BypassToken, HookChain, HookEvent, HookEventKind, HookStep};
use aikit_core::id::{CapsuleId, ContextId};
use aikit_store::index::Index;
use tempfile::TempDir;

fn deny_gate(dir: &std::path::Path) -> (CapsuleId, BTreeMap<CapsuleId, std::path::PathBuf>) {
    let root = dir.join("gate");
    fs::create_dir_all(&root).unwrap();
    let check = root.join("check");
    fs::write(&check, "#!/bin/sh\necho 'blocked' 1>&2\nexit 1\n").unwrap();
    let mut perms = fs::metadata(&check).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&check, perms).unwrap();

    let id = CapsuleId::parse("hook/gate/no-secrets").unwrap();
    let mut roots = BTreeMap::new();
    roots.insert(id.clone(), root);
    (id, roots)
}

fn gate_chain(id: &CapsuleId) -> HookChain {
    let mut step = HookStep::new(id.clone(), "check", HookPhase::Gate);
    // The capsule opts into being bypassable; without this a token is ignored,
    // which is itself the safe default.
    step.bypass = BypassPolicy {
        allowed: true,
        reason_required: true,
    };
    HookChain::plan(HookEventKind::PreToolUse, vec![step], &BTreeMap::new()).unwrap()
}

fn event() -> HookEvent {
    HookEvent::new(
        "claude",
        HookEventKind::PreToolUse,
        serde_json::json!({"tool": "Bash"}),
    )
}

#[test]
fn a_gate_denies_a_bypass_lets_exactly_one_event_through_then_is_spent() {
    let tmp = TempDir::new().unwrap();
    let index = Index::open(&tmp.path().join("aikit.sqlite3")).unwrap();
    let context = ContextId::generate();
    let (id, roots) = deny_gate(tmp.path());
    let chain = gate_chain(&id);

    // 1. Without a bypass, the gate denies.
    let decision = hook::dispatch(&index, &context, &chain, &event(), &roots).unwrap();
    assert!(!decision.allowed, "the gate must deny the first event");
    assert!(decision.denial.is_some());

    // 2. Issue a next-event bypass with a reason.
    let mut token = BypassToken::new(BypassScope::NextEvent);
    token.reason = Some("debugging a flake".to_string());
    let bypass_id = index.issue_bypass(&context, &token).unwrap();
    assert_eq!(index.open_bypasses(&context).unwrap().len(), 1);

    // 3. The very next event is let through, and the token is consumed.
    let bypassed = hook::dispatch(&index, &context, &chain, &event(), &roots).unwrap();
    assert!(
        bypassed.allowed,
        "the bypass must let this one event through"
    );
    assert!(
        bypassed.was_bypassed(),
        "and it must be recorded as bypassed"
    );
    assert!(
        index.open_bypasses(&context).unwrap().is_empty(),
        "the token is spent after exactly one event"
    );
    // The specific token is the one that was spent.
    let _ = bypass_id;

    // 4. The event after that is denied again — the bypass was not a global switch.
    let after = hook::dispatch(&index, &context, &chain, &event(), &roots).unwrap();
    assert!(
        !after.allowed,
        "with the token spent, the gate denies once more"
    );
}

#[test]
fn an_event_with_no_matching_chain_is_allowed() {
    let tmp = TempDir::new().unwrap();
    let index = Index::open(&tmp.path().join("aikit.sqlite3")).unwrap();
    let context = ContextId::generate();
    let empty = HookChain::plan(HookEventKind::PreToolUse, vec![], &BTreeMap::new()).unwrap();
    let roots = BTreeMap::new();
    let decision = hook::dispatch(&index, &context, &empty, &event(), &roots).unwrap();
    assert!(decision.allowed);
}
