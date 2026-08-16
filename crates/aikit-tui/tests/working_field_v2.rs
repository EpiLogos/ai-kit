use std::collections::BTreeSet;

use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_tui::{
    select_working_field_subject, PermissionProjection, ResourceListItem, ResourceListReadModel,
    SurfaceProjection, TerminalContributionKind, TerminalWorkingField, TuiState,
    WorkingFieldAvailability, WorkingFieldItem,
};

const CENTRAL_PERSONAL_REVISION: &str = "3f0551090ae39bcef260a27b1a9db0da4729d8a3";
const ACTUATION_AGENCY_REVISION: &str = "b977939ec25c32b3dc8f5ed251b70e4c26933086";

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn permission(owner: &str, meaning: &str) -> PermissionProjection {
    PermissionProjection {
        authority_owner: r(owner),
        policy_ref: None,
        meaning: meaning.into(),
    }
}

fn fixture_field() -> TerminalWorkingField {
    // SessionSpace now has a live AIKit runtime/read-model boundary. This parity
    // fixture keeps only the cross-product Surface statement; lifecycle detail is
    // exercised by working_field_session_space_v2 rather than duplicated here.
    let session_space = WorkingFieldItem {
        subject: r("session-space/current"),
        semantic_kind: "SessionSpace".into(),
        owner: r("ai-kit"),
        actions: vec![],
        surfaces: vec![
            SurfaceProjection {
                surface: r("surface/aikit/tui"),
                terminal_representation: true,
                alternate_reason: None,
            },
            SurfaceProjection {
                surface: r("surface/oi/desktop"),
                terminal_representation: false,
                alternate_reason: Some(
                    "O:I desktop PR #34 consumes the AIKit-owned read model as a peer Surface host; it does not own SessionSpace activation".into(),
                ),
            },
        ],
        contribution_kinds: BTreeSet::from([
            TerminalContributionKind::Reading,
            TerminalContributionKind::Relation,
            TerminalContributionKind::CommandNavigation,
        ]),
        permission: permission(
            "ai-kit",
            "SessionSpace composition transfers reference/possibility, never ambient trust or Action authority",
        ),
        provenance: vec!["aikit.session-space/v1 live read model".into()],
        availability: WorkingFieldAvailability::Available,
    };

    // Central PR #53 is now a real native producer. Preserve its actual read-model
    // and Action refs instead of keeping the earlier placeholder contract fixture.
    let personal = WorkingFieldItem {
        subject: r("personal.show"),
        semantic_kind: "action".into(),
        owner: r("central"),
        actions: vec![
            r("personal.show"),
            r("control.propose-change"),
            r("control.review-proposal"),
            r("control.apply-proposal"),
            r("personal.notify"),
        ],
        surfaces: vec![SurfaceProjection {
            surface: r("surface/aikit/tui"),
            terminal_representation: true,
            alternate_reason: None,
        }],
        contribution_kinds: BTreeSet::from([
            TerminalContributionKind::Reading,
            TerminalContributionKind::Inspector,
            TerminalContributionKind::ActionBinding,
        ]),
        permission: permission(
            "central",
            "Central owns authored Control/Work plus proposal/review/apply authority; notification delivery is not human acknowledgement",
        ),
        provenance: vec![format!(
            "EpiLogos/Central#53@{CENTRAL_PERSONAL_REVISION}:ctrl/src/personal.rs"
        )],
        availability: WorkingFieldAvailability::Available,
    };

    // Actuation PR #6 publishes a stable agency read model. Its executable root
    // fixture uses agency:root-position; root remains a positional WorldBinding
    // relation and never becomes a RootAgent subtype.
    let agency = WorkingFieldItem {
        subject: r("agency:root-position"),
        semantic_kind: "agency_reading".into(),
        owner: r("actuation"),
        actions: vec![],
        surfaces: vec![SurfaceProjection {
            surface: r("surface/aikit/tui"),
            terminal_representation: true,
            alternate_reason: None,
        }],
        contribution_kinds: BTreeSet::from([
            TerminalContributionKind::Reading,
            TerminalContributionKind::Relation,
            TerminalContributionKind::Inspector,
        ]),
        permission: permission(
            "actuation",
            "WorldBinding constraints, metagency grants, delegated autonomy and Return recognition retain Actuation meaning",
        ),
        provenance: vec![format!(
            "EpiLogos/Actuation#6@{ACTUATION_AGENCY_REVISION}:contracts/agency.mjs:agencyReadModel:agency:root-position"
        )],
        availability: WorkingFieldAvailability::Available,
    };

    // Factory #144/#145 have not yet produced the native Build read model/Actions.
    // Keep only the explicit host contract fixture; do not counterfeit Candidate Actions.
    let build = WorkingFieldItem {
        subject: r("factory.surface/build"),
        semantic_kind: "Surface".into(),
        owner: r("software-factory"),
        actions: vec![],
        surfaces: vec![
            SurfaceProjection {
                surface: r("surface/aikit/tui"),
                terminal_representation: true,
                alternate_reason: None,
            },
            SurfaceProjection {
                surface: r("surface/oi/factory-build"),
                terminal_representation: false,
                alternate_reason: Some(
                    "desktop Build remains a pending native Factory adapter, not an AIKit-owned Build model".into(),
                ),
            },
        ],
        contribution_kinds: BTreeSet::from([
            TerminalContributionKind::Reading,
            TerminalContributionKind::Relation,
            TerminalContributionKind::Inspector,
            TerminalContributionKind::Trajectory,
        ]),
        permission: permission(
            "software-factory",
            "Factory owns Candidate, HumanRequest, Recognition and Return semantics when its native adapter lands",
        ),
        provenance: vec!["EpiLogos/agent-system-design#143/#144/#145: native adapter pending".into()],
        availability: WorkingFieldAvailability::ContractFixture {
            live_gate: "Factory #144/#145 native Build read model/Actions".into(),
        },
    };

    // #66's connection adapter now has an explicit AgentSession→SessionSpace
    // bridge. Protocol operations still do not become canonical Actions.
    let agent_session = WorkingFieldItem {
        subject: r("agent-session/live-acp"),
        semantic_kind: "AgentSession".into(),
        owner: r("ai-kit"),
        actions: vec![],
        surfaces: vec![SurfaceProjection {
            surface: r("surface/aikit/tui"),
            terminal_representation: true,
            alternate_reason: None,
        }],
        contribution_kinds: BTreeSet::from([
            TerminalContributionKind::Reading,
            TerminalContributionKind::Relation,
        ]),
        permission: permission(
            "ai-kit",
            "ACP permission requests retain transport-native provenance until an owning product explicitly governs/projects them",
        ),
        provenance: vec!["aikit.connection-adapter/acp/v1 -> aikit.session-space/v1".into()],
        availability: WorkingFieldAvailability::Available,
    };

    let rich_component = WorkingFieldItem {
        subject: r("component/deepseek/client-ui-conversation"),
        semantic_kind: "ComponentContribution".into(),
        owner: r("deepseek-ai/deepseek-harness"),
        actions: vec![],
        surfaces: vec![
            SurfaceProjection {
                surface: r("surface/aikit/tui"),
                terminal_representation: true,
                alternate_reason: None,
            },
            SurfaceProjection {
                surface: r("surface/deepseek/web-conversation"),
                terminal_representation: false,
                alternate_reason: Some(
                    "rich React conversation UI is disclosed as another Surface, not cloned".into(),
                ),
            },
        ],
        contribution_kinds: BTreeSet::from([
            TerminalContributionKind::Reading,
            TerminalContributionKind::Inspector,
        ]),
        permission: permission(
            "deepseek-ai/deepseek-harness",
            "target-native UI contribution remains DeepSeek/Cordis owned",
        ),
        provenance: vec!["deepseek-ai/deepseek-harness exact-revision conformance".into()],
        availability: WorkingFieldAvailability::Available,
    };

    TerminalWorkingField::new(
        "fixture/oi-field-v2",
        vec![
            session_space,
            personal,
            agency,
            build,
            agent_session,
            rich_component,
        ],
    )
    .unwrap()
    .with_enclosing_world(r("session-space/current"))
}

