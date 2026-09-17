//! Collate: surveying the skill trees already on a machine and answering the
//! question nothing on it can currently answer — **which version of a skill is
//! actually running, and where.**
//!
//! ## Read-only, always
//!
//! A foreign root is *indexed*, not owned (Spec II §3). Everything here reads and
//! reports; changing a foreign root is Adoption, which is a Procedure with an
//! explicit confirmation. That separation is the whole reason this module can be
//! pointed at somebody's real, working setup without a moment's hesitation.
//!
//! ## Automate the provable, queue the ambiguous
//!
//! Spec II §7 draws the line, and this module implements exactly it:
//!
//! * **identical after normalization** → a duplicate. A dedup, not a decision. The
//!   nineteen byte-identical symlink aliases on the reference machine are this.
//! * **anything else** → a `VersionConflict` for a human. Two live versions of one
//!   skill pack, or six copies with four contents, are ambiguity, and ambiguity is
//!   a human's.
//!
//! Reporting a duplicate as a conflict would bury the conflicts that matter, so the
//! distinction is load-bearing rather than cosmetic.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aikit_adapters::clients::agent_skills;

use aikit_core::Result;
use aikit_store::channel::{Evidence, InboxChannel, InboxItem, InboxKind, NewItem};
use aikit_store::index::Index;

/// A foreign skill root to survey: a label a user recognises, and where it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignRootRef {
    pub label: String,
    pub path: PathBuf,
}

/// One skill, as found in one root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillObservation {
    /// The skill's own name — the thing that collides across roots.
    pub name: String,
    /// Which root it was found in, e.g. `@claude`.
    pub root_label: String,
    pub path: PathBuf,
    /// `version:` from the frontmatter, when the author declared one.
    pub version: Option<String>,
    /// Content hash of the whole skill directory: what makes "the same skill twice"
    /// distinguishable from "two different skills with one name".
    pub content: String,
}

/// Every observation of one name, across every root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameCluster {
    pub name: String,
    pub observations: Vec<SkillObservation>,
}

impl NameCluster {
    /// How many genuinely different contents wear this name.
    pub fn distinct_contents(&self) -> usize {
        let mut hashes: Vec<&str> = self
            .observations
            .iter()
            .map(|o| o.content.as_str())
            .collect();
        hashes.sort_unstable();
        hashes.dedup();
        hashes.len()
    }

    /// Every declared version, sorted and deduplicated.
    pub fn versions(&self) -> Vec<String> {
        let mut versions: Vec<String> = self
            .observations
            .iter()
            .filter_map(|o| o.version.clone())
            .collect();
        versions.sort();
        versions.dedup();
        versions
    }

    /// More than one copy, all byte-identical: a dedup, not a decision.
    pub fn is_duplicate(&self) -> bool {
        self.observations.len() > 1 && self.distinct_contents() == 1
    }

    /// Two or more different things wearing one name. A human decides.
    pub fn is_conflict(&self) -> bool {
        self.distinct_contents() > 1
    }

    /// The roots this name appears in, in order.
    pub fn roots(&self) -> Vec<&str> {
        let mut roots: Vec<&str> = self
            .observations
            .iter()
            .map(|o| o.root_label.as_str())
            .collect();
        roots.dedup();
        roots
    }
}

/// Survey every root, read-only.
///
/// Walks two levels so both the flat layout (`<root>/<skill>/`) and the two-level
/// container layout (`<root>/<category>/<skill>/`) are seen — an indexer that
/// walked one level would miss whole categories (PRIOR-ART-ACTIONS #29).
pub fn survey(roots: &[ForeignRootRef]) -> Vec<SkillObservation> {
    let mut out = Vec::new();
    for root in roots {
        collect(&root.path, &root.label, 0, &mut out);
    }
    // Sorted so a report is stable between runs.
    out.sort_by(|a, b| (&a.name, &a.root_label, &a.path).cmp(&(&b.name, &b.root_label, &b.path)));
    out
}

