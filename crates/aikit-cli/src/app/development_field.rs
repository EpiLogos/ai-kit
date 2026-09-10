//! Bounded Development Field application reading.
//!
//! This is an application composition over owner-native Resource records and the
//! existing VersionedWorld provider. It is not a second source store, QL engine,
//! Workcell lifecycle, or intelligence grammar.

use aikit_adapters::native_git::NativeGitProvider;
use aikit_core::resource::{
    read_development_field, DevelopmentFieldExecutableBasis, DevelopmentFieldExecutableModality,
    DevelopmentFieldGitBasis, DevelopmentFieldReadRequest, DevelopmentFieldReading, ResourceRef,
    VersionRevision,
};
use aikit_core::{AikitError, Result};
use aikit_tui::backend::PaletteBackend;

use super::Service;

#[derive(Debug, Clone)]
pub struct DevelopmentFieldApplicationRequest {
    pub subjects: Vec<ResourceRef>,
    pub limit: usize,
    pub base_revision: Option<VersionRevision>,
    pub max_diff_bytes: usize,
    pub expected_aikit_revision: Option<VersionRevision>,
}

impl Default for DevelopmentFieldApplicationRequest {
    fn default() -> Self {
        Self {
            subjects: Vec::new(),
            limit: aikit_core::resource::DEFAULT_DEVELOPMENT_FIELD_READ_LIMIT,
            base_revision: None,
            max_diff_bytes: 256 * 1024,
            expected_aikit_revision: None,
        }
    }
}

impl Service {
    /// Read the current bounded Development Field through the same application
    /// backend used by CLI/TUI composition. Owner records enter through the
    /// canonical Resource field; this operation only composes their declared
    /// relations with exact process and Git basis.
    pub fn development_field_read(
        &self,
        request: DevelopmentFieldApplicationRequest,
    ) -> Result<DevelopmentFieldReading> {
        let records = <Self as PaletteBackend>::context_resource_records(self)?;
        let resources =
            aikit_tui::project_world_service::resource_index_with_records(self, records)?;
        let executable_basis = current_executable_basis();
        verify_expected_revision(&executable_basis, request.expected_aikit_revision.as_ref())?;
        let git_basis =
            self.development_field_git_basis(request.base_revision.clone(), request.max_diff_bytes);
        Ok(read_development_field(
            &resources,
            &DevelopmentFieldReadRequest {
                subjects: request.subjects,
                limit: request.limit,
            },
            executable_basis,
            git_basis,
        ))
    }

    fn development_field_git_basis(
        &self,
        base_revision: Option<VersionRevision>,
        max_diff_bytes: usize,
    ) -> Result<Option<DevelopmentFieldGitBasis>> {
        let Some(root) = self.descriptor.project_root.as_deref() else {
            return Ok(None);
        };
        let Some(binding) = <Self as PaletteBackend>::project_binding(self)? else {
            return Ok(None);
        };
        let provider = NativeGitProvider::new()?;
        match provider.development_field_basis(
            &binding.project,
            &root.to_string_lossy(),
            base_revision,
            max_diff_bytes,
        ) {
            Ok(basis) => Ok(Some(basis)),
            Err(error) if error.code() == "versioned_world.git_failed" => Ok(None),
            Err(error) => Err(error),
        }
    }
}

fn current_executable_basis() -> DevelopmentFieldExecutableBasis {
    let source_revision = option_env!("AIKIT_BUILD_SOURCE_REVISION")
        .filter(|value| !value.trim().is_empty())
        .map(VersionRevision::new);
    let source_dirty = option_env!("AIKIT_BUILD_SOURCE_DIRTY") == Some("1");
    let modality = match source_revision.as_ref() {
        Some(_) if cfg!(debug_assertions) => DevelopmentFieldExecutableModality::Developer,
        Some(_) => DevelopmentFieldExecutableModality::Source,
        None => DevelopmentFieldExecutableModality::Installed,
    };
    DevelopmentFieldExecutableBasis {
        executable: std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "<unavailable>".into()),
        package_version: env!("CARGO_PKG_VERSION").into(),
        modality,
        source_revision,
        source_dirty,
    }
}

fn verify_expected_revision(
    basis: &DevelopmentFieldExecutableBasis,
    expected: Option<&VersionRevision>,
) -> Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let exact = basis
        .source_revision
        .as_ref()
        .is_some_and(|actual| actual == expected)
        && !basis.source_dirty;
    if exact {
        return Ok(());
    }
    Err(AikitError::new(
        "resource.development_field_executable_revision_mismatch",
        "the active AIKit executable does not exactly represent the requested source revision",
    )
    .with("expected_revision", expected.as_str())
    .with(
        "actual_revision",
        basis
            .source_revision
            .as_ref()
            .map(VersionRevision::as_str)
            .unwrap_or("unavailable"),
    )
    .with("source_dirty", basis.source_dirty.to_string())
    .with("executable", basis.executable.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_revision_rejects_both_a_different_build_and_a_dirty_matching_build() {
        let expected = VersionRevision::new("abc");
        let mut basis = DevelopmentFieldExecutableBasis {
            executable: "/tmp/aikit".into(),
            package_version: "0.0.0".into(),
            modality: DevelopmentFieldExecutableModality::Source,
            source_revision: Some(VersionRevision::new("def")),
            source_dirty: false,
        };
        assert_eq!(
            verify_expected_revision(&basis, Some(&expected))
                .unwrap_err()
                .code(),
            "resource.development_field_executable_revision_mismatch"
        );
        basis.source_revision = Some(expected.clone());
        basis.source_dirty = true;
        assert!(verify_expected_revision(&basis, Some(&expected)).is_err());
        basis.source_dirty = false;
        assert!(verify_expected_revision(&basis, Some(&expected)).is_ok());
    }
}
