//! Building the tree's six roots from the live system.
//!
//! Everything here is a **read**. The tree is a view over the resolved state, the
//! catalogue, the sets on disk, the foreign roots and the inbox — assembling it
//! changes nothing, which is what makes it safe to point at somebody's real
//! machine before they have decided to let AIKit touch anything.
//!
//! The two roots that earn their place are `hooks/` and `registries/`:
//!
//! * `hooks/` shows the **resolved chain in execution order**, which is the one
//!   screen that answers "what actually runs when Claude edits a file" — and
//!   therefore the one that makes hook scripts sitting on disk wired to nothing
//!   visible as the absence they are.
//! * `registries/` counts each foreign root's problems, so a dead symlink is a
//!   finding rather than a surprise months later.

use std::collections::BTreeMap;

use aikit_core::capsule::Kind;
use aikit_core::catalog::Catalog;
use aikit_core::skillset;
use aikit_core::Result;

use aikit_tui::tree::{Node, NodeKind, Root, TreeState};

use crate::app::Service;
use crate::foreign;

/// Assemble the whole tree.
pub fn build(service: &Service) -> Result<TreeState> {
    let roots = vec![
        sets_root(service)?,
        kinds_root(service),
        hooks_root(service),
        contexts_root(service)?,
        registries_root(service),
        inbox_root(service)?,
    ];
    Ok(TreeState::new(roots))
}

/// `sets/` — the folders, each reporting what it would actually project here.
fn sets_root(service: &Service) -> Result<Node> {
    let view = service.resolved();
    let sets = aikit_store::skillsets::load_all(service.home())?;

    let children: Vec<Node> = sets
        .iter()
        .map(|set| {
            let projection = skillset::project(set, view);
            let members: Vec<Node> = set
                .all_members()
                .into_iter()
                .map(|id| {
                    // A member's row says whether it projects, and if not, why —
                    // the set's reply reaches the row a user stands on.
                    let summary = projection
                        .withheld
                        .iter()
                        .find(|w| w.capsule == id)
                        .map(|w| w.reason.describe())
                        .unwrap_or_else(|| "projected".to_string());
                    Node::leaf(NodeKind::Capability { id }, summary)
                })
                .collect();
            Node::branch(
                NodeKind::Set {
                    name: set.name.clone(),
                    observed: !set.provenance.is_writable(),
                },
                projection.summarize(&format!("sets/{}", set.name)),
                members,
            )
        })
        .collect();

    let summary = match children.len() {
        0 => "no sets yet — `aikit set create <name>`, or just mkdir one".to_string(),
        n => format!("{n} set{}", if n == 1 { "" } else { "s" }),
    };
    Ok(Node::branch(NodeKind::Root(Root::Sets), summary, children))
}

/// `kinds/` — everything catalogued, by what it is.
fn kinds_root(service: &Service) -> Node {
    let view = service.resolved();
    let mut by_kind: BTreeMap<Kind, Vec<Node>> = BTreeMap::new();

    for (id, entry) in &view.catalog_index {
        let state = if view.is_active(id) {
            "active"
        } else if view.unavailable.contains_key(id) {
            "unavailable"
        } else {
            "inactive"
        };
        by_kind.entry(entry.kind).or_default().push(Node::leaf(
            NodeKind::Capability { id: id.clone() },
            format!("{state} · {}", entry.description),
        ));
    }

    let total = view.catalog_index.len();
    let children: Vec<Node> = by_kind
        .into_iter()
        .map(|(kind, items)| {
            Node::branch(
                NodeKind::Group {
                    label: kind.as_str().to_string(),
                },
                format!("{}", items.len()),
                items,
            )
        })
        .collect();

    Node::branch(
        NodeKind::Root(Root::Kinds),
        format!("{total} catalogued"),
        children,
    )
}

