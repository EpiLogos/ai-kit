//! Projection contracts.
//!
//! Core plans; `aikit-store` materializes. The two properties that have to hold
//! here are the ones a later I/O bug could not fix:
//!
//! * A plan can only ever write inside its own projection root. A capsule that
//!   names `../../.ssh/authorized_keys` is refused at plan time, before anything
//!   has a file handle.
//! * A plan that has not changed is *detectably* unchanged, so a no-op apply is a
//!   digest comparison rather than a rebuild.
//!
//! And the one honesty property: a target that needs an isolated working tree in
//! order to give a context its own skill surface must say so when the task is
//! shared, rather than writing into a tree its siblings can see.

mod common;

use common::*;

use std::collections::BTreeMap;
use std::path::PathBuf;

use aikit_core::capsule::Kind;
use aikit_core::context::Isolation;
use aikit_core::id::CapsuleId;
use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, MaterializationMode, ProjectionItem, ProjectionPlan, ResolvedContext,
    TargetAdapter, TargetCapabilities,
};
use aikit_core::scope::ScopeKind;
use aikit_core::Result;

// ---------------------------------------------------------------------------
// Destinations may never escape the projection root
// ---------------------------------------------------------------------------

#[test]
fn a_destination_that_climbs_out_of_the_projection_root_is_rejected() {
    let error = ProjectionItem::link("/registry/payload", "../../.ssh/authorized_keys").unwrap_err();
    assert_eq!(error.code(), "projection.destination_escapes_root");
}

#[test]
fn an_interior_parent_segment_is_rejected_too() {
    // `skills/../../etc` normalizes outside the root, and a projection root is a
    // security boundary, not a formatting convention.
    assert_eq!(
        ProjectionItem::copy("/registry/payload", "skills/../../etc/passwd")
            .unwrap_err()
            .code(),
        "projection.destination_escapes_root"
    );
}

#[test]
fn an_absolute_destination_is_rejected() {
    assert_eq!(
        ProjectionItem::write("/etc/passwd", "root:x:0:0")
            .unwrap_err()
            .code(),
        "projection.destination_escapes_root"
    );
}

#[test]
fn every_item_kind_validates_its_destination() {
    assert!(ProjectionItem::link("/from", "../out").is_err());
    assert!(ProjectionItem::copy("/from", "../out").is_err());
    assert!(ProjectionItem::write("../out", "x").is_err());
}

#[test]
fn a_relative_destination_is_accepted_and_normalized() {
    let item = ProjectionItem::link("/registry/skill/payload", "./.claude/skills/review").unwrap();
    assert_eq!(
        item.destination(),
        Some(PathBuf::from(".claude/skills/review").as_path())
    );
}

#[test]
fn an_empty_destination_is_rejected() {
    assert_eq!(
        ProjectionItem::write("", "x").unwrap_err().code(),
        "projection.invalid_destination"
    );
    assert_eq!(
        ProjectionItem::write("./.", "x").unwrap_err().code(),
        "projection.invalid_destination"
    );
}

#[test]
fn a_shim_name_that_is_really_a_path_is_rejected() {
    let capsule = cid("script/test/cargo-nextest");
    assert_eq!(
        ProjectionItem::shim("../evil", capsule.clone(), "cargo-nextest")
            .unwrap_err()
            .code(),
        "projection.invalid_shim_name"
    );
    assert_eq!(
        ProjectionItem::shim("bin/nt", capsule.clone(), "nt")
            .unwrap_err()
            .code(),
        "projection.invalid_shim_name"
    );
    assert!(ProjectionItem::shim("nt", capsule, "nt").is_ok());
}

#[test]
fn a_shim_has_no_destination_because_the_adapter_chooses_the_bin_directory() {
    let shim = ProjectionItem::shim("nt", cid("script/test/cargo-nextest"), "nt").unwrap();
    assert_eq!(shim.destination(), None);
}

// ---------------------------------------------------------------------------
// The digest
// ---------------------------------------------------------------------------

fn plan_with(items: Vec<ProjectionItem>) -> ProjectionPlan {
    ProjectionPlan::new(TargetId::claude_code(), ActivationEffect::live()).with_items(items)
}

fn link(from: &str, to: &str) -> ProjectionItem {
    ProjectionItem::link(from, to).unwrap()
}

