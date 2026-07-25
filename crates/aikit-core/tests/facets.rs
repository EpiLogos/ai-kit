//! `metadata.aikit` facets: the internal/external seam, the surfacing hint, and
//! the shared-language classification.
//!
//! The rule that governs all of them is Part I rule 6 — **facets drive search,
//! presentation and surfacing; they never drive activation.** Nothing becomes
//! active because it carries a facet, exactly as nothing becomes active because it
//! matches a tag. The tests below pin both halves: the facets are read and
//! preserved, and a capsule carrying one is no more active than a capsule without.

mod common;
use common::*;

use aikit_core::capsule::{Capsule, Facing, LanguageFacet, Surface};
use aikit_core::scope::ScopeKind;

#[test]
fn facing_defaults_to_internal_and_parses_all_three_values() {
    // The deliverable of most skills feeds the agent's own work, so `internal` is
    // the default a manifest never has to state.
    let plain = script("script/test/plain");
    assert_eq!(plain.facets.facing, Facing::Internal);

    let external = script_with(
        "script/show/chart",
        "[metadata.aikit]\nfacing = \"external\"\nsurface = \"browser\"",
    );
    assert_eq!(external.facets.facing, Facing::External);
    assert_eq!(external.facets.surface, Some(Surface::Browser));

    let both = guidance_with(
        "guidance/language/deep-modules",
        "[metadata.aikit]\nfacing = \"both\"\nlanguage = \"vocabulary\"",
    );
    assert_eq!(both.facets.facing, Facing::Both);
    assert_eq!(both.facets.language, Some(LanguageFacet::Vocabulary));
}

#[test]
fn an_external_facing_capsule_is_not_active_merely_because_it_is_external() {
    // Rule 6, restated for facets: a facet is a description, not a selection. The
    // external-facing capsule is catalogued and findable; it is active only
    // because a scope selected it.
    let shown = script_with(
        "script/show/chart",
        "[metadata.aikit]\nfacing = \"external\"\nsurface = \"browser\"",
    );
    let ordinary = script("script/test/plain");

    let view = Fixture::new(vec![shown, ordinary])
        .with_layers(vec![layer(ScopeKind::Project, &["script/test/plain"], &[])])
        .resolve()
        .expect("resolves");

    assert!(view.is_active(&cid("script/test/plain")));
    assert!(
        !view.is_active(&cid("script/show/chart")),
        "a facet must never select anything"
    );
    assert!(
        view.catalog_index.contains_key(&cid("script/show/chart")),
        "but it is catalogued and findable"
    );
}

#[test]
fn unknown_metadata_keys_are_preserved_rather_than_rejected_or_dropped() {
    // PRIOR-ART-ACTIONS #30 / SKILLS-ECOSYSTEM §4.1: dropping unknown keys silently
    // degrades every skill AIKit touches, and rejecting them makes AIKit unable to
    // read its neighbours' files at all.
    let capsule = script_with(
        "script/test/foreign",
        "[metadata.aikit]\nfacing = \"external\"\n\n[metadata.someoneelse]\nflavour = \"vanilla\"",
    );
    assert_eq!(capsule.facets.facing, Facing::External);
    assert!(
        capsule.metadata.contains_key("someoneelse"),
        "a foreign metadata namespace is carried, not discarded: {:?}",
        capsule.metadata.keys().collect::<Vec<_>>()
    );
}

#[test]
fn an_unknown_facet_value_is_refused_rather_than_silently_defaulted() {
    // A typo'd `facing = "externl"` must not read as `internal`: silently doing
    // less and reporting success is the failure STANDARDS §1 names.
    let src = r#"schema = 1
id = "script/test/typo"
kind = "script"
name = "typo"
description = "Has a misspelled facet."

[metadata.aikit]
facing = "externl"

[script]
entry = "payload/run.sh"
"#;
    let error = Capsule::from_toml_str(src).unwrap_err();
    assert_eq!(error.code(), "manifest.invalid_facet");
    assert!(
        error.message().contains("externl"),
        "the error names the offending value: {}",
        error.message()
    );
}

#[test]
fn a_surface_is_only_meaningful_on_an_external_facing_capsule() {
    // Declaring where output should land while declaring that output feeds the
    // agent is a contradiction, and dead configuration (STANDARDS §1).
    let src = r#"schema = 1
id = "script/test/contradiction"
kind = "script"
name = "contradiction"
description = "Internal but declares a surface."

[metadata.aikit]
facing = "internal"
surface = "browser"

[script]
entry = "payload/run.sh"
"#;
    let error = Capsule::from_toml_str(src).unwrap_err();
    assert_eq!(error.code(), "manifest.invalid_facet");
}
