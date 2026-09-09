//! `aikit wiki` — the command side of the Agent Wiki write pipeline.
//!
//! Every command here names the file it touches, runs the same core pipeline
//! (parse → mutate in memory → validate the whole → render), and persists the
//! result through a temp-file rename gated by an optimistic concurrency check.
//! The wiki is agent-maintained, so concurrent writers — two agents, or an
//! agent and a human — are the normal case: each write captures the SHA-256 of
//! the exact bytes it read, and refuses at the rename if a peer's write already
//! landed, rather than silently discarding it. Nothing discovers a file to
//! mutate: a caller passes `--file`, or, for the Central root only, a `--root`
//! that is resolved read-only from the working directory. Wiki tooling is
//! AVAILABLE, NOT ENFORCED.
//!
//! The root commands (`wiki root …`) are the only ones that *guess* a path, and
//! they guess from the Central layout — `<central>/Control/agents/wiki/wiki.json`
//! with projects at `<central>/Work/<dir>/ProjectCentral` — because a root
//! federation is meaningless outside that layout.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json as jval, Value};
use sha2::Digest;

use aikit_core::knowledge_ingest::{ingest_corpus, select_ingestable_records};
use aikit_core::knowledge_source_pool::SourceMaterial;
use aikit_core::knowledge_wiki::{
    WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject, WikiProvenanceRef, WikiSpace, OKF_WIKI_PROFILE,
};
use aikit_core::knowledge_wiki_write::{
    apply_wiki_mutation, project_id_from_space_ref, project_wiki_space_ref, WikiDocument,
    WikiMutationLedger, WikiMutationOutcome, ROOT_WIKI_SPACE_REF,
};
use aikit_core::projectcentral::{CENTRAL_ROOT_WIKI_SOURCE, PROJECTCENTRAL_WIKI_SOURCE};
use aikit_core::resource::{ResourceRef, SourceRef};
use aikit_core::{AikitError, Result, SemanticWikiIndex};

use crate::cli::{
    WikiCmd, WikiEdgeArgs, WikiIngestArgs, WikiNodeArgs, WikiQueryRefArgs, WikiQuerySearchArgs,
    WikiQuerySub, WikiRootAdoptArgs, WikiRootAnchorArgs, WikiRootArgs, WikiRootPruneArgs,
    WikiSpaceCreateArgs, WikiSpaceLinkArgs, WikiStageArgs,
};
use crate::json;

/// The manifest the root commands read a project id from, relative to a project
/// root. Its full schema belongs to ctrl; the Wiki needs only the identity.
const PROJECT_MANIFEST_SOURCE: &str = "ProjectCentral/project.json";

/// What a wiki command did, before the envelope is wrapped around it.
pub struct WikiOutcome {
    pub data: Value,
    pub warnings: Vec<String>,
    /// `wiki validate` reports its findings *and* exits non-zero when the
    /// document does not hold: a machine reading the envelope must not have to
    /// re-derive `valid` from the error list.
    pub exit_code: i32,
}

impl WikiOutcome {
    /// A command that named a file and changed it.
    fn wrote(data: Value, outcome: &WikiMutationOutcome) -> Self {
        Self {
            data,
            warnings: outcome.warnings.clone(),
            exit_code: json::EXIT_OK,
        }
    }

    /// A command that reports without the write gate in its return path.
    fn reported(data: Value, warnings: Vec<String>, exit_code: i32) -> Self {
        Self {
            data,
            warnings,
            exit_code,
        }
    }
}

