//! Content drift between canonical skill sources and their projected copies.
//!
//! The harness-visible skill copies are AIKit projections: symlinks from the
//! harness roots into `<home>/sources/<id>/snapshots/<digest>/registry/...`
//! payloads. Those payloads pin the bytes a snapshot captured; the canonical
//! source directory keeps living. Set membership (`aikit diff`'s would_add /
//! would_drop) cannot see that drift — this module can, read-only.
//!
//! Detection anchors on the projected side: every context's `current`
//! generation is walked for symlinks that resolve into a source snapshot
//! `capsules/skill/.../payload`. Each unique payload is then compared, byte by
//! byte, against the canonical directory the source spec names. All contexts
//! are walked because harness visibility is anchored per context (a harness may
//! symlink through a context the invoking cwd never resolves to) while drift is
//! a property of the projected bytes, not of the cwd.
//!
//! Only directory sources are compared: a Git source's canonical tree is pinned
//! to an exact revision and a Central source's tree is owner-mediated (its
//! staleness is caught by the owner-revision gate at generation publication).
//! A projected copy whose payload cannot be traced to a live directory source
//! keeps today's silence — no new noise. A directory source whose canonical
//! skill directory is gone is drift too, reported honestly as
//! `canonical_missing`.
//!
//! Read-only by contract: nothing here repairs. Repair is the existing
//! sync -> promote -> apply cycle's job.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use aikit_core::Result;
use aikit_store::home::AikitHome;

use crate::skill_sources::{source_spec, SourceKind};

/// Why a projected copy is drifting from its canonical source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    /// Canonical bytes exist and differ from the projected bytes.
    ContentDrift,
    /// The canonical skill directory is gone; the projection is orphaned.
    CanonicalMissing,
}

impl DriftKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ContentDrift => "content_drift",
            Self::CanonicalMissing => "canonical_missing",
        }
    }
}

/// Which side moved after the projection was taken, judged by file mtimes.
/// `undetermined` is the honest answer when the clock cannot separate them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftDirection {
    CanonicalNewer,
    ProjectedNewer,
    Undetermined,
}

impl DriftDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CanonicalNewer => "canonical-newer",
            Self::ProjectedNewer => "projected-newer",
            Self::Undetermined => "undetermined",
        }
    }
}

/// One drifted skill: the canonical location, the harness-visible projected
/// copy, and what differs. Copies of the same skill are aggregated: the entry
/// names the most recently materialised copy and counts the rest.
#[derive(Debug, Clone, Serialize)]
pub struct DriftEntry {
    /// The capsule ref, e.g. `skill/personal/central-placement`.
    pub skill: String,
    pub kind: DriftKind,
    /// The canonical skill directory (or the path it would occupy).
    pub source: PathBuf,
    /// A harness-visible projected copy (the projection symlink path).
    pub projected: PathBuf,
    /// The context whose projection drifts, when the projected path names one.
    pub context_id: Option<String>,
    /// The exact native repair for this entry: re-materialise the owning
    /// context from the current active sources. Absent when the projected
    /// path is not a context projection.
    pub repair: Option<String>,
    /// How many harness-visible copies of this skill drift in this way.
    pub drifted_copies: usize,
    pub direction: DriftDirection,
    /// Files that differ (relative paths), for content drift.
    pub differing_files: Vec<String>,
}

impl DriftEntry {
    /// The one-line reading `aikit diff` reports:
    /// `content_drift: <skill-ref> (<source path> vs <projected path>, <direction>)`.
    /// When several copies drift, the count rides along; when the owning
    /// context is known, so does the repair.
    pub fn summary(&self) -> String {
        let copies = match self.drifted_copies {
            0 | 1 => String::new(),
            n => format!(" — {n} drifted copies"),
        };
        let repair = self
            .repair
            .as_deref()
            .map(|hint| format!("; repair: {hint}"))
            .unwrap_or_default();
        format!(
            "{}: {} ({} vs {}, {}{}){}",
            self.kind.as_str(),
            self.skill,
            self.source.display(),
            self.projected.display(),
            match self.kind {
                DriftKind::CanonicalMissing => "canonical gone",
                DriftKind::ContentDrift => self.direction.as_str(),
            },
            copies,
            repair
        )
    }
}

/// The context id owning a projected path, e.g.
/// `.../state/contexts/ctx_X/current/projections/...` → `ctx_X`.
fn context_id_of(projected: &Path) -> Option<String> {
    let mut components = projected.components();
    while let Some(component) = components.next() {
        if component.as_os_str() == "contexts" {
            return components.next()?.as_os_str().to_str().map(String::from);
        }
    }
    None
}

