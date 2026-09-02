from pathlib import Path

shape = Path('crates/aikit-core/src/knowledge_wiki_shape.rs')
source = shape.read_text()

import_anchor = '''use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::Value;

use crate::knowledge_living::{
'''
import_replacement = '''use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::Value;

use crate::flow::{
    explicit_flow_contemplate, flow_contemplate_preflight, FlowContemplateExecutor,
    FlowContemplateGenerated, FlowContemplateOutcome, FlowContemplatePreflight,
    FlowContemplateRequest,
};
use crate::knowledge_living::{
'''
if source.count(import_anchor) != 1:
    raise SystemExit('shape import anchor drifted')
source = source.replace(import_anchor, import_replacement, 1)

const_anchor = 'pub const WIKI_QL_SHAPE_VERSION: &str = "aikit.wiki-ql-shape/v1";\n'
if source.count(const_anchor) != 1:
    raise SystemExit('shape version anchor drifted')
source = source.replace(
    const_anchor,
    const_anchor + 'pub const WIKI_QL_SHAPED_FLOW_VERSION: &str = "aikit.wiki-ql-shaped-flow/v1";\n',
    1,
)

adapter_anchor = '''struct QlShapedExecutorAdapter<'a> {
'''
flow_shape_code = r'''/// One composition of the accepted Flow and QL-shaped Living Knowledge preflights.
/// It does not introduce another Flow store, Wiki, parser or contemplation aperture.
#[derive(Debug, Clone, PartialEq)]
pub struct QlShapedFlowContemplatePreflight {
    pub version: String,
    pub flow: FlowContemplatePreflight,
    pub shaped: QlShapedContemplatePreflight,
    /// Both constituent preflights are deterministic and remain outside the Agent/model aperture.
    pub automatic_agent_or_model_invocation: bool,
}

pub trait QlShapedFlowContemplateExecutor {
    /// Explicit execution still crosses the existing `Contemplate(FlowRef)` aperture exactly once.
    fn execute(
        &mut self,
        preflight: &QlShapedFlowContemplatePreflight,
    ) -> Result<FlowContemplateGenerated>;
}

pub fn ql_shaped_flow_contemplate_preflight(
    request: &FlowContemplateRequest<'_>,
    shape_budget: usize,
) -> Result<QlShapedFlowContemplatePreflight> {
    let flow = flow_contemplate_preflight(request)?;
    let shaped = ql_shaped_contemplate_preflight(
        request.living,
        request.resource_dependencies,
        request.object_budget,
        request.relation_depth,
        shape_budget,
    )?;
    if flow.bounded != shaped.base {
        return Err(AikitError::new(
            "knowledge.wiki_ql_shaped_flow_preflight_drift",
            "Flow and QL-shaped Contemplate must disclose the same bounded Living Knowledge field",
        ));
    }
    Ok(QlShapedFlowContemplatePreflight {
        version: WIKI_QL_SHAPED_FLOW_VERSION.into(),
        flow,
        shaped,
        automatic_agent_or_model_invocation: false,
    })
}

struct QlShapedFlowExecutorAdapter<'a> {
    preflight: &'a QlShapedFlowContemplatePreflight,
    executor: &'a mut dyn QlShapedFlowContemplateExecutor,
}

impl FlowContemplateExecutor for QlShapedFlowExecutorAdapter<'_> {
    fn execute(
        &mut self,
        flow: &FlowContemplatePreflight,
    ) -> Result<FlowContemplateGenerated> {
        if flow != &self.preflight.flow {
            return Err(AikitError::new(
                "knowledge.wiki_ql_shaped_flow_preflight_drift",
                "Flow preflight changed between QL-shape assembly and explicit execution",
            ));
        }
        self.executor.execute(self.preflight)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QlShapedFlowContemplateOutcome {
    pub preflight: QlShapedFlowContemplatePreflight,
    pub flow: FlowContemplateOutcome,
}

pub fn explicit_ql_shaped_flow_contemplate(
    request: &FlowContemplateRequest<'_>,
    shape_budget: usize,
    executor: &mut dyn QlShapedFlowContemplateExecutor,
) -> Result<QlShapedFlowContemplateOutcome> {
    let preflight = ql_shaped_flow_contemplate_preflight(request, shape_budget)?;
    let mut adapter = QlShapedFlowExecutorAdapter {
        preflight: &preflight,
        executor,
    };
    let flow = explicit_flow_contemplate(request, &mut adapter)?;
    Ok(QlShapedFlowContemplateOutcome { preflight, flow })
}

'''
if source.count(adapter_anchor) != 1:
    raise SystemExit('shape executor adapter anchor drifted')
source = source.replace(adapter_anchor, flow_shape_code + adapter_anchor, 1)
shape.write_text(source)

