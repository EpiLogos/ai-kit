//! File context (W1/W3, CASE 05): the composed engine reaction that puts a
//! file's semantic context in front of the operation about to touch it.
//!
//! Descope law: this runs only when the active composition selected
//! `hook/continuity/file-context` — never ambient. The content is what the
//! project already declares or holds, never invented here:
//!
//! * **Wiki relations** — nodes from the project's own wiki
//!   (`ProjectCentral/agents/wiki/wiki.json`) whose `source_refs` cite the
//!   file, with their immediate links and backlinks (bounded).
//! * **Domain guidance** — declared KnowledgeDomains whose `path_patterns`
//!   address the file; the same ordinary/standing laws as prompt activation
//!   apply, and the explanation names domain, path and source.
//!
//! Dedup law, as behaviour: the key is the rendered content scoped per file,
//! so an unchanged file with unchanged relations re-injects nothing at all;
//! a changed file (new relations, new guidance) re-arms injection. Standing
//! rules stay dedup-exempt by their explicit classification, visibly.
//!
//! Fail-open: unreadable wiki state or ledger trouble downgrades to a
//! warning on the decision; the operation proceeds.

use std::path::Path;

use aikit_core::domain::{dedup_hash, render_guidance_lines, KnowledgeDomain};
use aikit_core::hooks::HookEvent;
use aikit_core::skillset::glob_matches;
use aikit_core::{parse_wiki_objects, SemanticWikiIndex, WikiObject};
use aikit_store::index::Index;

/// How many wiki relations per direction survive the budget.
const NEIGHBOUR_LIMIT: usize = 5;
/// How many wiki nodes citing the file survive the budget.
const NODE_LIMIT: usize = 4;

/// The file path a pre-tool event carries, if any: the explicit path fields
/// of the tool input (`file_path`, `path`, `notebook_path`), never a guessed
/// one. Any tool that declares an explicit file path is in scope — the
/// reaction is about the file, not about a tool table.
pub fn file_path_of(event: &HookEvent) -> Option<String> {
    let input = event
        .payload
        .get("tool_input")
        .or_else(|| event.payload.get("input"))
        .unwrap_or(&event.payload);
    ["file_path", "path", "notebook_path"]
        .iter()
        .find_map(|key| input.get(*key))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

/// Load the project's own wiki objects (honest absence when the project
/// keeps no wiki; invalid state is disclosed, never fatal).
pub fn load_project_wiki(project_root: &Path) -> (Vec<WikiObject>, Vec<String>) {
    let path = project_root.join("ProjectCentral/agents/wiki/wiki.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => return (Vec::new(), Vec::new()),
    };
    match parse_wiki_objects(&text) {
        Ok(objects) => (objects, Vec::new()),
        Err(error) => (
            Vec::new(),
            vec![format!(
                "continuity/file-context project wiki unreadable ({}): {error}",
                path.display()
            )],
        ),
    }
}

/// The project-relative form of the path, for pattern matching and keys.
fn relative<'a>(project_root: &Path, path: &'a str) -> String {
    Path::new(path)
        .strip_prefix(project_root)
        .unwrap_or_else(|_| Path::new(path))
        .to_string_lossy()
        .into_owned()
}

fn path_touches(reference: &str, relative_path: &str) -> bool {
    // Source refs carry scheme prefixes (`source:project:<path>`,
    // `central:source:...:<path>`, plain paths); the file matches when the
    // reference ends with its project-relative path.
    reference.trim_start_matches("./").ends_with(relative_path)
}

/// The wiki-relation lines for the file, or an empty vector when the wiki
/// holds nothing about it (unrelated wiki material never arrives).
fn wiki_lines(index: &SemanticWikiIndex, relative_path: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cited = Vec::new();
    for resource in index.discover() {
        let Some(node) = index.node(&resource) else {
            continue;
        };
        if node
            .source_refs
            .iter()
            .any(|source| path_touches(source.as_str(), relative_path))
        {
            cited.push(node);
        }
    }
    for node in cited.iter().take(NODE_LIMIT) {
        lines.push(format!(
            "- {} — {}",
            node.ref_id,
            node.title.as_deref().unwrap_or(&node.node_type)
        ));
        for neighbour in index
            .neighbours(&node.ref_id, NEIGHBOUR_LIMIT)
            .iter()
            .take(NEIGHBOUR_LIMIT)
        {
            lines.push(format!("  links to: {} ({})", neighbour.resource, neighbour.relation));
        }
        for neighbour in index.backlinks(&node.ref_id).iter().take(NEIGHBOUR_LIMIT) {
            lines.push(format!("  cited by: {} ({})", neighbour.resource, neighbour.relation));
        }
    }
    if cited.len() > NODE_LIMIT {
        lines.push(format!(
            "- … {} further nodes citing this file withheld by the disclosure budget",
            cited.len() - NODE_LIMIT
        ));
    }
    lines
}

