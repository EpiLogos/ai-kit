use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::context::ContextDescriptor;
use crate::id::ProjectId;
use crate::resource::{ProviderRef, SourceRef};
use crate::{AikitError, Result};

use super::{ProjectConstituentRef, ProjectRef};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProjectBindingLocator {
    LocalDirectory { path: PathBuf },
    Repository { repository: String },
    Remote { locator: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBinding {
    pub project: ProjectRef,
    pub constituent: ProjectConstituentRef,
    pub locator: ProjectBindingLocator,
    #[serde(default)]
    pub provider: Option<ProviderRef>,
    #[serde(default)]
    pub source: Option<SourceRef>,
    #[serde(default)]
    pub legacy_aikit_project_id: Option<ProjectId>,
    #[serde(default)]
    pub legacy_project_spec_id: Option<String>,
}

impl ProjectBinding {
    pub fn new(
        project: ProjectRef,
        constituent: ProjectConstituentRef,
        locator: ProjectBindingLocator,
    ) -> Self {
        Self {
            project,
            constituent,
            locator,
            provider: None,
            source: None,
            legacy_aikit_project_id: None,
            legacy_project_spec_id: None,
        }
    }

    pub fn with_legacy_project_spec_id(mut self, id: impl Into<String>) -> Self {
        self.legacy_project_spec_id = Some(id.into());
        self
    }

    pub fn from_legacy_context(
        project: ProjectRef,
        constituent: ProjectConstituentRef,
        context: &ContextDescriptor,
    ) -> Result<Self> {
        let root = context.project_root.clone().ok_or_else(|| {
            AikitError::new(
                "project.binding_has_no_locator",
                "legacy context has no project root to bind",
            )
        })?;
        let mut binding = Self::new(
            project,
            constituent,
            ProjectBindingLocator::LocalDirectory { path: root },
        );
        binding.legacy_aikit_project_id = context.project_id.clone();
        Ok(binding)
    }
}
