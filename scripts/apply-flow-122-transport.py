from pathlib import Path


def replace(path: str, old: str, new: str, count: int = 1):
    p = Path(path)
    text = p.read_text()
    actual = text.count(old)
    if actual < count:
        raise SystemExit(f"anchor missing in {path}: expected {count}, got {actual}: {old[:120]!r}")
    p.write_text(text.replace(old, new, count))

flow = "crates/aikit-core/src/flow.rs"

replace(
    flow,
    'use serde::{Deserialize, Serialize};\n',
    'use serde::{Deserialize, Serialize};\nuse serde_json::Value;\n',
)
replace(
    flow,
    '''use crate::knowledge_living::{
    explicit_contemplate, ContemplateExecutor, ContemplateGenerated, ContemplateOutcome,
    ContemplatePreflight, ContemplateRequest, KnowledgeDependency,
};''',
    '''use crate::knowledge_living::{
    explicit_contemplate, ContemplateExecutor, ContemplateGenerated, ContemplateOutcome,
    ContemplatePreflight, ContemplateRequest, KnowledgeDependency,
};
use crate::knowledge_living_transport::parse_contemplate_generated;''',
)
replace(
    flow,
    'pub const FLOW_CONTEMPLATE_VERSION: &str = "aikit.flow-contemplate/v1";\n',
    'pub const FLOW_CONTEMPLATE_VERSION: &str = "aikit.flow-contemplate/v1";\npub const FLOW_CONTEMPLATE_RETURN_VERSION: &str = "aikit.flow-contemplate-return/v1";\n',
)

anchor = '''#[derive(Debug, Clone, PartialEq)]
pub struct FlowContemplateGenerated {
    pub living: ContemplateGenerated,
    pub flow_mutations: Vec<FlowMutationIntent>,
}
'''
addition = anchor + r'''

#[derive(Debug, Deserialize)]
struct FlowContemplateReturnEnvelope {
    version: String,
    living: Value,
    #[serde(default)]
    flow_mutations: Vec<FlowMutationIntent>,
}

/// Parse one transport response for explicit `Contemplate(FlowRef)` into the
/// native AIKit return. The nested `living` object is validated by the already-
/// accepted Living Knowledge transport parser; Flow mutation intents remain
/// typed, owner-directed expected-revision requests.
pub fn parse_flow_contemplate_generated(input: &str) -> Result<FlowContemplateGenerated> {
    let envelope: FlowContemplateReturnEnvelope = serde_json::from_str(input).map_err(|error| {
        AikitError::new(
            "flow.contemplate_return_invalid_json",
            format!("Flow Contemplate return must be structured JSON: {error}"),
        )
    })?;
    if envelope.version != FLOW_CONTEMPLATE_RETURN_VERSION {
        return Err(AikitError::new(
            "flow.contemplate_return_version_unsupported",
            format!(
                "Flow Contemplate return version `{}` is not `{FLOW_CONTEMPLATE_RETURN_VERSION}`",
                envelope.version
            ),
        ));
    }
    let living = serde_json::to_string(&envelope.living).map_err(|error| {
        AikitError::new(
            "flow.contemplate_return_living_unserializable",
            format!("could not recover nested Living Knowledge return: {error}"),
        )
    })?;
    Ok(FlowContemplateGenerated {
        living: parse_contemplate_generated(&living)?,
        flow_mutations: envelope.flow_mutations,
    })
}
'''
replace(flow, anchor, addition)

# Public export is part of the owner contract consumed by O:I/ACP/A2A hosts.
lib = "crates/aikit-core/src/lib.rs"
replace(
    lib,
    '''    first_party_flow_guidance, first_party_flow_method, first_party_flow_resource_records,
    flow_contemplate_preflight, flow_knowledge_dependency, FlowAuthorityRef, FlowBinding,''',
    '''    first_party_flow_guidance, first_party_flow_method, first_party_flow_resource_records,
    flow_contemplate_preflight, flow_knowledge_dependency, parse_flow_contemplate_generated,
    FlowAuthorityRef, FlowBinding,''')
replace(
    lib,
    '''    FLOW_CONTEMPLATE_ACTION_REF, FLOW_CONTEMPLATE_VERSION, FLOW_GUIDANCE_CAPSULE,
''',
    '''    FLOW_CONTEMPLATE_ACTION_REF, FLOW_CONTEMPLATE_RETURN_VERSION, FLOW_CONTEMPLATE_VERSION,
    FLOW_GUIDANCE_CAPSULE,
''')

# Transport acceptance belongs beside Flow's deliberate invocation tests.
test_anchor = '''    #[test]
    fn first_party_flow_praxis_uses_guidance_skill_method_and_explainable_resolution() {'''
test = r'''    #[test]
    fn flow_transport_keeps_living_validation_and_owner_mutation_intent_typed() {
        let parsed = parse_flow_contemplate_generated(
            r#"{
              "version":"aikit.flow-contemplate-return/v1",
              "living":{
                "version":"aikit.contemplate-return/v1",
                "wiki_upserts":[],
                "integrative_readings":[],
                "candidates":["candidate:flow"],
                "tensions":[],
                "human_source_proposals":[]
              },
              "flow_mutations":[{
                "version":"aikit.flow-context/v1",
                "flow_ref":"flow:notes-house:thread-1",
                "expected_revision":"notes-house-r7",
                "replacement":"refined source",
                "actor":"agent:test",
                "agency":"agency:test",
                "agent_session":"agent-session/test",
                "context_resolution_version":"aikit.context-resolution/v2",
                "method":"method:contemplate-flow",
                "invocation_ref":"flow-contemplate/abc"
              }]
            }"#,
        )
        .unwrap();
        assert_eq!(parsed.living.candidates, vec!["candidate:flow"]);
        assert_eq!(parsed.flow_mutations.len(), 1);
        assert_eq!(parsed.flow_mutations[0].flow_ref.as_str(), "flow:notes-house:thread-1");
        assert_eq!(parsed.flow_mutations[0].expected_revision.as_str(), "notes-house-r7");

        let prose = parse_flow_contemplate_generated("please edit the Flow").unwrap_err();
        assert_eq!(prose.code(), "flow.contemplate_return_invalid_json");
        let bad_nested = parse_flow_contemplate_generated(
            r#"{"version":"aikit.flow-contemplate-return/v1","living":{"version":"wrong"}}"#,
        )
        .unwrap_err();
        assert_eq!(bad_nested.code(), "knowledge.contemplate_return_version_unsupported");
    }

'''
replace(flow, test_anchor, test + test_anchor)

Path("scripts/apply-flow-122-transport.py").unlink()
Path(".github/workflows/flow-122-transport.yml").unlink()
