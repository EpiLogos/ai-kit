//! The continuity tuning: the W1 descope law as code.
//!
//! The default composition reports only the floor (temporal reground). A
//! continuity reaction is operative only when the active composition selects
//! it, and the engine answers "not composed" honestly when asked.

mod common;

use aikit_core::continuity::{
    is_continuity_capability, ContinuityTuning, CONTINUITY_NAMESPACE, FLOOR_CAPABILITY,
};
use aikit_core::capsule::Capsule;
use aikit_core::id::CapsuleId;
use aikit_core::scope::ScopeKind;
use common::*;

fn turn_ledger_capsule() -> Capsule {
    hook_table(
        "hook/continuity/turn-ledger",
        "",
        "entry = \"payload/ledger\"\nevents = [\"UserPromptSubmit\"]",
    )
}

fn entity_disclosure_capsule() -> Capsule {
    hook_table(
        "hook/continuity/entity-disclosure",
        "",
        "entry = \"payload/entity-disclosure\"\nevents = [\"SessionStart\"]",
    )
}

fn cid(s: &str) -> CapsuleId {
    CapsuleId::parse(s).unwrap()
}

#[test]
fn only_hook_capsules_in_the_continuity_namespace_are_continuity_capabilities() {
    assert!(is_continuity_capability(&cid("hook/continuity/turn-ledger")));
    assert!(!is_continuity_capability(&cid("hook/other/turn-ledger")));
    assert!(!is_continuity_capability(&cid("skill/continuity/turn-ledger")));
    assert!(!is_continuity_capability(&cid("script/continuity/turn-ledger")));
}

#[test]
fn the_default_composition_reports_only_the_floor() {
    let fixture = Fixture::new(vec![turn_ledger_capsule()]);
    let view = fixture.resolve().unwrap();

    let tuning = ContinuityTuning::resolve(&view);
    assert_eq!(tuning.floor, FLOOR_CAPABILITY);
    assert!(
        tuning.composed.is_empty(),
        "the descope floor composes nothing: {:?}",
        tuning.composed
    );
    assert!(tuning.allows(FLOOR_CAPABILITY));
    assert!(!tuning.allows("turn-ledger"));

    let report = tuning.describe();
    assert_eq!(report.get("floor").unwrap(), &[FLOOR_CAPABILITY.to_string()]);
    assert!(report.get("composed").unwrap().is_empty());
}

#[test]
fn a_known_but_uncomposed_reaction_is_answered_not_composed() {
    // With the capsule known to the catalog but not selected by any layer,
    // the engine answers "not composed" instead of staying silent.
    let fixture = Fixture::new(vec![turn_ledger_capsule()]);
    let view = fixture.resolve().unwrap();

    let tuning = ContinuityTuning::resolve(&view);
    assert!(
        tuning
            .not_composed
            .iter()
            .any(|name| name == "turn-ledger"),
        "turn-ledger is known but not composed and must be answered as such: {tuning}"
    );
}

#[test]
fn composing_the_delta_makes_exactly_that_reaction_operative() {
    let fixture = Fixture::new(vec![turn_ledger_capsule()]).with_layers(vec![layer(
        ScopeKind::Session,
        &["hook/continuity/turn-ledger"],
        &[],
    )]);
    let view = fixture.resolve().unwrap();

    let tuning = ContinuityTuning::resolve(&view);
    assert_eq!(tuning.composed, vec!["turn-ledger".to_string()]);
    assert!(tuning.allows("turn-ledger"));
    // Composing one capability composes nothing else.
    assert!(!tuning.allows("orientation-packet"));

    let report = tuning.describe();
    assert_eq!(report.get("composed").unwrap(), &["turn-ledger".to_string()]);
}

#[test]
fn the_floor_is_law_and_never_gated() {
    // Even an empty composition keeps the floor: temporal reground is not a
    // capability someone forgot to enable.
    let fixture = Fixture::new(vec![]);
    let view = fixture.resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);
    assert!(tuning.allows(FLOOR_CAPABILITY));
    assert_eq!(CONTINUITY_NAMESPACE, "continuity");
}

#[test]
fn entity_disclosure_is_known_but_not_composed_by_default() {
    // W10 V6: the reaction exists and is answerable, but the descope floor
    // does not compose it.
    let fixture = Fixture::new(vec![entity_disclosure_capsule()]);
    let view = fixture.resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);
    assert!(
        tuning.not_composed.iter().any(|name| name == "entity-disclosure"),
        "entity-disclosure must be answerable as not composed: {tuning}"
    );
    assert!(!tuning.allows("entity-disclosure"));
}

#[test]
fn composing_entity_disclosure_is_exactly_that_delta() {
    let fixture = Fixture::new(vec![entity_disclosure_capsule()]).with_layers(vec![layer(
        ScopeKind::Session,
        &["hook/continuity/entity-disclosure"],
        &[],
    )]);
    let view = fixture.resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);
    assert_eq!(tuning.composed, vec!["entity-disclosure".to_string()]);
    assert!(tuning.allows("entity-disclosure"));
    // Composing entity-disclosure composes nothing else.
    assert!(!tuning.allows("turn-ledger"));
}
