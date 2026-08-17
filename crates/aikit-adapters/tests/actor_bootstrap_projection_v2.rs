mod common;

use std::path::{Path, PathBuf};

use aikit_adapters::clients::claude::ClaudeAdapter;
use aikit_adapters::clients::codex::CodexAdapter;
use aikit_core::actor_bootstrap::{ActorBootstrap, ResourceSetSummary, ACTOR_BOOTSTRAP_VERSION};
use aikit_core::context::Isolation;
use aikit_core::platform::TargetId;
use aikit_core::project::{
    ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
};
use aikit_core::projection::{ActivationEffect, ProjectionItem, TargetAdapter};
use aikit_core::resource::ResourceRef;

use common::{write_payload_skill, ContextBuilder};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn empty_summary() -> ResourceSetSummary {
    ResourceSetSummary {
        total: 0,
        available: 0,
        unresolved: 0,
        unavailable: 0,
        examples: Vec::new(),
        truncated: false,
    }
}

fn actor_bootstrap() -> ActorBootstrap {
    ActorBootstrap {
        version: ACTOR_BOOTSTRAP_VERSION.to_string(),
        project: ProjectBinding::new(
            ProjectRef::parse("project/test").unwrap(),
            ProjectConstituentRef::parse("constituent/source").unwrap(),
            ProjectBindingLocator::LocalDirectory {
                path: PathBuf::from("/work/test"),
            },
        ),
        run: Some(r("run/client-supplied")),
        profiles: vec!["profile/code/base".into()],
        scopes: Vec::new(),
        agent: None,
        agency: None,
        host: None,
        harness: None,
        model: None,
        agent_session: Some("session/alpha".into()),
        capabilities: empty_summary(),
        actions: empty_summary(),
        context_sources: empty_summary(),
        projection_targets: vec![TargetId::claude_code(), TargetId::codex()],
        runtime_body: None,
        warnings: Vec::new(),
    }
}

fn bootstrap_write<'a>(plan: &'a aikit_core::ProjectionPlan, suffix: &str) -> &'a str {
    plan.items
        .iter()
        .find_map(|item| match item {
            ProjectionItem::Write { path, contents }
                if path.to_string_lossy().ends_with(suffix) =>
            {
                Some(contents.as_str())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected managed bootstrap at {suffix}"))
}

fn skill_root(dir: &Path) -> PathBuf {
    let capsule_root = dir.join("skill-root");
    write_payload_skill(&capsule_root, "review", "review the project");
    capsule_root
}

#[test]
fn the_same_exact_project_and_run_seed_projects_through_claude_and_isolated_codex() {
    let dir = tempfile::tempdir().unwrap();
    let root = skill_root(dir.path());
    let bootstrap = actor_bootstrap();

    let context = ContextBuilder::new()
        .isolation(Isolation::Worktree)
        .project_skill("skill/code/review", "review", root)
        .actor_bootstrap(bootstrap)
        .build();

    let claude = ClaudeAdapter::new(dir.path().join("generation"));
    let claude_plan = claude.plan(&context).unwrap();
    let codex = CodexAdapter::new(dir.path().join("task-tree"));
    let codex_plan = codex.plan(&context).unwrap();

    let claude_seed = bootstrap_write(
        &claude_plan,
        ".claude/skills/aikit-context/SKILL.md",
    );
    let codex_seed = bootstrap_write(
        &codex_plan,
        ".agents/skills/aikit-context/SKILL.md",
    );

    for seed in [claude_seed, codex_seed] {
        assert!(seed.contains("Project: `project/test`"));
        assert!(seed.contains("Run: `run/client-supplied`"));
        assert!(seed.contains("AgentSession: `session/alpha`"));
        assert!(!seed.contains("component_bindings"));
        assert!(!seed.contains("HarnessComposition"));
    }
    assert_eq!(claude_seed, codex_seed);
    assert_eq!(
        claude_plan.effect,
        ActivationEffect::restart_client("Claude")
    );
    assert!(matches!(
        codex_plan.effect,
        ActivationEffect::NextSessionOnly { .. }
    ));
}

#[test]
fn shared_codex_withholds_the_context_specific_actor_seed_and_reports_brokered_degradation() {
    let dir = tempfile::tempdir().unwrap();
    let root = skill_root(dir.path());
    let context = ContextBuilder::new()
        .isolation(Isolation::Shared)
        .project_skill("skill/code/review", "review", root)
        .actor_bootstrap(actor_bootstrap())
        .build();
    let codex = CodexAdapter::new(dir.path().join("shared-tree"));

    let plan = codex.plan(&context).unwrap();

    assert!(matches!(plan.effect, ActivationEffect::Brokered { .. }));
    assert!(!plan.items.iter().any(|item| match item {
        ProjectionItem::Write { path, .. } => path
            .to_string_lossy()
            .ends_with(".agents/skills/aikit-context/SKILL.md"),
        _ => false,
    }));
    assert!(plan
        .notes
        .iter()
        .any(|note| note.contains("Run/session/runtime-body identity")));
}