/// Dispatch one `aikit wiki` invocation.
pub fn run(cwd: &Path, command: WikiCmd) -> Result<WikiOutcome> {
    use crate::cli::{WikiEdgeSub, WikiNodeSub, WikiRootSub, WikiSpaceSub, WikiSub};
    match command.command {
        WikiSub::Validate(args) => validate(&args.path),
        WikiSub::Node(node) => match node.command {
            WikiNodeSub::Create(args) => node_create(&args),
            WikiNodeSub::Update(args) => node_update(&args),
        },
        WikiSub::Edge(edge) => match edge.command {
            WikiEdgeSub::Add(args) => edge_add(&args),
        },
        WikiSub::Space(space) => match space.command {
            WikiSpaceSub::Create(args) => space_create(&args),
            WikiSpaceSub::Link(args) => space_link(&args),
        },
        WikiSub::Root(root) => match root.command {
            WikiRootSub::Doctor(args) => root_doctor(cwd, &args),
            WikiRootSub::Prune(args) => root_prune(cwd, &args),
            WikiRootSub::Adopt(args) => root_adopt(cwd, &args),
            WikiRootSub::Anchor(args) => root_anchor(cwd, &args),
        },
        WikiSub::Stage(args) => stage(&args),
        WikiSub::Ingest(args) => ingest(&args),
        WikiSub::Query(query) => match query.command {
            WikiQuerySub::Search(args) => query_search(&args),
            WikiQuerySub::Neighbours(args) => query_neighbours(&args),
            WikiQuerySub::Backlinks(args) => query_backlinks(&args),
        },
    }
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

/// Parse a Wiki file, rebuild the index over the whole and publish every
/// finding. Read-only: a document that does not hold is a report and a non-zero
/// exit, never a rewrite.
fn validate(path: &Path) -> Result<WikiOutcome> {
    let input = read(path)?;
    let document = WikiDocument::parse(&input)?;
    let report = document.report();
    let exit_code = if report.is_valid() {
        json::EXIT_OK
    } else {
        json::EXIT_GENERIC
    };
    Ok(WikiOutcome::reported(
        jval!({
            "path": path.display().to_string(),
            "valid": report.is_valid(),
            "objects": report.objects,
            "spaces": report.spaces,
            "nodes": report.nodes,
            "edges": report.edges,
            "frames": report.frames,
            "readings": report.readings,
            "duplicates": report.duplicates,
            "dangling": report.dangling,
            "errors": report.errors,
            "index_revision": report.index_revision,
        }),
        Vec::new(),
        exit_code,
    ))
}

// ---------------------------------------------------------------------------
// node
// ---------------------------------------------------------------------------

fn node_create(args: &WikiNodeArgs) -> Result<WikiOutcome> {
    let node = if args.stdin {
        stdin_node(&args.node_ref)?
    } else {
        let node_type = args.node_type.as_deref().ok_or_else(|| {
            AikitError::new(
                "cli.usage",
                "`wiki node create` requires --type; the node's type is written, not guessed",
            )
        })?;
        WikiNode {
            profile: OKF_WIKI_PROFILE.to_string(),
            ref_id: ResourceRef::parse(&args.node_ref)?,
            revision: 1,
            provenance: provenance_from_sources(&args.source)?,
            node_type: node_type.to_string(),
            title: args.title.clone(),
            space_refs: parse_refs(&args.space, "space")?,
            source_refs: parse_source_refs(&args.source)?,
            local_space_ref: None,
            extensions: BTreeMap::new(),
        }
    };
    let spaces = node.space_refs.clone();
    let node_ref = node.ref_id.to_string();
    let outcome = mutate_file(&args.file, |doc, ledger| {
        let created = doc.create_object(WikiObject::Node(node.clone()))?;
        ledger.record(created);
        let synced = doc.sync_space_memberships(&node)?;
        ledger.record(synced);
        Ok(())
    })?;
    Ok(WikiOutcome::wrote(
        jval!({
            "command": "node.create",
            "file": args.file.display().to_string(),
            "ref": node_ref,
            "spaces": refs_json(&spaces),
            "outcome": mutation_outcome(&outcome),
        }),
        &outcome,
    ))
}

fn node_update(args: &WikiNodeArgs) -> Result<WikiOutcome> {
    let replacement = if args.stdin {
        stdin_node(&args.node_ref)?
    } else {
        // Flag mode patches over the node the file already holds: a caller that
        // names one field must not have to repeat the rest to keep it.
        let existing = existing_node(&args.file, &args.node_ref)?;
        WikiNode {
            profile: existing.profile.clone(),
            ref_id: existing.ref_id.clone(),
            revision: existing.revision,
            provenance: match_provenance(&existing.provenance, &args.source)?,
            node_type: args
                .node_type
                .clone()
                .unwrap_or_else(|| existing.node_type.clone()),
            title: args.title.clone().or_else(|| existing.title.clone()),
            space_refs: if args.space.is_empty() {
                existing.space_refs.clone()
            } else {
                parse_refs(&args.space, "space")?
            },
            source_refs: if args.source.is_empty() {
                existing.source_refs.clone()
            } else {
                parse_source_refs(&args.source)?
            },
            local_space_ref: existing.local_space_ref.clone(),
            extensions: existing.extensions.clone(),
        }
    };
    let spaces = replacement.space_refs.clone();
    let node_ref = replacement.ref_id.to_string();
    let outcome = mutate_file(&args.file, |doc, ledger| {
        let updated = doc.update_object(WikiObject::Node(replacement.clone()))?;
        ledger.record(updated);
        let synced = doc.sync_space_memberships(&replacement)?;
        ledger.record(synced);
        Ok(())
    })?;
    Ok(WikiOutcome::wrote(
        jval!({
            "command": "node.update",
            "file": args.file.display().to_string(),
            "ref": node_ref,
            "spaces": refs_json(&spaces),
            "outcome": mutation_outcome(&outcome),
        }),
        &outcome,
    ))
}

/// The whole node body from `--stdin`. Any other object kind is a usage error,
/// not a silent reinterpretation, and the body may not rename itself.
fn stdin_node(requested: &str) -> Result<WikiNode> {
    let object = WikiObject::parse(&serde_json::from_str::<Value>(&read_stdin()?).map_err(
        |error| {
            AikitError::new(
                "knowledge.wiki_invalid_json",
                format!("invalid node JSON on stdin: {error}"),
            )
        },
    )?)?;
    let kind = kind_of(&object);
    let WikiObject::Node(node) = object else {
        return Err(AikitError::new(
            "cli.usage",
            format!("`wiki node` writes a node; stdin carried a {kind}"),
        ));
    };
    let requested = ResourceRef::parse(requested)?;
    if node.ref_id != requested {
        return Err(AikitError::new(
            "cli.usage",
            format!(
                "the body names {} but the command was given {requested}; a write never rewrites identity",
                node.ref_id
            ),
        )
        .with("body_ref", node.ref_id.to_string())
        .with("requested", requested.to_string()));
    }
    Ok(node)
}

fn existing_node(file: &Path, raw_ref: &str) -> Result<WikiNode> {
    let document = WikiDocument::parse(&read(file)?)?;
    let node_ref = ResourceRef::parse(raw_ref)?;
    match document.object(&node_ref) {
        Some(WikiObject::Node(node)) => Ok(node.clone()),
        Some(other) => Err(AikitError::new(
            "cli.usage",
            format!("{node_ref} is a {}, not a node", kind_of(other)),
        )
        .with("ref", node_ref.to_string())),
        None => Err(AikitError::new(
            "knowledge.wiki_object_missing",
            format!("{node_ref} is not in {}", file.display()),
        )
        .with("ref", node_ref.to_string())),
    }
}

// ---------------------------------------------------------------------------
// edge
// ---------------------------------------------------------------------------

fn edge_add(args: &WikiEdgeArgs) -> Result<WikiOutcome> {
    let origin: WikiEdgeOrigin = serde_json::from_value(Value::String(args.origin.clone()))
        .map_err(|_| {
            AikitError::new(
                "cli.usage",
                format!(
                    "`{}` is not a Wiki edge origin; use authored, mechanical, compiled, \
                     inferred, learned, QL-derived or MEF-derived",
                    args.origin
                ),
            )
        })?;
    let from = ResourceRef::parse(&args.from_ref)?;
    let to = ResourceRef::parse(&args.to_ref)?;
    let edge_ref = match &args.edge_ref {
        Some(raw) => ResourceRef::parse(raw)?,
        // Deterministic and readable: the same endpoints always name the same
        // edge, so re-adding a relation is refused rather than duplicated.
        None => ResourceRef::parse(format!("{}|{}|{}", from, args.relation, to))?,
    };

    // The index holds edges to no endpoint rule, so endpoints are checked here,
    // where the caller's intent (`--allow-dangling`) is known.
    let file = &args.file;
    let held = WikiDocument::parse(&read(file)?)?;
    let mut warnings = Vec::new();
    for (role, endpoint) in [("from_ref", &from), ("to_ref", &to)] {
        if held.holds(endpoint) {
            continue;
        }
        let message = format!(
            "{endpoint} does not resolve in {}; the edge records a relation to a peer Wiki object",
            file.display()
        );
        if args.allow_dangling {
            warnings.push(message);
        } else {
            return Err(AikitError::new(
                "knowledge.wiki_edge_dangling_endpoint",
                format!(
                    "{endpoint} does not resolve in {}; pass --allow-dangling to record a relation across a federation boundary",
                    file.display()
                ),
            )
            .with("endpoint", endpoint.to_string())
            .with("role", role));
        }
    }

    let edge = WikiEdge {
        profile: OKF_WIKI_PROFILE.to_string(),
        ref_id: edge_ref.clone(),
        revision: 1,
        provenance: Vec::new(),
        from_ref: from.clone(),
        to_ref: to.clone(),
        relation: args.relation.clone(),
        origin,
        origin_ref: args
            .origin_ref
            .as_deref()
            .map(ResourceRef::parse)
            .transpose()?,
        extensions: BTreeMap::new(),
    };
    let outcome = mutate_file(file, |doc, ledger| {
        let added = doc.create_object(WikiObject::Edge(edge.clone()))?;
        ledger.record(added);
        Ok(())
    })?;
    let mut reply = WikiOutcome::wrote(
        jval!({
            "command": "edge.add",
            "file": file.display().to_string(),
            "ref": edge_ref.to_string(),
            "from": from.to_string(),
            "relation": args.relation,
            "to": to.to_string(),
            "outcome": mutation_outcome(&outcome),
        }),
        &outcome,
    );
    reply.warnings.extend(warnings);
    Ok(reply)
}

// ---------------------------------------------------------------------------
// space
// ---------------------------------------------------------------------------

fn space_create(args: &WikiSpaceCreateArgs) -> Result<WikiOutcome> {
    let space_ref = ResourceRef::parse(&args.space_ref)?;
    let parent = args.parent.as_deref().map(ResourceRef::parse).transpose()?;
    let space = WikiSpace {
        profile: OKF_WIKI_PROFILE.to_string(),
        ref_id: space_ref.clone(),
        revision: 1,
        provenance: Vec::new(),
        title: Some(args.title.clone()),
        parent_space_refs: parent.clone().into_iter().collect(),
        child_space_refs: Vec::new(),
        node_refs: Vec::new(),
        anchor_ref: None,
        extensions: BTreeMap::new(),
    };
    let outcome = mutate_file(&args.file, |doc, ledger| {
        let created = doc.create_object(WikiObject::Space(space.clone()))?;
        ledger.record(created);
        if let Some(parent) = &parent {
            // Reciprocity is written from whichever side this file holds; a
            // federated parent is the norm and is reported, not chased.
            let linked = doc.link(parent, &space_ref)?;
            ledger.record(linked);
        }
        Ok(())
    })?;
    Ok(WikiOutcome::wrote(
        jval!({
            "command": "space.create",
            "file": args.file.display().to_string(),
            "ref": space_ref.to_string(),
            "title": args.title,
            "parent": parent.as_ref().map(|parent| parent.to_string()),
            "outcome": mutation_outcome(&outcome),
        }),
        &outcome,
    ))
}

fn space_link(args: &WikiSpaceLinkArgs) -> Result<WikiOutcome> {
    let parent = ResourceRef::parse(&args.parent_ref)?;
    let child = ResourceRef::parse(&args.child_ref)?;
    let link = |doc: &mut WikiDocument, ledger: &mut WikiMutationLedger| {
        let linked = doc.link(&parent, &child)?;
        ledger.record(linked);
        Ok(())
    };
    match &args.child_file {
        // One file holding both sides: one document, one write.
        None => {
            let outcome = mutate_file(&args.file, link)?;
            Ok(WikiOutcome::wrote(
                jval!({
                    "command": "space.link",
                    "files": [args.file.display().to_string()],
                    "parent": parent.to_string(),
                    "child": child.to_string(),
                    "outcome": mutation_outcome(&outcome),
                }),
                &outcome,
            ))
        }
        // The federated case: each side is written in its own file, through its
        // own gate, each advancing only the revisions it touches.
        Some(child_file) => {
            let parent_outcome = mutate_file(&args.file, link)?;
            let child_outcome = mutate_file(child_file, link)?;
            let mut warnings = parent_outcome.warnings.clone();
            warnings.extend(child_outcome.warnings.clone());
            Ok(WikiOutcome::reported(
                jval!({
                    "command": "space.link",
                    "files": [
                        args.file.display().to_string(),
                        child_file.display().to_string(),
                    ],
                    "parent": parent.to_string(),
                    "child": child.to_string(),
                    "parent_file": mutation_outcome(&parent_outcome),
                    "child_file": mutation_outcome(&child_outcome),
                }),
                warnings,
                json::EXIT_OK,
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// root
// ---------------------------------------------------------------------------

/// Resolve every `child_space_refs` entry of the root Space against the
/// filesystem. Read-only: a dangling child is a finding to act on with
/// `wiki root prune` or `wiki root adopt`, never something rewritten here.
fn root_doctor(cwd: &Path, args: &WikiRootArgs) -> Result<WikiOutcome> {
    let root = resolve_root_wiki(cwd, args.root.as_deref())?;
    let document = WikiDocument::parse(&read(&root)?)?;
    let central = central_root(&root)?;
    let root_space = root_space(&document)?;

    let mut healthy = Vec::new();
    let mut dangling = Vec::new();
    for child in &root_space.child_space_refs {
        let Some(project_id) = project_id_from_space_ref(child) else {
            dangling.push(jval!({
                "ref": child.to_string(),
                "reason": "the ref is not a project Wiki Space ref",
            }));
            continue;
        };
        let project = Some(project_id.to_string());
        let expected = resolve_project_dir(&central, project_id);
        match project_wiki(&expected, project_id) {
            Ok(wiki_path) => {
                let holds = std::fs::read_to_string(&wiki_path)
                    .ok()
                    .and_then(|text| WikiDocument::parse(&text).ok())
                    .is_some_and(|wiki| wiki.holds(child));
                if holds {
                    healthy.push(jval!({
                        "ref": child.to_string(),
                        "project": project,
                        "wiki": wiki_path.display().to_string(),
                    }));
                } else {
                    dangling.push(jval!({
                        "ref": child.to_string(),
                        "project": project,
                        "expected": wiki_path.display().to_string(),
                        "reason": "the project Wiki is missing or does not hold this Space",
                    }));
                }
            }
            Err(reason) => dangling.push(jval!({
                "ref": child.to_string(),
                "project": project,
                "expected": expected
                    .join(PROJECTCENTRAL_WIKI_SOURCE)
                    .display()
                    .to_string(),
                "reason": reason,
            })),
        }
    }

    Ok(WikiOutcome::reported(
        jval!({
            "root": root.display().to_string(),
            "root_ref": root_space.ref_id.to_string(),
            "anchor": root_space
                .anchor_ref
                .as_ref()
                .map(|anchor| anchor.to_string()),
            "children": root_space.child_space_refs.len(),
            "healthy": healthy,
            "dangling": dangling,
        }),
        Vec::new(),
        json::EXIT_OK,
    ))
}

/// Retract one child ref from the root Space. Dry run by default: the same
/// pipeline runs and the same gate holds, but nothing is persisted, so what the
/// dry run reports is exactly what `--apply` would write.
fn root_prune(cwd: &Path, args: &WikiRootPruneArgs) -> Result<WikiOutcome> {
    let root = resolve_root_wiki(cwd, args.root.as_deref())?;
    let child = ResourceRef::parse(&args.child_ref)?;
    let input = read(&root)?;
    let base_hash = content_hash(input.as_bytes());
    let federated = WikiDocument::parse(&input)?
        .objects()
        .iter()
        .any(|object| {
            matches!(object, WikiObject::Space(space) if space.child_space_refs.contains(&child))
        });
    if !federated {
        return Err(AikitError::new(
            "knowledge.wiki_ref_missing",
            format!("{child} is not federated in {}", root.display()),
        )
        .with("child", child.to_string()));
    }

    let (rendered, outcome) = apply_wiki_mutation(&input, |doc, ledger| {
        let pruned = doc.unlink(&child)?;
        ledger.record(pruned);
        Ok(())
    })?;
    if !args.apply {
        return Ok(WikiOutcome::reported(
            jval!({
                "root": root.display().to_string(),
                "child": child.to_string(),
                "applied": false,
                "would_change": outcome.changed,
                "outcome": mutation_outcome(&outcome),
                "note": "dry run; re-run with --apply to write this retraction",
            }),
            outcome.warnings.clone(),
            json::EXIT_OK,
        ));
    }
    persist(&root, &rendered, &base_hash)?;
    Ok(WikiOutcome::wrote(
        jval!({
            "command": "root.prune",
            "root": root.display().to_string(),
            "child": child.to_string(),
            "applied": true,
            "outcome": mutation_outcome(&outcome),
        }),
        &outcome,
    ))
}

/// Idempotently federate an existing project Wiki into the root Space.
///
/// Adoption federates what is already authored: the project must carry a
/// ProjectCentral manifest naming its id and a Wiki file holding its own Space.
/// It creates neither — that is the project's act, not the root's.
fn root_adopt(cwd: &Path, args: &WikiRootAdoptArgs) -> Result<WikiOutcome> {
    let root = resolve_root_wiki(cwd, args.root.as_deref())?;
    let project = args.project_path.as_path();
    let project_id = manifest_project_id(&project.join(PROJECT_MANIFEST_SOURCE))?;
    let child_ref = project_wiki_space_ref(&project_id)?;
    let child_wiki = project.join(PROJECTCENTRAL_WIKI_SOURCE);
    if !child_wiki.is_file() {
        return Err(AikitError::new(
            "knowledge.wiki_project_wiki_missing",
            format!(
                "{} has no Wiki file at {}; adopt federates an authored Wiki, it does not author one",
                project.display(),
                child_wiki.display()
            ),
        )
        .with("project", project_id.clone())
        .with("expected", child_wiki.display().to_string()));
    }
    let child_document = WikiDocument::parse(&read(&child_wiki)?)?;
    if !child_document.holds(&child_ref) {
        return Err(AikitError::new(
            "knowledge.wiki_project_space_missing",
            format!(
                "{} does not hold {child_ref}; the project Wiki must name its own Space before the root federates it",
                child_wiki.display()
            ),
        )
        .with("project", project_id.clone())
        .with("space", child_ref.to_string()));
    }

    let input = read(&root)?;
    let base_hash = content_hash(input.as_bytes());
    let already = WikiDocument::parse(&input)?
        .objects()
        .iter()
        .any(|object| {
            matches!(object, WikiObject::Space(space) if space.child_space_refs.contains(&child_ref))
        });
    if already {
        return Ok(WikiOutcome::reported(
            jval!({
                "command": "root.adopt",
                "root": root.display().to_string(),
                "project": project_id,
                "space": child_ref.to_string(),
                "changed": false,
                "note": "the root already federates this project; nothing was written",
            }),
            Vec::new(),
            json::EXIT_OK,
        ));
    }

    let (rendered, outcome) = apply_wiki_mutation(&input, |doc, ledger| {
        let root_ref = ResourceRef::parse(ROOT_WIKI_SPACE_REF)?;
        let linked = doc.link(&root_ref, &child_ref)?;
        ledger.record(linked);
        Ok(())
    })?;
    persist(&root, &rendered, &base_hash)?;
    Ok(WikiOutcome::wrote(
        jval!({
            "command": "root.adopt",
            "root": root.display().to_string(),
            "project": project_id,
            "space": child_ref.to_string(),
            "project_wiki": child_wiki.display().to_string(),
            "changed": outcome.changed,
            "outcome": mutation_outcome(&outcome),
        }),
        &outcome,
    ))
}

/// Ensure a Space is anchored on its root node: the Central root Space on a
/// minimal user identity node, or one project's Space on its project root
/// node named from the project. Minimal by design — the node carries identity
/// (ref, type, title, source link) and nothing else; content is the world's
/// business, not the anchor's. Idempotent in both directions: an already
/// anchored Space reports no change, and an existing node is never rewritten
/// to become an anchor.
fn root_anchor(cwd: &Path, args: &WikiRootAnchorArgs) -> Result<WikiOutcome> {
    let (wiki_path, space_label, mut node) = match &args.project {
        Some(project) => {
            let project_id = manifest_project_id(&project.join(PROJECT_MANIFEST_SOURCE))?;
            let space_ref = project_wiki_space_ref(&project_id)?;
            let wiki = project.join(PROJECTCENTRAL_WIKI_SOURCE);
            if !wiki.is_file() {
                return Err(AikitError::new(
                    "knowledge.wiki_project_wiki_missing",
                    format!(
                        "{} has no Wiki file at {}; anchor anchors an authored Wiki Space",
                        project.display(),
                        wiki.display()
                    ),
                )
                .with("project", project_id)
                .with("expected", wiki.display().to_string()));
            }
            let name = project
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| project_id.clone());
            let slug: String = name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            (
                wiki,
                space_ref.to_string(),
                minimal_root_node(
                    &format!("wiki:node:project-root/{slug}"),
                    "project-root",
                    &name,
                    Some("ProjectCentral/project.json"),
                )?,
            )
        }
        None => {
            let root = resolve_root_wiki(cwd, args.root.as_deref())?;
            let identity_file = central_root(&root)?.join("Control/user/identity.md");
            (
                root,
                ROOT_WIKI_SPACE_REF.to_string(),
                minimal_root_node(
                    "wiki:node:identity",
                    "identity",
                    "User identity",
                    identity_file.is_file().then_some("Control/user/identity.md"),
                )?,
            )
        }
    };
    node.space_refs = vec![ResourceRef::parse(&space_label)?];
    let node_ref = node.ref_id.clone();

    let outcome = mutate_file(&wiki_path, |doc, ledger| {
        if !doc.holds(&node.ref_id) {
            let created = doc.create_object(WikiObject::Node(node.clone()))?;
            ledger.record(created);
            let synced = doc.sync_space_memberships(&node)?;
            ledger.record(synced);
        }
        let space_ref = ResourceRef::parse(&space_label)?;
        let mut space = match doc.object(&space_ref) {
            Some(WikiObject::Space(space)) => space.clone(),
            Some(other) => {
                return Err(AikitError::new(
                    "knowledge.wiki_root_space_missing",
                    format!("{space_ref} is held as a {}, not a Space", kind_of(other)),
                )
                .with("space", space_ref.to_string()))
            }
            None => {
                return Err(AikitError::new(
                    "knowledge.wiki_root_space_missing",
                    format!("the Wiki file holds no {space_ref} Space"),
                )
                .with("space", space_ref.to_string()))
            }
        };
        if space.anchor_ref.as_ref() != Some(&node.ref_id) {
            space.anchor_ref = Some(node.ref_id.clone());
            let updated = doc.update_object(WikiObject::Space(space))?;
            ledger.record(updated);
        }
        Ok(())
    })?;
    Ok(WikiOutcome::wrote(
        jval!({
            "command": "root.anchor",
            "file": wiki_path.display().to_string(),
            "space": space_label,
            "anchor": node_ref.to_string(),
            "title": node.title,
            "outcome": mutation_outcome(&outcome),
        }),
        &outcome,
    ))
}

/// The minimal root node: identity and a source link, no content. What the
/// world hangs off the anchor is authored elsewhere, by its own laws.
fn minimal_root_node(
    node_ref: &str,
    node_type: &str,
    title: &str,
    source: Option<&str>,
) -> Result<WikiNode> {
    let sources: Vec<String> = source.into_iter().map(str::to_string).collect();
    Ok(WikiNode {
        profile: OKF_WIKI_PROFILE.to_string(),
        ref_id: ResourceRef::parse(node_ref)?,
        revision: 1,
        provenance: provenance_from_sources(&sources)?,
        node_type: node_type.to_string(),
        title: Some(title.to_string()),
        space_refs: Vec::new(),
        source_refs: parse_source_refs(&sources)?,
        local_space_ref: None,
        extensions: BTreeMap::new(),
    })
}

/// The directory in `Work/` that declares `project_id`. The direct
/// `Work/<project_id>` path is the norm; when it misses, the manifests are
/// scanned, because a declared id and its directory name may differ (an id
/// like `project:ai-kit` lives in `Work/ai-kit`). Unreadable directories are
/// skipped: the doctor reports what it can see.
fn resolve_project_dir(central: &Path, project_id: &str) -> PathBuf {
    let direct = central.join("Work").join(project_id);
    if direct.join(PROJECT_MANIFEST_SOURCE).is_file() {
        return direct;
    }
    let work = central.join("Work");
    let Ok(entries) = std::fs::read_dir(&work) else {
        return direct;
    };
    for entry in entries.flatten() {
        let candidate = entry.path();
        if candidate.join(PROJECT_MANIFEST_SOURCE).is_file()
            && manifest_project_id(&candidate.join(PROJECT_MANIFEST_SOURCE))
                .map(|declared| declared == project_id)
                .unwrap_or(false)
        {
            return candidate;
        }
    }
    direct
}

/// The project Wiki file for `project_id`, found by scanning the Central
/// horizon's `Work/` directories for the manifest that declares it. A directory
/// that cannot be read is skipped: the doctor reports what it can see.
fn project_wiki(project_dir: &Path, project_id: &str) -> std::result::Result<PathBuf, String> {
    let manifest = project_dir.join(PROJECT_MANIFEST_SOURCE);
    let manifest_id = std::fs::read_to_string(&manifest)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| {
            value
                .get("project_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    match manifest_id {
        Some(id) if id == project_id => Ok(project_dir.join(PROJECTCENTRAL_WIKI_SOURCE)),
        _ => Err(format!(
            "no project directory at {} declares `{project_id}` in its ProjectCentral manifest",
            project_dir.display()
        )),
    }
}

/// Read `project_id` out of a ProjectCentral manifest.
fn manifest_project_id(manifest: &Path) -> Result<String> {
    let text = read(manifest).map_err(|_| {
        AikitError::new(
            "knowledge.wiki_project_manifest_missing",
            format!(
                "{} has no readable ProjectCentral manifest; a project is adopted by its declared id, not guessed",
                manifest.parent().unwrap_or(manifest).display()
            ),
        )
        .with("manifest", manifest.display().to_string())
    })?;
    let value: Value = serde_json::from_str(&text).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_project_manifest_invalid",
            format!("invalid ProjectCentral manifest: {error}"),
        )
        .with("manifest", manifest.display().to_string())
    })?;
    value
        .get("project_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_project_manifest_invalid",
                "the ProjectCentral manifest declares no `project_id`",
            )
            .with("manifest", manifest.display().to_string())
        })
}

// ---------------------------------------------------------------------------
// stage
// ---------------------------------------------------------------------------

/// The authored QL alignment a source file's frontmatter declares. The block
/// is deliberately minimal: `ql:` with flat `key: value` pairs. Positions are
/// relative to a named unit (the local sixfold that gives 0–5 their meaning),
/// which is why a position without a unit is refused.
struct QlAlignment {
    position: Option<u8>,
    unit: Option<String>,
    face: Option<String>,
    node_type: Option<String>,
    labels: BTreeMap<String, String>,
}

impl QlAlignment {
    /// Parse the `ql:` block out of a document's frontmatter. Only this block
    /// is read: the prose body stays prose, and the handwriting is never
    /// rewritten. Unknown keys under `ql:` are preserved verbatim as labels.
    fn from_markdown(text: &str) -> Result<Option<Self>> {
        let Some(frontmatter) = strip_frontmatter(text) else {
            return Ok(None);
        };
        let mut in_ql = false;
        let mut labels = BTreeMap::new();
        for line in frontmatter.lines() {
            let trimmed_end = line.trim_end();
            if !in_ql {
                if trimmed_end == "ql:" {
                    in_ql = true;
                }
                continue;
            }
            if trimmed_end.is_empty() {
                continue;
            }
            if !line.starts_with(' ') && !line.starts_with('\t') {
                // A new top-level frontmatter key: the ql block is over.
                break;
            }
            let Some((key, value)) = split_label(trimmed_end.trim()) else {
                return Err(AikitError::new(
                    "knowledge.wiki_stage_frontmatter",
                    format!("`{trimmed_end}` is not a `key: value` label under `ql:`"),
                ));
            };
            labels.insert(key.to_string(), unquote(value));
        }
        if !in_ql {
            return Ok(None);
        }
        let position = match labels.remove("position") {
            Some(raw) => {
                let value: u8 = raw.parse().map_err(|_| {
                    AikitError::new(
                        "knowledge.wiki_stage_alignment",
                        format!(
                            "`{raw}` is not a position; positions are integers relative to \
                             their unit's sixfold"
                        ),
                    )
                })?;
                if value > 5 {
                    return Err(AikitError::new(
                        "knowledge.wiki_stage_alignment",
                        format!(
                            "position {value} is out of range; a unit's positions run 0–5"
                        ),
                    ));
                }
                Some(value)
            }
            None => None,
        };
        let unit = labels.remove("unit");
        if position.is_some() && unit.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_none()
        {
            return Err(AikitError::new(
                "knowledge.wiki_stage_alignment",
                "a position requires its unit: positions are relative to the local sixfold \
                 that gives 0–5 their meaning, never global",
            ));
        }
        Ok(Some(Self {
            position,
            unit: unit.filter(|value| !value.trim().is_empty()),
            face: labels.remove("face"),
            node_type: labels.remove("type"),
            labels,
        }))
    }

    /// The alignment as the node's `ql` extension. What was authored rides
    /// whole; nothing is added that the frontmatter did not declare.
    fn extension(&self) -> Value {
        let mut ql = serde_json::Map::new();
        if let Some(position) = self.position {
            ql.insert("position".into(), jval!(position));
        }
        if let Some(unit) = &self.unit {
            ql.insert("unit".into(), jval!(unit));
        }
        if let Some(face) = &self.face {
            ql.insert("face".into(), jval!(face));
        }
        for (key, value) in &self.labels {
            ql.insert(key.clone(), jval!(value));
        }
        Value::Object(ql)
    }
}

fn strip_frontmatter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

fn split_label(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value.trim()))
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

