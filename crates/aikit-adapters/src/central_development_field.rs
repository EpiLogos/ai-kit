//! Central S1 Development Field projection into AIKit's existing Resource field.
//!
//! Central owns source identity, standing, tier/UX/EX relations and the public
//! read contract. This adapter invokes that native Action and projects only what
//! Central explicitly returned. It does not parse `ProjectCentral/self/**` as a
//! semantic API and does not infer relations from location or filenames.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use aikit_core::resource::{
    DevelopmentFieldBinding, DevelopmentFieldCarrierKind, DevelopmentFieldCarrierProjection,
    DevelopmentFieldRelation, OwnerRef, ResourceDescriptor, ResourceKind, ResourceLocator,
    ResourceRecord, ResourceRef, ResourceSource, SourceAuthority, SourceRef, SourceRevision,
    SourceState,
};
use aikit_core::{AikitError, Result};
use serde_json::{json, Value};

use crate::runner::CommandRunner;

pub const CENTRAL_DEVELOPMENT_FIELD_READING: &str = "central.development-field-reading/v1";
pub const PROJECT_SELF_INSPECT_ACTION: &str = "projectcentral.self.inspect";

/// Read Central's accepted S1 Project self-description contract and project it
/// into ordinary AIKit Resource records. A Project outside `Central/Work` has no
/// implicit Central binding and therefore contributes no records.
pub fn project_development_field_resources<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project_root: &Path,
) -> Result<Vec<ResourceRecord>> {
    let Some(member) = project_member(central_root, project_root) else {
        return Ok(Vec::new());
    };
    let reading = central_action(
        runner,
        central_root,
        PROJECT_SELF_INSPECT_ACTION,
        json!({ "project": member }),
    )?;
    project_reading_resources(project_root, &reading)
}

