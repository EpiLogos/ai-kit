//! Two owner reads over the existing Living Knowledge and ProjectMap
//! federation, both answered by the library's own deterministic operations.
//!
//! * `knowledge impact`: the transitive Living Knowledge impact of an owner
//!   change horizon over the dependency manifests this project's Semantic
//!   Wiki carries (`wiki_living_dependencies`), narrowed to a set of readings
//!   when the caller names them. It is the same closure `flow changed-since`
//!   and the Contemplate preflight compute, asked for readings rather than
//!   for one recorded thought.
//! * `knowledge reflection verify`: the verdict of a target-owned reflection
//!   law (`verify_reflection_law`) against the explicit bindings of the
//!   runtime's ProjectMap.
//! * `knowledge reflection laws`: the reflection laws the project itself
//!   declares, one `ReflectionLaw` per JSON file in its own
//!   `.aikit/reflection-laws/`, each as written, and each file that is not a
//!   law named with why (O-I #786: Docs asks each listed law's verdict).
//!
//! None records anything, invokes an Agent or model, or reads a source's
//! payload. The horizon and the law are owner seams the caller supplies or the
//! project writes; the surface fabricates neither.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use aikit_core::knowledge_living::KnowledgeChangeHorizon;
use aikit_core::knowledge_living_relations::{
    deterministic_transitive_knowledge_impact, KnowledgeTransitiveImpact,
};
use aikit_core::project_reflection::{
    verify_reflection_law, ReflectionLaw, ReflectionVerification,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{wiki_living_dependencies, AikitError};
use serde::{Deserialize, Serialize};

use super::Service;

pub const KNOWLEDGE_IMPACT_READING_VERSION: &str = "aikit.knowledge-impact-reading/v1";
pub const REFLECTION_VERDICT_VERSION: &str = "aikit.reflection-verdict/v1";
pub const REFLECTION_LAWS_VERSION: &str = "aikit.reflection-laws/v1";
/// Where a project declares its reflection laws, relative to its root: the
/// project's own AIKit directory, beside its `profile.toml`.
pub const REFLECTION_LAWS_DIR: &str = ".aikit/reflection-laws";
/// The most law files one listing reads, and the largest it reads whole.
const REFLECTION_LAW_FILES: usize = 256;
const REFLECTION_LAW_BYTES: u64 = 1024 * 1024;

/// The impact of one horizon, for every reading the Wiki's manifests reach or
/// for the named readings only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeImpactReading {
    pub version: String,
    /// The readings asked for; empty means every resource the closure reached.
    pub readings: Vec<ResourceRef>,
    /// How many Wiki objects and dependency manifests the closure followed.
    pub wiki_objects: usize,
    pub source_dependencies: usize,
    pub resource_dependencies: usize,
    /// The library's closure. With named readings, `direct.affected`,
    /// `transitive`, `paths` and `pending_integration` keep only those
    /// readings' rows; `changed_sources` and `truncated` stay as computed.
    pub impact: KnowledgeTransitiveImpact,
    /// Deterministic: never an Agent or model.
    pub automatic_agent_or_model_invocation: bool,
}

/// One target-owned reflection law's verdict over this project's ProjectMap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReflectionVerdict {
    pub version: String,
    pub law: String,
    /// How many explicit bindings the map carried when the law was verified.
    pub map_bindings: usize,
    pub verification: ReflectionVerification,
    pub automatic_agent_or_model_invocation: bool,
}