fn collect(dir: &Path, label: &str, depth: usize, out: &mut Vec<SkillObservation>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // A dead symlink resolves to nothing; there is no skill there to observe.
        if path.is_symlink() && std::fs::metadata(&path).is_err() {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        if path.join(agent_skills::SKILL_FILE).exists() {
            if let Some(observation) = observe(&path, label) {
                out.push(observation);
            }
        }
        // Recurse regardless of leaf-ness: a directory can be both a skill and a
        // container of skills.
        if depth == 0 {
            collect(&path, label, depth + 1, out);
        }
    }
}

/// Read one skill directory into an observation.
fn observe(path: &Path, label: &str) -> Option<SkillObservation> {
    let source = std::fs::read_to_string(path.join(agent_skills::SKILL_FILE)).ok()?;
    let frontmatter = agent_skills::parse_frontmatter(&source).ok()?;
    let name = frontmatter
        .get("name")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        // A skill with no usable name still exists on disk and still collides by
        // directory name, so fall back rather than dropping it from the survey.
        .or_else(|| path.file_name().map(|n| n.to_string_lossy().to_string()))?;

    Some(SkillObservation {
        name,
        root_label: label.to_string(),
        path: path.to_path_buf(),
        version: frontmatter.get("version").map(|v| v.trim().to_string()),
        content: content_hash(path),
    })
}