/// Stage one source file into the Wiki by its authored QL frontmatter. The
/// source stays the ground — the node records its alignment and links back to
/// it; the prose is never copied or rewritten. A file without an alignment is
/// a refusal, not a guess: plain nodes belong to `wiki node create`.
fn stage(args: &WikiStageArgs) -> Result<WikiOutcome> {
    let text = read(&args.source)?;
    let alignment = QlAlignment::from_markdown(&text)?.ok_or_else(|| {
        AikitError::new(
            "knowledge.wiki_stage_alignment",
            format!(
                "{} declares no `ql:` frontmatter; staging records an authored alignment, \
                 it does not guess one",
                args.source.display()
            ),
        )
        .with("source", args.source.display().to_string())
    })?;
    if let Some(face) = &alignment.face {
        if face != "direct" && face != "conjugate" {
            return Err(AikitError::new(
                "knowledge.wiki_stage_alignment",
                format!("`{face}` is not a face; use direct or conjugate"),
            )
            .with("source", args.source.display().to_string()));
        }
    }

    let stem = args
        .source
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "source".to_string());
    let slug: String = stem
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let node_ref = ResourceRef::parse(
        args.node_ref
            .as_deref()
            .unwrap_or(&format!("wiki:node:staged/{slug}")),
    )?;
    let node_type = alignment
        .node_type
        .clone()
        .unwrap_or_else(|| "staged-source".to_string());
    let title = args
        .title
        .clone()
        .or_else(|| first_heading(&text))
        .unwrap_or_else(|| stem.clone());
    let source_label = args
        .source_ref
        .clone()
        .unwrap_or_else(|| format!("staging/{slug}"));

    let mut extensions = BTreeMap::new();
    extensions.insert("ql".to_string(), alignment.extension());
    let node = WikiNode {
        profile: OKF_WIKI_PROFILE.to_string(),
        ref_id: node_ref.clone(),
        revision: 1,
        provenance: provenance_from_sources(std::slice::from_ref(&source_label))?,
        node_type,
        title: Some(title),
        space_refs: parse_refs(&args.space, "space")?,
        source_refs: parse_source_refs(&[source_label])?,
        local_space_ref: None,
        extensions,
    };

    // A held ref is an update when the caller says so, a refusal otherwise —
    // the same law as `node create`/`node update`, so staging never quietly
    // replaces an authored node.
    let held = WikiDocument::parse(&read(&args.file)?)?;
    if held.holds(&node.ref_id) && !args.update {
        return Err(AikitError::new(
            "knowledge.wiki_ref_exists",
            format!(
                "{} is already held by {}; pass --update to advance its revision",
                node.ref_id,
                args.file.display()
            ),
        )
        .with("ref", node.ref_id.to_string()));
    }

    let spaces = node.space_refs.clone();
    let outcome = mutate_file(&args.file, |doc, ledger| {
        let written = if held.holds(&node.ref_id) {
            doc.update_object(WikiObject::Node(node.clone()))?
        } else {
            doc.create_object(WikiObject::Node(node.clone()))?
        };
        ledger.record(written);
        let synced = doc.sync_space_memberships(&node)?;
        ledger.record(synced);
        Ok(())
    })?;
    Ok(WikiOutcome::wrote(
        jval!({
            "command": "stage",
            "file": args.file.display().to_string(),
            "source": args.source.display().to_string(),
            "ref": node_ref.to_string(),
            "spaces": refs_json(&spaces),
            "alignment": alignment.extension(),
            "outcome": mutation_outcome(&outcome),
        }),
        &outcome,
    ))
}