#[test]
fn the_digest_ignores_the_order_items_were_added_in() {
    let a = plan_with(vec![
        link("/reg/a", "skills/a"),
        link("/reg/b", "skills/b"),
        ProjectionItem::copy("/reg/c", "skills/c").unwrap(),
    ]);
    let b = plan_with(vec![
        ProjectionItem::copy("/reg/c", "skills/c").unwrap(),
        link("/reg/b", "skills/b"),
        link("/reg/a", "skills/a"),
    ]);
    assert_eq!(a.digest(), b.digest());
}

#[test]
fn the_digest_changes_when_written_contents_change() {
    let a = plan_with(vec![ProjectionItem::write("guidance.md", "read first").unwrap()]);
    let b = plan_with(vec![ProjectionItem::write("guidance.md", "write first").unwrap()]);
    assert_ne!(
        a.digest(),
        b.digest(),
        "a plan is content-addressed, not path-addressed"
    );
}

#[test]
fn the_digest_changes_when_a_link_becomes_a_copy() {
    let linked = plan_with(vec![link("/reg/a", "skills/a")]);
    let copied = plan_with(vec![ProjectionItem::copy("/reg/a", "skills/a").unwrap()]);
    assert_ne!(linked.digest(), copied.digest());
}

#[test]
fn the_digest_is_specific_to_the_target() {
    let items = vec![link("/reg/a", "skills/a")];
    let claude = ProjectionPlan::new(TargetId::claude_code(), ActivationEffect::live())
        .with_items(items.clone());
    let codex =
        ProjectionPlan::new(TargetId::codex(), ActivationEffect::live()).with_items(items);
    assert_ne!(claude.digest(), codex.digest());
}

#[test]
fn notes_and_the_activation_effect_do_not_change_the_digest() {
    // The effect is a consequence of comparing plans; folding it into the digest
    // would make the comparison circular and a no-op apply undetectable.
    let plain = plan_with(vec![link("/reg/a", "skills/a")]);
    let annotated = plan_with(vec![link("/reg/a", "skills/a")])
        .with_note("fell back to the project-stable surface")
        .with_effect(ActivationEffect::restart_client("codex"));
    assert_eq!(plain.digest(), annotated.digest());
}

#[test]
fn an_unchanged_plan_is_detectably_a_no_op() {
    let previous = plan_with(vec![link("/reg/a", "skills/a")]);
    let same = plan_with(vec![link("/reg/a", "skills/a")]);
    let changed = plan_with(vec![link("/reg/a", "skills/a"), link("/reg/b", "skills/b")]);

    assert!(same.is_noop_against(Some(&previous)));
    assert!(!changed.is_noop_against(Some(&previous)));
    assert!(
        !same.is_noop_against(None),
        "there is nothing to compare against on a first apply"
    );
}

// ---------------------------------------------------------------------------
// Activation effects
// ---------------------------------------------------------------------------

#[test]
fn activation_effects_render_the_phrases_the_palette_prints_verbatim() {
    assert_eq!(
        ActivationEffect::live().describe_for(&TargetId::claude_code()),
        "Claude: live"
    );
    assert_eq!(
        ActivationEffect::immediate("task worktree").describe_for(&TargetId::codex()),
        "Codex: task worktree"
    );
    assert_eq!(
        ActivationEffect::restart_client("codex").describe(),
        "restart codex"
    );
    assert_eq!(
        ActivationEffect::next_session_only("the shared tree is already projected")
            .describe_for(&TargetId::codex()),
        "Codex: next session only — the shared tree is already projected"
    );
    assert_eq!(
        ActivationEffect::brokered("no isolated tree for this task").describe(),
        "brokered — no isolated tree for this task"
    );
    assert_eq!(
        ActivationEffect::unsupported("this client has no skill surface").describe(),
        "unsupported — this client has no skill surface"
    );
}

#[test]
fn an_effect_says_plainly_whether_the_user_has_to_do_something() {
    assert!(ActivationEffect::live().takes_effect_now());
    assert!(ActivationEffect::immediate("task worktree").takes_effect_now());
    assert!(!ActivationEffect::restart_client("codex").takes_effect_now());
    assert!(ActivationEffect::restart_client("codex").needs_user_action());
    assert!(!ActivationEffect::next_session_only("x").takes_effect_now());
    assert!(!ActivationEffect::unsupported("x").takes_effect_now());
}

// ---------------------------------------------------------------------------
// Isolation honesty
// ---------------------------------------------------------------------------