/// The native repair command for a drifted copy: re-materialise its owning
/// context from the current active sources. The project root comes from the
/// context's current resolution lock, so the regeneration resolves the same
/// declarations the context was created with.
fn repair_hint(projected: &Path) -> Option<String> {
    let context_id = context_id_of(projected)?;
    let mut context_dir = PathBuf::new();
    for component in projected.components() {
        context_dir.push(component);
        if component.as_os_str() == "contexts" {
            continue;
        }
        if context_dir
            .file_name()
            .is_some_and(|name| name.to_str() == Some(context_id.as_str()))
        {
            break;
        }
    }
    let lock = fs::read_to_string(context_dir.join("current").join("resolution.lock.toml")).ok();
    let project_root = lock.and_then(|lock| {
        lock.lines()
            .find(|line| line.trim_start().starts_with("project_root"))
            .and_then(|line| line.split('"').nth(1))
            .map(String::from)
    });
    Some(match project_root {
        Some(root) => format!("AIKIT_CONTEXT_ID={context_id} aikit apply -C {root}"),
        None => format!("AIKIT_CONTEXT_ID={context_id} aikit apply"),
    })
}

/// A projected skill payload traced back to its source, before comparison.
#[derive(Debug)]
struct ProjectedPayload {
    source_id: String,
    /// The skill's path inside the source root, e.g. `central-placement`.
    source_path: String,
    skill_ref: String,
    payload: PathBuf,
    /// One harness-visible projection path (the first in sorted walk order;
    /// `detect` later prefers the most recently materialised copy).
    projected: PathBuf,
}

/// Detect content drift for every projected skill in the home. Read-only.
///
/// One entry per (skill, drift kind), however many harness-visible copies
/// drift: the same skill projected from several generations is one fact about
/// that skill, named once. `projected` carries the most recently materialised
/// copy and `drifted_copies` counts the rest.
pub fn detect(home: &AikitHome) -> Result<Vec<DriftEntry>> {
    let mut payloads: BTreeMap<PathBuf, ProjectedPayload> = BTreeMap::new();
    walk_context_projections(home, &mut payloads)?;

    // Compare first, aggregate second: each drifted (skill, kind) collapses to
    // one entry whose projected path names the newest drifting copy.
    struct DriftedCopy {
        projected: PathBuf,
        payload: PathBuf,
        materialised: Option<SystemTime>,
        differing: BTreeSet<String>,
    }
    let mut drifted: BTreeMap<(String, DriftKind), (PathBuf, Vec<DriftedCopy>)> = BTreeMap::new();
    for projected in payloads.into_values() {
        let Ok(spec) = source_spec(home, &projected.source_id) else {
            // The source is gone or unreadable: no reachable canonical
            // counterpart, so today's silence stands.
            continue;
        };
        let SourceKind::Directory { path, .. } = &spec.kind else {
            // Git pins its revision; Central is owner-mediated. Neither has
            // live canonical bytes to compare here.
            continue;
        };
        let canonical = path.join(&projected.source_path);
        let (kind, differing) = if !canonical.is_dir() {
            (DriftKind::CanonicalMissing, BTreeSet::<String>::new())
        } else {
            let differing = diff_trees(&canonical, &projected.payload);
            if differing.is_empty() {
                continue;
            }
            (DriftKind::ContentDrift, differing.into_iter().collect())
        };
        let entry = drifted
            .entry((projected.skill_ref.clone(), kind))
            .or_insert_with(|| (canonical, Vec::new()));
        let materialised = fs::metadata(&projected.payload)
            .and_then(|meta| meta.modified())
            .ok();
        entry.1.push(DriftedCopy {
            projected: projected.projected,
            payload: projected.payload,
            materialised,
            differing,
        });
    }

    let mut entries = Vec::new();
    for ((skill, kind), (source, mut copies)) in drifted {
        // The most recently materialised copy is the one most likely live;
        // ties (and clocks that cannot say) fall back to the sorted-first path.
        copies.sort_by(|left, right| {
            right
                .materialised
                .cmp(&left.materialised)
                .then_with(|| left.projected.cmp(&right.projected))
        });
        let newest = &copies[0];
        let direction = if kind == DriftKind::CanonicalMissing {
            DriftDirection::Undetermined
        } else {
            compare_direction(&source, &newest.payload)
        };
        let mut differing: Vec<String> = copies
            .iter()
            .flat_map(|copy| copy.differing.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if kind == DriftKind::CanonicalMissing {
            differing.clear();
        }
        let projected = newest.projected.clone();
        entries.push(DriftEntry {
            skill,
            kind,
            source,
            context_id: context_id_of(&projected),
            repair: repair_hint(&projected),
            projected,
            direction,
            differing_files: differing,
            drifted_copies: copies.len(),
        });
    }
    Ok(entries)
}

/// The drift summaries `aikit status` warns about, as plain lines.
pub fn warnings(entries: &[DriftEntry]) -> Vec<String> {
    let count = entries.len();
    if count == 0 {
        return Vec::new();
    }
    let (noun, verb, possessive) = if count == 1 {
        ("skill", "differs", "its")
    } else {
        ("skills", "differ", "their")
    };
    vec![format!(
        "{count} projected {noun} {verb} from {possessive} canonical sources; run `aikit diff` for the named entries"
    )]
}

/// Walk every context's `current/projections` tree, collecting skill payloads
/// the projections symlink into, deduplicated by payload path.
fn walk_context_projections(
    home: &AikitHome,
    payloads: &mut BTreeMap<PathBuf, ProjectedPayload>,
) -> Result<()> {
    let contexts = home.contexts();
    let entries = match fs::read_dir(&contexts) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(aikit_core::AikitError::new(
                "projection.drift_unreadable",
                format!("could not read {}: {error}", contexts.display()),
            ))
        }
    };
    let mut context_dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            aikit_core::AikitError::new(
                "projection.drift_unreadable",
                format!("could not read {}: {error}", contexts.display()),
            )
        })?;
        let path = entry.path();
        if path.is_dir() {
            context_dirs.push(path);
        }
    }
    context_dirs.sort();
    for context_dir in context_dirs {
        // A context with no materialised generation simply has nothing to say.
        let projections = context_dir.join("current").join("projections");
        if projections.is_dir() {
            walk_projections_tree(&projections, home, payloads)?;
        }
    }
    Ok(())
}

