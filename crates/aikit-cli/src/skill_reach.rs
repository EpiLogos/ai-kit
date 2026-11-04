//! Does a skill that is in the spool actually reach an agent?
//!
//! A skill passes through four hand-offs before a harness lists it: authored in
//! a spool, enabled in a scope, projected into a generation, and linked into the
//! harness root. Each hand-off used to fail silently — an apply receipt said
//! success while a new skill sat unenabled, withheld for want of a manifest,
//! generated but unlinked, or applied into a context the harness root does not
//! resolve through (AIKit#299 left this as a documented manual step).
//!
//! `settle` runs after every apply. It keeps the harness roots a generation is
//! already wired into complete (adding absent links through a reversible
//! Procedure, and removing links whose skill the generation no longer carries)
//! and turns every other hand-off failure into a warning that names the skill
//! and the one-line way out.
//!
//! The user-level roots (`~/.claude/skills`, `~/.agents/skills`) belong to the
//! user scope alone: they are wired to the user-scope context, which resolves
//! the global profile and nothing else, so no project or session layer can leak
//! into every session the user starts.
//!
//! `check` is the delivery check (`aikit skill reach`): it compares what the
//! Control spools author, and what each catalogued skill set names, with what
//! each harness root a session reads actually holds, one copy each, and
//! reports every gap with its way out. It reads only:
//! settling is `apply`'s job, and a check that repaired what it found would
//! hide the hand-off that failed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use aikit_core::capsule::{ControlStanding, Kind};
use aikit_core::platform::TargetId;
use aikit_core::resolve::{ResolvedView, UnavailableReason};
use aikit_core::skillset::SkillSet;
use aikit_core::ContextId;
use aikit_store::home::AikitHome;

use crate::app::{Service, USER_SCOPE_CONTEXT};

/// How many names a warning spells out before it counts the rest.
const NAMED: usize = 8;

/// One harness root's standing against the generation it is wired to.
#[derive(Debug, Clone, Serialize)]
pub struct WiredRoot {
    pub root: String,
    pub context: String,
    pub state: &'static str,
    pub linked: usize,
    pub procedure: Option<String>,
    pub undo: Option<String>,
    pub shadowed: Vec<String>,
    /// Links removed because the generation no longer carries the skill.
    pub pruned: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Default)]
pub struct Settlement {
    pub roots: Vec<WiredRoot>,
    pub warnings: Vec<String>,
}

/// The context a link target projects through, and the skills tree it sits in:
/// `…/contexts/<ctx>/current/projections/<harness>/<…>/skills/<name>`.
fn wired_target(target: &Path) -> Option<(String, PathBuf)> {
    let parts: Vec<Component<'_>> = target.components().collect();
    let at = parts.iter().position(|c| c.as_os_str() == "contexts")?;
    let context = parts
        .get(at + 1)?
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    if parts.get(at + 2)?.as_os_str() != "current"
        || parts.get(at + 3)?.as_os_str() != "projections"
    {
        return None;
    }
    Some((context, target.parent()?.to_path_buf()))
}

/// Each context a root already links into, with the generation tree it uses.
fn wirings(root: &Path) -> BTreeMap<String, PathBuf> {
    let mut found = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        if let Some((context, tree)) = wired_target(&target) {
            found.entry(context).or_insert(tree);
        }
    }
    found
}

fn named(items: &[String]) -> String {
    let shown = items
        .iter()
        .take(NAMED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    match items.len().saturating_sub(NAMED) {
        0 => shown,
        more => format!("{shown} (+{more} more)"),
    }
}

/// Generated skills whose root entry exists but is not this generation's link:
/// a real directory or another link that `apply` will not overwrite.
fn shadowed(root: &Path, tree: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(tree) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let path = root.join(&name);
        let ours =
            path.is_symlink() && std::fs::read_link(&path).ok().as_deref() == Some(&entry.path());
        if (path.exists() || path.is_symlink()) && !ours && entry.path().join("SKILL.md").is_file()
        {
            out.push(name.to_string_lossy().into_owned());
        }
    }
    out.sort();
    out
}