fn project_reading_resources(project_root: &Path, reading: &Value) -> Result<Vec<ResourceRecord>> {
    let schema = required_str(reading, "schema")?;
    if schema != CENTRAL_DEVELOPMENT_FIELD_READING {
        return Err(AikitError::new(
            "central_development_field.schema_unsupported",
            format!(
                "Central Development Field reading uses {schema}; expected {CENTRAL_DEVELOPMENT_FIELD_READING}"
            ),
        ));
    }
    let scope_ref = required_str(reading, "scope_ref")?;
    let owner = OwnerRef::parse(format!("central:{scope_ref}"))?;

    let mut sources = BTreeMap::<String, ResourceSource>::new();
    let mut source_records = BTreeMap::<String, ResourceRecord>::new();

    if let Some(aperture) = reading.get("self_aperture") {
        for source in array(aperture, "linked_sources") {
            register_resolved_source(project_root, &owner, source, false, &mut sources, &mut source_records)?;
        }
        for source in array(aperture, "unbound_sources") {
            register_resolved_source(project_root, &owner, source, true, &mut sources, &mut source_records)?;
        }
    }
    for tier in array(reading, "tier_bindings") {
        for source in array(tier, "sources") {
            if !source.is_null() {
                register_resolved_source(project_root, &owner, source, false, &mut sources, &mut source_records)?;
            }
        }
    }
    for ux in array(reading, "ux") {
        if let Some(source) = ux.get("source").filter(|value| !value.is_null()) {
            register_resolved_source(project_root, &owner, source, false, &mut sources, &mut source_records)?;
        }
    }
    for ex in array(reading, "ex") {
        if let Some(source) = ex.get("source").filter(|value| !value.is_null()) {
            register_resolved_source(project_root, &owner, source, false, &mut sources, &mut source_records)?;
        }
    }

    let mut records = source_records.into_values().collect::<Vec<_>>();

    if let Some(aperture) = reading.get("self_aperture") {
        let exists = aperture.get("exists").and_then(Value::as_bool).unwrap_or(false);
        let status = aperture.get("status").and_then(Value::as_str).unwrap_or("unknown");
        if exists && status == "present" {
            let mut binding = DevelopmentFieldBinding::new(DevelopmentFieldCarrierKind::SelfDescription);
            let linked = array(aperture, "linked_sources");
            binding.relations = source_relations(&linked, "central:self-source")?;
            let mut descriptor = carrier_descriptor(
                &format!("central:self:{scope_ref}"),
                &owner,
                "Central Project self-description",
                "Central-owned Project self-description aperture",
            )?;
            descriptor.sources = resolved_sources(&linked, &sources);
            annotate(&mut descriptor, "central.schema", schema);
            annotate(&mut descriptor, "central.scope_ref", scope_ref);
            annotate(&mut descriptor, "central.self.path", value_str(aperture, "path"));
            annotate(&mut descriptor, "central.self.status", status);
            annotate_json(&mut descriptor, "central.self.issues", aperture.get("issues"));
            records.push(
                DevelopmentFieldCarrierProjection { descriptor, binding }.into_record()?,
            );
        }
    }

    for tier in array(reading, "tier_bindings") {
        let Some(number) = tier.get("tier").and_then(Value::as_u64) else {
            return Err(invalid("Central tier binding lacks integer tier"));
        };
        let source_values = array(tier, "sources")
            .into_iter()
            .filter(|value| !value.is_null())
            .collect::<Vec<_>>();
        let mut binding = DevelopmentFieldBinding::new(DevelopmentFieldCarrierKind::TierBinding);
        binding.relations = source_relations(&source_values, "central:tier-source")?;
        let mut descriptor = carrier_descriptor(
            &format!("central:tier:{scope_ref}:{number}"),
            &owner,
            format!("Central Development Field tier {number}"),
            "Central-owned document-stage tier binding",
        )?;
        descriptor.sources = resolved_sources(&source_values, &sources);
        annotate(&mut descriptor, "central.schema", schema);
        annotate(&mut descriptor, "central.scope_ref", scope_ref);
        annotate(&mut descriptor, "central.tier", number.to_string());
        annotate(&mut descriptor, "central.semantic_office", value_str(tier, "semantic_office"));
        if let Some(value) = tier.get("canonical_label").and_then(Value::as_str) {
            annotate(&mut descriptor, "central.canonical_label", value);
        }
        if let Some(value) = tier.get("canonical_path").and_then(Value::as_str) {
            annotate(&mut descriptor, "central.canonical_path", value);
        }
        records.push(DevelopmentFieldCarrierProjection { descriptor, binding }.into_record()?);
    }

    let mut ux_ids = BTreeMap::<String, ResourceRef>::new();
    for ux in array(reading, "ux") {
        let ux_ref = required_str(ux, "ux_ref")?;
        let id = ResourceRef::parse(format!("central:ux:{scope_ref}:{ux_ref}"))?;
        ux_ids.insert(ux_ref.to_owned(), id.clone());
        let source_values = ux
            .get("source")
            .filter(|value| !value.is_null())
            .into_iter()
            .collect::<Vec<_>>();
        let mut binding = DevelopmentFieldBinding::new(DevelopmentFieldCarrierKind::UserExperience);
        binding.relations = source_relations(&source_values, "central:ux-source")?;
        let mut descriptor = ResourceDescriptor::new(
            id,
            ResourceKind::ContextSource,
            ux_ref,
            "Central-owned intended-experience source relation",
        );
        descriptor.owner = Some(owner.clone());
        descriptor.sources = resolved_sources(&source_values, &sources);
        annotate(&mut descriptor, "central.schema", schema);
        annotate(&mut descriptor, "central.scope_ref", scope_ref);
        annotate(&mut descriptor, "central.ux_ref", ux_ref);
        annotate_json(&mut descriptor, "central.ux.flags", Some(ux));
        records.push(DevelopmentFieldCarrierProjection { descriptor, binding }.into_record()?);
    }

    for ex in array(reading, "ex") {
        let ex_ref = required_str(ex, "ex_ref")?;
        let source_values = ex
            .get("source")
            .filter(|value| !value.is_null())
            .into_iter()
            .collect::<Vec<_>>();
        let mut binding = DevelopmentFieldBinding::new(DevelopmentFieldCarrierKind::ExperienceMetadata);
        binding.relations = source_relations(&source_values, "central:ex-source")?;
        for ux_ref in string_array(ex, "ux_refs") {
            if let Some(target) = ux_ids.get(&ux_ref) {
                binding.relations.push(DevelopmentFieldRelation {
                    relation: "central:ex-ux".into(),
                    target: target.clone(),
                });
            }
        }
        let mut descriptor = carrier_descriptor(
            &format!("central:ex:{scope_ref}:{ex_ref}"),
            &owner,
            ex_ref,
            "Central-owned human experiential-return metadata",
        )?;
        descriptor.sources = resolved_sources(&source_values, &sources);
        annotate(&mut descriptor, "central.schema", schema);
        annotate(&mut descriptor, "central.scope_ref", scope_ref);
        annotate(&mut descriptor, "central.ex_ref", ex_ref);
        annotate_json(&mut descriptor, "central.ex.metadata", Some(ex));
        records.push(DevelopmentFieldCarrierProjection { descriptor, binding }.into_record()?);
    }

    records.sort_by(|left, right| left.descriptor.id.cmp(&right.descriptor.id));
    Ok(records)
}