/// `hooks/` — by event, in dispatch order. The chain, visible.
fn hooks_root(service: &Service) -> Node {
    let chains = aikit_core::hooks::build_chains(service.resolved(), service.snapshot());

    let children: Vec<Node> = match &chains {
        Ok(chains) => chains
            .iter()
            .map(|(event, chain)| {
                let steps: Vec<Node> = chain
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(index, step)| {
                        Node::leaf(
                            NodeKind::HookStep {
                                capsule: step.capsule.clone(),
                                phase: step.phase.as_str().to_string(),
                                position: index + 1,
                            },
                            format!(
                                "{:?} · {}",
                                step.failure,
                                if step.serial { "serial" } else { "parallel" }
                            )
                            .to_lowercase(),
                        )
                    })
                    .collect();
                Node::branch(
                    NodeKind::Group {
                        label: event.clone(),
                    },
                    format!("{} step{}", steps.len(), if steps.len() == 1 { "" } else { "s" }),
                    steps,
                )
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    // Foreign hook scripts: on disk, and whether anything dispatches them. This is
    // the "looks installed, never runs" case, and it is invisible without exactly
    // this comparison.
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let survey = foreign::default_hook_survey(&home);
    let mut children = children;
    if !survey.hooks.is_empty() {
        let orphans: Vec<Node> = survey
            .hooks
            .iter()
            .map(|hook| {
                Node::leaf(
                    NodeKind::Entry {
                        label: hook.name.clone(),
                        detail: hook.path.display().to_string(),
                    },
                    if hook.wired {
                        "wired".to_string()
                    } else {
                        "WIRED TO NOTHING — on disk, never dispatched".to_string()
                    },
                )
            })
            .collect();
        children.push(Node::branch(
            NodeKind::Group {
                label: "@claude-scripts".to_string(),
            },
            format!(
                "{} of {} wired — {} run on nothing",
                survey.wired(),
                survey.hooks.len(),
                survey.orphaned().len()
            ),
            orphans,
        ));
    }

    // The number that matters: hook capsules AIKit knows about versus hook steps
    // actually in a chain. A hook that is catalogued but in no chain runs never,
    // and that is exactly the thing nothing else on a machine will tell you.
    let catalogued = service
        .resolved()
        .catalog_index
        .values()
        .filter(|e| e.kind == Kind::Hook)
        .count();
    let wired: usize = children
        .iter()
        .filter(|c| !matches!(&c.kind, NodeKind::Group { label } if label.starts_with('@')))
        .map(|c| c.children.len())
        .sum();
    let summary = if catalogued > wired {
        format!(
            "{wired} of {catalogued} hook capsule{} are in a chain — {} run{} on nothing",
            if catalogued == 1 { "" } else { "s" },
            catalogued - wired,
            if catalogued - wired == 1 { "s" } else { "" }
        )
    } else {
        format!("{wired} step{} across {} event{}",
            if wired == 1 { "" } else { "s" },
            children.len(),
            if children.len() == 1 { "" } else { "s" })
    };

    Node::branch(NodeKind::Root(Root::Hooks), summary, children)
}

/// `contexts/` — this session, its tasks, other sessions.
fn contexts_root(service: &Service) -> Result<Node> {
    use aikit_store::state::StateStore;
    let store = StateStore::new(service.index());
    let current = &service.descriptor().context_id;

    let children: Vec<Node> = store
        .contexts()?
        .into_iter()
        .map(|c| {
            let label = c
                .project_root
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| c.context_id.to_string());
            let here = if &c.context_id == current { " · current" } else { "" };
            Node::leaf(
                NodeKind::Entry {
                    label: c.context_id.to_string(),
                    detail: label,
                },
                format!("{}{here}", c.isolation.as_str()),
            )
        })
        .collect();

    Ok(Node::branch(
        NodeKind::Root(Root::Contexts),
        format!("{} context{}", children.len(), if children.len() == 1 { "" } else { "s" }),
        children,
    ))
}

/// `registries/` — where things came from, with each root's problems counted.
fn registries_root(service: &Service) -> Node {
    let mut children = Vec::new();

    // AIKit's own registries, from the catalogue.
    let mut owned: BTreeMap<String, usize> = BTreeMap::new();
    for entry in service.resolved().catalog_index.keys() {
        if let Some(capsule) = Catalog::get(service.snapshot(), entry) {
            if let Some(source) = &capsule.source {
                *owned.entry(source.to_string()).or_default() += 1;
            }
        }
    }
    for (source, count) in owned {
        children.push(Node::leaf(
            NodeKind::Entry {
                label: source,
                detail: "owned".to_string(),
            },
            format!("{count} capsules"),
        ));
    }

    // Foreign roots: indexed, not owned, and their problems made visible.
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    for root in foreign::discover(&foreign::default_roots(&home)) {
        let mut summary = format!("foreign · {} skills", root.skills);
        if root.problems() > 0 {
            summary.push_str(&format!(
                " · ⚠ {} dead symlink{}, {} missing frontmatter",
                root.dead_symlinks,
                if root.dead_symlinks == 1 { "" } else { "s" },
                root.missing_frontmatter
            ));
        }
        children.push(Node::leaf(
            NodeKind::Entry {
                label: root.label.clone(),
                detail: root.path.display().to_string(),
            },
            summary,
        ));
    }

    Node::branch(
        NodeKind::Root(Root::Registries),
        format!("{} registr{}", children.len(), if children.len() == 1 { "y" } else { "ies" }),
        children,
    )
}

/// `inbox/` — what needs you.
fn inbox_root(service: &Service) -> Result<Node> {
    let items = service.inbox_items(false)?;
    let children: Vec<Node> = items
        .iter()
        .map(|item| {
            Node::leaf(
                NodeKind::Entry {
                    label: item.id.to_string(),
                    detail: item.kind.as_str().to_string(),
                },
                item.title.clone(),
            )
        })
        .collect();

    Ok(Node::branch(
        NodeKind::Root(Root::Inbox),
        format!("{} open", children.len()),
        children,
    ))
}
