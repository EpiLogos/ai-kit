//! One journey through the whole crate, reaching only for the crate-root surface.
//!
//! The five modules added in this phase are not independent: a resolved view
//! feeds the hook chain, the guidance composer, the palette's documents and a
//! projection plan, and all four have to agree about the same context. This test
//! walks that path once end to end.
//!
//! It also pins the export surface deliberately. Every name below is written as
//! `aikit_core::Thing`, never `aikit_core::session::Thing`, because the four
//! consuming crates are supposed to be able to build a tmux adapter or a palette
//! row without learning core's module layout first. A name that has to be reached
//! through its module is a name this test refuses to compile without.

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use aikit_core::{
    build_chains, compile_session, compose, estimate_tokens, hook_matches, parse_query, resolve,
    score, target_label, ActivationEffect, Attach, CompositionEntry, CompositionRequest, Denial,
    Direction, Dispatcher, DocStatus, FastPrefix, FragmentStatus, GuidanceFragment, HookEvent,
    HookEventKind, HookPhase, HookStep, Isolation, Lifecycle, MaterializationMode, PaneStep,
    Placement, ProjectionItem, ProjectionPlan, RankingSignals, ResolvedContext, Restart, ScopeKind,
    SearchDoc, SessionSpec, Split, StatusFilter, StepResult, TargetAdapter, TargetCapabilities,
    TargetId, UsageStats, ViewPlan,
};

use common::{cid, guidance_table, hook_table, layer, script_exporting, skill, Fixture};

/// A catalog with one gate hook, one guidance capsule and one script, all enabled
/// by a session overlay — the smallest thing that still exercises every module.
fn view() -> aikit_core::ResolvedView {
    let fixture = Fixture::new(vec![
        hook_table(
            "hook/guard/no-force-push",
            "",
            r#"entry = "payload/check.sh"
events = ["PreToolUse"]
matcher = "^Bash$"
phase = "gate"
order = 10
failure = "closed""#,
        ),
        guidance_table(
            "guidance/mode/review",
            "",
            r#"entry = "payload/guidance.md"
order = 20
token_budget = 40"#,
        ),
        script_exporting("script/ci/deploy", &["deploy"]),
    ])
    .with_layers(vec![layer(
        ScopeKind::Session,
        &[
            "hook/guard/no-force-push",
            "guidance/mode/review",
            "script/ci/deploy",
        ],
        &[],
    )]);

    resolve(&fixture.catalog, &fixture.trust, &fixture.request())
        .expect("the fixture view should resolve")
}

#[test]
fn a_resolved_view_feeds_a_hook_chain_that_denies_the_tool_it_guards() {
    let view = view();
    let fixture_catalog = Fixture::new(vec![hook_table(
        "hook/guard/no-force-push",
        "",
        r#"entry = "payload/check.sh"
events = ["PreToolUse"]
matcher = "^Bash$"
phase = "gate"
order = 10
failure = "closed""#,
    )]);

    let chains = build_chains(&view, &fixture_catalog.catalog).expect("chains should build");
    let chain = chains
        .get("PreToolUse")
        .expect("the hook declares PreToolUse");
    assert_eq!(chain.steps.len(), 1);

    let step = &chain.steps[0];
    assert_eq!(step.phase, HookPhase::Gate);
    assert_eq!(step.entry, "payload/check.sh");

    // The matcher is a real regex against the tool name, not a substring test.
    let bash = HookEvent::new("claude", HookEventKind::PreToolUse, serde_json::json!({}))
        .with_tool_name("Bash");
    let edit = HookEvent::new("claude", HookEventKind::PreToolUse, serde_json::json!({}))
        .with_tool_name("Edit");
    assert!(hook_matches(step, &bash));
    assert!(!hook_matches(step, &edit));

    let mut runner = |step: &HookStep, event: &HookEvent| {
        assert_eq!(step.capsule, cid("hook/guard/no-force-push"));
        assert_eq!(event.tool_name.as_deref(), Some("Bash"));
        StepResult::deny("force pushes to main are not allowed here")
            .taking(Duration::from_millis(7))
    };

    let decision = Dispatcher::new().run(chain, &bash, &mut runner);
    assert!(!decision.allowed);

    let denial: &Denial = decision.denial.as_ref().expect("the gate denied");
    assert!(!denial.from_system_failure, "this was a decision, not a crash");
    assert!(denial.describe().contains("denied this event"));

    let record = decision
        .step("hook/guard/no-force-push")
        .expect("every step is recorded");
    assert_eq!(record.duration, Some(Duration::from_millis(7)));
    assert!(!record.bypassed);

    // The same chain against a tool the matcher does not name allows the event
    // and never invokes the runner.
    let mut never = |_: &HookStep, _: &HookEvent| panic!("the runner must not be called");
    assert!(Dispatcher::new().run(chain, &edit, &mut never).allowed);
}