/// The reflection laws a project declares, each as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReflectionLawListing {
    pub version: String,
    /// Where the laws are declared, relative to the project's root.
    pub declared_in: String,
    /// The laws, by their file's path.
    pub laws: Vec<DeclaredReflectionLaw>,
    /// Each file there that is not a law, or repeats a law's id, with why.
    pub unreadable: Vec<UnreadableReflectionLaw>,
    pub automatic_agent_or_model_invocation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclaredReflectionLaw {
    /// The law's file, relative to the project's root.
    pub path: String,
    pub law: ReflectionLaw,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnreadableReflectionLaw {
    pub path: String,
    pub error: String,
}

/// Read every `*.json` file directly in `<root>/.aikit/reflection-laws/` as
/// one `ReflectionLaw`, in path order. A project with no such directory
/// declares no law; a file that cannot be read whole, is not a law, or names
/// an id an earlier file already declared stands as unreadable with why.
pub fn declared_reflection_laws(root: &Path) -> aikit_core::Result<ReflectionLawListing> {
    let dir = root.join(REFLECTION_LAWS_DIR);
    let mut names = Vec::new();
    match std::fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|error| unreadable_dir(&dir, &error))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".json") && !name.starts_with('.') {
                    names.push(name);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(unreadable_dir(&dir, &error)),
    }
    names.sort();
    if names.len() > REFLECTION_LAW_FILES {
        return Err(AikitError::new(
            "knowledge.reflection_laws_unreadable",
            format!(
                "{REFLECTION_LAWS_DIR} holds {} law files; one listing reads at most {REFLECTION_LAW_FILES}",
                names.len()
            ),
        ));
    }
    let mut laws = Vec::new();
    let mut unreadable = Vec::new();
    let mut declared: BTreeMap<String, String> = BTreeMap::new();
    for name in names {
        let path = format!("{REFLECTION_LAWS_DIR}/{name}");
        let file = dir.join(&name);
        let law = std::fs::metadata(&file)
            .map_err(|error| error.to_string())
            .and_then(|meta| {
                if !meta.is_file() {
                    Err("not a file".to_string())
                } else if meta.len() > REFLECTION_LAW_BYTES {
                    Err(format!(
                        "{} bytes; a law file is read whole only up to {REFLECTION_LAW_BYTES}",
                        meta.len()
                    ))
                } else {
                    std::fs::read(&file).map_err(|error| error.to_string())
                }
            })
            .and_then(|bytes| {
                serde_json::from_slice::<ReflectionLaw>(&bytes)
                    .map_err(|error| format!("not a ReflectionLaw: {error}"))
            })
            .and_then(|law| {
                if law.id.trim().is_empty() || law.id != law.id.trim() {
                    Err("a law's id is empty or padded".into())
                } else if let Some(first) = declared.get(&law.id) {
                    Err(format!("law {} is already declared by {first}", law.id))
                } else {
                    Ok(law)
                }
            });
        match law {
            Ok(law) => {
                declared.insert(law.id.clone(), path.clone());
                laws.push(DeclaredReflectionLaw { path, law });
            }
            Err(error) => unreadable.push(UnreadableReflectionLaw { path, error }),
        }
    }
    Ok(ReflectionLawListing {
        version: REFLECTION_LAWS_VERSION.into(),
        declared_in: REFLECTION_LAWS_DIR.into(),
        laws,
        unreadable,
        automatic_agent_or_model_invocation: false,
    })
}

fn unreadable_dir(dir: &Path, error: &std::io::Error) -> AikitError {
    AikitError::new(
        "knowledge.reflection_laws_unreadable",
        format!("cannot read {}: {error}", dir.display()),
    )
}

impl Service {
    pub fn knowledge_impact(
        &self,
        horizon: &KnowledgeChangeHorizon,
        readings: &[ResourceRef],
        max_depth: usize,
        max_affected: usize,
    ) -> aikit_core::Result<KnowledgeImpactReading> {
        self.with_knowledge(|runtime, _| {
            let mut objects = Vec::new();
            if let Some(index) = runtime.wiki_index() {
                for resource in index.discover() {
                    if let Some(object) = index.resolve(&resource) {
                        objects.push(object);
                    }
                }
            }
            let (source_dependencies, resource_dependencies) = wiki_living_dependencies(&objects)?;
            let mut impact = deterministic_transitive_knowledge_impact(
                horizon,
                &source_dependencies,
                &resource_dependencies,
                max_depth,
                max_affected,
            )?;
            if !readings.is_empty() {
                let wanted = readings.iter().collect::<BTreeSet<_>>();
                impact
                    .direct
                    .affected
                    .retain(|row| wanted.contains(&row.resource));
                impact
                    .transitive
                    .retain(|row| wanted.contains(&row.resource));
                impact.paths.retain(|path| wanted.contains(&path.resource));
                impact
                    .pending_integration
                    .retain(|resource| wanted.contains(resource));
            }
            Ok(KnowledgeImpactReading {
                version: KNOWLEDGE_IMPACT_READING_VERSION.into(),
                readings: readings.to_vec(),
                wiki_objects: objects.len(),
                source_dependencies: source_dependencies.len(),
                resource_dependencies: resource_dependencies.len(),
                impact,
                automatic_agent_or_model_invocation: false,
            })
        })
    }

    /// The reflection laws this invocation's project declares (its root as
    /// the context resolved it). Reads no ProjectMap: each law's verdict is
    /// `reflection_verdict`'s.
    pub fn reflection_laws(&self) -> aikit_core::Result<ReflectionLawListing> {
        let root = self.descriptor.project_root.as_deref().ok_or_else(|| {
            AikitError::new(
                "knowledge.reflection_laws_no_project",
                format!(
                    "no project is resolved here, so no {REFLECTION_LAWS_DIR} is read; run inside a project"
                ),
            )
        })?;
        declared_reflection_laws(root)
    }

    pub fn reflection_verdict(&self, law: &ReflectionLaw) -> aikit_core::Result<ReflectionVerdict> {
        self.with_knowledge(|runtime, _| {
            let map = runtime.project_map();
            Ok(ReflectionVerdict {
                version: REFLECTION_VERDICT_VERSION.into(),
                law: law.id.clone(),
                map_bindings: map.bindings().len(),
                verification: verify_reflection_law(map, law),
                automatic_agent_or_model_invocation: false,
            })
        })
    }
}
