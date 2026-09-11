//! Source-bound consumption of Actuation's existing agency actualisation operation.
//!
//! AIKit does not mint Agent/Agency/WorldBinding identities, validate a foreign
//! authority language, or treat a profile as a grant. The native owner evaluates
//! the supplied source on each admission. The exact byte basis remains pinned;
//! replacing or withdrawing that source revokes this admission, not its history.
use crate::runner::CommandRunner;
use aikit_core::{AikitError, ResourceRef, Result, SourceRevision};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io::Read, path::PathBuf};

pub const AGENCY_ACTUALISATION_SCHEMA: &str = "actuation.agency-actualisation/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgencySourceBasis {
    pub source_ref: ResourceRef,
    pub revision: SourceRevision,
    pub path: PathBuf,
    pub content_digest: String,
}
impl AgencySourceBasis {
    pub fn read(&self) -> Result<Vec<u8>> {
        ResourceRef::parse(self.source_ref.as_str())?;
        SourceRevision::parse(self.revision.as_str())?;
        let digest = self
            .content_digest
            .strip_prefix("blake3:")
            .filter(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .ok_or_else(|| invalid("A source needs a lowercase blake3 digest"))?;
        if !self.path.is_absolute()
            || self
                .path
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err(invalid(
                "Agency source must have an absolute, non-traversing locator",
            ));
        }
        let canonical = self.path.canonicalize().map_err(unavailable)?;
        if canonical != self.path {
            return Err(invalid("Agency source must not redirect through a symlink"));
        }
        let mut file = std::fs::File::open(&canonical).map_err(unavailable)?;
        let meta = file.metadata().map_err(unavailable)?;
        const LIMIT: u64 = 1024 * 1024;
        if !meta.is_file() || meta.len() > LIMIT {
            return Err(invalid(
                "Agency source must be a regular file of at most 1 MiB",
            ));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(unavailable)?;
        if bytes.len() as u64 > LIMIT || blake3::hash(&bytes).to_hex().as_str() != digest {
            return Err(AikitError::new(
                "agency_admission.stale",
                "The admitted Agency source changed; explicitly recompose before acting",
            ));
        }
        Ok(bytes)
    }
}

/// An owner receipt, not permanent residence, process liveness or human Recognition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdmittedAgency {
    pub basis: AgencySourceBasis,
    pub agent_ref: ResourceRef,
    pub agency_ref: ResourceRef,
    pub world_binding_ref: ResourceRef,
    pub world_ref: ResourceRef,
    pub scope_ref: ResourceRef,
    pub receipt: Value,
}
impl AdmittedAgency {
    /// Preserve an explicitly admitted World's identity as the operative ground
    /// when there is no narrower local Project. In particular, Central root
    /// agency is not a binding-free or synthetic child-Project context.
    ///
    /// Revalidate the source/receipt correlation, including deserialized fields.
    /// This is context evidence, not a new admission or a filesystem grant;
    /// dispatch still invokes the native owner's current authority checks.
    pub fn context_binding(&self) -> Result<aikit_core::project::ProjectBinding> {
        use aikit_core::project::{
            ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
        };
        use aikit_core::resource::{ProviderRef, SourceRef};

        let bytes = self.basis.read()?;
        let request: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
        if request["schema"] != AGENCY_ACTUALISATION_SCHEMA {
            return Err(invalid("Expected an Actuation agency actualisation request source"));
        }
        validate_receipt(&request, &self.receipt)?;
        for (field, expected) in [
            ("agent_ref", &self.agent_ref),
            ("agency_ref", &self.agency_ref),
            ("world_ref", &self.world_ref),
            ("binding_ref", &self.world_binding_ref),
            ("scope_ref", &self.scope_ref),
        ] {
            if request["differentiated_binding"][field].as_str() != Some(expected.as_str()) {
                return Err(invalid(format!("Admitted {field} differs from its native source")));
            }
        }
        if self.scope_ref.as_str() != "scope:root" {
            return Err(AikitError::new(
                "agency_admission.project_binding_required",
                "Only declared root scope is a meta-project; a child World requires its native Project binding",
            ));
        }
        let mut binding = ProjectBinding::new(
            ProjectRef::parse(self.world_ref.as_str())?,
            ProjectConstituentRef::parse(self.world_binding_ref.as_str())?,
            ProjectBindingLocator::NativeWorld {
                world: self.world_ref.clone(),
                binding: self.world_binding_ref.clone(),
                scope: self.scope_ref.clone(),
                source_revision: self.basis.revision.clone(),
                content_digest: self.basis.content_digest.clone(),
            },
        );
        binding.provider = Some(ProviderRef::parse("provider/actuation")?);
        binding.source = Some(SourceRef::parse(self.basis.source_ref.as_str())?);
        Ok(binding)
    }