/// Run the reaction for a file about to be touched. Returns the blocks to
/// inject plus warnings — wiki relations first, then file-addressed domain
/// guidance, everything deduped through the ledger.
pub fn run(
    index: &Index,
    scope: &str,
    project_root: &Path,
    path: &str,
    domains: &[KnowledgeDomain],
    wiki_objects: Vec<WikiObject>,
) -> (Vec<String>, Vec<String>) {
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    let relative_path = relative(project_root, path);

    // Wiki relations: one block, deduped as a whole per file.
    if !wiki_objects.is_empty() {
        match SemanticWikiIndex::rebuild(wiki_objects) {
            Ok(wiki) => {
                let lines = wiki_lines(&wiki, &relative_path);
                if !lines.is_empty() {
                    let hash = dedup_hash("file-context:wiki", &[relative_path.clone(), lines.join("\n")]);
                    if seen(index, scope, &hash, &mut warnings) {
                        // Unchanged file, unchanged relations: nothing arrives.
                    } else {
                        if let Err(error) = index.record_injection(scope, &hash) {
                            warnings.push(format!(
                                "continuity/file-context ledger unavailable: {error}"
                            ));
                        }
                        blocks.push(format!(
                            "[continuity/file-context] wiki relations for {relative_path} (composed):\n{}",
                            lines.join("\n")
                        ));
                    }
                }
            }
            Err(error) => warnings
                .push(format!("continuity/file-context wiki index unavailable: {error}")),
        }
    }

    // File-addressed domain guidance: per-domain blocks under the same laws.
    for domain in domains {
        let matched = domain.path_patterns.iter().any(|pattern| {
            glob_matches(pattern, &relative_path) || glob_matches(pattern, path)
        });
        if !matched {
            continue;
        }
        let (ordinary, standing) = render_guidance_lines(domain);
        let hash = dedup_hash(&format!("{}@{relative_path}", domain.id), &ordinary);
        let deduped = !ordinary.is_empty() && seen(index, scope, &hash, &mut warnings);
        if deduped && standing.is_empty() {
            continue;
        }
        if !deduped && !ordinary.is_empty() {
            if let Err(error) = index.record_injection(scope, &hash) {
                warnings.push(format!(
                    "continuity/file-context ledger unavailable: {error}"
                ));
            }
        }
        let lines: Vec<&String> = if deduped {
            standing.iter().collect()
        } else {
            standing.iter().chain(ordinary.iter()).collect()
        };
        let mut block = format!(
            "[continuity/file-context] domain {} armed for {relative_path} — horizon: {}; \
             source: {} revision {};{}{}",
            domain.id,
            domain
                .horizon_range
                .as_ref()
                .map(|range| range.render())
                .unwrap_or_else(|| "unbounded".into()),
            domain.source,
            domain.revision,
            if deduped {
                " ordinary payload deduped (unchanged rendered content);"
            } else {
                ""
            },
            if !standing.is_empty() {
                " standing rules reasserted (dedup-exempt by classification)"
            } else {
                ""
            }
        );
        for line in lines {
            block.push('\n');
            block.push_str(line);
        }
        blocks.push(block);
    }
    (blocks, warnings)
}

/// Ledger access with the fail-open law: trouble becomes a warning and the
/// injection proceeds.
fn seen(index: &Index, scope: &str, hash: &str, warnings: &mut Vec<String>) -> bool {
    match index.injection_seen(scope, hash) {
        Ok(seen) => seen,
        Err(error) => {
            warnings.push(format!(
                "continuity/file-context dedup check unavailable: {error}"
            ));
            false
        }
    }
}
