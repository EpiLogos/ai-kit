mod common;

use common::*;

use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::scope::ScopeKind;
use aikit_tui::{PaletteBackend, ResourceExplanation, ResourceMutation};

fn rref(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.keep();
    Fixture::new(
        &root,
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    )
}

#[test]
fn v1_capabilities_are_exposed_through_stable_resource_refs() {
    let backend = fixture();
    let summaries = backend.resource_summaries();

    assert_eq!(summaries.len(), 2);
    assert!(summaries.iter().all(|summary| summary.kind == ResourceKind::Capability));
    assert!(summaries
        .iter()
        .any(|summary| summary.resource == rref("skill/rust/review")));
    assert!(summaries
        .iter()
        .any(|summary| summary.resource == rref("script/ops/deploy")));

    let explanation = backend
        .explain_resource(&rref("skill/rust/review"))
        .expect("legacy capability has a core explanation");
    assert!(matches!(explanation, ResourceExplanation::Capability(_)));

    assert!(backend.context_resolution().is_none());
    assert!(backend.context_source_disclosure().is_empty());
}

#[test]
fn representative_v1_preview_and_apply_flow_works_through_resource_mutations() {
    let mut backend = fixture();
    let mutation = ResourceMutation::new(rref("skill/rust/review"), true);

    let projected = backend
        .preview_resources(ScopeKind::Session, std::slice::from_ref(&mutation))
        .expect("resource preview delegates to the existing resolver-backed application service");
    assert!(projected.view.is_declared_enabled(&cid("skill/rust/review")));
    assert!(backend.applied.is_empty(), "preview must remain non-mutating");

    backend
        .apply_resources(ScopeKind::Session, &[mutation])
        .expect("resource apply delegates to the existing single write path");
    assert_eq!(backend.applied.len(), 1);
    assert_eq!(backend.applied[0].0, ScopeKind::Session);
    assert_eq!(backend.applied[0].1[0].capsule, cid("skill/rust/review"));
    assert!(backend.applied[0].1[0].enable);
}

#[test]
fn non_capability_resource_mutation_is_explicitly_refused_by_v1_adapter() {
    let backend = fixture();
    let error = backend
        .preview_resources(
            ScopeKind::Session,
            &[ResourceMutation::new(rref("context-source:docs"), true)],
        )
        .unwrap_err();

    assert_eq!(error.code(), "tui.resource_mutation_unsupported");
    assert_eq!(
        error.details().get("resource").map(String::as_str),
        Some("context-source:docs")
    );
}