fn codex_like() -> TargetCapabilities {
    TargetCapabilities {
        live_reload: false,
        symlinks: true,
        isolated_per_context: true,
        requires_isolated_tree_for_isolation: true,
        brokered_fallback: true,
        watches_for_changes: false,
    }
}

fn claude_like() -> TargetCapabilities {
    TargetCapabilities {
        live_reload: true,
        symlinks: true,
        isolated_per_context: true,
        requires_isolated_tree_for_isolation: false,
        brokered_fallback: true,
        watches_for_changes: true,
    }
}

#[test]
fn a_target_that_needs_its_own_tree_cannot_isolate_a_shared_task() {
    let caps = codex_like();
    assert!(!caps.can_isolate(Isolation::Shared));
    assert!(caps.can_isolate(Isolation::Directory));
    assert!(caps.can_isolate(Isolation::Worktree));
}

#[test]
fn a_target_that_takes_an_arbitrary_directory_does_not_care_about_the_tree() {
    // Claude Code takes a context-specific `--add-dir`, so it can give two
    // sessions in one checkout different skills.
    let caps = claude_like();
    assert!(caps.can_isolate(Isolation::Shared));
    assert!(caps.can_isolate(Isolation::Worktree));
}

#[test]
fn the_fallback_reason_names_the_shared_tree_rather_than_shrugging() {
    let reason = codex_like()
        .fallback_reason(Isolation::Shared)
        .expect("a shared task must have a stated reason");
    assert!(
        reason.contains("shared"),
        "the user has to be told why, got: {reason}"
    );
    assert_eq!(codex_like().fallback_reason(Isolation::Worktree), None);
    assert_eq!(claude_like().fallback_reason(Isolation::Shared), None);
}

#[test]
fn a_target_with_no_isolation_and_no_broker_is_simply_unsupported() {
    let caps = TargetCapabilities::default();
    assert!(!caps.can_isolate(Isolation::Worktree));
    assert!(caps.fallback_reason(Isolation::Worktree).is_some());
}

// ---------------------------------------------------------------------------
// Materialization mode
// ---------------------------------------------------------------------------

#[test]
fn auto_prefers_links_and_falls_back_to_copies() {
    let mut caps = claude_like();
    assert_eq!(
        MaterializationMode::Auto.resolve_for(&caps),
        MaterializationMode::Link
    );
    caps.symlinks = false;
    assert_eq!(
        MaterializationMode::Auto.resolve_for(&caps),
        MaterializationMode::Copy
    );
}

#[test]
fn an_explicit_link_degrades_to_a_copy_and_admits_it() {
    let mut caps = claude_like();
    caps.symlinks = false;

    assert_eq!(
        MaterializationMode::Link.resolve_for(&caps),
        MaterializationMode::Copy
    );
    assert!(MaterializationMode::Link.degrades_for(&caps));
    assert!(!MaterializationMode::Auto.degrades_for(&caps));
    assert!(!MaterializationMode::Copy.degrades_for(&caps));
}

#[test]
fn an_explicit_copy_is_never_upgraded_to_a_link() {
    assert_eq!(
        MaterializationMode::Copy.resolve_for(&claude_like()),
        MaterializationMode::Copy
    );
}

// ---------------------------------------------------------------------------
// Adapters plan from a real resolved view
// ---------------------------------------------------------------------------

/// A stand-in for the Claude Code adapter, complete enough to prove the trait is
/// usable: it links every active skill's payload into a context-specific
/// `.claude/skills/` directory and never touches the project's own.
struct SkillLinker {
    capabilities: TargetCapabilities,
}

impl TargetAdapter for SkillLinker {
    fn target(&self) -> TargetId {
        TargetId::claude_code()
    }

    fn capabilities(&self) -> TargetCapabilities {
        self.capabilities.clone()
    }

    fn plan(&self, context: &ResolvedContext) -> Result<ProjectionPlan> {
        let isolation = context.view.context.isolation;
        let mut plan = ProjectionPlan::new(
            self.target(),
            match self.capabilities.fallback_reason(isolation) {
                Some(reason) => ActivationEffect::brokered(reason),
                None => ActivationEffect::live(),
            },
        );
        for capability in context.view.active_of_kind(Kind::Skill) {
            let root = context
                .root_of(&capability.id)
                .ok_or_else(|| aikit_core::AikitError::new("test.missing_root", "no root"))?;
            plan = plan.with_item(ProjectionItem::link(
                root.join("payload"),
                format!(".claude/skills/{}", capability.id.leaf()),
            )?);
        }
        Ok(plan)
    }

