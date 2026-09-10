use std::path::PathBuf;

use aikit_cli::project_binding;
use aikit_cli::projects::{ProjectMatch, ProjectSpec};
use aikit_core::project::{ProjectConstituentRef, ProjectRef};

#[test]
fn project_spec_id_remains_migration_provenance_not_project_identity() {
    let matched = ProjectMatch {
        spec: ProjectSpec {
            schema: 1,
            id: "legacy-ai-kit".into(),
            directories: vec![PathBuf::from("/work/ai-kit")],
            repositories: vec!["github.com/epilogos/ai-kit".into()],
            inherit_default_skill_sets: true,
            skill_sets: vec!["code".into()],
        },
        root: PathBuf::from("/work/ai-kit"),
        matched_by: "directory",
    };

    let binding = project_binding::from_match(
        ProjectRef::parse("project:epilogos/ai-kit").unwrap(),
        ProjectConstituentRef::parse("constituent:source").unwrap(),
        &matched,
    );

    assert_eq!(binding.project.as_str(), "project:epilogos/ai-kit");
    assert_eq!(
        binding.legacy_project_spec_id.as_deref(),
        Some("legacy-ai-kit")
    );
    assert_ne!(binding.project.as_str(), matched.spec.id);
}
