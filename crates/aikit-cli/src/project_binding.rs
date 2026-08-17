//! Compatibility bridge from the current persisted ProjectSpec matcher to V2
//! ProjectBinding. ProjectSpec continues to own matching/default skill-set data;
//! the caller must supply the externally meaningful Project and constituent refs.

use aikit_core::project::{
    ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
};

use crate::projects::ProjectMatch;

pub fn from_match(
    project: ProjectRef,
    constituent: ProjectConstituentRef,
    matched: &ProjectMatch,
) -> ProjectBinding {
    ProjectBinding::new(
        project,
        constituent,
        ProjectBindingLocator::LocalDirectory {
            path: matched.root.clone(),
        },
    )
    .with_legacy_project_spec_id(matched.spec.id.clone())
}
