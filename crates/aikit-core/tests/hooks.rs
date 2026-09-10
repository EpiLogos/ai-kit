//! Hook chains are the one place where AIKit can say "no" to an agent, so the
//! tests here are about the properties that make that trustworthy: a fixed order,
//! a real short-circuit, observers that can never veto, and — above all — a system
//! failure that is never mistaken for a policy decision.
//!
//! The chains are built from real resolved views over real manifests, and the
//! dispatcher is driven with a real runner closure that records what it was asked
//! to do, so a stubbed implementation would not pass.

mod common;

use common::*;

use std::collections::BTreeMap;

use aikit_core::capsule::HookPhase;
use aikit_core::hooks::{
    build_chains, BypassScope, BypassToken, Dispatcher, HookChain, HookEvent, HookEventKind,
    HookStep, StepOutcome, StepResult,
};
use aikit_core::scope::ScopeKind;

fn enabled(capsules: Vec<aikit_core::capsule::Capsule>, enable: &[&str]) -> Fixture {
    Fixture::new(capsules).with_layers(vec![layer(ScopeKind::Project, enable, &[])])
}

fn chains(f: &Fixture) -> BTreeMap<String, HookChain> {
    let view = f.resolve().expect("fixture must resolve");
    build_chains(&view, &f.catalog).expect("fixture chains must be orderable")
}

fn chain(f: &Fixture, event: &str) -> HookChain {
    chains(f)
        .remove(event)
        .unwrap_or_else(|| panic!("no chain for {event}"))
}

fn pre_tool_use(tool: &str) -> HookEvent {
    HookEvent::new("claude", HookEventKind::PreToolUse, serde_json::json!({}))
        .with_tool_name(tool)
        .in_cwd("/work/payments")
}