#[test]
fn the_active_guidance_composes_within_its_budget_and_accounts_for_what_it_dropped() {
    let view = view();
    let active = view.active_of_kind(aikit_core::Kind::Guidance);
    assert_eq!(active.len(), 1, "one guidance capsule is active");

    let short = "Read the diff before you judge it.";
    let long = "x".repeat(400);

    let fragments = vec![
        GuidanceFragment::new(active[0].id.clone(), short)
            .with_order(20)
            .with_per_fragment_budget(40),
        GuidanceFragment::new(cid("guidance/mode/verbose"), long.clone()).with_order(30),
    ];

    let request = CompositionRequest::new(HookEventKind::SessionStart, TargetId::claude_code(), 30);
    let composition = compose(fragments, &request);

    assert_eq!(composition.text, short, "the short fragment fits whole");
    assert!(composition.used_tokens <= 30);
    assert!(estimate_tokens(&composition.text) <= composition.used_tokens);

    let dropped: &CompositionEntry = composition
        .entry("guidance/mode/verbose")
        .expect("a dropped fragment is still in the record");
    assert!(
        matches!(
            dropped.status,
            FragmentStatus::SkippedOverTotalBudget { .. }
        ),
        "an oversized fragment is skipped whole, never truncated: {dropped:?}"
    );
    assert!(!composition.text.contains('x'), "no half a fragment leaked");

    let record = composition.render_record();
    assert!(record.contains("guidance/mode/review"));
    assert!(record.contains("guidance/mode/verbose"));
    assert!(record.contains("30 tokens"), "the record prints the budget");
}

#[test]
fn the_palette_ranks_the_same_view_through_the_crate_root_policy() {
    let view = view();
    let signals = RankingSignals::default();

    let deploy = SearchDoc::from_view(
        &view,
        &cid("script/ci/deploy"),
        UsageStats {
            successful_runs: 20,
            failed_runs: 0,
            last_success_age: Some(Duration::from_secs(365 * 24 * 60 * 60)),
        },
    )
    .expect("an active script is a document");
    assert_eq!(deploy.status, DocStatus::Active);
    assert_eq!(deploy.scope, Some(ScopeKind::Session));
    assert!(deploy.runnable);

    // `>` is the runnable lane; a guidance capsule is not something you run.
    let run_lane = parse_query("> deploy");
    assert_eq!(run_lane.prefix, Some(FastPrefix::Run));
    assert_eq!(run_lane.text, "deploy");
    assert!(run_lane.matches_filters(&deploy));

    let review = SearchDoc::from_view(&view, &cid("guidance/mode/review"), UsageStats::default())
        .expect("active guidance is a document");
    assert!(!run_lane.matches_filters(&review));

    // Filters compose with the lane, and `status:` is the three-state question.
    let filtered = parse_query("kind:script status:active deploy");
    assert_eq!(filtered.status, Some(StatusFilter::Active));
    assert!(filtered.matches_filters(&deploy));
    assert!(!filtered.matches_filters(&review));

    // A year-old habit, however frequent, cannot outrank a fresh direct hit.
    let fresh = SearchDoc::from_view(
        &view,
        &cid("guidance/mode/review"),
        UsageStats {
            successful_runs: 1,
            failed_runs: 0,
            last_success_age: Some(Duration::ZERO),
        },
    )
    .unwrap();
    assert!(
        score(&filtered, &fresh, 1.0, &signals) > score(&filtered, &deploy, 0.2, &signals),
        "usage is a tiebreaker, never a promotion"
    );
}