/// Hash a skill's whole directory: every file's relative path and bytes, sorted.
///
/// Normalizes line endings so a checkout that differs only in CRLF is recognised
/// as the same content rather than reported as a conflict a human must resolve.
fn content_hash(root: &Path) -> String {
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if let Ok(bytes) = std::fs::read(entry.path()) {
            let normalized = match String::from_utf8(bytes.clone()) {
                Ok(text) => text.replace("\r\n", "\n").into_bytes(),
                Err(_) => bytes,
            };
            files.insert(relative, normalized);
        }
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit-skill-content-v1\n");
    for (relative, bytes) in files {
        hasher.update(&(relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    hasher.finalize().to_hex().to_string()
}

/// Group observations by name.
pub fn cluster(observations: Vec<SkillObservation>) -> Vec<NameCluster> {
    let mut by_name: BTreeMap<String, Vec<SkillObservation>> = BTreeMap::new();
    for observation in observations {
        by_name
            .entry(observation.name.clone())
            .or_default()
            .push(observation);
    }
    by_name
        .into_iter()
        .map(|(name, observations)| NameCluster { name, observations })
        .collect()
}

/// File a `VersionConflict` for every genuine ambiguity.
///
/// Duplicates are deliberately **not** filed: a dedup is not a decision, and
/// filling the inbox with them would bury the conflicts that need a human.
/// Publishing is deduplicated on the cluster's own content, so re-collating an
/// unchanged machine returns the existing items rather than nagging.
pub fn report_conflicts(index: &Index, clusters: &[NameCluster]) -> Result<Vec<InboxItem>> {
    let channel = InboxChannel::new(index);
    let mut filed = Vec::new();

    for cluster in clusters.iter().filter(|c| c.is_conflict()) {
        let versions = cluster.versions();
        let mut body = format!(
            "`{}` exists in {} place{} with {} different content{}.\n\n",
            cluster.name,
            cluster.observations.len(),
            if cluster.observations.len() == 1 {
                ""
            } else {
                "s"
            },
            cluster.distinct_contents(),
            if cluster.distinct_contents() == 1 {
                ""
            } else {
                "s"
            },
        );
        for observation in &cluster.observations {
            body.push_str(&format!(
                "- {} — {}{}\n",
                observation.root_label,
                observation.path.display(),
                observation
                    .version
                    .as_ref()
                    .map(|v| format!(" (version {v})"))
                    .unwrap_or_else(|| " (no declared version)".to_string()),
            ));
        }
        body.push_str(
            "\nAIKit resolves the provable and queues the ambiguous: these differ, so the \
             choice is yours. Nothing has been changed — every root above is indexed, not \
             owned.",
        );

        // Dedup on the name plus the exact set of contents, so the item returns
        // unchanged until something on disk actually changes.
        let mut contents: Vec<&str> = cluster
            .observations
            .iter()
            .map(|o| o.content.as_str())
            .collect();
        contents.sort_unstable();
        contents.dedup();
        let dedup = format!("collate:{}:{}", cluster.name, contents.join(","));

        let evidence: Vec<Evidence> = cluster
            .observations
            .iter()
            .map(|o| Evidence::File {
                path: o.path.display().to_string(),
            })
            .collect();

        let title = match versions.len() {
            0 => format!(
                "{} differs across {} roots",
                cluster.name,
                cluster.roots().len()
            ),
            _ => format!("{} is live at {}", cluster.name, versions.join(" and ")),
        };

        filed.push(
            channel.publish(
                NewItem::new(InboxKind::VersionConflict, title, body)
                    .with_evidence(evidence)
                    .deduped_by(dedup),
            )?,
        );
    }

    Ok(filed)
}

/// A summary of a whole collate run, for the CLI's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollateReport {
    pub roots: usize,
    pub skills: usize,
    pub names: usize,
    pub conflicts: usize,
    pub duplicates: usize,
}

/// Survey, cluster and file — the whole read-only pass, summarised.
pub fn collate(
    index: &Index,
    roots: &[ForeignRootRef],
) -> Result<(CollateReport, Vec<NameCluster>)> {
    let observations = survey(roots);
    let skills = observations.len();
    let clusters = cluster(observations);
    report_conflicts(index, &clusters)?;

    let report = CollateReport {
        roots: roots.len(),
        skills,
        names: clusters.len(),
        conflicts: clusters.iter().filter(|c| c.is_conflict()).count(),
        duplicates: clusters.iter().filter(|c| c.is_duplicate()).count(),
    };
    Ok((report, clusters))
}

// ---------------------------------------------------------------------------
// Plugin roots: the layout the version conflicts actually live in
// ---------------------------------------------------------------------------

/// One installed plugin, as declared by its own manifest.
///
/// Plugin caches are where the interesting conflicts hide: a marketplace cache
/// keeps `<plugin>/<version>/` side by side, so two live versions of one plugin
/// coexist with nothing on the machine willing to say which one an agent loads.
/// Reading `plugin.json` / `marketplace.json` as a provenance source is
/// PRIOR-ART-ACTIONS #33 — import, not conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginObservation {
    pub name: String,
    pub version: Option<String>,
    /// The directory the manifest was found beside.
    pub path: PathBuf,
    /// How many skills ship inside it.
    pub skills: usize,
}

/// Path fragments that mark a location as transient rather than live.
///
/// A backup, a temp clone and a `node_modules` tree all legitimately hold older
/// copies — that is what a backup *is*. Counting them as installations would
/// report a hundred "conflicts" that are nobody's problem and bury the handful
/// that are. `contrib/bkmr` already set this precedent for the same reason: its
/// capsules skip `*_backup_*` explicitly rather than globbing every `*.db`.
const TRANSIENT_MARKERS: [&str; 6] = [
    "/.tmp/",
    "-backup-",
    "-clone-",
    "/node_modules/",
    "/.git/",
    "/.Trash/",
];

/// Whether a path is a transient copy rather than a live installation.
pub fn is_transient(path: &Path) -> bool {
    let rendered = path.to_string_lossy();
    TRANSIENT_MARKERS.iter().any(|m| rendered.contains(m))
}

/// The manifest filenames a plugin may declare itself with, across harnesses.
const PLUGIN_MANIFESTS: [&str; 4] = [
    ".claude-plugin/plugin.json",
    ".codex-plugin/plugin.json",
    ".cursor-plugin/plugin.json",
    ".kimi-plugin/plugin.json",
];

/// Find every installed plugin under `roots`, read-only.
///
/// Bounded to `max_depth` directory levels: plugin caches are deep
/// (`cache/<marketplace>/<plugin>/<version>/`) but not unbounded, and a survey
/// that walked a whole home directory would be both slow and wrong.
pub fn survey_plugins(roots: &[PathBuf], max_depth: usize) -> Vec<PluginObservation> {
    let mut out: BTreeMap<PathBuf, PluginObservation> = BTreeMap::new();

    for root in roots {
        for entry in walkdir::WalkDir::new(root)
            .max_depth(max_depth)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            if !entry.file_type().is_dir() {
                continue;
            }
            let dir = entry.path();
            if is_transient(dir) {
                continue;
            }
            for manifest in PLUGIN_MANIFESTS {
                let path = dir.join(manifest);
                if !path.is_file() {
                    continue;
                }
                if let Some(observation) = read_plugin_manifest(&path, dir) {
                    // One plugin may ship several harness manifests; keep one entry
                    // per directory, preferring whichever names a version.
                    out.entry(dir.to_path_buf())
                        .and_modify(|existing| {
                            if existing.version.is_none() && observation.version.is_some() {
                                *existing = observation.clone();
                            }
                        })
                        .or_insert(observation);
                }
            }
        }
    }

    let mut observations: Vec<PluginObservation> = out.into_values().collect();
    observations
        .sort_by(|a, b| (&a.name, &a.version, &a.path).cmp(&(&b.name, &b.version, &b.path)));
    observations
}

