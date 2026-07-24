//! `related_skills` (PRIOR-ART-ACTIONS L5) is first-class capsule metadata: it
//! parses from the manifest, rides through resolution in the catalog index, and
//! is surfaced by `explain` — the palette and the tree read the same field.

mod common;
use common::*;

use aikit_core::scope::ScopeKind;

#[test]
fn related_skills_parse_from_the_manifest_and_surface_in_explain() {
    let review = skill_with(
        "skill/rust/review",
        "related_skills = [\"skill/rust/unsafe-audit\"]",
    );
    let audit = skill("skill/rust/unsafe-audit");

    let view = Fixture::new(vec![review, audit])
        .with_layers(vec![layer(ScopeKind::Project, &["skill/rust/review"], &[])])
        .resolve()
        .expect("resolves");

    let explanation = view
        .explain(&cid("skill/rust/review"))
        .expect("review is catalogued");
    assert_eq!(
        explanation.related_skills,
        vec![cid("skill/rust/unsafe-audit")],
        "the related pointer is carried, not dropped"
    );

    let rendered = explanation.render();
    assert!(
        rendered.contains("Often used with"),
        "explain surfaces the relationship: {rendered}"
    );
    assert!(rendered.contains("skill/rust/unsafe-audit"));
}