#[test]
fn a_session_spec_compiles_into_a_script_an_adapter_can_run_with_no_lookahead() {
    let spec = SessionSpec::from_toml_str(
        r#"
schema = 1
id = "payments"
name = "Payments"
root = "/work/payments"
backend = "tmux"
attach = "if-created"
lifecycle = "persist"

[capabilities]
enable = ["script/ci/deploy"]

[[views]]
id = "code"

[[views.panes]]
id = "editor"
command = ["nvim"]
focus = true

[[views.panes]]
id = "tests"
split_from = "editor"
direction = "down"
ratio = 0.3
restart = "if-exited"
command = ["cargo", "watch", "-x", "test"]

[task]
agent = "claude"
placement = "new-pane"
"#,
    )
    .expect("the documented TOML parses");

    assert_eq!(spec.attach, Attach::IfCreated);
    assert_eq!(spec.lifecycle, Lifecycle::Persist);

    let plan = compile_session(&spec).expect("the spec compiles");
    assert_eq!(plan.pane_count(), 2);

    // Worktrees are opt-in: a `[task]` with no isolation key shares the tree.
    let task = plan.task.as_ref().expect("the spec declares a task");
    assert_eq!(task.isolation, Isolation::Shared);
    assert!(!task.is_isolated());
    assert_eq!(task.placement, Placement::NewPane);

    // Walk the plan the way an adapter would: one command per step, and every
    // parent named by a split has already been created by an earlier step.
    let code: &ViewPlan = plan.view("code").expect("the view is in the plan");
    let mut created: Vec<&str> = Vec::new();
    for step in &code.steps {
        let step: &PaneStep = step;
        match &step.split {
            None => assert!(created.is_empty(), "only the root pane has no split"),
            Some(Split {
                from,
                direction,
                ratio,
            }) => {
                assert!(
                    created.contains(&from.as_str()),
                    "`{from}` must already exist when `{}` splits off it",
                    step.pane
                );
                assert_eq!(*direction, Direction::Down);
                assert!(!direction.is_horizontal());
                assert_eq!(*ratio, Some(0.3));
            }
        }
        created.push(&step.pane);
    }
    assert_eq!(created, vec!["editor", "tests"]);
    assert_eq!(code.focus.as_deref(), Some("editor"));
    assert_eq!(code.steps[1].restart, Restart::IfExited);
    assert_eq!(code.steps[0].restart, Restart::Never, "the safe default");
}

/// A Codex-shaped adapter: it can take a per-context skill directory, but only
/// when the task owns its working tree, because the directory lives in the tree.
struct TreeBoundAdapter;

impl TargetAdapter for TreeBoundAdapter {
    fn target(&self) -> TargetId {
        TargetId::codex()
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: false,
            symlinks: true,
            isolated_per_context: true,
            requires_isolated_tree_for_isolation: true,
            brokered_fallback: true,
            watches_for_changes: false,
        }
    }

    fn plan(&self, context: &ResolvedContext) -> aikit_core::Result<ProjectionPlan> {
        let caps = self.capabilities();
        let isolation = context.isolation();

        // The whole point of the trait: consult isolation, then degrade honestly.
        let Some(reason) = caps.fallback_reason(isolation) else {
            let mut plan =
                ProjectionPlan::new(self.target(), ActivationEffect::immediate("task worktree"));
            for skill in context.view.active_of_kind(aikit_core::Kind::Skill) {
                let from = context
                    .payload_path(&skill.id, "payload")
                    .expect("the store supplied a root");
                plan = plan.with_item(ProjectionItem::link(
                    from,
                    format!(".agents/skills/{}", skill.id.leaf()),
                )?);
            }
            return Ok(plan);
        };

        Ok(
            ProjectionPlan::new(self.target(), ActivationEffect::brokered(reason.clone()))
                .with_note(reason),
        )
    }

    fn activation_effect(
        &self,
        old: Option<&ProjectionPlan>,
        new: &ProjectionPlan,
    ) -> ActivationEffect {
        if new.is_noop_against(old) {
            ActivationEffect::immediate("unchanged")
        } else {
            new.effect.clone()
        }
    }
}