#[test]
fn parity_fixture_consumes_live_central_and_actuation_refs_without_reowning_them() {
    let field = fixture_field();

    let personal = field.item(&r("personal.show")).unwrap();
    assert_eq!(personal.owner, r("central"));
    assert_eq!(
        personal.actions,
        vec![
            r("personal.show"),
            r("control.propose-change"),
            r("control.review-proposal"),
            r("control.apply-proposal"),
            r("personal.notify"),
        ]
    );
    assert_eq!(personal.permission.authority_owner, r("central"));
    assert!(matches!(
        &personal.availability,
        WorkingFieldAvailability::Available
    ));
    assert!(personal.provenance[0].contains(CENTRAL_PERSONAL_REVISION));

    let agency = field.item(&r("agency:root-position")).unwrap();
    assert_eq!(agency.owner, r("actuation"));
    assert_eq!(agency.semantic_kind, "agency_reading");
    assert!(agency.actions.is_empty());
    assert!(matches!(
        &agency.availability,
        WorkingFieldAvailability::Available
    ));
    assert!(agency.provenance[0].contains(ACTUATION_AGENCY_REVISION));
    assert!(agency.permission.meaning.contains("Return recognition"));
}

#[test]
fn live_session_space_and_agent_session_replace_obsolete_contract_fixtures() {
    let field = fixture_field();

    let session_space = field.item(&r("session-space/current")).unwrap();
    assert!(session_space.actions.is_empty());
    assert!(matches!(
        &session_space.availability,
        WorkingFieldAvailability::Available
    ));
    assert!(session_space.provenance[0].contains("aikit.session-space/v1"));

    let session = field.item(&r("agent-session/live-acp")).unwrap();
    assert!(session.actions.is_empty());
    assert!(matches!(
        &session.availability,
        WorkingFieldAvailability::Available
    ));

    let build = field.item(&r("factory.surface/build")).unwrap();
    assert!(build.actions.is_empty());
    assert!(matches!(
        &build.availability,
        WorkingFieldAvailability::ContractFixture { .. }
    ));
}