fn carrier_descriptor(
    id: &str,
    owner: &OwnerRef,
    name: impl Into<String>,
    description: impl Into<String>,
) -> Result<ResourceDescriptor> {
    let mut descriptor = ResourceDescriptor::new(
        ResourceRef::parse(id)?,
        ResourceKind::ContextSource,
        name,
        description,
    );
    descriptor.owner = Some(owner.clone());
    Ok(descriptor)
}

fn register_resolved_source(
    project_root: &Path,
    owner: &OwnerRef,
    source: &Value,
    unbound_self_source: bool,
    sources: &mut BTreeMap<String, ResourceSource>,
    records: &mut BTreeMap<String, ResourceRecord>,
) -> Result<()> {
    let source_ref = source
        .get("ref")
        .or_else(|| source.get("source_ref"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("Central source reading lacks ref/source_ref"))?;
    if sources.contains_key(source_ref) {
        return Ok(());
    }
    let path = required_str(source, "path")?;
    let revision = source
        .pointer("/revision/revision")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("Central source reading lacks revision.revision"))?;
    let observed = ResourceSource {
        source: SourceRef::parse(source_ref)?,
        authority: Some(SourceAuthority::Observed),
        revision: Some(SourceRevision::parse(revision)?),
        locator: Some(ResourceLocator::Path(project_root.join(path))),
        state: SourceState::Available,
    };
    let id = ResourceRef::parse(source_ref)?;
    let mut descriptor = ResourceDescriptor::new(
        id,
        ResourceKind::ContextSource,
        path,
        "Central-owned Development Field source observed through the S1 public read contract",
    );
    descriptor.owner = Some(owner.clone());
    descriptor.sources.push(observed.clone());
    annotate(&mut descriptor, "central.provenance", value_str(source, "provenance"));
    annotate(&mut descriptor, "central.standing", value_str(source, "standing"));
    if let Some(value) = source.get("treatment").and_then(Value::as_str) {
        annotate(&mut descriptor, "central.treatment", value);
    }
    if let Some(value) = source.pointer("/revision/byte_len").and_then(Value::as_u64) {
        annotate(&mut descriptor, "central.source_byte_len", value.to_string());
    }
    annotate_json(&mut descriptor, "central.roles", source.get("roles"));
    if unbound_self_source {
        annotate(&mut descriptor, "central.unbound_self_source", "true");
        annotate(&mut descriptor, "central.authority_from_location", "false");
    }
    records.insert(source_ref.to_owned(), ResourceRecord::new(descriptor));
    sources.insert(source_ref.to_owned(), observed);
    Ok(())
}