lib = Path('crates/aikit-core/src/lib.rs')
source = lib.read_text()
old = '''    explicit_ql_shaped_contemplate, explicit_ql_shaped_resolve_contemplate,
    ql_shaped_contemplate_preflight, ql_shaped_resolve_contemplate_preflight,
    wiki_constellation_grain, wiki_ql_shape_fields, QlOperativeContext,
    QlRelationalGenerationAttribution, QlShapedContemplateExecutor, QlShapedContemplateOutcome,
    QlShapedContemplatePreflight, WikiQlConstellationGrain, WikiQlCoordinate, WikiQlGenerationSite,
'''
new = '''    explicit_ql_shaped_contemplate, explicit_ql_shaped_flow_contemplate,
    explicit_ql_shaped_resolve_contemplate, ql_shaped_contemplate_preflight,
    ql_shaped_flow_contemplate_preflight, ql_shaped_resolve_contemplate_preflight,
    wiki_constellation_grain, wiki_ql_shape_fields, QlOperativeContext,
    QlRelationalGenerationAttribution, QlShapedContemplateExecutor, QlShapedContemplateOutcome,
    QlShapedContemplatePreflight, QlShapedFlowContemplateExecutor,
    QlShapedFlowContemplateOutcome, QlShapedFlowContemplatePreflight,
    WikiQlConstellationGrain, WikiQlCoordinate, WikiQlGenerationSite,
'''
if source.count(old) != 1:
    raise SystemExit('lib shape export anchor drifted')
source = source.replace(old, new, 1)
old = '''    WIKI_QL_SHAPE_VERSION,
};
'''
new = '''    WIKI_QL_SHAPED_FLOW_VERSION, WIKI_QL_SHAPE_VERSION,
};
'''
if source.count(old) != 1:
    raise SystemExit('lib shape version export anchor drifted')
source = source.replace(old, new, 1)
lib.write_text(source)

flow = Path('crates/aikit-core/src/flow.rs')
source = flow.read_text()
old = '''    use crate::knowledge_wiki::WikiObject;
'''
new = '''    use crate::knowledge_wiki::{
        WikiConstellation, WikiConstellationMember, WikiConstellationReturn, WikiFrame, WikiObject,
        WikiReading, OKF_WIKI_PROFILE,
    };
    use crate::knowledge_wiki_shape::{
        attribute_ql_relational_generation, explicit_ql_shaped_flow_contemplate,
        ql_shaped_flow_contemplate_preflight, QlRelationalGenerationAttribution,
        QlShapedFlowContemplateExecutor, QlShapedFlowContemplatePreflight, WikiQlShapeKind,
        DEFAULT_QL_SHAPE_BUDGET, QL_RELATIONAL_GENERATION_EXTENSION,
        QL_RELATIONAL_SIXFOLD_SHAPE_REF, QL_SHAPE_CONTRACT_REF,
    };
'''
if source.count(old) != 1:
    raise SystemExit('flow test Wiki import anchor drifted')
source = source.replace(old, new, 1)