fn first_heading(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .filter(|heading| !heading.is_empty())
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// ingest — the corpus on-ramp
// ---------------------------------------------------------------------------

/// A defensive bound, not a real limit: the Return of Zero corpus itself is
/// ~2,600 files, so this is headroom against pointing the walker at the
/// wrong directory entirely (a home directory, a mounted drive), not
/// against a corpus this shape is meant for.
const WIKI_INGEST_MAX_FILES: usize = 50_000;

/// Walk `root` into [`ingest_corpus`]'s input shape: `(relative path, text)`
/// pairs, sorted lexicographically so ingestion never depends on filesystem
/// iteration order — the record-id collision policy in
/// `select_ingestable_records` keeps the *first* claim to an id, so a stable
/// order is what makes that choice reproducible across machines and runs,
/// not just deterministic on one. A file that cannot be read, or does not
/// decode as UTF-8, is set aside with an honest diagnostic rather than
/// aborting the whole walk; a tree this size always has a few of both
/// (a stray binary asset with a `.md`-adjacent name, a symlink into
/// somewhere unreadable).
/// The raw corpus read from disk: every readable, UTF-8 `.<extension>` file
/// as `(relative path, text)`, plus the diagnostics for what the walk itself
/// could not read.
struct WalkedCorpus {
    files: Vec<(String, String)>,
    skipped: Vec<String>,
}

fn walk_corpus(root: &Path, extension: &str) -> Result<WalkedCorpus> {
    if !root.is_dir() {
        return Err(AikitError::new(
            "knowledge.ingest_corpus_unreadable",
            format!(
                "{} is not a directory; ingest walks a corpus root, not a single file",
                root.display()
            ),
        )
        .with("corpus", root.display().to_string()));
    }
    let suffix = format!(".{extension}");
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !name.ends_with(&suffix) {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        found.push((relative, path.to_path_buf()));
    }
    if found.len() > WIKI_INGEST_MAX_FILES {
        return Err(AikitError::new(
            "knowledge.ingest_corpus_too_large",
            format!(
                "{} holds {} `.{extension}` files, past the {WIKI_INGEST_MAX_FILES}-file bound; \
                 point ingest at a narrower corpus root",
                root.display(),
                found.len()
            ),
        )
        .with("corpus", root.display().to_string()));
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));

    let mut corpus = Vec::with_capacity(found.len());
    let mut skipped = Vec::new();
    for (relative, path) in found {
        match std::fs::read(&path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => corpus.push((relative, text)),
                Err(_) => skipped.push(format!(
                    "{relative}: not valid UTF-8; set aside, not ingested"
                )),
            },
            Err(error) => skipped.push(format!(
                "{relative}: unreadable ({error}); set aside, not ingested"
            )),
        }
    }
    Ok(WalkedCorpus {
        files: corpus,
        skipped,
    })
}

