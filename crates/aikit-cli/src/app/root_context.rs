//! Central is the enclosing root meta-project, not an absent child Project.
//!
//! This is read-only location/binding work. It does not read human payloads,
//! adopt source, create ProjectCentral, select a Profile or grant authority.
//! Central's existing `control:root` protocol identity remains its own; an
//! Actuation-admitted World keeps its independently supplied native identity.
use std::path::{Path, PathBuf};

use aikit_adapters::central_world_sources::ROOT_WORLD_REF;
use aikit_core::project::{
    ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
};
use aikit_core::{AikitError, ProviderRef, Result, SourceRef};

use crate::discover::{DiscoveredProject, ProjectLayer};

/// Resolve only the configured Central's root/Control/Work-container scope.
/// An ordinary child Project stays narrower; no arbitrary cwd is made a root.
pub(super) fn discover<F>(
    cwd: &Path,
    env: &F,
    discovered: Option<DiscoveredProject>,
) -> Result<(Option<DiscoveredProject>, Option<PathBuf>)>
where
    F: Fn(&str) -> Option<String>,
{
    let explicit = env("CENTRAL_ROOT").filter(|s| !s.is_empty());
    let candidate = explicit.as_ref().map(PathBuf::from).or_else(|| {
        env("HOME")
            .or_else(|| env("USERPROFILE"))
            .filter(|s| !s.is_empty())
            .map(|home| PathBuf::from(home).join("Central"))
    });
    let Some(candidate) = candidate else {
        return Ok((discovered, None));
    };
    let Ok(root) = candidate.canonicalize() else {
        return Ok((discovered, None));
    };
    let current = cwd.canonicalize().map_err(|e| invalid(e.to_string()))?;
    if !current.starts_with(&root) {
        return Ok((discovered, None));
    }

    // Discovery can arrive through an alias while Central's root is canonical.
    // Compare the same filesystem locations so an alias cannot drop a narrower
    // profile layer (or replace an existing reusable Project specification).
    let mut discovered = discovered;
    if let Some(project) = &mut discovered {
        project.root = project
            .root
            .canonicalize()
            .map_err(|e| invalid(e.to_string()))?;
        for layer in &mut project.chain {
            layer.dir = layer
                .dir
                .canonicalize()
                .map_err(|e| invalid(e.to_string()))?;
        }
    }

    let at_root = current == root
        || current == root.join("Work")
        || current.starts_with(root.join("Control"));
    if at_root {
        if explicit.is_none() && !root.join("Control").exists() && !root.join("Work").exists() {
            return Ok((discovered, None));
        }
        // Reuse the published filesystem relation; absence of a Profile is not
        // absence of this World. Source readability is checked by its owner later.
        binding(&root)?;
        let chain = scope_chain(&root, discovered.as_ref());
        return Ok((
            Some(DiscoveredProject {
                root: root.clone(),
                chain,
                specification: discovered
                    .as_ref()
                    .filter(|p| p.root == root)
                    .and_then(|p| p.specification.clone()),
                skill_sets: discovered
                    .as_ref()
                    .filter(|p| p.root == root)
                    .map(|p| p.skill_sets.clone())
                    .unwrap_or_default(),
            }),
            Some(root),
        ));
    }

    // ProjectCentral identity does not depend on an AIKit profile marker either.
    // Only the immediate Work member participates in this filesystem protocol.
    if let Ok(relative) = current.strip_prefix(root.join("Work")) {
        if let Some(member) = relative.components().next() {
            let child = root.join("Work").join(member.as_os_str());
            match std::fs::symlink_metadata(child.join("ProjectCentral/project.json")) {
                Ok(_) => {
                    // Preserve an explicit narrower Project Specification. The
                    // existing ProjectCentral adapter validates manifest semantics.
                    if discovered
                        .as_ref()
                        .is_some_and(|p| p.root.starts_with(&child))
                    {
                        return Ok((discovered, None));
                    }
                    let chain = scope_chain(&child, discovered.as_ref());
                    return Ok((
                        Some(DiscoveredProject {
                            root: child,
                            chain,
                            specification: None,
                            skill_sets: Vec::new(),
                        }),
                        None,
                    ));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(invalid(e.to_string())),
            }
        }
    }
    Ok((discovered, None))
}

fn scope_chain(root: &Path, discovered: Option<&DiscoveredProject>) -> Vec<ProjectLayer> {
    let mut dirs = vec![root.to_path_buf()];
    if let Some(discovered) = discovered {
        for layer in &discovered.chain {
            if layer.dir.starts_with(root) && !dirs.contains(&layer.dir) {
                dirs.push(layer.dir.clone());
            }
        }
    }
    dirs.into_iter()
        .enumerate()
        .map(|(depth, dir)| ProjectLayer {
            dir,
            depth: depth as u32,
        })
        .collect()
}

/// Revalidate location before returning a binding. A removed/redirected root is
/// not converted to an ungrounded successful context. No source body is read.
pub(super) fn binding(root: &Path) -> Result<ProjectBinding> {
    if root.canonicalize().map_err(|e| invalid(e.to_string()))? != root {
        return Err(invalid("Central root changed location"));
    }
    for member in ["Control", "Work"] {
        let metadata = std::fs::symlink_metadata(root.join(member))
            .map_err(|e| invalid(format!("Central {member} is unavailable: {e}")))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid(format!(
                "Central {member} must be a native directory"
            )));
        }
    }
    let mut binding = ProjectBinding::new(
        ProjectRef::parse(ROOT_WORLD_REF)?,
        ProjectConstituentRef::parse("central:source:control:root:Control")?,
        ProjectBindingLocator::LocalDirectory {
            path: root.to_path_buf(),
        },
    );
    binding.provider = Some(ProviderRef::parse("central")?);
    binding.source = Some(SourceRef::parse("central:source:control:root:Control")?);
    Ok(binding)
}

fn invalid(message: impl Into<String>) -> AikitError {
    AikitError::new("central.root_context_unavailable", message)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn aliased_root_entry_keeps_the_existing_profile_chain_and_specification() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Central");
        let nested = root.join("Control/nested");
        for path in [root.join(".aikit"), root.join("Work"), nested.join(".aikit")] {
            std::fs::create_dir_all(path).unwrap();
        }
        let alias = temp.path().join("chosen-root-alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let cwd = alias.join("Control/nested");
        let mut existing = crate::discover::discover_project(&cwd).unwrap();
        existing.specification = Some("root-spec".into());
        existing.skill_sets = vec!["root-skills".into()];
        let (project, meta_root) = discover(
            &cwd,
            &|key| (key == "CENTRAL_ROOT").then(|| alias.display().to_string()),
            Some(existing),
        )
        .unwrap();
        let project = project.unwrap();
        let canonical = root.canonicalize().unwrap();
        assert_eq!(meta_root.as_ref(), Some(&canonical));
        assert_eq!(project.root, canonical);
        assert_eq!(project.specification.as_deref(), Some("root-spec"));
        assert_eq!(project.skill_sets, vec!["root-skills"]);
        assert_eq!(
            project.chain.iter().map(|layer| layer.dir.clone()).collect::<Vec<_>>(),
            vec![canonical, nested.canonicalize().unwrap()]
        );
        assert_eq!(
            project.chain.iter().map(|layer| layer.depth).collect::<Vec<_>>(),
            vec![0, 1]
        );
    }
}