insert_anchor = '''    #[test]
    fn flow_transport_keeps_living_validation_and_owner_mutation_intent_typed() {
'''
acceptance = r'''    fn etymological_shape_frame(flow_ref: &ResourceRef) -> WikiObject {
        let frame_ref = resource("wiki:frame:etymology");
        let anchor_ref = resource("wiki:anchor:etymology");
        let mut members = Vec::new();
        for position in 0_u8..6 {
            members.push(WikiConstellationMember {
                ref_id: if position == 0 {
                    flow_ref.clone()
                } else {
                    resource(&format!("wiki:etymology:direct-{position}"))
                },
                position: Some(position),
                conjugate: false,
                extensions: BTreeMap::new(),
            });
            members.push(WikiConstellationMember {
                ref_id: resource(&format!("wiki:etymology:conjugate-{position}")),
                position: Some(position),
                conjugate: true,
                extensions: BTreeMap::new(),
            });
        }
        WikiObject::Frame(WikiFrame {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: frame_ref,
            revision: 1,
            provenance: Vec::new(),
            inquiry_ref: Some(flow_ref.clone()),
            space_refs: Vec::new(),
            member_refs: members.iter().map(|member| member.ref_id.clone()).collect(),
            external_refs: Vec::new(),
            constellations: vec![WikiConstellation {
                anchor_ref: anchor_ref.clone(),
                members,
                returns: vec![WikiConstellationReturn {
                    through_anchor_ref: anchor_ref,
                    ground_ref: flow_ref.clone(),
                    extensions: BTreeMap::new(),
                }],
                conjugate_ref: None,
                extensions: BTreeMap::new(),
            }],
            extensions: BTreeMap::new(),
        })
    }

    #[derive(Default)]
    struct EtymologicalShapedFlowExecutor {
        calls: usize,
    }

    impl QlShapedFlowContemplateExecutor for EtymologicalShapedFlowExecutor {
        fn execute(
            &mut self,
            preflight: &QlShapedFlowContemplatePreflight,
        ) -> Result<FlowContemplateGenerated> {
            self.calls += 1;
            assert!(!preflight.automatic_agent_or_model_invocation);
            assert_eq!(preflight.flow.bounded, preflight.shaped.base);
            let flow_ref = preflight.flow.standing.binding.flow_ref.clone();
            let relational = preflight
                .shaped
                .shapes
                .iter()
                .find(|shape| {
                    shape.kind == WikiQlShapeKind::RelationalSixfold
                        && shape.shape_ref.as_deref() == Some(QL_RELATIONAL_SIXFOLD_SHAPE_REF)
                })
                .expect("etymological Flow frame exposes relational sixfold");
            let site = relational
                .generation_sites
                .iter()
                .find(|site| site.direct_ref == flow_ref)
                .expect("Flow is an actual coordinate in the active relational shape");
            let generated_ref = resource("wiki:reading:criterion-through-distinction");
            let frame_ref = resource("wiki:frame:etymology");
            let reading = WikiReading {
                profile: OKF_WIKI_PROFILE.into(),
                ref_id: generated_ref.clone(),
                revision: 1,
                provenance: vec![crate::living_wiki_provenance(
                    preflight.flow.standing.binding.source_ref.clone(),
                    preflight.flow.standing.binding.flow_revision.clone(),
                )],
                frame_ref: frame_ref.clone(),
                reading_type: "integrative/relational-v1".into(),
                artifact_ref: None,
                derived_by_ref: Some(resource("agent:test")),
                extensions: BTreeMap::new(),
            };
            let integrated = crate::build_integrative_reading(
                reading,
                vec![
                    crate::ReadingBasisNode {
                        resource: flow_ref.clone(),
                        source: Some(preflight.flow.standing.binding.source_ref.clone()),
                        source_revision: Some(
                            preflight.flow.standing.binding.flow_revision.clone(),
                        ),
                        roles: vec!["flow-leaf".into(), "relational-basis".into()],
                    },
                    crate::ReadingBasisNode {
                        resource: site.conjugate_ref.clone(),
                        source: None,
                        source_revision: None,
                        roles: vec!["materially-present-conjugate".into()],
                    },
                ],
                Vec::new(),
                vec![crate::ReadingReturnPath {
                    from_basis: flow_ref.clone(),
                    through: vec![relational.anchor_ref.clone()],
                    to_whole: generated_ref.clone(),
                }],
                crate::KnowledgeFreshness::Fresh,
            )?;
            let attributed = attribute_ql_relational_generation(
                integrated,
                QlRelationalGenerationAttribution {
                    contract_ref: QL_SHAPE_CONTRACT_REF.into(),
                    source_shape_ref: relational
                        .shape_ref
                        .clone()
                        .expect("relational shape has stable ref"),
                    operator_ref: site.operator_ref.clone(),
                    frame_ref,
                    basis_refs: vec![flow_ref, site.conjugate_ref.clone()],
                    generation_positions: vec![site.position],
                    generated_ref,
                    return_anchor_ref: relational.return_anchor_ref.clone(),
                },
            )?;
            Ok(FlowContemplateGenerated {
                living: ContemplateGenerated {
                    wiki_upserts: vec![WikiObject::Reading(attributed.reading.clone())],
                    integrative_readings: vec![attributed],
                    candidates: Vec::new(),
                    tensions: Vec::new(),
                    human_source_proposals: Vec::new(),
                },
                flow_mutations: Vec::new(),
            })
        }
    }

    #[test]
    fn etymological_flow_enters_shape_generates_whole_and_returns_to_exact_source_basis() {
        let context = context();
        let mut provider = MemoryFlowProvider::central_style("notes/criterion-thread.md");
        provider.body = "criterion / distinction; retain the difference and ask what their relation determines".into();
        let session = resource("agent-session/etymology");
        let standing = bind_flow_for_act(
            &provider,
            &context,
            &provider.flow.flow_ref,
            session.clone(),
            Some(resource("agent:test")),
            Some(resource("agency:test")),
        )
        .unwrap();
        let horizon = horizon(&provider.flow);
        let method = first_party_flow_method(Some(revision("method-r1"))).unwrap();
        let praxis = praxis(&context, &method);
        let runtime = runtime(session.as_str());
        let frame_ref = resource("wiki:frame:etymology");
        let objects = vec![etymological_shape_frame(&provider.flow.flow_ref)];
        let dependencies = Vec::<KnowledgeDependency>::new();
        let living = ContemplateRequest {
            project: ProjectRef::parse("project:test").unwrap(),
            focus: vec![provider.flow.flow_ref.clone(), frame_ref.clone()],
            horizon: &horizon,
            dependencies: &dependencies,
            current_wiki_objects: &objects,
            runtime: &runtime,
            method: Some(&method),
            ql: None,
        };
        let authority_refs = vec![FlowAuthorityRef {
            authority: FlowContextAuthority::Flow,
            reference: standing.binding.flow_ref.clone(),
        }];
        let request = FlowContemplateRequest::with_defaults(
            &standing,
            &living,
            &[],
            &praxis,
            &authority_refs,
        );

        let preflight =
            ql_shaped_flow_contemplate_preflight(&request, DEFAULT_QL_SHAPE_BUDGET).unwrap();
        assert!(!preflight.automatic_agent_or_model_invocation);
        assert!(!preflight.flow.automatic_agent_or_model_invocation);
        assert!(!preflight.shaped.automatic_agent_or_model_invocation);
        assert_eq!(preflight.flow.standing.binding.flow_ref, provider.flow.flow_ref);
        assert_eq!(
            preflight.flow.standing.binding.flow_revision,
            provider.flow.revision
        );
        assert!(preflight.shaped.shapes.iter().any(|shape| {
            shape.kind == WikiQlShapeKind::RelationalSixfold
                && shape
                    .generation_sites
                    .iter()
                    .any(|site| site.direct_ref == provider.flow.flow_ref)
        }));
        assert!(living.ql.is_none(), "portable shape operation needs no live QL-MEF provider");

        let original_body = provider.body.clone();
        let mut executor = EtymologicalShapedFlowExecutor::default();
        let outcome = explicit_ql_shaped_flow_contemplate(
            &request,
            DEFAULT_QL_SHAPE_BUDGET,
            &mut executor,
        )
        .unwrap();
        assert_eq!(executor.calls, 1);
        assert!(outcome.flow.flow_mutations.is_empty());
        assert_eq!(provider.body, original_body, "generated Agent knowledge does not rewrite authored Flow source");
        assert!(outcome.flow.living.agent_wiki.human_source_proposals.is_empty());
        assert_eq!(outcome.flow.living.integrative_readings.len(), 1);

        let generated = &outcome.flow.living.integrative_readings[0];
        let flow_basis = generated
            .basis
            .iter()
            .find(|basis| basis.resource == provider.flow.flow_ref)
            .expect("generated whole retains Flow basis");
        assert_eq!(flow_basis.source.as_ref(), Some(&provider.flow.source_ref));
        assert_eq!(flow_basis.source_revision.as_ref(), Some(&provider.flow.revision));
        assert!(generated.return_paths.iter().any(|path| {
            path.from_basis == provider.flow.flow_ref
                && path.to_whole == generated.reading.ref_id
                && !path.through.is_empty()
        }));
        let value = generated
            .reading
            .extensions
            .get(QL_RELATIONAL_GENERATION_EXTENSION)
            .expect("generated reading retains QL relation attribution")
            .clone();
        let attribution: QlRelationalGenerationAttribution = serde_json::from_value(value).unwrap();
        assert_eq!(attribution.frame_ref, frame_ref);
        assert!(attribution.basis_refs.contains(&provider.flow.flow_ref));
        assert_eq!(attribution.generated_ref, generated.reading.ref_id);

        let later_ref = resource("wiki:reading:criterion-history");
        let later = crate::build_integrative_reading(
            WikiReading {
                profile: OKF_WIKI_PROFILE.into(),
                ref_id: later_ref.clone(),
                revision: 1,
                provenance: Vec::new(),
                frame_ref: resource("wiki:frame:later-etymology"),
                reading_type: "integrative/recursive-v1".into(),
                artifact_ref: None,
                derived_by_ref: Some(resource("agent:test")),
                extensions: BTreeMap::new(),
            },
            vec![crate::ReadingBasisNode {
                resource: generated.reading.ref_id.clone(),
                source: None,
                source_revision: None,
                roles: vec!["prior-generated-whole".into()],
            }],
            Vec::new(),
            vec![crate::ReadingReturnPath {
                from_basis: generated.reading.ref_id.clone(),
                through: Vec::new(),
                to_whole: later_ref,
            }],
            crate::KnowledgeFreshness::Fresh,
        )
        .unwrap();
        assert_eq!(later.basis[0].resource, generated.reading.ref_id);
        assert!(generated
            .reading
            .extensions
            .contains_key(QL_RELATIONAL_GENERATION_EXTENSION));
    }

'''
if source.count(insert_anchor) != 1:
    raise SystemExit('flow acceptance insertion anchor drifted')
source = source.replace(insert_anchor, acceptance + insert_anchor, 1)
flow.write_text(source)
