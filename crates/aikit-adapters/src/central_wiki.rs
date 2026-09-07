//! Native Central wiki discovery. Only the canonical paths disclosed by
//! central.world participate; repository fixtures and copied JSON are not roots.
use crate::runner::CommandRunner;
use aikit_core::{parse_wiki_objects, AikitError, Result, WikiObject};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

pub struct CentralWikiReading {
    pub objects: Vec<WikiObject>,
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
    let mut absences = Vec::new();
    let mut paths = BTreeSet::new();
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
        let reading = (|| -> Result<Vec<WikiObject>> {
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
            let text = fs::read_to_string(&path)
                .map_err(|e| AikitError::new("central.wiki_source_unavailable", e.to_string()))?;
            parse_wiki_objects(&text)
        })();
        match reading {
            Ok(read) => {
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
    Ok(CentralWikiReading { objects, absences })
}
