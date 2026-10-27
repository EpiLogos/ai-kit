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
//! already wired into complete (adding only absent links, through a reversible
//! Procedure) and turns every other hand-off failure into a warning that names
//! the skill and the one-line way out.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use aikit_core::capsule::{ControlStanding, Kind};
use aikit_core::resolve::UnavailableReason;

use crate::app::Service;

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

pub fn settle(service: &Service) -> Settlement {
    let mut settlement = Settlement::default();
    let home_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let current = service.descriptor().context_id.to_string();

    for (_, root) in crate::foreign::roots_for(&home_dir, service.invocation_cwd()) {
        for (context, tree) in wirings(&root) {
            if context != current {
                settlement.warnings.push(format!(
                    "skill-reach: {} resolves through {context}, not this context ({current}); \
                     this apply did not refresh it — run `AIKIT_CONTEXT_ID={context} aikit apply`",
                    root.display()
                ));
                continue;
            }
            let mut wired = WiredRoot {
                root: root.display().to_string(),
                context: context.clone(),
                state: "satisfied",
                linked: 0,
                procedure: None,
                undo: None,
                shadowed: shadowed(&root, &tree),
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

    // The two hand-offs before the generation: enabled-but-withheld, and
    // authored in a Control-ground spool but enabled nowhere.
    if let Ok(view) = aikit_store::generation::read_lock(&service.context_projection_root()) {
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
        if !withheld.is_empty() {
            settlement.warnings.push(format!(
                "skill-reach: {} enabled skills are withheld and reach no agent: {}",
                withheld.len(),
                named(&withheld)
            ));
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
        if !unassigned.is_empty() {
            settlement.warnings.push(format!(
                "skill-reach: {} skills authored in a Control spool are enabled nowhere, so no agent has them: {} \
                 — `aikit enable <id> --scope user`, or `aikit disable <id>` to decide against it",
                unassigned.len(),
                named(&unassigned)
            ));
        }
    }
    settlement
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