#[test]
fn unsupported_rich_ui_is_disclosed_as_an_alternate_surface_not_fake_terminal_parity() {
    let field = fixture_field();
    let component = field
        .item(&r("component/deepseek/client-ui-conversation"))
        .unwrap();
    let alternate = component
        .alternate_surfaces()
        .find(|surface| surface.surface == r("surface/deepseek/web-conversation"))
        .unwrap();
    assert!(!alternate.terminal_representation);
    assert!(alternate
        .alternate_reason
        .as_deref()
        .unwrap()
        .contains("not cloned"));
}

#[test]
fn working_field_selection_reuses_authoritative_tui_state_and_exact_resource_ref() {
    let field = fixture_field();
    let selected = r("personal.show");
    let state = TuiState {
        read_model: ResourceListReadModel {
            revision: "fixture/oi-field-v2".into(),
            resources: field
                .items
                .iter()
                .map(|item| ResourceListItem {
                    resource: item.subject.clone(),
                    kind: ResourceKind::Project,
                    label: item.semantic_kind.clone(),
                    summary: item.owner.to_string(),
                })
                .collect(),
        },
        ..TuiState::default()
    };

    let reduction = select_working_field_subject(state, &field, selected.clone()).unwrap();
    assert_eq!(reduction.state.selected, Some(selected.clone()));
    assert_eq!(reduction.effects.len(), 1);
    assert!(reduction
        .effects
        .iter()
        .any(|effect| matches!(effect, aikit_tui::UiEffect::LoadContextualActions { subject } if subject == &selected)));
}

#[test]
fn parity_fixture_cannot_hide_missing_provenance_or_an_unrenderable_surface_reason() {
    let mut field = fixture_field();
    let mut item = field.items.remove(0);
    item.provenance.clear();
    let error = TerminalWorkingField::new("bad", vec![item]).unwrap_err();
    assert_eq!(error.code(), "tui.working_field.missing_provenance");

    let mut field = fixture_field();
    let mut item = field.items.remove(0);
    item.surfaces.push(SurfaceProjection {
        surface: r("surface/other/rich"),
        terminal_representation: false,
        alternate_reason: None,
    });
    let error = TerminalWorkingField::new("bad-surface", vec![item]).unwrap_err();
    assert_eq!(
        error.code(),
        "tui.working_field.alternate_surface_reason_required"
    );
}