/// Ingest an authored corpus directory into a Wiki file.
///
/// Three stages, each disclosed in the reply rather than folded away: the
/// filesystem walk (`walk_corpus`, IO — this crate's business, never
/// `aikit-core`'s), record selection over the real mixed tree
/// (`select_ingestable_records`), and compilation (`ingest_corpus`). Dry run
/// by default, like `wiki root prune`: `--apply` is required to actually
/// write, and `--update` is required to replace a ref the file already
/// holds — ingest never silently overwrites an authored or previously
/// ingested object.
fn ingest(args: &WikiIngestArgs) -> Result<WikiOutcome> {
    let walked = walk_corpus(&args.corpus, &args.extension)?;
    let (raw_corpus, io_skipped) = (walked.files, walked.skipped);
    let selection = select_ingestable_records(&raw_corpus);
    let compiled = ingest_corpus(&selection.records, &selection.sources, args.room_depth)?;
    let (objects, material, absences) =
        (compiled.objects, compiled.material, compiled.absences);
    let pool_dir = source_pool_dir(args);

    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(io_skipped.iter().cloned());
    warnings.extend(selection.unparseable.iter().map(|path| {
        format!(
            "{path}: a `---` frontmatter fence opened but carried no readable `key: value` \
             pairs; set aside, not ingested"
        )
    }));
    warnings.extend(selection.duplicate_record_id.iter().cloned());
    warnings.extend(selection.duplicate_source_id.iter().cloned());
    // Files carrying corpus metadata but no identity are named, not counted:
    // this is where an authored tag vocabulary hides when ingest cannot
    // place it.
    warnings.extend(selection.skipped_unaddressable.iter().cloned());
    warnings.extend(absences.iter().cloned());

    let (mut nodes, mut edges, mut spaces) = (0usize, 0usize, 0usize);
    for object in &objects {
        match object {
            WikiObject::Node(_) => nodes += 1,
            WikiObject::Edge(_) => edges += 1,
            WikiObject::Space(_) => spaces += 1,
            WikiObject::Frame(_) | WikiObject::Reading(_) => {}
        }
    }
    let mut tag_vocabulary: BTreeMap<String, usize> = BTreeMap::new();
    for item in &material {
        for tag in &item.binding.tags {
            *tag_vocabulary.entry(tag.clone()).or_default() += 1;
        }
    }
    let mut summary = jval!({
        "command": "ingest",
        "corpus": args.corpus.display().to_string(),
        "file": args.file.display().to_string(),
        "source_pool": pool_dir.display().to_string(),
        "room_depth": args.room_depth,
        "files_read": raw_corpus.len(),
        "io_skipped": io_skipped.len(),
        "records_selected": selection.records.len(),
        "sources_selected": selection.sources.len(),
        "skipped_inert": selection.skipped_inert,
        "skipped_unaddressable": selection.skipped_unaddressable.len(),
        "duplicate_record_id": selection.duplicate_record_id.len(),
        "duplicate_source_id": selection.duplicate_source_id.len(),
        "unparseable": selection.unparseable.len(),
        "objects": objects.len(),
        "nodes": nodes,
        "edges": edges,
        "spaces": spaces,
        "source_bindings": material.len(),
        "tag_vocabulary": tag_vocabulary.len(),
        "tagged_bindings": material
            .iter()
            .filter(|item| !item.binding.tags.is_empty())
            .count(),
        "absences": absences.len(),
    });

    if !args.apply {
        let held = WikiDocument::parse(&read(&args.file)?)?;
        let already_held = objects
            .iter()
            .filter(|object| held.holds(object.ref_id()))
            .count();
        summary["applied"] = jval!(false);
        summary["already_held"] = jval!(already_held);
        summary["source_pool_files"] = jval!(0);
        summary["note"] = jval!(
            "dry run; re-run with --apply to write these objects \
             (pass --update too if any are already held and should advance)"
        );
        return Ok(WikiOutcome::reported(summary, warnings, json::EXIT_OK));
    }

    let update = args.update;
    let file_display = args.file.display().to_string();
    let outcome = mutate_file(&args.file, |doc, ledger| {
        for object in objects {
            let ref_id = object.ref_id().clone();
            let touched = if doc.holds(&ref_id) {
                if !update {
                    return Err(AikitError::new(
                        "knowledge.wiki_ref_exists",
                        format!(
                            "{ref_id} is already held by {file_display}; pass --update to \
                             advance its revision"
                        ),
                    )
                    .with("ref", ref_id.to_string()));
                }
                doc.update_object(object)?
            } else {
                doc.create_object(object)?
            };
            ledger.record(touched);
        }
        Ok(())
    })?;
    let written = write_source_pool(&pool_dir, &material)?;
    summary["applied"] = jval!(true);
    summary["source_pool_files"] = jval!(written);
    summary["outcome"] = mutation_outcome(&outcome);
    let mut reply = WikiOutcome::wrote(summary, &outcome);
    reply.warnings.extend(warnings);
    Ok(reply)
}