fn step_ids(chain: &HookChain) -> Vec<String> {
    chain.steps.iter().map(|s| s.capsule.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Event normalization
// ---------------------------------------------------------------------------

#[test]
fn event_kinds_round_trip_through_the_clients_own_spelling() {
    for spelling in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "Stop",
        "SessionEnd",
        "PreCompact",
        "Notification",
    ] {
        let kind = HookEventKind::parse(spelling);
        assert_eq!(kind.as_str(), spelling, "{spelling} must render back");
    }
}

#[test]
fn an_unknown_event_is_carried_through_rather_than_dropped() {
    let kind = HookEventKind::parse("SubagentStop");
    assert_eq!(kind, HookEventKind::Other("SubagentStop".into()));
    assert_eq!(kind.as_str(), "SubagentStop");
}

#[test]
fn a_manifest_may_spell_an_event_in_kebab_or_snake_case() {
    assert_eq!(
        HookEventKind::parse("pre-tool-use"),
        HookEventKind::PreToolUse
    );
    assert_eq!(
        HookEventKind::parse("pre_tool_use"),
        HookEventKind::PreToolUse
    );
    assert_eq!(
        HookEventKind::parse("PRETOOLUSE"),
        HookEventKind::PreToolUse
    );
}

#[test]
fn chains_are_keyed_by_the_normalized_event_name_whatever_the_manifest_wrote() {
    let f = enabled(
        vec![hook_table(
            "hook/gate/boundary",
            "",
            "entry = \"payload/check\"\nevents = [\"pre-tool-use\"]",
        )],
        &["hook/gate/boundary"],
    );
    let built = chains(&f);
    assert_eq!(built.keys().collect::<Vec<_>>(), vec!["PreToolUse"]);
}

#[test]
fn a_hook_only_joins_the_chains_for_the_events_it_declares() {
    let f = enabled(
        vec![
            hook_table(
                "hook/gate/boundary",
                "",
                "entry = \"payload/check\"\nevents = [\"PreToolUse\", \"PostToolUse\"]",
            ),
            hook_table(
                "hook/capture/transcript",
                "",
                "entry = \"payload/capture\"\nevents = [\"Stop\"]\nphase = \"capture\"",
            ),
        ],
        &["hook/gate/boundary", "hook/capture/transcript"],
    );
    let built = chains(&f);
    assert_eq!(
        built.keys().map(|k| k.as_str()).collect::<Vec<_>>(),
        vec!["PostToolUse", "PreToolUse", "Stop"]
    );
    assert_eq!(step_ids(&built["Stop"]), vec!["hook/capture/transcript"]);
    assert_eq!(step_ids(&built["PreToolUse"]), vec!["hook/gate/boundary"]);
}

// ---------------------------------------------------------------------------
// Ordering
// ---------------------------------------------------------------------------

#[test]
fn a_chain_is_ordered_by_phase_then_order_then_capsule_id() {
    let f = enabled(
        vec![
            hook_table(
                "hook/observe/z-log",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"observe\"\norder = 1",
            ),
            hook_table(
                "hook/verify/b-second",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"\norder = 50",
            ),
            hook_table(
                "hook/verify/a-first",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"\norder = 50",
            ),
            hook_table(
                "hook/gate/early",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"gate\"\norder = 90",
            ),
        ],
        &[
            "hook/observe/z-log",
            "hook/verify/b-second",
            "hook/verify/a-first",
            "hook/gate/early",
        ],
    );

    assert_eq!(
        step_ids(&chain(&f, "PreToolUse")),
        vec![
            "hook/gate/early",
            "hook/verify/a-first",
            "hook/verify/b-second",
            "hook/observe/z-log",
        ],
        "phase must dominate numeric order, and capsule id must break ties"
    );
}

#[test]
fn a_hook_that_requires_another_runs_after_it_even_when_the_numbers_say_otherwise() {
    let f = enabled(
        vec![
            hook_table(
                "hook/gate/a-consumer",
                "\n[[requires]]\nid = \"hook/gate/z-producer\"\n",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"gate\"\norder = 10",
            ),
            hook_table(
                "hook/gate/z-producer",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"gate\"\norder = 90",
            ),
        ],
        &["hook/gate/a-consumer"],
    );

    assert_eq!(
        step_ids(&chain(&f, "PreToolUse")),
        vec!["hook/gate/z-producer", "hook/gate/a-consumer"],
        "a declared dependency outranks the declared order"
    );
}

#[test]
fn a_dependency_that_would_have_to_run_in_an_earlier_phase_is_rejected_not_reordered() {
    // The consumer verifies; its dependency only observes. Phases are stages, so
    // no ordering satisfies both. Quietly promoting the observer would change what
    // the capsule author asked for, so this fails visibly instead.
    let f = enabled(
        vec![
            hook_table(
                "hook/verify/consumer",
                "\n[[requires]]\nid = \"hook/observe/producer\"\n",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"",
            ),
            hook_table(
                "hook/observe/producer",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"observe\"",
            ),
        ],
        &["hook/verify/consumer"],
    );

    let view = f.resolve().unwrap();
    let error = build_chains(&view, &f.catalog).unwrap_err();
    assert_eq!(error.code(), "hook.chain_order_impossible");
    assert_eq!(
        error.details().get("capability").map(String::as_str),
        Some("hook/verify/consumer")
    );
    assert_eq!(
        error.details().get("requires").map(String::as_str),
        Some("hook/observe/producer")
    );
}

#[test]
fn a_cycle_between_two_hooks_in_one_chain_is_rejected() {
    let a = cid("hook/gate/a");
    let b = cid("hook/gate/b");
    let steps = vec![
        HookStep::new(a.clone(), "payload/a", HookPhase::Gate),
        HookStep::new(b.clone(), "payload/b", HookPhase::Gate),
    ];
    let mut deps: BTreeMap<_, Vec<_>> = BTreeMap::new();
    deps.insert(a.clone(), vec![b.clone()]);
    deps.insert(b, vec![a]);

    let error = HookChain::plan(HookEventKind::PreToolUse, steps, &deps).unwrap_err();
    assert_eq!(error.code(), "hook.chain_order_impossible");
}

// ---------------------------------------------------------------------------
// Matchers
// ---------------------------------------------------------------------------

#[test]
fn a_matcher_selects_only_the_tools_it_names() {
    let f = enabled(
        vec![hook_table(
            "hook/gate/write-guard",
            "",
            "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nmatcher = \"^(Edit|Write)$\"",
        )],
        &["hook/gate/write-guard"],
    );
    let chain = chain(&f, "PreToolUse");
    let step = &chain.steps[0];

    assert!(aikit_core::hooks::matches(step, &pre_tool_use("Edit")));
    assert!(aikit_core::hooks::matches(step, &pre_tool_use("Write")));
    assert!(!aikit_core::hooks::matches(step, &pre_tool_use("Bash")));
}

#[test]
fn a_step_with_no_matcher_runs_for_every_event_including_ones_with_no_tool() {
    let f = enabled(
        vec![hook_table(
            "hook/observe/log",
            "",
            "entry = \"payload/x\"\nevents = [\"Stop\"]\nphase = \"observe\"",
        )],
        &["hook/observe/log"],
    );
    let chain = chain(&f, "Stop");
    let event = HookEvent::new("claude", HookEventKind::Stop, serde_json::json!({}));
    assert!(aikit_core::hooks::matches(&chain.steps[0], &event));
}

#[test]
fn a_matcher_that_does_not_compile_is_rejected_at_chain_build_time() {
    let f = enabled(
        vec![hook_table(
            "hook/gate/broken",
            "",
            "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nmatcher = \"([unclosed\"",
        )],
        &["hook/gate/broken"],
    );
    let view = f.resolve().unwrap();
    let error = build_chains(&view, &f.catalog).unwrap_err();
    assert_eq!(error.code(), "hook.invalid_matcher");
}

#[test]
fn a_step_whose_matcher_misses_is_recorded_as_not_matched_and_never_invoked() {
    let f = enabled(
        vec![hook_table(
            "hook/gate/write-guard",
            "",
            "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nmatcher = \"^Write$\"",
        )],
        &["hook/gate/write-guard"],
    );
    let chain = chain(&f, "PreToolUse");

    let mut invoked: Vec<String> = Vec::new();
    let mut runner = |step: &HookStep, _: &HookEvent| {
        invoked.push(step.capsule.to_string());
        StepResult::allow()
    };
    let decision = Dispatcher::new().run(&chain, &pre_tool_use("Bash"), &mut runner);

    assert!(
        invoked.is_empty(),
        "a non-matching step must not be executed"
    );
    assert!(decision.allowed);
    assert_eq!(decision.steps[0].outcome, StepOutcome::NotMatched);
}

// ---------------------------------------------------------------------------
// Denial and short-circuit
// ---------------------------------------------------------------------------

fn gate_verify_observe_fixture() -> Fixture {
    enabled(
        vec![
            hook_table(
                "hook/gate/boundary",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"gate\"",
            ),
            hook_table(
                "hook/verify/secrets",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"",
            ),
            hook_table(
                "hook/observe/audit",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"observe\"",
            ),
        ],
        &[
            "hook/gate/boundary",
            "hook/verify/secrets",
            "hook/observe/audit",
        ],
    )
}

#[test]
fn a_gate_denial_short_circuits_the_verifiers_but_the_observers_still_run() {
    let f = gate_verify_observe_fixture();
    let chain = chain(&f, "PreToolUse");

    let mut invoked: Vec<String> = Vec::new();
    let mut runner = |step: &HookStep, _: &HookEvent| {
        invoked.push(step.capsule.to_string());
        if step.phase == HookPhase::Gate {
            StepResult::deny("writes outside the project are not allowed here")
        } else {
            StepResult::allow()
        }
    };
    let decision = Dispatcher::new().run(&chain, &pre_tool_use("Write"), &mut runner);

    assert!(!decision.allowed);
    assert_eq!(
        invoked,
        vec!["hook/gate/boundary", "hook/observe/audit"],
        "the verifier must be skipped and the observer must still run"
    );

    let denial = decision.denial.as_ref().expect("a denial must be recorded");
    assert_eq!(denial.capsule.to_string(), "hook/gate/boundary");
    assert_eq!(denial.phase, HookPhase::Gate);
    assert!(!denial.from_system_failure);
    assert_eq!(
        denial.reason,
        "writes outside the project are not allowed here"
    );

    let verifier = decision.step("hook/verify/secrets").unwrap();
    assert_eq!(verifier.outcome, StepOutcome::ShortCircuited);
}

#[test]
fn an_observer_can_never_deny_the_event() {
    let f = enabled(
        vec![hook_table(
            "hook/observe/audit",
            "",
            "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"observe\"",
        )],
        &["hook/observe/audit"],
    );
    let chain = chain(&f, "PreToolUse");

    let mut runner = |_: &HookStep, _: &HookEvent| StepResult::deny("I disapprove");
    let decision = Dispatcher::new().run(&chain, &pre_tool_use("Bash"), &mut runner);

    assert!(
        decision.allowed,
        "an observe-phase denial must not stop the event"
    );
    assert!(decision.denial.is_none());
    assert!(
        decision.warnings.iter().any(|w| w.contains("observe")),
        "the ignored denial must still be surfaced: {:?}",
        decision.warnings
    );
}

// ---------------------------------------------------------------------------
// System failure vs policy denial
// ---------------------------------------------------------------------------

fn failure_fixture(policy: &str) -> Fixture {
    enabled(
        vec![hook_table(
            "hook/gate/flaky",
            "",
            &format!(
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"gate\"\nfailure = \"{policy}\""
            ),
        )],
        &["hook/gate/flaky"],
    )
}

fn run_failing(f: &Fixture) -> aikit_core::hooks::HookDecision {
    let chain = chain(f, "PreToolUse");
    let mut runner =
        |_: &HookStep, _: &HookEvent| StepResult::system_failure("exited with signal 9");
    Dispatcher::new().run(&chain, &pre_tool_use("Bash"), &mut runner)
}

#[test]
fn a_failure_closed_hook_denies_but_is_recorded_as_a_failure_not_as_a_policy_denial() {
    let decision = run_failing(&failure_fixture("closed"));
    assert!(!decision.allowed);

    let denial = decision.denial.as_ref().unwrap();
    assert!(
        denial.from_system_failure,
        "conflating a crash with a decision is how a control quietly stops working"
    );
    assert!(denial.reason.contains("signal 9"));

    let record = decision.step("hook/gate/flaky").unwrap();
    assert_eq!(
        record.outcome,
        StepOutcome::SystemFailure {
            policy: aikit_core::capsule::FailurePolicy::Closed
        }
    );
    assert_ne!(record.outcome, StepOutcome::Denied);
}

#[test]
fn a_failure_open_hook_allows_the_event_through() {
    let decision = run_failing(&failure_fixture("open"));
    assert!(decision.allowed);
    assert!(decision.denial.is_none());
    assert_eq!(
        decision.step("hook/gate/flaky").unwrap().outcome,
        StepOutcome::SystemFailure {
            policy: aikit_core::capsule::FailurePolicy::Open
        }
    );
}

#[test]
fn a_failure_warn_hook_allows_the_event_and_says_so() {
    let decision = run_failing(&failure_fixture("warn"));
    assert!(decision.allowed);
    assert!(decision.denial.is_none());
    assert!(
        decision
            .warnings
            .iter()
            .any(|w| w.contains("hook/gate/flaky") && w.contains("signal 9")),
        "a warn policy must name the hook and the failure: {:?}",
        decision.warnings
    );
}

#[test]
fn a_failing_observer_never_denies_even_under_a_closed_failure_policy() {
    let f = enabled(
        vec![hook_table(
            "hook/capture/transcript",
            "",
            "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"capture\"\nfailure = \"closed\"",
        )],
        &["hook/capture/transcript"],
    );
    let chain = chain(&f, "PreToolUse");
    let mut runner = |_: &HookStep, _: &HookEvent| StepResult::system_failure("disk full");
    let decision = Dispatcher::new().run(&chain, &pre_tool_use("Bash"), &mut runner);

    assert!(
        decision.allowed,
        "a capture failure must not be able to block the user's work"
    );
    assert!(decision.warnings.iter().any(|w| w.contains("disk full")));
}

// ---------------------------------------------------------------------------
// Transform and inject
// ---------------------------------------------------------------------------

#[test]
fn a_transform_rewrites_the_payload_that_every_later_step_sees() {
    let f = enabled(
        vec![
            hook_table(
                "hook/transform/redact",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"transform\"",
            ),
            hook_table(
                "hook/verify/after",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"",
            ),
        ],
        &["hook/transform/redact", "hook/verify/after"],
    );
    let chain = chain(&f, "PreToolUse");

    let mut seen_by_verifier = serde_json::Value::Null;
    let mut runner = |step: &HookStep, event: &HookEvent| {
        if step.phase == HookPhase::Transform {
            StepResult::transform(serde_json::json!({ "command": "echo ***" }))
        } else {
            seen_by_verifier = event.payload.clone();
            StepResult::allow()
        }
    };

    let event = HookEvent::new(
        "claude",
        HookEventKind::PreToolUse,
        serde_json::json!({ "command": "echo hunter2" }),
    )
    .with_tool_name("Bash");
    let decision = Dispatcher::new().run(&chain, &event, &mut runner);

    assert_eq!(
        seen_by_verifier,
        serde_json::json!({ "command": "echo ***" })
    );
    assert_eq!(
        decision.payload,
        serde_json::json!({ "command": "echo ***" })
    );
    assert_eq!(
        decision.step("hook/transform/redact").unwrap().outcome,
        StepOutcome::Transformed
    );
}

#[test]
fn inject_steps_accumulate_their_text_in_chain_order() {
    let f = enabled(
        vec![
            hook_table(
                "hook/inject/b-second",
                "",
                "entry = \"payload/x\"\nevents = [\"UserPromptSubmit\"]\nphase = \"inject\"\norder = 20",
            ),
            hook_table(
                "hook/inject/a-first",
                "",
                "entry = \"payload/x\"\nevents = [\"UserPromptSubmit\"]\nphase = \"inject\"\norder = 10",
            ),
        ],
        &["hook/inject/b-second", "hook/inject/a-first"],
    );
    let chain = chain(&f, "UserPromptSubmit");

    let mut runner = |step: &HookStep, _: &HookEvent| {
        StepResult::inject(format!("note from {}", step.capsule.leaf()))
    };
    let event = HookEvent::new(
        "claude",
        HookEventKind::UserPromptSubmit,
        serde_json::json!({}),
    );
    let decision = Dispatcher::new().run(&chain, &event, &mut runner);

    assert_eq!(
        decision.injected,
        vec!["note from a-first", "note from b-second"]
    );
    assert_eq!(
        decision.injected_text(),
        "note from a-first\n\nnote from b-second"
    );
}

// ---------------------------------------------------------------------------
// Execution grouping
// ---------------------------------------------------------------------------

#[test]
fn independent_parallel_verifiers_are_grouped_while_gates_stay_serial() {
    let f = enabled(
        vec![
            hook_table(
                "hook/gate/boundary",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"gate\"\nserial = false",
            ),
            hook_table(
                "hook/verify/a-lint",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"\nserial = false",
            ),
            hook_table(
                "hook/verify/b-types",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"\nserial = false",
            ),
            hook_table(
                "hook/verify/c-slow",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"",
            ),
        ],
        &[
            "hook/gate/boundary",
            "hook/verify/a-lint",
            "hook/verify/b-types",
            "hook/verify/c-slow",
        ],
    );
    let chain = chain(&f, "PreToolUse");
    let groups = chain.execution_groups();

    // A gate is serial whatever the manifest asks for: it is the one place a
    // capsule can veto, and ordering there is a security property.
    assert_eq!(groups[0].capsules.len(), 1);
    assert!(!groups[0].parallel);
    assert_eq!(groups[0].capsules[0].to_string(), "hook/gate/boundary");

    assert!(groups[1].parallel);
    assert_eq!(
        groups[1]
            .capsules
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>(),
        vec!["hook/verify/a-lint", "hook/verify/b-types"]
    );

    assert!(!groups[2].parallel);
    assert_eq!(groups[2].capsules[0].to_string(), "hook/verify/c-slow");
}

#[test]
fn two_parallel_verifiers_are_not_grouped_when_one_depends_on_the_other() {
    let f = enabled(
        vec![
            hook_table(
                "hook/verify/a-consumer",
                "\n[[requires]]\nid = \"hook/verify/b-producer\"\n",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"\nserial = false",
            ),
            hook_table(
                "hook/verify/b-producer",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"\nserial = false",
            ),
        ],
        &["hook/verify/a-consumer"],
    );
    let chain = chain(&f, "PreToolUse");
    let groups = chain.execution_groups();

    assert_eq!(
        groups.len(),
        2,
        "a dependency must split the parallel group"
    );
    assert_eq!(groups[0].capsules[0].to_string(), "hook/verify/b-producer");
    assert_eq!(groups[1].capsules[0].to_string(), "hook/verify/a-consumer");
}

#[test]
fn the_decision_is_computed_in_chain_order_even_though_a_group_may_run_in_parallel() {
    let f = enabled(
        vec![
            hook_table(
                "hook/verify/a-lint",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"\nserial = false",
            ),
            hook_table(
                "hook/verify/b-types",
                "",
                "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"\nserial = false",
            ),
        ],
        &["hook/verify/a-lint", "hook/verify/b-types"],
    );
    let chain = chain(&f, "PreToolUse");

    // Both verifiers deny. Whichever finished first in wall-clock time, the
    // reported denial must be the first one in chain order, or two runs of the
    // same event would blame different capsules.
    let mut runner = |step: &HookStep, _: &HookEvent| {
        StepResult::deny(format!("{} says no", step.capsule.leaf()))
    };
    let decision = Dispatcher::new().run(&chain, &pre_tool_use("Bash"), &mut runner);

    assert_eq!(
        decision.denial.as_ref().unwrap().capsule.to_string(),
        "hook/verify/a-lint"
    );
    assert_eq!(decision.denial.as_ref().unwrap().reason, "a-lint says no");
}

// ---------------------------------------------------------------------------
// Bypass
// ---------------------------------------------------------------------------

fn bypassable(id: &str, bypass: &str) -> aikit_core::capsule::Capsule {
    hook_table(
        id,
        "",
        &format!(
            "entry = \"payload/x\"\nevents = [\"PreToolUse\"]\nphase = \"gate\"\nbypass = {bypass}"
        ),
    )
}

#[test]
fn a_capsule_that_forbids_bypass_is_never_bypassed() {
    let f = enabled(
        vec![bypassable("hook/gate/mandatory", "{ allowed = false }")],
        &["hook/gate/mandatory"],
    );
    let chain = chain(&f, "PreToolUse");

    let token = BypassToken::new(BypassScope::NextEvent).with_reason("rebasing, I know");
    let mut invoked = 0;
    let mut runner = |_: &HookStep, _: &HookEvent| {
        invoked += 1;
        StepResult::deny("nope")
    };
    let decision = Dispatcher::with_bypass(token).run(&chain, &pre_tool_use("Bash"), &mut runner);

    assert_eq!(invoked, 1, "the step must still have been executed");
    assert!(!decision.allowed);
    assert!(!decision.steps[0].bypassed);
    assert!(
        decision
            .warnings
            .iter()
            .any(|w| w.contains("hook/gate/mandatory") && w.contains("bypass")),
        "refusing a bypass must be visible: {:?}",
        decision.warnings
    );
}

#[test]
fn a_bypass_without_a_reason_is_refused_when_the_capsule_demands_one() {
    let f = enabled(
        vec![bypassable(
            "hook/gate/skippable",
            "{ allowed = true, reason_required = true }",
        )],
        &["hook/gate/skippable"],
    );
    let chain = chain(&f, "PreToolUse");

    let mut invoked = 0;
    let mut runner = |_: &HookStep, _: &HookEvent| {
        invoked += 1;
        StepResult::deny("nope")
    };
    let decision = Dispatcher::with_bypass(BypassToken::new(BypassScope::Session)).run(
        &chain,
        &pre_tool_use("Bash"),
        &mut runner,
    );

    assert_eq!(invoked, 1);
    assert!(!decision.allowed);
    assert!(!decision.steps[0].bypassed);
    assert!(decision.warnings.iter().any(|w| w.contains("reason")));
}

#[test]
fn a_bypassed_step_is_skipped_recorded_and_loudly_warned_about() {
    let f = enabled(
        vec![bypassable(
            "hook/gate/skippable",
            "{ allowed = true, reason_required = true }",
        )],
        &["hook/gate/skippable"],
    );
    let chain = chain(&f, "PreToolUse");

    let mut invoked = 0;
    let mut runner = |_: &HookStep, _: &HookEvent| {
        invoked += 1;
        StepResult::deny("nope")
    };
    let token = BypassToken::new(BypassScope::NextEvent).with_reason("hotfix for INC-4412");
    let decision = Dispatcher::with_bypass(token).run(&chain, &pre_tool_use("Bash"), &mut runner);

    assert_eq!(invoked, 0, "a bypassed step must not run at all");
    assert!(decision.allowed);
    assert!(decision.steps[0].bypassed);
    assert_eq!(decision.steps[0].outcome, StepOutcome::Bypassed);
    assert!(
        decision
            .warnings
            .iter()
            .any(|w| w.contains("hook/gate/skippable") && w.contains("hotfix for INC-4412")),
        "the reason must be recorded with the bypass: {:?}",
        decision.warnings
    );
    assert!(
        decision.bypass_consumed,
        "a next-event token is spent by the event it covered"
    );
}

#[test]
fn a_bypass_issued_for_one_capsule_does_not_cover_another() {
    let f = enabled(
        vec![
            bypassable(
                "hook/gate/a-one",
                "{ allowed = true, reason_required = false }",
            ),
            bypassable(
                "hook/gate/b-two",
                "{ allowed = true, reason_required = false }",
            ),
        ],
        &["hook/gate/a-one", "hook/gate/b-two"],
    );
    let chain = chain(&f, "PreToolUse");

    let token = BypassToken::new(BypassScope::Session).for_capsule(cid("hook/gate/a-one"));
    let mut invoked: Vec<String> = Vec::new();
    let mut runner = |step: &HookStep, _: &HookEvent| {
        invoked.push(step.capsule.to_string());
        StepResult::allow()
    };
    let decision = Dispatcher::with_bypass(token).run(&chain, &pre_tool_use("Bash"), &mut runner);

    assert_eq!(invoked, vec!["hook/gate/b-two"]);
    assert!(decision.steps[0].bypassed);
    assert!(!decision.steps[1].bypassed);
    assert!(
        !decision.bypass_consumed,
        "a session-scoped token outlives the event"
    );
}

// ---------------------------------------------------------------------------
// The record
// ---------------------------------------------------------------------------

#[test]
fn every_step_appears_in_the_decision_record_with_its_phase_and_duration() {
    let f = gate_verify_observe_fixture();
    let chain = chain(&f, "PreToolUse");

    let mut runner = |_: &HookStep, _: &HookEvent| {
        StepResult::allow().taking(std::time::Duration::from_millis(7))
    };
    let decision = Dispatcher::new().run(&chain, &pre_tool_use("Bash"), &mut runner);

    assert_eq!(decision.steps.len(), 3);
    assert_eq!(
        decision.steps.iter().map(|s| s.phase).collect::<Vec<_>>(),
        vec![HookPhase::Gate, HookPhase::Verify, HookPhase::Observe]
    );
    for record in &decision.steps {
        assert_eq!(record.duration, Some(std::time::Duration::from_millis(7)));
        assert_eq!(record.outcome, StepOutcome::Allowed);
        assert!(record.denial_reason.is_none());
    }
}

#[test]
fn a_step_carries_the_effective_config_the_resolver_produced_for_its_capsule() {
    let mut layer = layer(ScopeKind::Session, &["hook/verify/cargo-check"], &[]);
    let mut table = toml::value::Table::new();
    table.insert("mode".into(), toml::Value::String("changed-crates".into()));
    layer
        .patch
        .config
        .insert(cid("hook/verify/cargo-check"), table);

    let f = Fixture::new(vec![hook_table(
        "hook/verify/cargo-check",
        "",
        "entry = \"payload/check\"\nevents = [\"PreToolUse\"]\nphase = \"verify\"",
    )])
    .with_layers(vec![layer]);

    let chain = chain(&f, "PreToolUse");
    assert_eq!(
        chain.steps[0].config.get("mode").and_then(|v| v.as_str()),
        Some("changed-crates")
    );
}
