mod common;

use std::path::PathBuf;

use aikit_adapters::clients::bootstrap::render_managed_bootstrap;
use aikit_core::actor_bootstrap::{ActorBootstrap, ResourceSetSummary, ACTOR_BOOTSTRAP_VERSION};
use aikit_core::platform::TargetId;
use aikit_core::project::{
    ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
};
use aikit_core::resource::ResourceRef;
use aikit_core::session_space::SessionSpaceRef;

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
        harness_candidates: Vec::new(),
        model_candidates: Vec::new(),
        agent_session: Some("session/alpha".into()),
        session_space: Some(SessionSpaceRef::parse("session-space/test").unwrap()),
        capabilities: empty_summary(),
        actions: empty_summary(),
        context_sources: empty_summary(),
        projection_targets: vec![TargetId::claude_code(), TargetId::codex()],
        runtime_body: None,
        warnings: Vec::new(),
    }
}

fn actor_bootstrap_with_engineering_source() -> ActorBootstrap {
    let mut bootstrap = actor_bootstrap();
    bootstrap.context_sources = ResourceSetSummary {
        total: 2,
        available: 2,
        unresolved: 0,
        unavailable: 0,
        examples: vec![
            r("central:source:control:root:Control/agents/governance/engineering/foundational-prompt.md"),
            r("context-source/project/readme"),
        ],
        truncated: false,
    };
    bootstrap
}

#[test]
fn renders_engineering_ground_provenance_line_when_resolved() {
    let rendered = render_managed_bootstrap(&actor_bootstrap_with_engineering_source());
    assert!(rendered.contains("## Engineering ground"));
    assert!(rendered.contains(
        "central:source:control:root:Control/agents/governance/engineering/foundational-prompt.md"
    ));
    assert!(rendered.contains("retrieve on demand"));
    // Named, not copied: the payload must not be inlined.
    assert!(
        rendered
            .lines()
            .filter(|line| line.contains("You branch small"))
            .count()
            == 0
    );
}

#[test]
fn omits_engineering_ground_section_when_absent() {
    let rendered = render_managed_bootstrap(&actor_bootstrap());
    assert!(!rendered.contains("## Engineering ground"));
}