    fn activation_effect(
        &self,
        old: Option<&ProjectionPlan>,
        new: &ProjectionPlan,
    ) -> ActivationEffect {
        if new.is_noop_against(old) {
            ActivationEffect::immediate("no change")
        } else {
            new.effect.clone()
        }
    }
}

fn skill_context(isolation: Isolation) -> ResolvedContext {
    let f = Fixture::new(vec![
        skill("skill/rust/review"),
        skill("skill/rust/perf"),
        script("script/test/cargo-nextest"),
    ])
    .with_layers(vec![layer(
        ScopeKind::Project,
        &["skill/rust/review", "skill/rust/perf"],
        &[],
    )])
    .with_descriptor(aikit_core::context::ContextDescriptor {
        isolation,
        ..descriptor()
    });

    let view = f.resolve().unwrap();
    let mut roots: BTreeMap<CapsuleId, PathBuf> = BTreeMap::new();
    for id in ["skill/rust/review", "skill/rust/perf"] {
        roots.insert(cid(id), PathBuf::from("/registry/personal").join(id));
    }
    ResolvedContext {
        view,
        capsule_roots: roots,
    }
}

#[test]
fn a_target_adapter_is_object_safe() {
    let adapter: Box<dyn TargetAdapter> = Box::new(SkillLinker {
        capabilities: claude_like(),
    });
    assert_eq!(adapter.target(), TargetId::claude_code());
    assert!(adapter.capabilities().live_reload);
}

#[test]
fn an_adapter_plans_one_item_per_active_capability_of_its_kind() {
    let adapter = SkillLinker {
        capabilities: claude_like(),
    };
    let plan = adapter.plan(&skill_context(Isolation::Shared)).unwrap();

    assert_eq!(
        plan.items.len(),
        2,
        "the inactive script must not be projected"
    );
    let destinations: Vec<String> = plan
        .items
        .iter()
        .filter_map(|i| i.destination())
        .map(|p| p.display().to_string())
        .collect();
    assert_eq!(
        destinations,
        vec![".claude/skills/perf", ".claude/skills/review"]
    );
}

#[test]
fn re_planning_an_unchanged_context_yields_an_identical_digest() {
    let adapter = SkillLinker {
        capabilities: claude_like(),
    };
    let context = skill_context(Isolation::Shared);
    let first = adapter.plan(&context).unwrap();
    let second = adapter.plan(&context).unwrap();

    assert_eq!(first.digest(), second.digest());
    assert_eq!(
        adapter.activation_effect(Some(&first), &second),
        ActivationEffect::immediate("no change")
    );
}

#[test]
fn a_target_needing_its_own_tree_falls_back_honestly_in_a_shared_task() {
    let adapter = SkillLinker {
        capabilities: codex_like(),
    };
    let shared = adapter.plan(&skill_context(Isolation::Shared)).unwrap();
    let isolated = adapter.plan(&skill_context(Isolation::Worktree)).unwrap();

    match &shared.effect {
        ActivationEffect::Brokered { reason } => assert!(reason.contains("shared")),
        other => panic!("a shared task must not claim an isolated projection: {other:?}"),
    }
    assert_eq!(isolated.effect, ActivationEffect::live());
    assert_eq!(
        shared.digest(),
        isolated.digest(),
        "isolation changes the effect, not the files this adapter writes"
    );
}

// ---------------------------------------------------------------------------
// Resolved context
// ---------------------------------------------------------------------------

#[test]
fn a_resolved_context_locates_a_capsules_payload_and_reports_its_isolation() {
    let context = skill_context(Isolation::Directory);
    assert_eq!(
        context.root_of(&cid("skill/rust/review")),
        Some(PathBuf::from("/registry/personal/skill/rust/review").as_path())
    );
    assert_eq!(context.root_of(&cid("skill/rust/absent")), None);
    assert_eq!(context.isolation(), Isolation::Directory);
    assert!(context.isolation().is_isolated());
}

#[test]
fn a_payload_path_is_only_offered_for_a_capsule_the_context_knows() {
    let context = skill_context(Isolation::Shared);
    assert_eq!(
        context.payload_path(&cid("skill/rust/review"), "SKILL.md"),
        Some(PathBuf::from("/registry/personal/skill/rust/review/SKILL.md"))
    );
    assert_eq!(context.payload_path(&cid("skill/rust/absent"), "SKILL.md"), None);
}
