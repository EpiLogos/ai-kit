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
        governance_sources: Vec::new(),
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

#[test]
fn project_line_states_it_is_resolved_per_session_from_cwd() {
    // A materialised bootstrap is a file, not a live per-session render: a
    // reader must never take "Project" as an ambient claim about where the
    // *current* session stands. This is what closes O:I #65's
    // actor-bootstrap Project defect (the file could otherwise keep naming
    // a stale Project long after the session moved elsewhere).
    let rendered = render_managed_bootstrap(&actor_bootstrap());
    assert!(rendered.contains("Project: `project/test`"));
    assert!(rendered.contains("resolved per session from the working directory"));
}

fn actor_bootstrap_with_governance_sources() -> ActorBootstrap {
    let mut bootstrap = actor_bootstrap_with_engineering_source();
    // The engineering-scoped ref also carries governance standing (the root
    // scanner marks every file under Control/agents/governance, engineering
    // included, `human-governance`) — it must not be listed twice. The two
    // ordinary refs are root and Project governance respectively, proving
    // both scopes surface together and that `governance_sources` is never
    // truncated by the 12-item `context_sources.examples` cap it bypasses.
    bootstrap.governance_sources = vec![
        r("central:source:control:root:Control/agents/governance/engineering/foundational-prompt.md"),
        r("central:source:control:root:Control/agents/governance/authorship-and-return/responsibility.md"),
        r("source:central:O-I:governance:repo-content.md"),
    ];
    bootstrap
}

#[test]
fn renders_governance_ground_naming_root_and_project_governance() {
    let rendered = render_managed_bootstrap(&actor_bootstrap_with_governance_sources());
    assert!(rendered.contains("## Governance ground"));
    assert!(rendered.contains(
        "central:source:control:root:Control/agents/governance/authorship-and-return/responsibility.md"
    ));
    assert!(rendered.contains("source:central:O-I:governance:repo-content.md"));
    assert!(rendered.contains("retrieve on demand"));
    // Payload never inlined, same law as Engineering ground.
    assert!(
        rendered
            .lines()
            .filter(|line| line.contains("If you find a hole"))
            .count()
            == 0
    );
}

#[test]
fn governance_ground_does_not_duplicate_a_ref_already_named_under_engineering_ground() {
    let rendered = render_managed_bootstrap(&actor_bootstrap_with_governance_sources());
    let engineering_ref =
        "central:source:control:root:Control/agents/governance/engineering/foundational-prompt.md";
    assert_eq!(
        rendered.matches(engineering_ref).count(),
        1,
        "an engineering-scoped governance ref must be named once, under Engineering ground, not again under Governance ground: {rendered}"
    );
}

#[test]
fn omits_governance_ground_section_when_absent() {
    let rendered = render_managed_bootstrap(&actor_bootstrap());
    assert!(!rendered.contains("## Governance ground"));
}