/// Recursively walk one generation's projections tree. Symlinks are resolved
/// and (when they name a skill payload) recorded; only real directories are
/// recursed into, so a projected payload is never walked as part of the tree.
fn walk_projections_tree(
    dir: &Path,
    home: &AikitHome,
    payloads: &mut BTreeMap<PathBuf, ProjectedPayload>,
) -> Result<()> {
    let mut entries: Vec<PathBuf> = list_sorted(dir)?;
    // The order of visits decides which harness-visible path an entry reports;
    // sorting keeps that choice deterministic across machines.
    entries.sort();
    for entry in entries {
        let Ok(meta) = fs::symlink_metadata(&entry) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            if let Some(projected) = trace_payload(&entry, home) {
                payloads
                    .entry(projected.payload.clone())
                    .or_insert(projected);
            }
        } else if meta.is_dir() {
            walk_projections_tree(&entry, home, payloads)?;
        }
    }
    Ok(())
}

/// If `link` is a projection symlink into a skill snapshot payload, resolve it
/// to (`source id`, `source path`, capsule ref, payload path).
fn trace_payload(link: &Path, home: &AikitHome) -> Option<ProjectedPayload> {
    let target = fs::canonicalize(link).ok()?;
    // The target is fully resolved, so the sources root must be resolved too —
    // on macOS, /var versus /private/var would otherwise hide every payload.
    let sources_root = home.root().join("sources");
    let sources_root = fs::canonicalize(&sources_root).unwrap_or(sources_root);
    let rest = target.strip_prefix(&sources_root).ok()?;
    // <source>/snapshots/<digest>/registry/capsules/<kind>/<tail...>/payload,
    // positionally: source id, marker, content-addressed digest, marker,
    // marker, capsule kind, tail, payload.
    let parts: Vec<&str> = rest
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<&str>>>()?;
    let [source_id, snapshots, _digest, registry, capsules, kind, tail @ ..] = parts.as_slice()
    else {
        return None;
    };
    if *snapshots != "snapshots" || *registry != "registry" {
        return None;
    }
    if *capsules != "capsules" || *kind != "skill" {
        return None;
    }
    // The capsule id is `skill/<source>/<path...>`, so the tail repeats the
    // source id as its namespace and ends at the payload directory:
    // tail = [<source id>, ...source path..., "payload"].
    let last = tail.last()?;
    if *last != "payload" || tail.len() < 3 {
        return None;
    }
    let namespace = &tail[0];
    if namespace != source_id {
        return None;
    }
    let source_path = tail[1..tail.len() - 1].join("/");
    Some(ProjectedPayload {
        skill_ref: format!("skill/{source_id}/{source_path}"),
        source_id: (*source_id).to_owned(),
        source_path,
        payload: target,
        projected: link.to_path_buf(),
    })
}

