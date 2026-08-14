use serde_json::Value;

use crate::project::ProjectRef;
use crate::{AikitError, Result};

use super::{
    OwnerRef, ProviderOffer, ProviderRef, ProviderState, ResourceDescriptor, ResourceKind,
    ResourceRecord, ResourceSource, SourceRef, SourceRevision, SourceState,
};

/// Read-only AIKit view over the Factory-owned CR-001 interoperability fixture.
///
/// This adapter deliberately does not reproduce the Factory schema. It accepts the
/// versioned language-neutral document, preserves Factory identities as opaque
/// references, and projects only the resource fields AIKit needs for indexing.
/// Factory remains the semantic owner of the source document.
#[derive(Debug, Clone)]
pub struct FactoryInteropView {
    document: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryResourceImport {
    pub record: ResourceRecord,
    /// A provider declared by the Factory descriptor. `Unresolved` means AIKit has
    /// not yet observed a live provider offer; declaration is not availability.
    pub declared_provider: Option<ProviderRef>,
}

impl FactoryInteropView {
    pub fn from_fixture_json(input: &str) -> Result<Self> {
        let document: Value = serde_json::from_str(input).map_err(|error| {
            AikitError::new(
                "resource.factory_interop_invalid_json",
                format!("invalid Factory interoperability JSON: {error}"),
            )
        })?;
        let view = Self { document };
        view.require_eq(&["fixtureVersion"], "factory.interop-fixtures/v1")?;
        view.require_eq(
            &["contract", "contractVersion"],
            "factory.interop/v1",
        )?;
        Ok(view)
    }

    pub fn action_resource(&self) -> Result<FactoryResourceImport> {
        let descriptor = self.object(&["contract", "actionDescriptor"])?;
        let mut resource = ResourceDescriptor::new(
            super::ResourceRef::parse(self.str_field(descriptor, "actionRef")?)?,
            ResourceKind::Action,
            self.str_field(descriptor, "name")?,
            self.str_field(descriptor, "description")?,
        );
        resource.owner = Some(OwnerRef::parse(self.str_field(descriptor, "ownerProjectRef")?)?);
        resource.sources.push(self.provenance_source(descriptor)?);
        self.annotate_if_string(&mut resource, descriptor, "catalogRef", "factory.catalog-ref");
        self.annotate_if_string(
            &mut resource,
            descriptor,
            "inputContractRef",
            "factory.input-contract-ref",
        );
        self.annotate_if_string(
            &mut resource,
            descriptor,
            "outputContractRef",
            "factory.output-contract-ref",
        );
        self.annotate_revision(&mut resource, descriptor, "definitionRevision");
        Ok(FactoryResourceImport {
            record: ResourceRecord::new(resource),
            declared_provider: None,
        })
    }

    pub fn capability_resource(&self) -> Result<FactoryResourceImport> {
        let descriptor = self.object(&["contract", "capabilityDescriptor"])?;
        let mut resource = ResourceDescriptor::new(
            super::ResourceRef::parse(self.str_field(descriptor, "capabilityRef")?)?,
            ResourceKind::Capability,
            self.str_field(descriptor, "name")?,
            format!(
                "Factory Capability descriptor ({})",
                self.str_field(descriptor, "kind")?
            ),
        );
        resource.owner = Some(OwnerRef::parse(self.str_field(descriptor, "ownerRef")?)?);
        resource.sources.push(self.provenance_source(descriptor)?);
        self.annotate_if_string(
            &mut resource,
            descriptor,
            "kind",
            "factory.capability-kind",
        );
        self.annotate_revision(&mut resource, descriptor, "definitionRevision");

        let provider = descriptor
            .get("providerRef")
            .and_then(Value::as_str)
            .map(ProviderRef::parse)
            .transpose()?;
        let mut record = ResourceRecord::new(resource);
        if let Some(provider_ref) = provider.clone() {
            record.providers.push(ProviderOffer {
                provider: provider_ref,
                locator: None,
                state: ProviderState::Unresolved,
            });
        }
        Ok(FactoryResourceImport {
            record,
            declared_provider: provider,
        })
    }

    /// Returns the Factory-owned project identity unchanged. AIKit does not turn
    /// the Factory ProjectBinding envelope into an operational AIKit binding until
    /// it separately has a real constituent and locator.
    pub fn project_ref(&self) -> Result<ProjectRef> {
        ProjectRef::parse(self.required_str(&["contract", "projectBinding", "projectRef"])? )
    }

    pub fn project_source(&self) -> Result<ResourceSource> {
        let binding = self.object(&["contract", "projectBinding"])?;
        Ok(ResourceSource {
            source: SourceRef::parse(self.str_field(binding, "sourceRef")?)?,
            authority: None,
            revision: Some(SourceRevision::parse(
                self.str_field(binding, "sourceRevision")?,
            )?),
            locator: None,
            state: SourceState::Unresolved,
        })
    }

    fn provenance_source(&self, descriptor: &serde_json::Map<String, Value>) -> Result<ResourceSource> {
        let provenance = descriptor
            .get("provenance")
            .and_then(Value::as_object)
            .ok_or_else(|| self.invalid("missing descriptor provenance"))?;
        Ok(ResourceSource {
            source: SourceRef::parse(self.str_field(provenance, "sourceRef")?)?,
            authority: None,
            revision: Some(SourceRevision::parse(
                self.str_field(provenance, "sourceRevision")?,
            )?),
            locator: None,
            state: SourceState::Unresolved,
        })
    }

    fn annotate_if_string(
        &self,
        resource: &mut ResourceDescriptor,
        descriptor: &serde_json::Map<String, Value>,
        field: &str,
        annotation: &str,
    ) {
        if let Some(value) = descriptor.get(field).and_then(Value::as_str) {
            resource
                .annotations
                .insert(annotation.to_string(), value.to_string());
        }
    }

    fn annotate_revision(
        &self,
        resource: &mut ResourceDescriptor,
        descriptor: &serde_json::Map<String, Value>,
        field: &str,
    ) {
        if let Some(value) = descriptor.get(field).and_then(Value::as_u64) {
            resource
                .annotations
                .insert("factory.definition-revision".into(), value.to_string());
        }
    }

    fn object(&self, path: &[&str]) -> Result<&serde_json::Map<String, Value>> {
        let value = self.path(path)?;
        value
            .as_object()
            .ok_or_else(|| self.invalid(&format!("{} must be an object", path.join("."))))
    }

    fn required_str(&self, path: &[&str]) -> Result<&str> {
        self.path(path)?
            .as_str()
            .ok_or_else(|| self.invalid(&format!("{} must be a string", path.join("."))))
    }

    fn str_field<'a>(
        &self,
        object: &'a serde_json::Map<String, Value>,
        field: &str,
    ) -> Result<&'a str> {
        object
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| self.invalid(&format!("missing or invalid Factory field `{field}`")))
    }

    fn require_eq(&self, path: &[&str], expected: &str) -> Result<()> {
        let actual = self.required_str(path)?;
        if actual == expected {
            Ok(())
        } else {
            Err(self.invalid(&format!(
                "unsupported {} `{actual}`; expected `{expected}`",
                path.join(".")
            )))
        }
    }

    fn path(&self, path: &[&str]) -> Result<&Value> {
        let mut value = &self.document;
        for segment in path {
            value = value
                .get(*segment)
                .ok_or_else(|| self.invalid(&format!("missing Factory field `{}`", path.join("."))))?;
        }
        Ok(value)
    }

    fn invalid(&self, message: &str) -> AikitError {
        AikitError::new("resource.factory_interop_invalid", message)
    }
}