/// Generated skills with nothing at all in the root yet.
fn absent(root: &Path, tree: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(tree) {
        for entry in entries.flatten() {
            let path = root.join(entry.file_name());
            if !path.exists() && !path.is_symlink() && entry.path().join("SKILL.md").is_file() {
                out.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    out
}

/// Links in `root` that point into `tree` at a skill the generation no longer
/// carries. A denied, retired or removed skill leaves one of these behind;
/// leaving it would keep naming a skill the profile has withdrawn.
fn dangling(root: &Path, tree: &Path) -> Vec<String> {
    let mut out = Vec::new();
    // A generation tree that is not there at all is a build that failed, not a
    // profile that withdrew every skill: never read it as licence to prune.
    if !tree.is_dir() {
        return out;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(target) = std::fs::read_link(&path) else {
            continue;
        };
        if target.parent() == Some(tree) && !target.join("SKILL.md").is_file() {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out.sort();
    out
}

/// The generation tree a harness root is fed from.
fn delivery_tree(projection_root: &Path, label: &str) -> Option<PathBuf> {
    match label {
        "@claude" => Some(projection_root.join("projections/claude/.claude/skills")),
        "@agents" | "@codex" => Some(projection_root.join("projections/codex/.agents/skills")),
        _ => None,
    }
}

pub fn settle(service: &Service) -> Settlement {
    let mut settlement = Settlement::default();
    let home_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let current = service.descriptor().context_id.to_string();

    let user_scope = current == USER_SCOPE_CONTEXT;

    for (label, root) in crate::foreign::roots_for(&home_dir, service.invocation_cwd()) {
        let wiring = wirings(&root);
        if user_scope && root.is_dir() && !wiring.contains_key(&current) {
            if let Some(tree) = delivery_tree(&service.context_projection_root(), &label) {
                let shown = if wiring.is_empty() {
                    "is not wired to the user scope".to_string()
                } else {
                    format!(
                        "is wired through {}, not the user scope",
                        wiring.keys().cloned().collect::<Vec<_>>().join(", ")
                    )
                };
                settlement.warnings.push(format!(
                    "skill-reach: {} {shown}; preview the wiring with `aikit adopt {} --projection {}`",
                    root.display(),
                    root.display(),
                    tree.display()
                ));
            }
            continue;
        }
        for (context, tree) in wiring {
            if context != current {
                // The user scope owns the user-level roots; an apply anywhere
                // else has no business with them.
                if context != USER_SCOPE_CONTEXT {
                    settlement.warnings.push(format!(
                        "skill-reach: {} resolves through {context}, not this context ({current}); \
                         this apply did not refresh it — run `AIKIT_CONTEXT_ID={context} aikit apply`",
                        root.display()
                    ));
                }
                continue;
            }
            let pruned = dangling(&root, &tree);
            for name in &pruned {
                let _ = std::fs::remove_file(root.join(name));
            }
            let mut wired = WiredRoot {
                root: root.display().to_string(),
                context: context.clone(),
                state: "satisfied",
                linked: 0,
                procedure: None,
                undo: None,
                shadowed: shadowed(&root, &tree),
                pruned,
                reason: None,
            };
            match crate::adopt::plan_projection_fill(service.home(), &root, &tree) {
                Ok(adoption) if adoption.capsules.is_empty() => {}
                Ok(adoption) => {
                    let runner = aikit_store::procedure::ProcedureRunner::new(service.home());
                    match runner.run(&adoption.procedure) {
                        // Procedures are content-addressed: a plan identical to one
                        // recorded earlier is skipped, so a link deleted since then
                        // would stay absent. Say so; never report it linked.
                        Ok(outcome)
                            if outcome.already_satisfied && !absent(&root, &tree).is_empty() =>
                        {
                            wired.state = "refused";
                            wired.reason = Some(format!(
                                "the same wiring was recorded as applied before but {} is still absent; \
                                 run `aikit procedure undo {}` and apply again",
                                named(&absent(&root, &tree)),
                                adoption.procedure.id
                            ));
                        }
                        Ok(_) => {
                            wired.state = "linked";
                            wired.linked = adoption.capsules.len();
                            wired.procedure = Some(adoption.procedure.id.to_string());
                            wired.undo =
                                Some(format!("aikit procedure undo {}", adoption.procedure.id));
                        }
                        Err(error) => {
                            wired.state = "refused";
                            wired.reason = Some(error.message().to_string());
                        }
                    }
                }
                Err(error) => {
                    wired.state = "refused";
                    wired.reason = Some(error.message().to_string());
                }
            }
            if wired.state == "refused" {
                settlement.warnings.push(format!(
                    "skill-reach: could not link the generation's new skills into {}: {}",
                    wired.root,
                    wired.reason.as_deref().unwrap_or("reason unavailable")
                ));
            }
            if !wired.shadowed.is_empty() {
                settlement.warnings.push(format!(
                    "skill-reach: {} generated skills are shadowed in {} by entries the generation does not own: {} \
                     — review with `aikit adopt {} --projection {}`",
                    wired.shadowed.len(),
                    wired.root,
                    named(&wired.shadowed),
                    wired.root,
                    tree.display()
                ));
            }
            settlement.roots.push(wired);
        }
    }

    if let Ok(view) = aikit_store::generation::read_lock(&service.context_projection_root()) {
        let (withheld, unassigned) = lock_gaps(&view);
        settlement
            .warnings
            .extend(lock_lines(&withheld, &unassigned));
    }
    settlement
}

/// The two hand-offs before the generation, read from its lock: skills enabled
/// but withheld, and skills authored in a Control-ground spool but enabled
/// nowhere. A retired skill is withheld on purpose; an exact disable or a deny
/// is a decision; neither is a gap.
fn lock_gaps(view: &ResolvedView) -> (Vec<String>, Vec<String>) {
    let mut withheld = Vec::new();
    for (id, reason) in &view.unavailable {
        let enabled = view.declared.get(id).is_some_and(|d| d.enabled);
        if id.kind() == Kind::Skill
            && enabled
            && !matches!(reason, UnavailableReason::RetiredStanding { .. })
        {
            withheld.push(format!("{id} ({})", reason.describe()));
        }
    }
    let mut unassigned = Vec::new();
    for (id, entry) in &view.catalog_index {
        let authored_active = entry
            .control
            .as_ref()
            .is_some_and(|c| c.standing == ControlStanding::Active);
        if entry.kind == Kind::Skill
            && authored_active
            && !view.declared.contains_key(id)
            && !view.active.contains_key(id)
        {
            unassigned.push(id.to_string());
        }
    }
    (withheld, unassigned)
}

fn lock_lines(withheld: &[String], unassigned: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    if !withheld.is_empty() {
        lines.push(format!(
            "skill-reach: {} enabled skills are withheld and reach no agent: {}",
            withheld.len(),
            named(withheld)
        ));
    }
    if !unassigned.is_empty() {
        lines.push(format!(
            "skill-reach: {} skills authored in a Control spool are enabled nowhere, so no agent has them: {} \
             — `aikit enable <id> --scope user`, or `aikit disable <id>` to decide against it",
            unassigned.len(),
            named(unassigned)
        ));
    }
    lines
}

/// The harnesses that read each root. pi reads its own root and the Agent
/// Skills root (pi docs/skills.md); Codex reads the Agent Skills root and its
/// own; Claude Code reads only its own.
const READERS: &[(&str, &[&str])] = &[
    ("@claude", &[TargetId::CLAUDE_CODE]),
    ("@agents", &[TargetId::CODEX, TargetId::PI]),
    ("@codex", &[TargetId::CODEX]),
    ("@hermes", &[TargetId::HERMES]),
    ("@pi", &[TargetId::PI]),
];

/// The projection target a fed root's generation tree is built for.
fn target_of(label: &str) -> Option<&'static str> {
    match label {
        "@claude" => Some(TargetId::CLAUDE_CODE),
        "@agents" | "@codex" => Some(TargetId::CODEX),
        _ => None,
    }
}

/// One harness root against the user scope's generation.
#[derive(Debug, Clone, Serialize)]
pub struct RootReach {
    pub harness: String,
    pub root: String,
    pub read_by: Vec<&'static str>,
    pub present: bool,
    /// The generation tree AIKit feeds this root from; none for a harness
    /// AIKit projects no skills tree for.
    pub fed_from: Option<String>,
    /// `user-scope`, `unwired`, `elsewhere` (wired only through other
    /// contexts), `unfed`, or `no-root`.
    pub wired: &'static str,
    /// The other contexts the root links into.
    pub through: Vec<String>,
    /// Links into the generation that resolve to a skill.
    pub linked: usize,
    /// Active skills the generation tree does not hold.
    pub unprojected: Vec<String>,
    /// Generated skills with nothing in the root.
    pub absent: Vec<String>,
    /// Generated skills whose place in the root another entry holds.
    pub shadowed: Vec<String>,
    /// Links into the generation at a skill it no longer carries.
    pub dangling: Vec<String>,
    /// Any other link to nothing.
    pub broken: Vec<String>,
}

/// One name a harness sees more than once, from different skills.
#[derive(Debug, Clone, Serialize)]
pub struct Duplicate {
    pub harness: &'static str,
    pub name: String,
    pub paths: Vec<String>,
}

/// A member of a catalogued skill set that reaches no agent: no copy of it is
/// active in the user scope, and no profile decided against it.
#[derive(Debug, Clone, Serialize)]
pub struct Unreached {
    pub skill: String,
    /// The sets that name it, by path (`mattpocock/wayfinder-foundation`).
    pub sets: Vec<String>,
}

/// Every skill member of `set` and its nested sets, with the set's path.
fn set_members<'a>(
    set: &'a SkillSet,
    path: &str,
    out: &mut Vec<(String, &'a aikit_core::CapsuleId)>,
) {
    for id in set.members.keys() {
        if id.kind() == Kind::Skill {
            out.push((path.to_string(), id));
        }
    }
    for child in &set.children {
        set_members(child, &format!("{path}/{}", child.name), out);
    }
}

/// Set members no copy reaches. A copy is the member itself, or any active
/// skill exported under its name (one copy each: the owner's `wayfinder`
/// stands for the upstream one). A member declared off — disabled, denied —
/// or retired is a decision; one enabled but withheld is counted as withheld.
fn unreached(sets: &[SkillSet], view: &ResolvedView) -> Vec<Unreached> {
    let exported: BTreeSet<&str> = view
        .active_of_kind(Kind::Skill)
        .into_iter()
        .map(aikit_adapters::clients::agent_skills::effective_export_name)
        .collect();
    let mut members = Vec::new();
    for set in sets {
        set_members(set, &set.name, &mut members);
    }
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (path, id) in members {
        let decided = view.declared.contains_key(id)
            || matches!(
                view.unavailable.get(id),
                Some(UnavailableReason::RetiredStanding { .. })
            );
        if !view.active.contains_key(id) && !exported.contains(id.leaf()) && !decided {
            found.entry(id.to_string()).or_default().insert(path);
        }
    }
    found
        .into_iter()
        .map(|(skill, sets)| Unreached {
            skill,
            sets: sets.into_iter().collect(),
        })
        .collect()
}

/// The delivery check's verdict (`aikit.skill-reach/v1`).
#[derive(Debug, Clone, Serialize)]
pub struct Reach {
    pub schema: &'static str,
    pub context: &'static str,
    /// The generation the roots are compared with; none when it is unreadable.
    pub generation: Option<String>,
    pub verdict: &'static str,
    pub gaps: usize,
    pub roots: Vec<RootReach>,
    pub duplicates: Vec<Duplicate>,
    pub withheld: Vec<String>,
    pub unassigned: Vec<String>,
    /// Members of catalogued skill sets that reach no agent.
    pub unreached: Vec<Unreached>,
    /// One line per kind of gap, with the way out.
    #[serde(skip)]
    pub lines: Vec<String>,
}

/// Links in `root` whose target does not exist.
fn broken(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_symlink() && !path.exists() {
                out.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    out
}

/// Two paths that resolve to the same file, however each is spelled.
fn same_file(a: &Path, b: &Path) -> bool {
    matches!(
        (std::fs::canonicalize(a), std::fs::canonicalize(b)),
        (Ok(a), Ok(b)) if a == b
    )
}

/// Generated skills the root reaches through a link, however it is spelled.
fn linked(root: &Path, tree: &Path) -> usize {
    std::fs::read_dir(tree)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    let path = root.join(entry.file_name());
                    entry.path().join("SKILL.md").is_file()
                        && path.is_symlink()
                        && same_file(&path, &entry.path())
                })
                .count()
        })
        .unwrap_or(0)
}

/// Active skills built for `target` whose export the generation tree lacks.
fn unprojected(view: &ResolvedView, tree: &Path, target: &str) -> Vec<String> {
    let mut out: Vec<String> = view
        .active_of_kind(Kind::Skill)
        .into_iter()
        .filter(|capability| {
            capability.targets.is_empty() || capability.targets.iter().any(|t| t.as_str() == target)
        })
        .filter(|capability| {
            let name = aikit_adapters::clients::agent_skills::effective_export_name(capability);
            !tree.join(name).join("SKILL.md").is_file()
        })
        .map(|capability| capability.id.to_string())
        .collect();
    out.sort();
    out
}

/// The names a root entry answers to: its directory and its declared name.
fn names_of(entry: &Path) -> Vec<String> {
    let mut names = vec![entry
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()];
    if let Some(declared) = std::fs::read_to_string(entry.join("SKILL.md"))
        .ok()
        .and_then(|source| aikit_adapters::clients::agent_skills::parse_frontmatter(&source).ok())
        .and_then(|frontmatter| frontmatter.get("name").cloned())
        .filter(|declared| !declared.is_empty() && !names.contains(declared))
    {
        names.push(declared);
    }
    names
}

/// Names one harness sees from more than one skill across the roots it reads.
/// Two links to the same skill are one copy (pi drops a second path to the
/// same file); two skills under one name are two.
fn duplicates(roots: &[(String, PathBuf)]) -> Vec<Duplicate> {
    let mut harnesses: Vec<&'static str> = READERS
        .iter()
        .flat_map(|(_, readers)| readers.iter().copied())
        .collect();
    harnesses.sort_unstable();
    harnesses.dedup();
    let mut out = Vec::new();
    for harness in harnesses {
        let mut seen: BTreeMap<String, BTreeMap<PathBuf, String>> = BTreeMap::new();
        for (label, root) in roots {
            let reads = READERS
                .iter()
                .any(|(l, readers)| l == label && readers.contains(&harness));
            let Ok(entries) = std::fs::read_dir(root).map(|e| e.flatten().collect::<Vec<_>>())
            else {
                continue;
            };
            if !reads {
                continue;
            }
            for entry in entries {
                let path = entry.path();
                if !path.join("SKILL.md").is_file() {
                    continue;
                }
                let Ok(real) = std::fs::canonicalize(&path) else {
                    continue;
                };
                for name in names_of(&path) {
                    seen.entry(name)
                        .or_default()
                        .entry(real.clone())
                        .or_insert_with(|| path.display().to_string());
                }
            }
        }
        for (name, copies) in seen {
            if copies.len() > 1 {
                out.push(Duplicate {
                    harness,
                    name,
                    paths: copies.into_values().collect(),
                });
            }
        }
    }
    out
}

/// The delivery check. `user_home` holds the harness roots; the generation is
/// the user scope's, which feeds them.
pub fn check(home: &AikitHome, user_home: &Path) -> Reach {
    let context = ContextId::parse(USER_SCOPE_CONTEXT).expect("the user scope is a context id");
    let generation = home.context_dir(&context).join("current");
    let view = aikit_store::generation::read_lock(&generation).ok();
    let mut lines = Vec::new();
    let mut gaps = 0;
    if view.is_none() {
        gaps += 1;
        lines.push(format!(
            "skill-reach: the user scope has no readable generation at {}, so nothing reaches its roots \
             — `aikit apply --user`",
            generation.display()
        ));
    }

    let mut labelled = crate::foreign::default_roots(user_home);
    labelled.push(("@pi".to_string(), user_home.join(".pi/agent/skills")));
    let mut roots = Vec::new();
    for (label, root) in &labelled {
        let tree = delivery_tree(&generation, label);
        let present = root.is_dir();
        let mut reach = RootReach {
            harness: label.clone(),
            root: root.display().to_string(),
            read_by: READERS
                .iter()
                .find(|(l, _)| l == label)
                .map(|(_, readers)| readers.to_vec())
                .unwrap_or_default(),
            present,
            fed_from: tree.as_ref().map(|tree| tree.display().to_string()),
            wired: if !present {
                "no-root"
            } else if tree.is_none() {
                "unfed"
            } else {
                "unwired"
            },
            through: Vec::new(),
            linked: 0,
            unprojected: Vec::new(),
            absent: Vec::new(),
            shadowed: Vec::new(),
            dangling: Vec::new(),
            broken: Vec::new(),
        };
        if !present {
            roots.push(reach);
            continue;
        }
        if let Some(tree) = &tree {
            let wiring = wirings(root);
            reach.through = wiring
                .keys()
                .filter(|context| context.as_str() != USER_SCOPE_CONTEXT)
                .cloned()
                .collect();
            // The tree as the links spell it (a home reached through a
            // symlinked prefix links by its resolved path), as `settle` reads it.
            if let Some(wired) = wiring.get(USER_SCOPE_CONTEXT) {
                reach.wired = "user-scope";
                reach.linked = linked(root, wired);
                reach.absent = absent(root, wired);
                // A link spelled otherwise that reaches the same skill is not
                // in its way.
                reach.shadowed = shadowed(root, wired)
                    .into_iter()
                    .filter(|name| !same_file(&root.join(name), &wired.join(name)))
                    .collect();
                reach.dangling = dangling(root, wired);
                if let (Some(view), Some(target)) = (&view, target_of(label)) {
                    reach.unprojected = unprojected(view, wired, target);
                }
            } else {
                reach.wired = if wiring.is_empty() {
                    "unwired"
                } else {
                    "elsewhere"
                };
                gaps += 1;
                let shown = if wiring.is_empty() {
                    "is not wired to the user scope".to_string()
                } else {
                    format!(
                        "is wired through {}, not the user scope",
                        reach.through.join(", ")
                    )
                };
                lines.push(format!(
                    "skill-reach: {} {shown}, so {} read none of its skills \
                     — preview the wiring with `aikit adopt {} --projection {}`",
                    root.display(),
                    reach.read_by.join(" and "),
                    root.display(),
                    tree.display()
                ));
            }
            if !reach.unprojected.is_empty() {
                lines.push(format!(
                    "skill-reach: {} active skills are missing from {}: {} — `aikit apply --user` rebuilds it",
                    reach.unprojected.len(),
                    tree.display(),
                    named(&reach.unprojected)
                ));
            }
            if !reach.absent.is_empty() {
                lines.push(format!(
                    "skill-reach: {} generated skills have no link in {}: {} — `aikit apply --user` links them",
                    reach.absent.len(),
                    root.display(),
                    named(&reach.absent)
                ));
            }
            if !reach.shadowed.is_empty() {
                lines.push(format!(
                    "skill-reach: {} generated skills are shadowed in {} by entries the generation does not own: {} \
                     — review with `aikit adopt {} --projection {}`",
                    reach.shadowed.len(),
                    root.display(),
                    named(&reach.shadowed),
                    root.display(),
                    tree.display()
                ));
            }
            if !reach.dangling.is_empty() {
                lines.push(format!(
                    "skill-reach: {} links skills the generation no longer carries: {} — `aikit apply --user` prunes them",
                    root.display(),
                    named(&reach.dangling)
                ));
            }
        }
        reach.broken = broken(root)
            .into_iter()
            .filter(|name| !reach.dangling.contains(name))
            .collect();
        if !reach.broken.is_empty() {
            lines.push(format!(
                "skill-reach: {} holds links to nothing: {} — remove them",
                root.display(),
                named(&reach.broken)
            ));
        }
        gaps += reach.unprojected.len()
            + reach.absent.len()
            + reach.shadowed.len()
            + reach.dangling.len()
            + reach.broken.len();
        roots.push(reach);
    }

    let duplicates = duplicates(&labelled);
    for duplicate in &duplicates {
        lines.push(format!(
            "skill-reach: {} sees `{}` more than once: {} — keep one copy",
            duplicate.harness,
            duplicate.name,
            duplicate.paths.join(", ")
        ));
    }
    let (withheld, unassigned) = view.as_ref().map(lock_gaps).unwrap_or_default();
    lines.extend(lock_lines(&withheld, &unassigned));
    gaps += duplicates.len() + withheld.len() + unassigned.len();

    // A set is switched on when every member reaches: "catalogued, but no
    // scope enables it" is a set whose members reach no agent.
    let unreached = match (aikit_store::skillsets::load_all(home), &view) {
        (Ok(sets), Some(view)) => unreached(&sets, view),
        (Ok(_), None) => Vec::new(),
        (Err(error), _) => {
            gaps += 1;
            lines.push(format!(
                "skill-reach: the skill sets under {} could not be read: {}",
                aikit_store::skillsets::root(home).display(),
                error.message()
            ));
            Vec::new()
        }
    };
    if !unreached.is_empty() {
        let shown: Vec<String> = unreached
            .iter()
            .map(|member| format!("{} ({})", member.skill, member.sets.join(", ")))
            .collect();
        lines.push(format!(
            "skill-reach: {} skills in catalogued sets reach no agent, with no copy enabled and no decision against them: {} \
             — `aikit enable <id> --scope user`, or `aikit disable <id>` to decide against it",
            unreached.len(),
            named(&shown)
        ));
    }
    gaps += unreached.len();

    Reach {
        schema: "aikit.skill-reach/v1",
        context: USER_SCOPE_CONTEXT,
        generation: view.as_ref().map(|_| generation.display().to_string()),
        verdict: if gaps == 0 { "complete" } else { "gap" },
        gaps,
        roots,
        duplicates,
        withheld,
        unassigned,
        unreached,
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_target_names_its_context_and_generation_tree() {
        let target = Path::new(
            "/h/.aikit/state/contexts/ctx_X/current/projections/claude/.claude/skills/telos",
        );
        let (context, tree) = wired_target(target).unwrap();
        assert_eq!(context, "ctx_X");
        assert_eq!(
            tree,
            Path::new("/h/.aikit/state/contexts/ctx_X/current/projections/claude/.claude/skills")
        );
    }

    #[test]
    fn a_link_into_authored_ground_is_not_a_wiring() {
        assert!(wired_target(Path::new("/h/Central/Control/user/skills/telos")).is_none());
    }

    #[test]
    fn long_lists_are_counted_not_dumped() {
        let items: Vec<String> = (0..11).map(|n| format!("s{n}")).collect();
        let text = named(&items);
        assert!(text.ends_with("(+3 more)"), "{text}");
        assert!(text.starts_with("s0, s1"), "{text}");
    }
}
