mod common;

use std::fs;

use aikit_core::catalog::Catalog;
use aikit_core::resource::SourceAuthority;
use aikit_core::trust::MemoryTrust;
use aikit_core::{
    ContextDescriptor, LayerOrigin, PoolPatch, RegistrySource, ResolveRequest, ScopeKind,
    ScopeLayer, TrustState,
};
use aikit_store::{compare_generation_worlds, registry, AikitHome, GenerationBuilder};
use common::{cid, RegistryFixture};

fn resolve_with(
    fixture: &RegistryFixture,
    context: &ContextDescriptor,
    enable: &[&str],
) -> aikit_core::ResolvedView {
    let load = registry::load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load.problems.is_empty(), "{:?}", load.problems);

    let mut trust = MemoryTrust::default();
    for capsule in Catalog::capsules(&load.catalog) {
        trust.set(
            capsule.source.clone().unwrap(),
            capsule.id.clone(),
            capsule.revision.clone().unwrap(),
            TrustState::Reviewed,
        );
    }
    let request = ResolveRequest {
        context: context.clone(),
        layers: vec![ScopeLayer::new(
            ScopeKind::Project,
            LayerOrigin::new("tests/generation-history"),
            PoolPatch {
                enable: enable.iter().map(|id| cid(id)).collect(),
                ..Default::default()
            },
        )],
        policy: Default::default(),
    };
    aikit_core::resolve(&load.catalog, &trust, &request).unwrap()
}

#[test]
fn comparison_reads_two_committed_locks_and_reports_real_capability_change() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path().join("home"));
    home.ensure_layout().unwrap();

    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.script("script/test/nt");
    fixture.skill("skill/rust/review");

    let context = ContextDescriptor::for_project(tmp.path().join("project"));
    let before_view = resolve_with(&fixture, &context, &["script/test/nt"]);
    let after_view = resolve_with(
        &fixture,
        &context,
        &["script/test/nt", "skill/rust/review"],
    );
    assert_ne!(before_view.hash, after_view.hash);

    let context_dir = home.context_dir(&context.context_id);
    fs::create_dir_all(context_dir.join("generations")).unwrap();
    let before = GenerationBuilder::new()
        .build(&context_dir, &before_view, &[])
        .unwrap()
        .commit(None)
        .unwrap()
        .id;
    let after = GenerationBuilder::new()
        .build(&context_dir, &after_view, &[])
        .unwrap()
        .commit(Some(&before))
        .unwrap()
        .id;

    let comparison =
        compare_generation_worlds(&home, &context.context_id, &before, &after).unwrap();
    assert_eq!(comparison.before, before);
    assert_eq!(comparison.after, after);
    assert_eq!(
        comparison.activated,
        vec![aikit_core::resource::ResourceRef::parse("skill/rust/review").unwrap()]
    );
    assert!(comparison.deactivated.is_empty());
    assert_eq!(
        comparison.evidence.authorities,
        vec![SourceAuthority::Generated, SourceAuthority::Derived]
    );
    assert!(comparison
        .evidence
        .canonical_refs
        .contains(&aikit_core::resource::ResourceRef::parse("skill/rust/review").unwrap()));
    assert!(!comparison.is_noop());
}

#[test]
fn arbitrary_historical_comparison_is_inspection_not_generic_restore() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path().join("home"));
    home.ensure_layout().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.script("script/test/nt");
    let context = ContextDescriptor::for_project(tmp.path().join("project"));
    let view = resolve_with(&fixture, &context, &["script/test/nt"]);

    let context_dir = home.context_dir(&context.context_id);
    fs::create_dir_all(context_dir.join("generations")).unwrap();
    let before = GenerationBuilder::new()
        .build(&context_dir, &view, &[])
        .unwrap()
        .commit(None)
        .unwrap()
        .id;

    let comparison =
        compare_generation_worlds(&home, &context.context_id, &before, &before).unwrap();
    assert!(comparison.is_noop());
    assert_eq!(
        comparison.evidence.recoverability,
        aikit_core::HistoryRecoverability::InspectOnly
    );
}
