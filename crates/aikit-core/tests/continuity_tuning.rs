//! The continuity tuning: the W1 descope law as code.
//!
//! The default composition reports only the floor (temporal reground). A
//! continuity reaction is operative only when the active composition selects
//! it, and the engine answers "not composed" honestly when asked.

mod common;

use aikit_core::capsule::Capsule;
use aikit_core::continuity::{
    is_continuity_capability, ContinuityTuning, CONTINUITY_NAMESPACE, FLOOR_CAPABILITY,
};
use aikit_core::id::CapsuleId;
use aikit_core::profile::{ConfigTable, PoolPatch};
use aikit_core::scope::ScopeKind;
use aikit_core::scope::{LayerOrigin, ScopeLayer};
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

fn domain_activation_capsule() -> Capsule {
    hook_table(
        "hook/continuity/domain-activation",
        "",
        "entry = \"payload/domain-activation\"\nevents = [\"UserPromptSubmit\"]",
    )
}

fn activity_evidence_capsule() -> Capsule {
    hook_table("hook/continuity/activity-evidence", "",
        "entry = \"payload/activity-evidence\"\nevents = [\"PostToolUse\"]")
}

fn cid(s: &str) -> CapsuleId {
    CapsuleId::parse(s).unwrap()
}

#[test]
fn only_hook_capsules_in_the_continuity_namespace_are_continuity_capabilities() {
    assert!(is_continuity_capability(&cid(
        "hook/continuity/turn-ledger"
    )));
    assert!(!is_continuity_capability(&cid("hook/other/turn-ledger")));
    assert!(!is_continuity_capability(&cid(
        "skill/continuity/turn-ledger"
    )));
    assert!(!is_continuity_capability(&cid(
        "script/continuity/turn-ledger"
    )));
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
    assert_eq!(report["floor"], serde_json::json!(FLOOR_CAPABILITY));
    assert_eq!(report["composed"], serde_json::json!([]));
    assert_eq!(
        report["tunings"],
        serde_json::json!({}),
        "an uncomposed field tunes nothing"
    );
}

#[test]
fn a_known_but_uncomposed_reaction_is_answered_not_composed() {
    // With the capsule known to the catalog but not selected by any layer,
    // the engine answers "not composed" instead of staying silent.
    let fixture = Fixture::new(vec![turn_ledger_capsule()]);
    let view = fixture.resolve().unwrap();

    let tuning = ContinuityTuning::resolve(&view);
    assert!(
        tuning.not_composed.iter().any(|name| name == "turn-ledger"),
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
    assert_eq!(report["composed"], serde_json::json!(["turn-ledger"]));
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
        tuning
            .not_composed
            .iter()
            .any(|name| name == "entity-disclosure"),
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

#[test]
fn domain_activation_is_known_but_not_composed_by_default() {
    // W1/CASE 03: the reaction exists and is answerable, but the descope
    // floor never arms domains.
    let fixture = Fixture::new(vec![domain_activation_capsule()]);
    let view = fixture.resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);
    assert!(
        tuning
            .not_composed
            .iter()
            .any(|name| name == "domain-activation"),
        "domain-activation must be answerable as not composed: {tuning}"
    );
    assert!(!tuning.allows("domain-activation"));
}

#[test]
fn composing_domain_activation_is_exactly_that_delta() {
    let fixture = Fixture::new(vec![domain_activation_capsule()]).with_layers(vec![layer(
        ScopeKind::Session,
        &["hook/continuity/domain-activation"],
        &[],
    )]);
    let view = fixture.resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);
    assert_eq!(tuning.composed, vec!["domain-activation".to_string()]);
    assert!(tuning.allows("domain-activation"));
    assert!(!tuning.allows("turn-ledger"));
    assert!(!tuning.allows("entity-disclosure"));
}

#[test]
fn activity_evidence_is_known_but_inoperative_until_composed() {
    let view = Fixture::new(vec![activity_evidence_capsule()]).resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);
    assert!(tuning.not_composed.iter().any(|n| n == "activity-evidence"));
    assert!(!tuning.allows("activity-evidence"));
}

#[test]
fn composing_activity_evidence_arms_only_that_reaction() {
    let fixture = Fixture::new(vec![activity_evidence_capsule()]).with_layers(vec![layer(
        ScopeKind::Session, &["hook/continuity/activity-evidence"], &[])]);
    let view = fixture.resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);
    assert_eq!(tuning.composed, vec!["activity-evidence".to_string()]);
    assert!(tuning.allows("activity-evidence"));
    assert!(!tuning.allows("turn-ledger"));
}

// ---------------------------------------------------------------------------
// CASE 14 clause 3 — adjusted tuning values are attributable to the active
// composition, never to ambient global state.
// ---------------------------------------------------------------------------

fn orientation_packet_capsule() -> Capsule {
    hook_table(
        "hook/continuity/orientation-packet",
        "",
        "entry = \"payload/orientation-packet\"\nevents = [\"SessionStart\"]",
    )
}