/// Where the corpus's SourcePool material is written: `--source-pool` if
/// given, else a `<wiki-file-stem>.sources/` directory beside `--file`.
fn source_pool_dir(args: &WikiIngestArgs) -> PathBuf {
    if let Some(dir) = &args.source_pool {
        return dir.clone();
    }
    let stem = args
        .file
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "wiki".to_owned());
    args.file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}.sources"))
}

/// A shard budget, not a limit: Knowledge discovery skips any candidate file
/// over 4 MiB, and a corpus of a few hundred records with their bodies is
/// comfortably past that in one file. Sharding at 1 MiB keeps every shard
/// discoverable with headroom for a single outsized record.
const SOURCE_POOL_SHARD_BYTES: usize = 1024 * 1024;

/// Write the SourcePool material as discoverable `corpus-NNN.json` shards.
///
/// Only files this command owns are touched: stale `corpus-*.json` shards
/// from a previous, larger run are removed so a shrinking corpus cannot
/// leave orphaned bindings behind, and nothing else in the directory is
/// read, moved or deleted.
fn write_source_pool(dir: &Path, material: &[SourceMaterial]) -> Result<usize> {
    std::fs::create_dir_all(dir).map_err(|error| {
        AikitError::new(
            "knowledge.ingest_source_pool_unwritable",
            format!("{} could not be created: {error}", dir.display()),
        )
    })?;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("corpus-") && name.ends_with(".json") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let mut shards = 0usize;
    let mut shard: Vec<&SourceMaterial> = Vec::new();
    let mut bytes = 0usize;
    let flush = |shard: &mut Vec<&SourceMaterial>, shards: &mut usize| -> Result<()> {
        if shard.is_empty() {
            return Ok(());
        }
        let path = dir.join(format!("corpus-{:03}.json", *shards));
        let text = serde_json::to_string_pretty(&shard).map_err(|error| {
            AikitError::new(
                "knowledge.ingest_source_pool_unwritable",
                format!("SourcePool material could not be rendered: {error}"),
            )
        })?;
        std::fs::write(&path, text).map_err(|error| {
            AikitError::new(
                "knowledge.ingest_source_pool_unwritable",
                format!("{} could not be written: {error}", path.display()),
            )
        })?;
        *shards += 1;
        shard.clear();
        Ok(())
    };
    for item in material {
        let size = item.body.len() + item.binding.title.len() + 512;
        if bytes + size > SOURCE_POOL_SHARD_BYTES && !shard.is_empty() {
            flush(&mut shard, &mut shards)?;
            bytes = 0;
        }
        shard.push(item);
        bytes += size;
    }
    flush(&mut shard, &mut shards)?;
    Ok(shards)
}

// ---------------------------------------------------------------------------
// query — read the semantic index over a Wiki file
// ---------------------------------------------------------------------------

/// Rebuild the semantic index over exactly what `file` holds. This is the
/// plain in-memory `SemanticWikiIndex` — the same read path `wiki validate`
/// already runs — not the materialised SQLite provider `aikit search` reads
/// from an AIKit home; a query needs only the file it names.
fn read_index(file: &Path) -> Result<SemanticWikiIndex> {
    let document = WikiDocument::parse(&read(file)?)?;
    SemanticWikiIndex::rebuild(document.objects().to_vec())
}

fn query_search(args: &WikiQuerySearchArgs) -> Result<WikiOutcome> {
    let index = read_index(&args.file)?;
    let hits = index.search(&args.query, args.limit);
    Ok(WikiOutcome::reported(
        jval!({
            "command": "query.search",
            "file": args.file.display().to_string(),
            "query": args.query,
            "hits": serde_json::to_value(&hits).unwrap_or_default(),
        }),
        Vec::new(),
        json::EXIT_OK,
    ))
}