fn read_plugin_manifest(path: &Path, dir: &Path) -> Option<PluginObservation> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let name = value.get("name")?.as_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let version = value
        .get("version")
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string());
    Some(PluginObservation {
        name,
        version,
        path: dir.to_path_buf(),
        skills: count_skills(&dir.join("skills")),
    })
}

fn count_skills(skills_dir: &Path) -> usize {
    if !skills_dir.is_dir() {
        return 0;
    }
    walkdir::WalkDir::new(skills_dir)
        .max_depth(3)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_name() == agent_skills::SKILL_FILE)
        .count()
}

/// Every plugin installed at more than one distinct version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginVersionConflict {
    pub name: String,
    pub installations: Vec<PluginObservation>,
}

impl PluginVersionConflict {
    pub fn versions(&self) -> Vec<String> {
        let mut versions: Vec<String> = self
            .installations
            .iter()
            .filter_map(|i| i.version.clone())
            .collect();
        versions.sort();
        versions.dedup();
        versions
    }
}

/// Group plugin observations and keep only those living at two or more versions.
pub fn plugin_conflicts(observations: Vec<PluginObservation>) -> Vec<PluginVersionConflict> {
    let mut by_name: BTreeMap<String, Vec<PluginObservation>> = BTreeMap::new();
    for observation in observations {
        by_name
            .entry(observation.name.clone())
            .or_default()
            .push(observation);
    }
    by_name
        .into_iter()
        .filter_map(|(name, installations)| {
            let mut versions: Vec<&str> = installations
                .iter()
                .filter_map(|i| i.version.as_deref())
                .collect();
            versions.sort_unstable();
            versions.dedup();
            (versions.len() > 1).then_some(PluginVersionConflict {
                name,
                installations,
            })
        })
        .collect()
}

/// File a `VersionConflict` per plugin living at multiple versions.
pub fn report_plugin_conflicts(
    index: &Index,
    conflicts: &[PluginVersionConflict],
) -> Result<Vec<InboxItem>> {
    let channel = InboxChannel::new(index);
    let mut filed = Vec::new();

    for conflict in conflicts {
        let versions = conflict.versions();
        let mut body = format!(
            "The plugin `{}` is installed at {} different versions ({}).\n\n",
            conflict.name,
            versions.len(),
            versions.join(", ")
        );
        for install in &conflict.installations {
            body.push_str(&format!(
                "- {} — {}{}\n",
                install.version.as_deref().unwrap_or("no declared version"),
                install.path.display(),
                if install.skills > 0 {
                    format!(" ({} skills)", install.skills)
                } else {
                    String::new()
                },
            ));
        }
        body.push_str(
            "\nWhich one an agent actually loads depends on which root its harness reads. \
             Nothing has been changed: these locations are indexed, not owned.",
        );

        let dedup = format!("collate:plugin:{}:{}", conflict.name, versions.join(","));
        let evidence: Vec<Evidence> = conflict
            .installations
            .iter()
            .map(|i| Evidence::File {
                path: i.path.display().to_string(),
            })
            .collect();

        filed.push(
            channel.publish(
                NewItem::new(
                    InboxKind::VersionConflict,
                    format!(
                        "{} is installed at {}",
                        conflict.name,
                        versions.join(" and ")
                    ),
                    body,
                )
                .with_evidence(evidence)
                .deduped_by(dedup),
            )?,
        );
    }
    Ok(filed)
}
