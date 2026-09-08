//! Explicit source loading into the existing Method/Praxis/Context contracts.
//! No registry, capability activation, human standing or authority is created.
use crate::app::Service;
use aikit_core::method::{Method, METHOD_VERSION};
use aikit_core::resource::{ResourceDescriptor, ResourceIndex, ResourceKind, ResourceRecord};
use aikit_core::session_space_application::ContextResolutionEvidence;
use aikit_core::{AikitError, ResourceRef, Result, SourceRef, SourceRevision};
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

pub fn resolve_source(service: &Service, path: &Path, focus: &[ResourceRef]) -> Result<Value> {
    let path = fs::canonicalize(path)
        .map_err(|error| AikitError::new("method.source_unavailable", error.to_string()))?;
    let mut file = fs::File::open(&path)
        .map_err(|error| AikitError::new("method.source_unavailable", error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| AikitError::new("method.source_unavailable", error.to_string()))?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(AikitError::new(
            "method.source_invalid",
            "Method source must be a regular JSON file no larger than 1 MiB",
        ));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AikitError::new("method.source_unavailable", error.to_string()))?;
    if bytes.len() > 1024 * 1024 {
        return Err(AikitError::new(
            "method.source_invalid",
            "Method source grew beyond its read bound",
        ));
    }
    let mut method: Method = serde_json::from_slice(&bytes)
        .map_err(|error| AikitError::new("method.source_invalid", error.to_string()))?;
    ResourceRef::parse(method.id.as_str())?;
    SourceRef::parse(method.source.as_str())?;
    method.validate()?;
    let declared_revision = method.revision.clone();
    if let Some(revision) = &declared_revision {
        SourceRevision::parse(revision.as_str())?;
    }
    // The actual byte read is the source loader's exact revision. Preserve the
    // authored revision separately; never rewrite the source declaration.
    let digest = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    method.revision = Some(SourceRevision::parse(&digest)?);
    let mut resources = aikit_tui::project_world_service::resource_index(service)?;
    let context = aikit_tui::project_world_service::context_resolution_from_resources(
        service, aikit_core::RequestedActors::default(), &resources,
    )?;
    if service.descriptor().project_root.is_none() {
        return Err(AikitError::new(
            "method.no_project",
            "Situated Method resolution requires an actual Project context",
        ));
    }
    // The shallow compatibility navigation may contain only a legacy Project
    // alias. Retain the canonical Project supplied by the native context owner.
    let project = ResourceRef::parse(context.project_binding.project.as_str())?;
    if resources.resource(&project).is_none() {
        resources.insert_resource(
            ResourceRecord::new(ResourceDescriptor::new(
                project.clone(),
                ResourceKind::Project,
                context.project_binding.project.to_string(),
                "Current Project from the native ContextResolution binding",
            )),
            vec![],
        );
    }
    let situated_focus = if focus.is_empty() {
        vec![project]
    } else {
        focus.to_vec()
    };
    for reference in &situated_focus {
        if resources.resource(reference).is_none() {
            return Err(AikitError::new(
                "method.focus_unresolved",
                format!("Situated Focus {reference} is absent from the current resource field"),
            ));
        }
    }
    let praxis = aikit_core::resolve_praxis(
        &context,
        &resources,
        std::slice::from_ref(&method),
        std::slice::from_ref(&method.id),
        &situated_focus,
    );
    if !praxis.warnings.is_empty() || praxis.methods.len() != 1 {
        return Err(AikitError::new(
            "method.unresolved",
            praxis.warnings.join("; "),
        ));
    }
    let context_evidence = ContextResolutionEvidence::from_resolution(&context)?;
    let skill_states: Vec<Value> = method
        .skills
        .iter()
        .map(|member| {
            let active = member
                .skill
                .as_str()
                .parse::<aikit_core::CapsuleId>()
                .ok()
                .is_some_and(|id| service.resolved().is_active(&id));
            json!({"skill":member.skill,"active":active})
        })
        .collect();
    Ok(json!({
        "method_contract":METHOD_VERSION,
        "source_read":{"path":path,"source":method.source,"declared_revision":declared_revision,"revision":digest,"standing":"observed-source-bytes"},
        "method":method,"praxis":praxis,"context_resolution":context_evidence,
        "skill_states":skill_states,
        "authority":"resolution-only-no-activation-or-authority-granted"
    }))
}
