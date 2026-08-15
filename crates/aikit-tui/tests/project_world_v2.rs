mod common;

use common::*;

use aikit_core::scope::ScopeKind;
use aikit_tui::{PaletteApplicationService, PaletteBackend, ProjectWorldReadModel, Toggle};

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(dir.path(), vec![script("script/ops/deploy")]);
    (dir, backend)
}

#[test]
fn project_world_discloses_current_context_without_fabricating_missing_providers() {
    let (_dir, backend) = fixture();
    let world = ProjectWorldReadModel::from_backend(&backend);

    assert_eq!(world.project.label, "payments");
    assert_eq!(world.project.root.as_deref(), Some("/work/payments"));
    assert_eq!(world.focus.as_deref(), Some("project"));
    assert!(world.mutation_scopes.contains(&ScopeKind::Project));
    assert!(world.mutation_scopes.contains(&ScopeKind::Session));
    assert!(world
        .projection
        .targets
        .iter()
        .any(|target| target == "shell"));

    // Host comes from the actual ContextDescriptor; richer actor/runtime bindings
    // remain explicit absences until a provider genuinely discloses them.
    assert_eq!(world.actor_runtime.hosts.resources.len(), 1);
    assert_eq!(world.actor_runtime.hosts.resources[0].label, "test-host");
    assert!(world.actor_runtime.agents.resources.is_empty());
    assert!(world.actor_runtime.agents.absence.is_some());
    assert!(world.actor_runtime.agencies.resources.is_empty());
    assert!(world.actor_runtime.models.resources.is_empty());
    assert!(world.actor_runtime.harnesses.resources.is_empty());

    // Descriptor disclosure is not retrieval. With no ContextSource provider in
    // this fixture the horizon says so rather than synthesising a source.
    assert!(world.information_horizon.context_sources.resources.is_empty());
    assert!(world.information_horizon.context_sources.absence.is_some());

    assert!(world.generation.generation.is_none());
    assert!(world.generation.absence.is_some());
}

#[test]
fn declared_capability_intent_and_effective_activation_remain_distinct_axes() {
    let (_dir, mut backend) = fixture();
    let resource = aikit_core::resource::ResourceRef::parse("script/ops/deploy").unwrap();

    let initial = ProjectWorldReadModel::from_backend(&backend);
    assert!(initial.capability_horizon.declared.is_empty());
    assert!(!initial.capability_horizon.effective.contains(&resource));

    PaletteBackend::apply(
        &mut backend,
        ScopeKind::Project,
        &[Toggle::new(cid("script/ops/deploy"), true)],
    )
    .unwrap();

    let applied = ProjectWorldReadModel::from_backend(&backend);
    let declared = applied
        .capability_horizon
        .declared
        .iter()
        .find(|entry| entry.resource == resource)
        .expect("applied authored intent should be disclosed");
    assert!(declared.enabled);
    assert_eq!(declared.scope, ScopeKind::Project);
    assert!(applied.capability_horizon.effective.contains(&resource));
}

#[test]
fn workspace_consumes_project_world_through_the_existing_application_service() {
    let (_dir, mut backend) = fixture();
    let service = PaletteApplicationService::new(&mut backend);
    let world = service.project_world();

    assert_eq!(world.project.label, "payments");
    assert_eq!(world.revision, format!("{}:{}", service.backend().view().catalog_revision, service.backend().view().hash));
}