/// Files that differ between the canonical directory and the projected copy,
/// as sorted relative paths: content differences, and files present on only
/// one side.
fn diff_trees(canonical: &Path, projected: &Path) -> Vec<String> {
    let left = collect_files(canonical);
    let right = collect_files(projected);
    let mut differing = BTreeSet::new();
    for (relative, bytes) in &left {
        match right.get(relative) {
            Some(other) if other == bytes => {}
            _ => {
                differing.insert(relative.clone());
            }
        }
    }
    for relative in right.keys() {
        if !left.contains_key(relative) {
            differing.insert(relative.clone());
        }
    }
    differing.into_iter().collect()
}

/// Every file under `root` as `relative path -> bytes`. Unreadable files are
/// treated as differing-by-construction (absent), never as a crash: drift
/// detection must not fail because one file could not be opened.
fn collect_files(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => stack.push(path),
                Ok(meta) if meta.is_file() => {
                    let relative = path
                        .strip_prefix(root)
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default();
                    let bytes = fs::read(&path).ok();
                    files.insert(relative, bytes);
                }
                _ => {}
            }
        }
    }
    files
}

/// Judge which side moved after the projection was taken, by newest mtime on
/// each side. Equality or clock trouble answers `undetermined`.
fn compare_direction(canonical: &Path, projected: &Path) -> DriftDirection {
    match (newest_mtime(canonical), newest_mtime(projected)) {
        (Some(canonical), Some(projected)) => match canonical.cmp(&projected) {
            std::cmp::Ordering::Greater => DriftDirection::CanonicalNewer,
            std::cmp::Ordering::Less => DriftDirection::ProjectedNewer,
            std::cmp::Ordering::Equal => DriftDirection::Undetermined,
        },
        _ => DriftDirection::Undetermined,
    }
}

fn newest_mtime(root: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                let Ok(modified) = meta.modified() else {
                    continue;
                };
                if newest.is_none_or(|current| modified > current) {
                    newest = Some(modified);
                }
            }
        }
    }
    newest
}

