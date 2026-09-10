//! Native Central wiki discovery. Only the canonical paths disclosed by
//! central.world participate; repository fixtures and copied JSON are not roots.
use crate::runner::CommandRunner;
use aikit_core::knowledge_wiki_provider::WikiRegisterRevision;
use aikit_core::{parse_wiki_objects, AikitError, ResourceRef, Result, WikiObject};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

pub struct CentralWikiReading {
    pub objects: Vec<WikiObject>,
    pub registers: Vec<WikiRegisterRevision>,
    pub absences: Vec<String>,
}

pub fn read_central_wiki<R: CommandRunner>(
    runner: &R,
    executable: &Path,
    root: &Path,
) -> Result<CentralWikiReading> {
    let argv = vec![
        executable.to_string_lossy().into_owned(),
        "--json".into(),
        "--root".into(),
        root.to_string_lossy().into_owned(),
        "action".into(),
        "run".into(),
        "central.world".into(),
        json!({}).to_string(),
    ];
    let output = runner
        .run(&argv)?
        .require(&argv, "central.wiki_world_unavailable")?;
    let envelope: Value = serde_json::from_str(&output.stdout)
        .map_err(|e| AikitError::new("central.wiki_world_invalid", e.to_string()))?;
    let world = &envelope["data"];
    if envelope["ok"] != true || world["schema"] != "central.world-map/v1" {
        return Err(AikitError::new(
            "central.wiki_world_invalid",
            "Central did not disclose a supported world map",
        ));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|e| AikitError::new("central.wiki_root_unavailable", e.to_string()))?;
    let returned_root = world["root"].as_str().map(Path::new).ok_or_else(|| {
        AikitError::new(
            "central.wiki_world_invalid",
            "Central world map has no root",
        )
    })?;
    if returned_root.canonicalize().ok().as_ref() != Some(&canonical_root) {
        return Err(AikitError::new(
            "central.wiki_root_mismatch",
            "Central returned a different root",
        ));
    }
    let mut declarations = vec![&world["control"]["agent_wiki"]["wiki"]];
    if let Some(projects) = world["work"]["projects"].as_array() {
        for project in projects {
            declarations.push(&project["projectcentral"]["agent_wiki"]["wiki"]);
        }
    }
    let mut objects = Vec::new();
    let mut registers = Vec::new();
    let mut absences = Vec::new();
    let mut paths = BTreeSet::new();
    let mut seen_registers = BTreeSet::new();
    let mut seen_refs = BTreeSet::new();
    for declaration in declarations {
        if let Some(error) = declaration["error"].as_str() {
            absences.push(format!("Central wiki declaration unavailable: {error}"));
        }
        if declaration["present"] != true {
            continue;
        }
        let Some(relative) = declaration["path"].as_str() else {
            absences.push("Central wiki declaration has no path".into());
            continue;
        };
        let relative = Path::new(relative);
        if !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        {
            absences.push("Central wiki declaration is not a relative source path".into());
            continue;
        }
        let path = canonical_root.join(relative);
        if !paths.insert(path.clone()) {
            continue;
        }
        let Some(register) = declaration["space_ref"].as_str() else {
            absences.push("Central wiki declaration has no owner-authored space_ref".into());
            continue;
        };
        let register = match ResourceRef::parse(register) {
            Ok(register) => register,
            Err(error) => {
                absences.push(format!("Central wiki declaration has invalid space_ref: {error}"));
                continue;
            }
        };
        if !seen_registers.insert(register.clone()) {
            absences.push(format!(
                "Canonical wiki {} repeats register {}; kept the first declaration",
                relative.display(),
                register
            ));
            continue;
        }
        let reading = (|| -> Result<(Vec<WikiObject>, WikiRegisterRevision)> {
            let actual = path
                .canonicalize()
                .map_err(|e| AikitError::new("central.wiki_source_unavailable", e.to_string()))?;
            if !actual.starts_with(&canonical_root) || actual != path {
                return Err(AikitError::new(
                    "central.wiki_source_redirected",
                    "Canonical wiki source must not redirect through a symlink",
                ));
            }
            if fs::metadata(&path)
                .map_err(|e| AikitError::new("central.wiki_source_unavailable", e.to_string()))?
                .len()
                > 4 * 1024 * 1024
            {
                return Err(AikitError::new(
                    "central.wiki_source_too_large",
                    "Canonical wiki exceeds the bounded read size",
                ));
            }
            let bytes = fs::read(&path)
                .map_err(|e| AikitError::new("central.wiki_source_unavailable", e.to_string()))?;
            let text = std::str::from_utf8(&bytes).map_err(|e| {
                AikitError::new("central.wiki_source_unavailable", e.to_string())
            })?;
            let objects = parse_wiki_objects(text)?;
            if !objects.iter().any(|object| {
                matches!(object, WikiObject::Space(space) if space.ref_id == register)
            }) {
                return Err(AikitError::new(
                    "central.wiki_register_identity_mismatch",
                    "Central's declared register is not the canonical Wiki space in that source",
                )
                .with("register", register.to_string()));
            }
            Ok((
                objects,
                WikiRegisterRevision {
                    register: register.clone(),
                    revision: format!("blake3:{}", blake3::hash(&bytes).to_hex()),
                },
            ))
        })();
        match reading {
            Ok((read, register_revision)) => {
                registers.push(register_revision);
                // Two checkouts of one project disclose the same wiki space;
                // same-ref objects are one logical source. First declaration
                // wins (canonical_root order), later duplicates are
                // disclosed. Distinct refs across projects still collide at
                // rebuild, so the duplicate-ref guarantee is preserved where
                // it matters.
                for object in read {
                    let object_ref = object.ref_id().as_str().to_owned();
                    if seen_refs.insert(object_ref.clone()) {
                        objects.push(object);
                    } else {
                        absences.push(format!(
                            "Canonical wiki {} re-declares {} from an earlier declaration; kept the first",
                            relative.display(),
                            object_ref
                        ));
                    }
                }
            }
            Err(error) => absences.push(format!(
                "Canonical wiki {} unavailable: {}",
                relative.display(),
                error.message()
            )),
        }
    }
    registers.sort_by(|left, right| left.register.cmp(&right.register));
    Ok(CentralWikiReading {
        objects,
        registers,
        absences,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{CommandRunner, Output};
    use serde_json::json;

    struct WorldRunner {
        root: std::path::PathBuf,
    }

    impl CommandRunner for WorldRunner {
        fn run(&self, _argv: &[String]) -> Result<Output> {
            Ok(Output::success(
                json!({
                    "ok": true,
                    "data": {
                        "schema": "central.world-map/v1",
                        "root": self.root,
                        "control": {"agent_wiki": {"wiki": {
                            "present": true,
                            "path": "Control/agents/wiki/wiki.json",
                            "space_ref": "central:wiki:root"
                        }}},
                        "work": {"projects": [{"projectcentral": {"agent_wiki": {"wiki": {
                            "present": true,
                            "path": "Work/Alpha/ProjectCentral/agents/wiki/wiki.json",
                            "space_ref": "central:wiki:project:alpha"
                        }}}}]}
                    }
                })
                .to_string(),
            ))
        }
    }

    fn wiki(space_ref: &str, revision: u64, title: &str) -> String {
        json!({"objects": [{
            "profile": "okf-wiki/v1",
            "object": "space",
            "ref": space_ref,
            "revision": revision,
            "provenance": [],
            "title": title,
            "parent_space_refs": [],
            "child_space_refs": [],
            "node_refs": []
        }]})
        .to_string()
    }

    #[test]
    fn canonical_register_revisions_are_independent_content_keys() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let root_path = root.join("Control/agents/wiki/wiki.json");
        let project_path = root.join("Work/Alpha/ProjectCentral/agents/wiki/wiki.json");
        fs::create_dir_all(root_path.parent().unwrap()).unwrap();
        fs::create_dir_all(project_path.parent().unwrap()).unwrap();
        fs::write(&root_path, wiki("central:wiki:root", 1, "Root")).unwrap();
        fs::write(
            &project_path,
            wiki("central:wiki:project:alpha", 1, "Alpha"),
        )
        .unwrap();
        let runner = WorldRunner {
            root: root.to_path_buf(),
        };

        let first = read_central_wiki(&runner, Path::new("ctrl"), root).unwrap();
        assert_eq!(first.registers.len(), 2);
        let root_before = first
            .registers
            .iter()
            .find(|item| item.register.as_str() == "central:wiki:root")
            .unwrap()
            .revision
            .clone();
        let project_before = first
            .registers
            .iter()
            .find(|item| item.register.as_str() == "central:wiki:project:alpha")
            .unwrap()
            .revision
            .clone();

        fs::write(
            &project_path,
            wiki("central:wiki:project:alpha", 2, "Alpha moved"),
        )
        .unwrap();
        let second = read_central_wiki(&runner, Path::new("ctrl"), root).unwrap();
        let root_after = second
            .registers
            .iter()
            .find(|item| item.register.as_str() == "central:wiki:root")
            .unwrap();
        let project_after = second
            .registers
            .iter()
            .find(|item| item.register.as_str() == "central:wiki:project:alpha")
            .unwrap();

        assert_eq!(root_after.revision, root_before);
        assert_ne!(project_after.revision, project_before);
        assert!(project_after.revision.starts_with("blake3:"));
    }
}
