//! Target-owned native lifecycle for SessionSpace contributions.
//!
//! This is deliberately not an O:I package runtime and not a universal plugin
//! interface. AIKit owns the meaning of a SessionSpace contribution and exposes
//! the smallest register/readback/remove seam required by external suite
//! composition. Removing a contribution removes only the registration relation;
//! it never deletes or closes an externally owned SessionSpace identity/runtime.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::resource::ResourceRef;
use crate::session_space::{SessionSpaceReadModel, SessionSpaceRef, SESSION_SPACE_VERSION};
use crate::{AikitError, Result};

pub const SESSION_SPACE_CONTRIBUTION_VERSION: &str = "aikit.session-space-contribution/v1";
pub const SESSION_SPACE_CONTRIBUTION_REGISTRY_VERSION: &str =
    "aikit.session-space-contribution-registry/v1";

/// Stable native contribution identity. This is not a package ref and not the
/// SessionSpace identity it references.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionSpaceContributionRef(ResourceRef);

impl SessionSpaceContributionRef {
    pub fn parse(raw: &str) -> Result<Self> {
        if !raw.starts_with("session-space-contribution/") {
            return Err(AikitError::new(
                "session_space_contribution.invalid_ref",
                format!(
                    "SessionSpace contribution ref `{raw}` must begin with `session-space-contribution/`"
                ),
            ));
        }
        Ok(Self(ResourceRef::parse(raw)?))
    }

    pub fn as_resource_ref(&self) -> &ResourceRef {
        &self.0
    }
}

impl std::fmt::Display for SessionSpaceContributionRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// AIKit-owned declaration of one native contribution that makes an existing
/// SessionSpace available for suite composition. The SessionSpace remains an
/// independently owned identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceContributionDefinition {
    pub version: String,
    pub id: SessionSpaceContributionRef,
    pub session_space: SessionSpaceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ResourceRef>,
    #[serde(default)]
    pub surface_refs: BTreeSet<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl SessionSpaceContributionDefinition {
    pub fn new(id: SessionSpaceContributionRef, session_space: SessionSpaceRef) -> Self {
        Self {
            version: SESSION_SPACE_CONTRIBUTION_VERSION.into(),
            id,
            session_space,
            provider: None,
            surface_refs: BTreeSet::new(),
            provenance: vec!["AIKit native SessionSpace contribution".into()],
        }
    }

    #[must_use]
    pub fn with_provider(mut self, provider: ResourceRef) -> Self {
        self.provider = Some(provider);
        self
    }

    #[must_use]
    pub fn with_surface(mut self, surface: ResourceRef) -> Self {
        self.surface_refs.insert(surface);
        self
    }

    #[must_use]
    pub fn with_provenance(mut self, provenance: impl Into<String>) -> Self {
        self.provenance.push(provenance.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceContributionRegistration {
    pub contribution: SessionSpaceContributionDefinition,
    pub native_registration_ref: ResourceRef,
    pub registry_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceContributionRemoval {
    pub contribution_ref: SessionSpaceContributionRef,
    pub native_registration_ref: ResourceRef,
    pub session_space: SessionSpaceRef,
    pub registry_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceContributionRegistryReadModel {
    pub version: String,
    pub revision: u64,
    pub registrations: Vec<SessionSpaceContributionRegistration>,
}

/// In-memory native registry used by an AIKit host/provider implementation.
/// Persistence belongs to the eventual provider/store integration, not O:I.
#[derive(Debug, Default)]
pub struct SessionSpaceContributionRegistry {
    revision: u64,
    registrations: BTreeMap<SessionSpaceContributionRef, SessionSpaceContributionRegistration>,
}

impl SessionSpaceContributionRegistry {
    pub fn register(
        &mut self,
        definition: SessionSpaceContributionDefinition,
    ) -> Result<SessionSpaceContributionRegistration> {
        validate_definition(&definition)?;
        if self.registrations.contains_key(&definition.id) {
            return Err(AikitError::new(
                "session_space_contribution.already_registered",
                format!("SessionSpace contribution {} is already registered", definition.id),
            ));
        }

        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| AikitError::new("session_space_contribution.revision_overflow", "registry revision overflow"))?;
        let native_registration_ref = ResourceRef::parse(&format!(
            "aikit-registration/{}",
            definition.id
        ))?;
        let registration = SessionSpaceContributionRegistration {
            contribution: definition.clone(),
            native_registration_ref,
            registry_revision: self.revision,
        };
        self.registrations
            .insert(definition.id.clone(), registration.clone());
        Ok(registration)
    }

    /// Target-owned verification/readback. Callers can only claim registration
    /// after AIKit returns this native record.
    pub fn read(
        &self,
        contribution_ref: &SessionSpaceContributionRef,
    ) -> Option<&SessionSpaceContributionRegistration> {
        self.registrations.get(contribution_ref)
    }

    pub fn remove(
        &mut self,
        contribution_ref: &SessionSpaceContributionRef,
    ) -> Result<SessionSpaceContributionRemoval> {
        let registration = self.registrations.remove(contribution_ref).ok_or_else(|| {
            AikitError::new(
                "session_space_contribution.not_registered",
                format!("SessionSpace contribution {contribution_ref} is not registered"),
            )
        })?;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| AikitError::new("session_space_contribution.revision_overflow", "registry revision overflow"))?;
        Ok(SessionSpaceContributionRemoval {
            contribution_ref: contribution_ref.clone(),
            native_registration_ref: registration.native_registration_ref,
            session_space: registration.contribution.session_space,
            registry_revision: self.revision,
        })
    }

    pub fn read_model(&self) -> SessionSpaceContributionRegistryReadModel {
        SessionSpaceContributionRegistryReadModel {
            version: SESSION_SPACE_CONTRIBUTION_REGISTRY_VERSION.into(),
            revision: self.revision,
            registrations: self.registrations.values().cloned().collect(),
        }
    }

    pub fn verify_session_space_read_model(
        &self,
        contribution_ref: &SessionSpaceContributionRef,
        read_model: &SessionSpaceReadModel,
    ) -> Result<()> {
        let registration = self.read(contribution_ref).ok_or_else(|| {
            AikitError::new(
                "session_space_contribution.not_registered",
                format!("SessionSpace contribution {contribution_ref} is not registered"),
            )
        })?;
        if read_model.version != SESSION_SPACE_VERSION {
            return Err(AikitError::new(
                "session_space_contribution.unsupported_read_model",
                format!("unsupported SessionSpace read-model version {}", read_model.version),
            ));
        }
        if read_model.id != registration.contribution.session_space {
            return Err(AikitError::new(
                "session_space_contribution.session_space_mismatch",
                format!(
                    "registered SessionSpace {} does not match observed {}",
                    registration.contribution.session_space, read_model.id
                ),
            ));
        }
        Ok(())
    }
}

fn validate_definition(definition: &SessionSpaceContributionDefinition) -> Result<()> {
    if definition.version != SESSION_SPACE_CONTRIBUTION_VERSION {
        return Err(AikitError::new(
            "session_space_contribution.unsupported_version",
            format!(
                "SessionSpace contribution {} uses unsupported version {}",
                definition.id, definition.version
            ),
        ));
    }
    if definition.provenance.iter().any(|entry| entry.trim().is_empty()) {
        return Err(AikitError::new(
            "session_space_contribution.invalid_provenance",
            "SessionSpace contribution provenance entries must not be empty",
        ));
    }
    Ok(())
}