fn source_relations(values: &[&Value], relation: &str) -> Result<Vec<DevelopmentFieldRelation>> {
    values
        .iter()
        .map(|value| {
            let source_ref = value
                .get("ref")
                .or_else(|| value.get("source_ref"))
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("Central bound source lacks ref/source_ref"))?;
            Ok(DevelopmentFieldRelation {
                relation: relation.to_owned(),
                target: ResourceRef::parse(source_ref)?,
            })
        })
        .collect()
}

fn resolved_sources(
    values: &[&Value],
    sources: &BTreeMap<String, ResourceSource>,
) -> Vec<ResourceSource> {
    values
        .iter()
        .filter_map(|value| {
            value
                .get("ref")
                .or_else(|| value.get("source_ref"))
                .and_then(Value::as_str)
                .and_then(|source_ref| sources.get(source_ref))
                .cloned()
        })
        .collect()
}

fn central_action<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    id: &str,
    input: Value,
) -> Result<Value> {
    let argv = vec![
        "ctrl".to_owned(),
        "--json".to_owned(),
        "--root".to_owned(),
        central_root.display().to_string(),
        "action".to_owned(),
        "run".to_owned(),
        id.to_owned(),
        input.to_string(),
    ];
    let output = runner.run(&argv)?;
    if !output.ok() {
        return Err(AikitError::new(
            "central_development_field.action_failed",
            format!("Central Action {id} exited {}", output.status),
        )
        .with("action", id)
        .with("stderr", output.stderr.trim().to_owned()));
    }
    let envelope: Value = serde_json::from_str(output.stdout.trim()).map_err(|error| {
        AikitError::new(
            "central_development_field.action_invalid",
            format!("Central Action {id} returned invalid JSON: {error}"),
        )
    })?;
    if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = envelope
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Central Action failed");
        return Err(AikitError::new(
            "central_development_field.action_failed",
            message,
        )
        .with("action", id));
    }
    envelope.get("data").cloned().ok_or_else(|| {
        AikitError::new(
            "central_development_field.action_invalid",
            format!("Central Action {id} succeeded without data"),
        )
    })
}

fn project_member(central_root: &Path, project_root: &Path) -> Option<String> {
    let relative = project_root.strip_prefix(central_root.join("Work")).ok()?;
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn array<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|values| values.iter().collect())
        .unwrap_or_default()
}

fn string_array(value: &Value, key: &str) -> Vec<String> {
    array(value, key)
        .into_iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid(format!("Central Development Field reading lacks {key}")))
}

fn value_str<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn annotate(descriptor: &mut ResourceDescriptor, key: &str, value: impl Into<String>) {
    descriptor.annotations.insert(key.to_owned(), value.into());
}

fn annotate_json(descriptor: &mut ResourceDescriptor, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        descriptor.annotations.insert(key.to_owned(), value.to_string());
    }
}