    pub fn authorises(&self, action: &ResourceRef) -> bool {
        let autonomy = &self.receipt["determination"]["delegated_autonomy"];
        let has = |field| {
            autonomy[field]
                .as_array()
                .is_some_and(|refs| refs.iter().any(|r| r.as_str() == Some(action.as_str())))
        };
        has("allowed_action_refs") && !has("denied_action_refs")
    }
}

/// Invoke the *existing* native operation. The staged input is precisely the
/// admitted source bytes, not an AIKit-authored replacement grant. The original
/// source is rechecked after the owner call. No --out, registry mutation, Factory
/// Recognition or provider execution is requested by this adapter.
pub fn admit_agency(
    runner: &dyn CommandRunner,
    actuation_bin: &str,
    basis: &AgencySourceBasis,
    selected_agent: &ResourceRef,
    world: &ResourceRef,
) -> Result<AdmittedAgency> {
    let bytes = basis.read()?;
    let request: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
    if request["schema"] != AGENCY_ACTUALISATION_SCHEMA {
        return Err(invalid(
            "Expected an Actuation agency actualisation request source",
        ));
    }
    let binding = &request["differentiated_binding"];
    if binding["agent_ref"].as_str() != Some(selected_agent.as_str())
        || binding["world_ref"].as_str() != Some(world.as_str())
    {
        return Err(AikitError::new(
            "agency_admission.scope_mismatch",
            "The selected Agent or World differs from the supplied native binding",
        ));
    }
    let staged = tempfile::NamedTempFile::new().map_err(unavailable)?;
    std::fs::write(staged.path(), &bytes).map_err(unavailable)?;
    let argv = vec![
        actuation_bin.into(),
        "agency".into(),
        "actualise".into(),
        staged.path().display().to_string(),
        "--json".into(),
    ];
    let output = runner.run(&argv)?;
    if output.status != 0 {
        // Owner errors may contain imported private source; do not copy its
        // stdout/stderr into a shared diagnostics surface.
        return Err(AikitError::new(
            "agency_admission.denied",
            format!(
                "Actuation refused Agency admission (exit {})",
                output.status
            ),
        ));
    }
    let receipt: Value = serde_json::from_str(&output.stdout).map_err(invalid)?;
    validate_receipt(&request, &receipt)?;
    basis.read()?;
    let reference = |name: &str| -> Result<ResourceRef> {
        ResourceRef::parse(
            binding[name]
                .as_str()
                .ok_or_else(|| invalid(format!("Missing native {name}")))?,
        )
    };
    let admitted = AdmittedAgency {
        basis: basis.clone(),
        agent_ref: reference("agent_ref")?,
        agency_ref: reference("agency_ref")?,
        world_binding_ref: reference("binding_ref")?,
        world_ref: reference("world_ref")?,
        scope_ref: reference("scope_ref")?,
        receipt,
    };
    if admitted.agent_ref == admitted.agency_ref
        || admitted.agency_ref == admitted.world_binding_ref
        || admitted.agent_ref == admitted.world_binding_ref
    {
        return Err(invalid(
            "Agent, Agency and WorldBinding are distinct native identities",
        ));
    }
    Ok(admitted)
}

fn validate_receipt(request: &Value, receipt: &Value) -> Result<()> {
    if !receipt.is_object()
        || receipt["schema"] != AGENCY_ACTUALISATION_SCHEMA
        || receipt["status"] != "actualised"
    {
        return Err(invalid(
            "Actuation did not return a successful native Agency receipt",
        ));
    }
    for key in [
        "request_ref",
        "requester_ref",
        "governing_binding",
        "differentiated_binding",
        "determination",
    ] {
        if request.get(key).is_none() || request.get(key) != receipt.get(key) {
            return Err(invalid(format!(
                "Actuation Agency receipt changed or omitted {key}"
            )));
        }
    }
    if receipt["bounds_refs"] != request["determination"]["bounds_refs"]
        || receipt["metagency"]["grant_ref"] != request["metagency_grant"]["grant_ref"]
        || receipt["metagency"]["authority_ref"] != request["metagency_grant"]["authority_ref"]
        || receipt["agent_identity"]["agent_ref"] != request["differentiated_binding"]["agent_ref"]
        || receipt["provenance"]["source_refs"] != request["provenance"]["source_refs"]
        || receipt["effects"]["materialisation"] != "not-performed"
        || receipt["effects"]["source_mutation"] != "not-performed"
    {
        return Err(invalid(
            "Actuation Agency receipt does not preserve its request's bounds, lineage or effects",
        ));
    }
    Ok(())
}
fn invalid(error: impl std::fmt::Display) -> AikitError {
    AikitError::new("agency_admission.invalid", error.to_string())
}
fn unavailable(error: impl std::fmt::Display) -> AikitError {
    AikitError::new("agency_admission.unavailable", error.to_string())
}