fn list_sorted(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(dir).map_err(|error| {
        aikit_core::AikitError::new(
            "projection.drift_unreadable",
            format!("could not read {}: {error}", dir.display()),
        )
    })?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            aikit_core::AikitError::new(
                "projection.drift_unreadable",
                format!("could not read {}: {error}", dir.display()),
            )
        })?;
        out.push(entry.path());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn identical_trees_report_no_differing_files() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("canonical");
        let right = temp.path().join("payload");
        write(&left.join("SKILL.md"), "same");
        write(&left.join("refs/a.md"), "same");
        write(&right.join("SKILL.md"), "same");
        write(&right.join("refs/a.md"), "same");
        assert!(diff_trees(&left, &right).is_empty());
    }

    #[test]
    fn edited_added_and_removed_files_are_all_named() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("canonical");
        let right = temp.path().join("payload");
        write(&left.join("SKILL.md"), "edited");
        write(&left.join("new.md"), "added canonically");
        write(&left.join("kept.md"), "same");
        write(&right.join("SKILL.md"), "original");
        write(&right.join("dropped.md"), "removed canonically");
        write(&right.join("kept.md"), "same");
        let differing = diff_trees(&left, &right);
        assert_eq!(
            differing,
            vec!["SKILL.md", "dropped.md", "new.md"],
            "differing files must be named and sorted"
        );
    }

    #[test]
    fn summaries_carry_both_paths_and_direction() {
        let entry = DriftEntry {
            skill: "skill/personal/central-placement".into(),
            kind: DriftKind::ContentDrift,
            source: PathBuf::from("/canonical/central-placement"),
            projected: PathBuf::from("/projections/codex/.agents/skills/central-placement"),
            context_id: None,
            repair: None,
            drifted_copies: 1,
            direction: DriftDirection::CanonicalNewer,
            differing_files: vec!["SKILL.md".into()],
        };
        let summary = entry.summary();
        assert!(summary.starts_with("content_drift: skill/personal/central-placement ("));
        assert!(summary.contains("/canonical/central-placement vs "));
        assert!(summary.ends_with(", canonical-newer)"));

        let mut many = entry.clone();
        many.drifted_copies = 4;
        assert!(many
            .summary()
            .ends_with(", canonical-newer — 4 drifted copies)"));

        let missing = DriftEntry {
            kind: DriftKind::CanonicalMissing,
            direction: DriftDirection::Undetermined,
            differing_files: Vec::new(),
            ..entry
        };
        assert!(missing.summary().starts_with("canonical_missing: "));
        assert!(missing.summary().contains(", canonical gone)"));
    }

    #[test]
    fn direction_names_which_side_the_clock_saw_last() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("canonical");
        let payload = temp.path().join("payload");
        // The payload is the snapshot: taken first, edited ground second.
        write(&payload.join("SKILL.md"), "projected");
        write(&canonical.join("SKILL.md"), "canonical");
        // Program order decides on every real filesystem: the later write has
        // the later mtime.
        assert_eq!(
            compare_direction(&canonical, &payload),
            DriftDirection::CanonicalNewer
        );
        write(&payload.join("SKILL.md"), "projected again");
        assert_eq!(
            compare_direction(&canonical, &payload),
            DriftDirection::ProjectedNewer
        );
    }

    #[test]
    fn drift_entries_name_their_context_and_native_repair() {
        let temp = tempfile::tempdir().unwrap();
        let ctx_dir = temp
            .path()
            .join("state/contexts/ctx_TESTCONTEXT000000000000");
        let projected =
            ctx_dir.join("current/projections/codex/.agents/skills/central-placement/SKILL.md");
        write(
            &ctx_dir.join("current/resolution.lock.toml"),
            "[context]\ncontext_id = \"ctx_TESTCONTEXT000000000000\"\nproject_root = \"/Users/admin/Central/Work/O-I\"\n",
        );
        assert_eq!(
            context_id_of(&projected).as_deref(),
            Some("ctx_TESTCONTEXT000000000000")
        );
        let hint = repair_hint(&projected).expect("a context projection has a repair");
        assert!(hint.contains("AIKIT_CONTEXT_ID=ctx_TESTCONTEXT000000000000"));
        assert!(hint.contains("aikit apply -C /Users/admin/Central/Work/O-I"));
        // A path outside any context carries no repair — no invented command.
        assert_eq!(context_id_of(Path::new("/usr/local/share/skill")), None);
        assert_eq!(repair_hint(Path::new("/usr/local/share/skill")), None);
    }

    #[test]
    fn status_warning_names_the_count_and_points_at_diff() {
        assert!(warnings(&[]).is_empty());
        let one = DriftEntry {
            skill: "skill/personal/a".into(),
            kind: DriftKind::ContentDrift,
            source: PathBuf::from("/a"),
            projected: PathBuf::from("/p/a"),
            context_id: None,
            repair: None,
            drifted_copies: 1,
            direction: DriftDirection::CanonicalNewer,
            differing_files: vec![],
        };
        let two = DriftEntry {
            skill: "skill/personal/b".into(),
            ..one.clone()
        };
        let lines = warnings(&[one.clone(), two]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("2 projected skills differ"));
        assert!(lines[0].contains("`aikit diff`"));
        let lines = warnings(&[one]);
        assert!(lines[0].starts_with("1 projected skill differs"));
    }

    #[test]
    fn projection_symlinks_trace_back_to_their_snapshot_payload() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let payload = home.root().join(
            "sources/canon/snapshots/deadbeef/registry/capsules/skill/canon/nested/name/payload",
        );
        write(&payload.join("SKILL.md"), "snapshotted");
        let link = temp
            .path()
            .join("ctx/current/projections/codex/.agents/skills/name");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&payload, &link).unwrap();

        let traced = trace_payload(&link, &home).expect("a skill projection must trace");
        assert_eq!(traced.source_id, "canon");
        assert_eq!(traced.source_path, "nested/name");
        assert_eq!(traced.skill_ref, "skill/canon/nested/name");
        // The resolved target is canonical (on macOS, /var resolves to
        // /private/var), so compare against the canonical form.
        assert_eq!(traced.payload, fs::canonicalize(&payload).unwrap());
        assert_eq!(traced.projected, link);

        // A payload outside the sources tree is nobody's projection.
        let foreign = temp.path().join("elsewhere/payload");
        write(&foreign.join("SKILL.md"), "foreign");
        let foreign_link = temp
            .path()
            .join("ctx/current/projections/codex/.agents/skills/foreign");
        fs::create_dir_all(foreign_link.parent().unwrap()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&foreign, &foreign_link).unwrap();
        assert!(trace_payload(&foreign_link, &home).is_none());
    }
}