#[test]
fn a_shared_task_gets_an_honest_fallback_and_an_isolated_one_gets_a_real_projection() {
    let adapter = TreeBoundAdapter;
    assert_eq!(target_label(&adapter.target()), "Codex");

    let mut view = view();

    // Resolve a skill in its own fixture and splice it into the view, so the
    // adapter has something real (with a revision and a trust state) to project.
    let fixture = Fixture::new(vec![skill("skill/rust/review")]).with_layers(vec![layer(
        ScopeKind::Session,
        &["skill/rust/review"],
        &[],
    )]);
    let with_skill = resolve(&fixture.catalog, &fixture.trust, &fixture.request()).unwrap();
    view.active.extend(with_skill.active.clone());

    // Shared — the default. No per-task directory is possible.
    assert_eq!(view.context.isolation, Isolation::Shared);
    let shared = ResolvedContext::new(view.clone())
        .with_root(cid("skill/rust/review"), "/registry/skill/rust/review");
    let shared_plan = adapter.plan(&shared).unwrap();
    assert!(shared_plan.is_empty(), "nothing is written into a shared tree");
    assert_eq!(
        shared_plan.effect.describe_for(&adapter.target()),
        "Codex: brokered — this task uses the session's shared working tree (shared), and this \
         client's skill directory lives in the tree, so a sibling task would see the same files"
    );
    assert!(!shared_plan.notes.is_empty(), "the reason is stated, not implied");

    // Opt in to a worktree and the same adapter can project natively.
    let mut isolated_view = view.clone();
    isolated_view.context.isolation = Isolation::Worktree;
    let isolated = ResolvedContext::new(isolated_view)
        .with_root(cid("skill/rust/review"), "/registry/skill/rust/review");
    let isolated_plan = adapter.plan(&isolated).unwrap();
    assert_eq!(
        isolated_plan.items,
        vec![ProjectionItem::link(
            "/registry/skill/rust/review/payload",
            ".agents/skills/review"
        )
        .unwrap()]
    );
    assert_eq!(
        isolated_plan.effect.describe_for(&adapter.target()),
        "Codex: task worktree"
    );

    // Re-planning an unchanged context is detectably a no-op.
    let again = adapter.plan(&isolated).unwrap();
    assert_eq!(
        adapter.activation_effect(Some(&isolated_plan), &again),
        ActivationEffect::immediate("unchanged")
    );
    assert_ne!(
        adapter.activation_effect(Some(&shared_plan), &again),
        ActivationEffect::immediate("unchanged")
    );

    // A capsule cannot name a destination outside the projection root.
    assert_eq!(
        ProjectionItem::link("/anywhere", "../../.ssh/authorized_keys")
            .unwrap_err()
            .code(),
        "projection.destination_escapes_root"
    );

    // The materialization mode is a function of what the target can do.
    assert_eq!(
        MaterializationMode::Auto.resolve_for(&adapter.capabilities()),
        MaterializationMode::Link
    );
}

#[test]
fn the_object_safe_adapter_trait_can_be_held_in_a_boxed_list() {
    // The CLI walks a `Vec<Box<dyn TargetAdapter>>`; if the trait ever stopped
    // being object-safe, this is where it would be noticed.
    let adapters: Vec<Box<dyn TargetAdapter>> = vec![Box::new(TreeBoundAdapter)];
    let mut plans: BTreeMap<String, ProjectionPlan> = BTreeMap::new();
    for adapter in &adapters {
        let context = ResolvedContext::new(view());
        plans.insert(
            adapter.target().as_str().to_string(),
            adapter.plan(&context).unwrap(),
        );
    }
    assert!(plans.contains_key("codex"));
}