/// Every object `--ref` points at, outgoing and incoming alike — the ordinary
/// traversal view. Tags and backlinks ride here exactly as any other edge:
/// nothing about a `tagged` relation or an ingested `references` edge is
/// special-cased.
fn query_neighbours(args: &WikiQueryRefArgs) -> Result<WikiOutcome> {
    let index = read_index(&args.file)?;
    let resource = ResourceRef::parse(&args.resource_ref)?;
    let neighbours = index.neighbours(&resource, args.limit);
    Ok(WikiOutcome::reported(
        jval!({
            "command": "query.neighbours",
            "file": args.file.display().to_string(),
            "ref": resource.to_string(),
            "neighbours": serde_json::to_value(&neighbours).unwrap_or_default(),
        }),
        absent_ref_warnings(&index, &resource),
        json::EXIT_OK,
    ))
}

/// An empty traversal has two very different causes: a curated object with
/// nothing attached, and a ref the field does not hold at all. Reported the
/// same way they are indistinguishable, and a caller reads "no relations"
/// where the truth is "not here". Say which.
///
/// A cited authored source lands in the second case by design — it is
/// findable through search without ever being a curated object — so the
/// warning names that rather than implying the ref is unknown.
fn absent_ref_warnings(index: &SemanticWikiIndex, resource: &ResourceRef) -> Vec<String> {
    if index.contains(resource) {
        return Vec::new();
    }
    let source = SourceRef::parse(resource.as_str())
        .ok()
        .filter(|source| !index.citing_nodes(source).is_empty());
    match source {
        Some(source) => vec![format!(
            "{resource} is an authored source cited by {} curated node(s), not a curated object: \
             it has no relations of its own. Search finds it; `query backlinks` on the nodes \
             that cite it shows the citation.",
            index.citing_nodes(&source).len()
        )],
        None => vec![format!(
            "{resource} is not in this Wiki file: an empty result here means absent, not unrelated"
        )],
    }
}

/// Every object that points *at* `--ref` — what cites it. First-class over
/// an ingested corpus: an argument's authored citations and a tag's members
/// are both ordinary backlinks here, not a derived view bolted on after.
fn query_backlinks(args: &WikiQueryRefArgs) -> Result<WikiOutcome> {
    let index = read_index(&args.file)?;
    let resource = ResourceRef::parse(&args.resource_ref)?;
    let mut backlinks = index.backlinks(&resource);
    backlinks.truncate(args.limit);
    let warnings = absent_ref_warnings(&index, &resource);
    Ok(WikiOutcome::reported(
        jval!({
            "command": "query.backlinks",
            "file": args.file.display().to_string(),
            "ref": resource.to_string(),
            "backlinks": serde_json::to_value(&backlinks).unwrap_or_default(),
        }),
        warnings,
        json::EXIT_OK,
    ))
}

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

/// The Central root Wiki: an explicit file, an explicit Central directory, or —
/// and only for the root commands — the nearest ancestor of the working
/// directory holding `Control/agents/wiki/wiki.json`. Discovery resolves a file
/// to *read*; nothing is discovered to *write* without a caller-named path.
fn resolve_root_wiki(cwd: &Path, root: Option<&Path>) -> Result<PathBuf> {
    match root {
        Some(path) if path.is_file() => Ok(path.to_path_buf()),
        Some(path) => {
            let wiki = path.join(CENTRAL_ROOT_WIKI_SOURCE);
            if wiki.is_file() {
                return Ok(wiki);
            }
            Err(AikitError::new(
                "knowledge.wiki_root_unresolved",
                format!(
                    "{path:?} holds no {CENTRAL_ROOT_WIKI_SOURCE}; pass the wiki.json itself or the Central directory"
                ),
            )
            .with("root", path.display().to_string()))
        }
        None => {
            let mut current = Some(cwd);
            while let Some(directory) = current {
                let candidate = directory.join(CENTRAL_ROOT_WIKI_SOURCE);
                if candidate.is_file() {
                    return Ok(candidate);
                }
                current = directory.parent();
            }
            Err(AikitError::new(
                "knowledge.wiki_root_unresolved",
                format!(
                    "no ancestor of {} holds {CENTRAL_ROOT_WIKI_SOURCE}; pass --root",
                    cwd.display()
                ),
            ))
        }
    }
}

/// `<central>` from `<central>/Control/agents/wiki/wiki.json`.
fn central_root(root_wiki: &Path) -> Result<PathBuf> {
    root_wiki
        .ancestors()
        .nth(4)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_root_unresolved",
                format!(
                    "{} is too shallow to be a Central root Wiki; expected {CENTRAL_ROOT_WIKI_SOURCE}",
                    root_wiki.display()
                ),
            )
        })
}

fn root_space(document: &WikiDocument) -> Result<&WikiSpace> {
    let root_ref = ResourceRef::parse(ROOT_WIKI_SPACE_REF)?;
    match document.object(&root_ref) {
        Some(WikiObject::Space(space)) => Ok(space),
        Some(other) => Err(AikitError::new(
            "knowledge.wiki_root_space_missing",
            format!(
                "{ROOT_WIKI_SPACE_REF} is held as a {}, not a Space",
                kind_of(other)
            ),
        )
        .with("space", root_ref.to_string())),
        None => Err(AikitError::new(
            "knowledge.wiki_root_space_missing",
            format!("the Wiki file holds no {ROOT_WIKI_SPACE_REF} Space"),
        )
        .with("space", root_ref.to_string())),
    }
}

// ---------------------------------------------------------------------------
// The pipeline's file half
// ---------------------------------------------------------------------------

/// Run one mutation against one file and persist it: read, mutate in memory,
/// validate the whole, render, atomic rename. A refusal anywhere leaves the
/// file byte-identical — the rendered text exists only after the gate. The
/// hash of what was read travels to `persist` as the compare-and-swap base, so
/// a peer's write landing between this read and the rename is refused rather
/// than silently overwritten.
fn mutate_file<F>(path: &Path, mutate: F) -> Result<WikiMutationOutcome>
where
    F: FnOnce(&mut WikiDocument, &mut WikiMutationLedger) -> Result<()>,
{
    let input = read(path)?;
    let base_hash = content_hash(input.as_bytes());
    let (rendered, outcome) = apply_wiki_mutation(&input, mutate)?;
    persist(path, &rendered, &base_hash)?;
    Ok(outcome)
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_file_unreadable",
            format!("could not read {}: {error}", path.display()),
        )
        .with("path", path.display().to_string())
    })
}

/// SHA-256 of exactly these bytes, hex-encoded. This is the compare-and-swap
/// base: a rewrite that happens to land byte-identical content is never a
/// conflict, only a peer write that actually changed the file is.
fn content_hash(bytes: &[u8]) -> String {
    let mut digest = sha2::Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

/// The current on-disk hash of `path`, or `None` when the file no longer
/// exists — itself a change from whatever a caller read, so a base hash can
/// never match it.
fn current_hash(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(content_hash(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AikitError::new(
            "knowledge.wiki_file_unreadable",
            format!(
                "could not re-read {} to verify it is unchanged before writing: {error}",
                path.display()
            ),
        )
        .with("path", path.display().to_string())),
    }
}

/// The atomic write: a temp file next to the target, then a rename — gated by
/// an optimistic concurrency check run immediately before the rename.
/// `base_hash` is the SHA-256 of the exact bytes the caller read before it
/// mutated in memory; every write path in this file captures it at the same
/// `read` call the mutation was built from. If the file on disk no longer
/// hashes to that value, a peer's write landed first: this write refuses
/// rather than silently discard it, and the target is left exactly as the
/// peer left it — nothing of the peer's write is touched, and nothing of this
/// mutation is applied. A crash mid-write still leaves the previous revision
/// on disk, never a half document.
fn persist(path: &Path, rendered: &str, base_hash: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "wiki.json".to_string());
    let temp = path.with_file_name(format!(".{file_name}.tmp-{}", std::process::id()));
    std::fs::write(&temp, rendered).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_write_failed",
            format!("could not write {}: {error}", temp.display()),
        )
        .with("path", temp.display().to_string())
    })?;

    if current_hash(path)?.as_deref() != Some(base_hash) {
        let _ = std::fs::remove_file(&temp);
        return Err(AikitError::new(
            "knowledge.wiki_concurrent_write",
            format!(
                "{} changed since it was read; a peer write landed first. Re-read the file and re-apply this mutation.",
                path.display()
            ),
        )
        .with("path", path.display().to_string()));
    }

    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        AikitError::new(
            "knowledge.wiki_write_failed",
            format!("could not replace {}: {error}", path.display()),
        )
        .with("path", path.display().to_string())
    })
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn mutation_outcome(outcome: &WikiMutationOutcome) -> Value {
    jval!({
        "changed": outcome.changed,
        "proposals": serde_json::to_value(&outcome.proposals).unwrap_or_default(),
        "touched": serde_json::to_value(&outcome.touched).unwrap_or_default(),
    })
}

