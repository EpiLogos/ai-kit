//! Operational binding of selected Methods to an already-resolved AIKit Context.
//!
//! ContextResolution remains the owner of what is available/operative. A Method
//! is selected around a Focus only after that resolution exists; selection never
//! grants trust, capability, Action authority, or SkillSet precedence.

use serde::{Deserialize, Serialize};

use crate::context_resolution::ContextResolution;
use crate::method::{resolve_method, Method, MethodResolution};
use crate::resource::{ResourceIndex, ResourceRef};

pub const PRAXIS_RESOLUTION_VERSION: &str = "aikit.praxis-resolution/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedMethod {
    pub method: ResourceRef,
    pub resolution: MethodResolution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PraxisResolution {
    pub version: String,
    /// Exact version of the operational ContextResolution under which this Method
    /// selection was made. The full ContextResolution remains the owner receipt.
    pub context_resolution_version: String,
    #[serde(default)]
    pub focus: Vec<ResourceRef>,
    #[serde(default)]
    pub methods: Vec<SelectedMethod>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Resolve explicitly selected Methods under an existing ContextResolution.
///
/// `available_methods` are source-loaded Method bodies keyed by stable MethodRef.
/// The V2 ResourceIndex is used only to resolve their referenced resources. No
/// Method member is enabled by this function; normal Profile/scope/ContextResolution
/// and Action authority continue to decide operativity.
pub fn resolve_praxis(
    context: &ContextResolution,
    resources: &dyn ResourceIndex,
    available_methods: &[Method],
    selected: &[ResourceRef],
    focus: &[ResourceRef],
) -> PraxisResolution {
    let mut methods = Vec::new();
    let mut warnings = Vec::new();

    for reference in selected {
        let Some(method) = available_methods.iter().find(|method| &method.id == reference) else {
            warnings.push(format!(
                "selected Method {reference} is absent from the source-loaded Method field"
            ));
            continue;
        };
        match resolve_method(method, resources) {
            Ok(resolution) => {
                warnings.extend(
                    resolution
                        .warnings
                        .iter()
                        .map(|warning| format!("Method {reference}: {warning}")),
                );
                methods.push(SelectedMethod {
                    method: reference.clone(),
                    resolution,
                });
            }
            Err(error) => warnings.push(format!(
                "Method {reference} is invalid under this ContextResolution: {}",
                error.message()
            )),
        }
    }

    PraxisResolution {
        version: PRAXIS_RESOLUTION_VERSION.into(),
        context_resolution_version: context.version.clone(),
        focus: focus.to_vec(),
        methods,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_resolution::{compose_context_resolution, RequestedActors};
    use crate::method::MethodSkillRef;
    use crate::project::{
        ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
    };
    use crate::resource::{
        MemoryResourceIndex, ResourceDescriptor, ResourceKind, ResourceRecord, SourceRef,
    };
    use crate::resolve::{resolve, ResolveRequest};
    use crate::trust::AlwaysTrusted;
    use crate::{ContextDescriptor, ManagedPolicy, MemoryCatalog};

    fn record(id: &str, kind: ResourceKind) -> ResourceRecord {
        ResourceRecord::new(ResourceDescriptor::new(
            ResourceRef::parse(id).unwrap(),
            kind,
            id,
            id,
        ))
    }

    #[test]
    fn method_selection_is_downstream_of_context_resolution_not_a_precedence_engine() {
        let catalog = MemoryCatalog::default();
        let trust = AlwaysTrusted;
        let request = ResolveRequest {
            context: ContextDescriptor::for_project("/tmp/test"),
            layers: vec![],
            policy: ManagedPolicy::default(),
        };
        let deterministic = resolve(&catalog, &trust, &request).unwrap();
        let mut resources = MemoryResourceIndex::default();
        resources.insert(record("cap:wayfinder", ResourceKind::Capability));
        let context = compose_context_resolution(
            &deterministic,
            ProjectBinding::new(
                ProjectRef::parse("project:test").unwrap(),
                ProjectConstituentRef::parse("constituent:test").unwrap(),
                ProjectBindingLocator::LocalDirectory {
                    path: "/tmp/test".into(),
                },
            ),
            &[],
            &resources,
            RequestedActors::default(),
        );
        let method = Method {
            id: ResourceRef::parse("method:orient").unwrap(),
            source: SourceRef::parse("source:method:orient").unwrap(),
            revision: None,
            name: "Orient".into(),
            description: String::new(),
            focus: vec![],
            project_domain: vec![],
            skills: vec![MethodSkillRef {
                skill: ResourceRef::parse("cap:wayfinder").unwrap(),
                usage_overlay: None,
            }],
            actions: vec![],
            capabilities: vec![],
            context_sources: vec![],
            verification: vec![],
            expected_return_forms: vec!["evidence".into()],
        };
        let resolved = resolve_praxis(
            &context,
            &resources,
            &[method],
            &[ResourceRef::parse("method:orient").unwrap()],
            &[],
        );
        assert_eq!(resolved.context_resolution_version, context.version);
        assert_eq!(resolved.methods.len(), 1);
        assert!(resolved.warnings.is_empty());
        assert_eq!(context.capabilities.len(), 1);
    }
}