fn invalid(message: impl Into<String>) -> AikitError {
    AikitError::new("central_development_field.reading_invalid", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::ScriptedRunner;
    use aikit_core::resource::{development_field_binding, DevelopmentFieldCarrierKind};

    fn source(reference: &str, path: &str, standing: &str) -> Value {
        json!({
            "ref": reference,
            "path": path,
            "roles": ["self-description-source"],
            "provenance": "human-authored",
            "standing": standing,
            "treatment": "retain-native-in-place",
            "revision": { "revision": "blake3:abc", "byte_len": 12 },
            "agent_retrieval_allowed": true,
            "retained_native": true
        })
    }

    fn reading() -> Value {
        let vision = source("central:source:vision", "VISION.md", "design-commitment");
        let return_source = source("central:source:return", "ProjectCentral/self/return.md", "observed-evidence");
        json!({
            "schema": CENTRAL_DEVELOPMENT_FIELD_READING,
            "scope_ref": "project:demo",
            "self_aperture": {
                "path": "ProjectCentral/self",
                "status": "present",
                "exists": true,
                "issues": [],
                "linked_sources": [vision.clone()],
                "unbound_sources": []
            },
            "tier_bindings": [{
                "tier": 1,
                "semantic_office": "intended experience / vision",
                "canonical_label": null,
                "canonical_path": null,
                "sources": [vision.clone()]
            }],
            "ux": [{
                "ux_ref": "ux:build",
                "source": vision,
                "intended_experience": true,
                "implementation_fact": false,
                "test_result": false,
                "agent_inference": false,
                "ex_human_return": false
            }],
            "ex": [{
                "ex_ref": "ex:returned",
                "ux_refs": ["ux:build"],
                "source": return_source,
                "artifact_refs": ["artifact:1"],
                "recorded_at_unix_seconds": 7,
                "human_experience_return": true
            }],
            "relation_source": "ProjectCentral/relations/development-field.json",
            "canonical_tier_labels_invented": false,
            "source_payloads_exposed": false,
            "automatic_agent_or_model_invocation": false
        })
    }

    #[test]
    fn projects_owner_native_self_tier_ux_and_ex_without_path_semantics() {
        let project = Path::new("/tmp/Central/Work/demo");
        let records = project_reading_resources(project, &reading()).unwrap();
        let by_id = records
            .iter()
            .map(|record| (record.descriptor.id.as_str(), record))
            .collect::<BTreeMap<_, _>>();
        assert!(by_id.contains_key("central:self:project:demo"));
        assert!(by_id.contains_key("central:tier:project:demo:1"));
        assert!(by_id.contains_key("central:ux:project:demo:ux:build"));
        assert!(by_id.contains_key("central:ex:project:demo:ex:returned"));
        assert!(by_id.contains_key("central:source:vision"));
        let ux = by_id["central:ux:project:demo:ux:build"];
        assert_eq!(ux.descriptor.owner.as_ref().unwrap().as_str(), "central:project:demo");
        let binding = development_field_binding(ux).unwrap().unwrap();
        assert_eq!(binding.carrier_kind, DevelopmentFieldCarrierKind::UserExperience);
        assert_eq!(binding.relations[0].target.as_str(), "central:source:vision");
        let ex = by_id["central:ex:project:demo:ex:returned"];
        let binding = development_field_binding(ex).unwrap().unwrap();
        assert!(binding.relations.iter().any(|relation| {
            relation.relation == "central:ex-ux"
                && relation.target.as_str() == "central:ux:project:demo:ux:build"
        }));
    }

    #[test]
    fn invokes_central_public_action_for_projects_under_work() {
        let output = json!({ "ok": true, "data": reading() }).to_string();
        let runner = ScriptedRunner::new().on(PROJECT_SELF_INSPECT_ACTION, &output);
        let central = Path::new("/tmp/Central");
        let project = Path::new("/tmp/Central/Work/demo");
        let records = project_development_field_resources(&runner, central, project).unwrap();
        assert!(records.iter().any(|record| {
            record.descriptor.id.as_str() == "central:self:project:demo"
        }));
    }

    #[test]
    fn absent_self_aperture_does_not_fabricate_a_self_description_carrier() {
        let mut value = reading();
        value["self_aperture"]["status"] = Value::String("legacy-migratable-absence".into());
        value["self_aperture"]["exists"] = Value::Bool(false);
        value["self_aperture"]["linked_sources"] = Value::Array(Vec::new());
        let records = project_reading_resources(Path::new("/tmp/Central/Work/demo"), &value).unwrap();
        assert!(!records.iter().any(|record| {
            record.descriptor.id.as_str() == "central:self:project:demo"
        }));
    }
}