fn refs_json(refs: &[ResourceRef]) -> Vec<String> {
    refs.iter().map(|resource| resource.to_string()).collect()
}

fn kind_of(object: &WikiObject) -> &'static str {
    match object {
        WikiObject::Space(_) => "Space",
        WikiObject::Node(_) => "node",
        WikiObject::Edge(_) => "edge",
        WikiObject::Frame(_) => "frame",
        WikiObject::Reading(_) => "reading",
    }
}

fn parse_refs(raw: &[String], flag: &str) -> Result<Vec<ResourceRef>> {
    raw.iter()
        .map(|value| {
            ResourceRef::parse(value).map_err(|error| error.with("flag", format!("--{flag}")))
        })
        .collect()
}

fn parse_source_refs(raw: &[String]) -> Result<Vec<SourceRef>> {
    raw.iter()
        .map(|value| SourceRef::parse(value).map_err(|error| error.with("flag", "--source")))
        .collect()
}

/// Each `--source` becomes one provenance entry at the revision the source
/// carries, which here is "unspecified": the Wiki records the ground, not a
/// claim about it.
fn provenance_from_sources(raw: &[String]) -> Result<Vec<WikiProvenanceRef>> {
    Ok(parse_source_refs(raw)?
        .into_iter()
        .map(|source_ref| WikiProvenanceRef {
            source_ref,
            source_revision: None,
            producer_ref: None,
            generation_ref: None,
            extensions: BTreeMap::new(),
        })
        .collect())
}

/// A node update that names `--source` replaces provenance wholesale — the
/// sources are the node's ground, and a partial ground is a lie — while a node
/// updated without `--source` keeps the ground it had.
fn match_provenance(
    existing: &[WikiProvenanceRef],
    requested: &[String],
) -> Result<Vec<WikiProvenanceRef>> {
    if requested.is_empty() {
        return Ok(existing.to_vec());
    }
    provenance_from_sources(requested)
}

fn read_stdin() -> Result<String> {
    use std::io::Read;
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|error| {
            AikitError::new(
                "cli.stdin_unreadable",
                format!("could not read stdin: {error}"),
            )
        })?;
    if buffer.trim().is_empty() {
        return Err(AikitError::new(
            "cli.usage",
            "--stdin was given but nothing was piped in",
        ));
    }
    Ok(buffer)
}

// ---------------------------------------------------------------------------
// Concurrency: the write gate against a peer that lands between read and rename
// ---------------------------------------------------------------------------
//
// These are unit tests, not `tests/wiki_commands.rs` integration tests, on
// purpose: the race this guards against is a *sequence* — read (capture base),
// a peer's independent read-mutate-persist, then this writer's persist — and
// the CLI's `aikit wiki …` commands each run that whole sequence inside one
// process invocation with no I/O in between the read and the rename. There is
// no pause point to land a second subprocess's write into from outside, short
// of adding a test-only delay hook to production code. Driving `read`,
// `apply_wiki_mutation` and `persist` directly gives the exact interleaving
// the race depends on, deterministically, using the very functions
// `mutate_file` composes them from — not a stand-in for the race, the race
// itself, minus OS scheduling.
#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn document(objects_json: &str) -> String {
        format!("{{\n  \"objects\": [\n    {objects_json}\n  ]\n}}\n")
    }

    fn node_json(ref_id: &str) -> String {
        format!(
            r#"{{
  "object": "node",
  "profile": "okf-wiki/v1",
  "provenance": [],
  "ref": "{ref_id}",
  "revision": 1,
  "space_refs": [],
  "title": "{ref_id}",
  "type": "Note"
}}"#
        )
    }

    fn new_node(ref_id: &str) -> WikiObject {
        WikiObject::Node(WikiNode {
            profile: OKF_WIKI_PROFILE.to_string(),
            ref_id: ResourceRef::parse(ref_id).unwrap(),
            revision: 1,
            provenance: Vec::new(),
            node_type: "Note".to_string(),
            title: Some(ref_id.to_string()),
            space_refs: Vec::new(),
            source_refs: Vec::new(),
            local_space_ref: None,
            extensions: BTreeMap::new(),
        })
    }

    #[test]
    fn an_uncontended_write_still_succeeds() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("wiki.json");
        write(&path, &document(&node_json("wiki:node:a")));

        let outcome = mutate_file(&path, |doc, ledger| {
            ledger.record(doc.create_object(new_node("wiki:node:b"))?);
            Ok(())
        })
        .unwrap();

        assert!(outcome.changed);
        let after = WikiDocument::parse(&read(&path).unwrap()).unwrap();
        assert!(after.holds(&ResourceRef::parse("wiki:node:b").unwrap()));
    }

    /// The race this whole change exists for: writer A reads the file and
    /// builds its mutation from that snapshot; before A commits, writer B
    /// independently reads, mutates and persists the *same* file; A then
    /// attempts to commit its now-stale mutation. Without the fix, A's
    /// `rename` simply wins and B's write vanishes with no trace. With it, A's
    /// `persist` must refuse: B's content is the only thing on disk
    /// afterwards, byte for byte, and no temp file is left behind.
    #[test]
    fn a_peer_write_between_read_and_rename_is_refused_and_survives_byte_identical() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("wiki.json");
        write(&path, &document(&node_json("wiki:node:a")));

        // Writer A: read (captures the base), then mutate in memory. No write
        // has happened yet — exactly where `mutate_file` stands right before
        // its own `persist` call.
        let input_a = read(&path).unwrap();
        let base_hash_a = content_hash(input_a.as_bytes());
        let (rendered_a, _) = apply_wiki_mutation(&input_a, |doc, ledger| {
            ledger.record(doc.create_object(new_node("wiki:node:from-a"))?);
            Ok(())
        })
        .unwrap();

        // Writer B: an independent, complete read-mutate-persist that lands
        // first, through the exact same production path.
        mutate_file(&path, |doc, ledger| {
            ledger.record(doc.create_object(new_node("wiki:node:from-b"))?);
            Ok(())
        })
        .unwrap();
        let after_b = fs::read(&path).unwrap();

        // Writer A now tries to commit its stale mutation.
        let error = persist(&path, &rendered_a, &base_hash_a).unwrap_err();
        assert_eq!(error.code(), "knowledge.wiki_concurrent_write");
        assert!(
            error.message().to_lowercase().contains("re-read"),
            "the refusal must say what to do next: {}",
            error.message()
        );

        // B's write is untouched: byte for byte, not just semantically.
        assert_eq!(
            fs::read(&path).unwrap(),
            after_b,
            "a refused write leaves the file exactly as the peer left it"
        );
        let surviving = WikiDocument::parse(&String::from_utf8(after_b).unwrap()).unwrap();
        assert!(surviving.holds(&ResourceRef::parse("wiki:node:from-b").unwrap()));
        assert!(!surviving.holds(&ResourceRef::parse("wiki:node:from-a").unwrap()));
        assert!(
            surviving.holds(&ResourceRef::parse("wiki:node:a").unwrap()),
            "the document is still whole and valid, not half-written"
        );

        // The refused write's temp file does not linger next to the target.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name() != "wiki.json")
            .collect();
        assert!(
            leftovers.is_empty(),
            "a refused write must not leave a temp file behind: {leftovers:?}"
        );
    }

    /// A rewrite that lands byte-identical content is not a peer's change —
    /// only a peer write that actually altered the file trips the gate.
    #[test]
    fn a_rewrite_that_lands_the_same_bytes_is_not_a_conflict() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("wiki.json");
        let original = document(&node_json("wiki:node:a"));
        write(&path, &original);

        let input = read(&path).unwrap();
        let base_hash = content_hash(input.as_bytes());

        // "Someone" rewrites the file to the exact bytes it already held —
        // e.g. a filesystem sync or an editor save with no real change.
        write(&path, &original);

        // Re-committing the same content over that base is not a conflict.
        persist(&path, &original, &base_hash).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
}