/// A layer that both composes a capability and tunes it, the way a real
/// delta does: `[config.<capsule-id>]` rides the same patch as the enable.
fn layer_tuning(kind: ScopeKind, enable: &str, config: ConfigTable) -> ScopeLayer {
    let id = cid(enable);
    let mut table = std::collections::BTreeMap::new();
    table.insert(id.clone(), config);
    ScopeLayer {
        kind,
        depth: 0,
        origin: LayerOrigin::new(format!("test:{}", kind.as_str())),
        patch: PoolPatch {
            profiles: vec![],
            uses: vec![],
            enable: vec![id],
            disable: vec![],
            config: table,
            skill_overlays: Default::default(),
        },
    }
}

fn budget(max_items: i64) -> ConfigTable {
    let mut t = ConfigTable::new();
    t.insert("max_items".to_string(), toml::Value::Integer(max_items));
    t
}

/// The clause: a tuning adjusted within a delta is reported *as the value in
/// effect*, not merely as the name of the capability carrying it. Before this,
/// `aikit context` printed capability names only — an operator could see that
/// orientation-packet was composed but not what budget it was running.
#[test]
fn an_adjusted_tuning_value_is_reported_as_the_value_in_effect() {
    let fixture = Fixture::new(vec![orientation_packet_capsule()]).with_layers(vec![layer_tuning(
        ScopeKind::Session,
        "hook/continuity/orientation-packet",
        budget(3),
    )]);
    let view = fixture.resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);

    assert_eq!(tuning.composed, vec!["orientation-packet".to_string()]);
    let effect = tuning
        .tunings
        .get("orientation-packet")
        .expect("a composed capability reports the values it runs under");
    assert_eq!(
        effect.values.get("max_items"),
        Some(&toml::Value::Integer(3)),
        "the adjusted value itself must be readable: {:?}",
        effect.values
    );
}

/// The other half of the clause: the value is *attributable* — the report
/// names the active composition that owns it, so it can never be mistaken for
/// ambient global state.
#[test]
fn the_value_in_effect_names_the_active_composition_that_owns_it() {
    let fixture = Fixture::new(vec![orientation_packet_capsule()]).with_layers(vec![layer_tuning(
        ScopeKind::Session,
        "hook/continuity/orientation-packet",
        budget(7),
    )]);
    let view = fixture.resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);

    let effect = &tuning.tunings["orientation-packet"];
    assert!(
        !effect.composition.is_empty(),
        "an attributable value always names its active composition"
    );
    assert!(
        effect.composition.contains("session"),
        "the report names the layer that selected the active composition, got: {}",
        effect.composition
    );
}

/// A composed capability that nobody tuned reports an *empty* value set, not
/// an absent one. "Running on its declared defaults, adjusted by nobody" is a
/// real answer; silence would be indistinguishable from "not composed".
#[test]
fn a_composed_but_untuned_capability_reports_an_empty_value_set_not_absence() {
    let fixture = Fixture::new(vec![orientation_packet_capsule()]).with_layers(vec![layer(
        ScopeKind::Session,
        &["hook/continuity/orientation-packet"],
        &[],
    )]);
    let view = fixture.resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);

    let effect = tuning
        .tunings
        .get("orientation-packet")
        .expect("composed means present in the report, tuned or not");
    assert!(
        effect.values.is_empty(),
        "nobody adjusted it: {:?}",
        effect.values
    );
}

/// An uncomposed capability contributes no tuning at all. The report cannot
/// suggest that a value is in effect for something that never runs.
#[test]
fn an_uncomposed_capability_contributes_no_tuning_to_the_report() {
    let fixture = Fixture::new(vec![orientation_packet_capsule()]);
    let view = fixture.resolve().unwrap();
    let tuning = ContinuityTuning::resolve(&view);

    assert!(tuning
        .not_composed
        .iter()
        .any(|name| name == "orientation-packet"));
    assert!(
        !tuning.tunings.contains_key("orientation-packet"),
        "a capability that does not run has no values in effect: {:?}",
        tuning.tunings
    );
}

/// The report `aikit context` renders carries the values, so the inspection
/// surface and the behaviour are read from one resolve of one view — they
/// cannot drift apart.
#[test]
fn the_inspection_report_carries_the_values_not_just_the_names() {
    let fixture = Fixture::new(vec![orientation_packet_capsule()]).with_layers(vec![layer_tuning(
        ScopeKind::Session,
        "hook/continuity/orientation-packet",
        budget(5),
    )]);
    let view = fixture.resolve().unwrap();
    let report = ContinuityTuning::resolve(&view).describe();

    assert_eq!(
        report["composed"],
        serde_json::json!(["orientation-packet"])
    );
    assert_eq!(
        report["tunings"]["orientation-packet"]["values"]["max_items"],
        serde_json::json!(5),
        "the rendered report is the attribution surface: {report}"
    );
    assert!(
        report["tunings"]["orientation-packet"]["composition"]
            .as_str()
            .is_some_and(|o| !o.is_empty()),
        "every reported value is attributed: {report}"
    );
}
