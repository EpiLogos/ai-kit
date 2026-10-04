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

use aikit_adapters::projectcentral::ProjectCentralFilesystemBinding;
use aikit_core::knowledge_ingest::{
    corpus_content_revision, ingest_corpus_with_origins, CorpusSelection, CorpusSelector, IngestOriginBinding,
};
use aikit_core::knowledge_source_pool::{SourceMaterial, SourceOrigin, SourceVisibility};
use aikit_core::knowledge_wiki::{
    WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject, WikiProvenanceRef, WikiSpace, OKF_WIKI_PROFILE,
};
use aikit_core::knowledge_wiki_write::{
    apply_wiki_mutation, project_id_from_space_ref, project_wiki_space_ref, WikiDocument,
    WikiMutationLedger, WikiMutationOutcome, ROOT_WIKI_SPACE_REF,
};
use aikit_core::projectcentral::{
    plan_agent_wiki_maintenance, AgentWikiMaintenancePlan, AgentWikiMaintenanceRequest,
    HumanSourceRevisionProposal, CENTRAL_ROOT_WIKI_SOURCE, PROJECTCENTRAL_WIKI_SOURCE,
};
use aikit_core::resource::{ResourceRef, SourceRef};
use aikit_core::{AikitError, Result, SemanticRevision, SemanticWikiIndex};

use crate::cli::{
    WikiCmd, WikiEdgeArgs, WikiIngestArgs, WikiMaintenanceArgs, WikiNodeArgs, WikiQueryRefArgs,
    WikiQuerySearchArgs, WikiQuerySub, WikiRootAdoptArgs, WikiRootAnchorArgs, WikiRootArgs,
    WikiRootPruneArgs, WikiSpaceCreateArgs, WikiSpaceLinkArgs, WikiStageArgs,
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
        WikiSub::Projection(args) => crate::wiki_projection::run(args),
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
        WikiSub::Ingest(args) => ingest(cwd, &args),
        WikiSub::Query(query) => match query.command {
            WikiQuerySub::Search(args) => query_search(&args),
            WikiQuerySub::Neighbours(args) => query_neighbours(&args),
            WikiQuerySub::Backlinks(args) => query_backlinks(&args),
        },
        WikiSub::Maintenance(args) => maintenance(cwd, &args),
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
            let parent_receipt = mutate_file_receipt(&args.file, link)?;
            let completed = parent_receipt.completed_effects();
            let child_receipt = mutate_file_receipt(child_file, link)
                .map_err(|error| extend_command_failure(error, &completed))?;
            let parent_outcome = parent_receipt.outcome;
            let child_outcome = child_receipt.outcome;
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
/// The exit is non-zero when anything dangles, so a scheduled or scripted run
/// that finds a broken federation is loud in the shell, not only in its JSON.
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

    let exit_code = if dangling.is_empty() {
        json::EXIT_OK
    } else {
        json::EXIT_GENERIC
    };
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
        exit_code,
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
            let central = central_root(&root)?;
            // The identity the anchor cites is the live manifest, not the
            // stub. `Control/user/identity.md` was folded into
            // `Control/user/identity/` on 2026-09-03 and says so in its own
            // body — it survives only so older named selections still
            // resolve. `central_entities::read_identity_entity` already reads
            // the manifest; anchoring against the tombstone made the two
            // paths cite different sources for the same node. The stub stays
            // as the fallback for a world that has not folded yet.
            let manifest = "Control/user/identity/manifest.json";
            let stub = "Control/user/identity.md";
            let source = if central.join(manifest).is_file() {
                Some(manifest)
            } else if central.join(stub).is_file() {
                Some(stub)
            } else {
                None
            };
            (
                root,
                ROOT_WIKI_SPACE_REF.to_string(),
                minimal_root_node("wiki:node:identity", "identity", "User identity", source)?,
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
                        format!("position {value} is out of range; a unit's positions run 0–5"),
                    ));
                }
                Some(value)
            }
            None => None,
        };
        let unit = labels.remove("unit");
        if position.is_some()
            && unit
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none()
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

// Operation-local qualification defaults, not compiler meaning or an RSS claim.
const WIKI_INGEST_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const WIKI_INGEST_READ_BYTES: usize = 256 * 1024 * 1024;
const WIKI_INGEST_SELECTED_BYTES: usize = 64 * 1024 * 1024;
const WIKI_INGEST_RENDERED_BYTES: usize = 128 * 1024 * 1024;
const SOURCE_POOL_DISCOVERY_BYTES: usize = 4 * 1024 * 1024;

fn corpus_capacity_error(dimension: &str, limit: usize, observed: usize) -> AikitError {
    AikitError::new("knowledge.ingest_corpus_capacity",
        "The complete selected corpus exceeds a native pipeline capacity; select a narrower corpus without clipping a Source")
        .with("dimension", dimension)
        .with("capacity_limit", limit.to_string())
        .with("observed_lower_bound", observed.to_string())
        .with("remaining_corpus", "unknown")
}

#[derive(Default)]
struct CorpusReadCapacity {
    observed_payload_bytes: usize,
    failed_payload_reserved_bytes: usize,
    selected_text_bytes: usize,
    failed_read_causes: Vec<Value>,
}

impl CorpusReadCapacity {
    fn next_read_limit(&self) -> Result<usize> {
        let used = self.observed_payload_bytes.checked_add(self.failed_payload_reserved_bytes)
            .ok_or_else(|| self.read_failure(None))?;
        let available = WIKI_INGEST_READ_BYTES.saturating_sub(used);
        if available == 0 {
            // Refuse before observing the next file: it may be empty. Known
            // payload and failed-attempt reservations exhaust the allowance,
            // but neither establishes an observed byte overage.
            return Err(AikitError::new("knowledge.ingest_corpus_capacity",
                "The initial payload read allowance is exhausted before observing the next Source")
                .with("dimension", "initial_read_payload")
                .with("capacity_limit", WIKI_INGEST_READ_BYTES.to_string())
                .with("capacity_used", used.to_string())
                .with("admission_reason", "initial_read_allowance_exhausted")
                .with("remaining_read_allowance", "0")
                .with("next_material", "unobserved")
                .with("remaining_corpus", "unknown"));
        }
        Ok(available.min(WIKI_INGEST_SOURCE_BYTES))
    }

    fn read_failure(&self, next_observed_lower_bound: Option<usize>) -> AikitError {
        let mut failure = AikitError::new("knowledge.ingest_corpus_capacity",
            "The selected corpus cannot complete within its logical read allowance")
            .with("dimension", "initial_read_payload")
            .with("capacity_limit", WIKI_INGEST_READ_BYTES.to_string())
            .with("observed_payload_bytes", self.observed_payload_bytes.to_string())
            .with("failed_payload_reserved_bytes", self.failed_payload_reserved_bytes.to_string())
            .with("failed_read_causes", jval!(self.failed_read_causes).to_string());
        let known = self.observed_payload_bytes as u128;
        let charged = known + self.failed_payload_reserved_bytes as u128;
        if let Some(next) = next_observed_lower_bound {
            // The actual physical reader saw at least limit+1 bytes. Prior
            // failed-read reservations remain budget charges, not observations.
            failure = failure.with("next_payload_observed_lower_bound", next.to_string())
                .with("observed_lower_bound", (known + next as u128).to_string())
                .with("budget_charge_lower_bound", (charged + next as u128).to_string())
                .with("lower_bound_basis", "returned_payload_plus_actual_next_physical_bound")
                .with("budget_charge_basis", "observed_payload_plus_failed_attempt_reservations_plus_actual_next_bound");
        } else {
            failure = failure.with("budget_charge_lower_bound", charged.to_string())
                .with("next_material", "unobserved")
                .with("arithmetic_refusal", "logical_counter_overflow");
        }
        failure.with("remaining_corpus", "unknown")
    }

    fn annotate_failure(&self, error: AikitError, files_read: usize) -> AikitError {
        error.with("files_read", files_read.to_string())
            .with("observed_payload_bytes", self.observed_payload_bytes.to_string())
            .with("failed_payload_reserved_bytes", self.failed_payload_reserved_bytes.to_string())
            .with("selected_text_bytes", self.selected_text_bytes.to_string())
            .with("failed_read_causes", jval!(self.failed_read_causes).to_string())
    }

    fn retain_selected(&mut self, text: &str) -> Result<()> {
        let next = self.selected_text_bytes.checked_add(text.len()).ok_or_else(|| {
            corpus_capacity_error("selected_text", WIKI_INGEST_SELECTED_BYTES, usize::MAX)
        })?;
        if next > WIKI_INGEST_SELECTED_BYTES {
            return Err(corpus_capacity_error("selected_text", WIKI_INGEST_SELECTED_BYTES, next));
        }
        self.selected_text_bytes = next;
        Ok(())
    }
}


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
/// The admitted selection, moved directly from one-file observations, plus
/// actual decoded-input counts and honest IO/participation diagnostics. Inert,
/// duplicate and unaddressable bodies are dropped before the next observation.
struct WalkedCorpus {
    selection: CorpusSelection,
    files_read: usize,
    observed_payload_bytes: usize,
    failed_payload_reserved_bytes: usize,
    selected_text_bytes: usize,
    skipped: Vec<String>,
    participation_warnings: Vec<String>,
    participation_observations: Vec<Value>,
}

fn corpus_io_error(error: std::io::Error) -> AikitError {
    AikitError::new("knowledge.ingest_corpus_unreadable", error.to_string())
        .with("cause_kind", format!("{:?}", error.kind()))
        .with("cause_raw_os_error", jval!(error.raw_os_error()).to_string())
        .with_io_source(error)
}

/// Native floors govern retrieval even when the explicit authored corpus is
/// not a registered native Source. A floor or a scratch path never mints one.
struct CorpusAdmission {
    project: Option<(PathBuf, ProjectCentralFilesystemBinding)>,
    central_root: Option<PathBuf>,
}

impl CorpusAdmission {
    fn new(cwd: &Path, corpus: &Path) -> Result<Self> {
        Self::new_with_selected_root(cwd, corpus, None)
    }

    fn new_with_selected_root(cwd: &Path, corpus: &Path, selected_root: Option<Option<&Path>>) -> Result<Self> {
        let mut remaining = 2 * WIKI_INGEST_SOURCE_BYTES;
        Self::new_with_read_budget(cwd, corpus, selected_root, None, &mut remaining)
    }

    fn new_with_read_budget(
        cwd: &Path, corpus: &Path, selected_root: Option<Option<&Path>>,
        deadline: Option<std::time::Instant>, remaining_payload_bytes: &mut usize,
    ) -> Result<Self> {
        if let Some(deadline) = deadline { corpus_remaining(deadline)?; }
        let invocation_cwd = if cwd.is_absolute() { cwd.to_path_buf() } else {
            std::env::current_dir().map_err(corpus_io_error)?.join(cwd)
        };
        let physical = std::fs::canonicalize(corpus).map_err(corpus_io_error)?;
        let central_root = if let Some(root) = selected_root {
            root.map(Path::to_path_buf)
        } else {
            // A Service current read has already resolved the invocation's actual
            // configured/optional owner. Do not reinterpret environment/global roots.
            // This existing invocation binding supplies a route, not semantic Source
            // identity. Locate below is the native owner operation that supplies it.
            let configured = std::env::var_os("CENTRAL_ROOT").filter(|value| !value.is_empty()).map(PathBuf::from);
            let root = configured.clone().or_else(|| crate::temporal::process_central_root(Some(cwd)))
                .or_else(|| crate::temporal::central_root_enclosing(Some(cwd)))
                // Native --root resolves relative to the actual invocation cwd.
                // Keep that lexical route, including its accepted root aliases.
                .map(|root| if root.is_absolute() { root } else { invocation_cwd.join(root) });
            let explicitly_configured = configured.is_some();
            let central_root = match root {
                // The configured invocation route survives a missing or unreadable
                // user aperture. Actual member/owner admission below remains required.
                Some(root) if explicitly_configured => Some(root),
                Some(root) => match std::fs::metadata(root.join("Control/user")) {
                        Ok(metadata) if metadata.is_dir() => Some(root),
                        Ok(_) => return Err(AikitError::new("knowledge.ingest_origin_invalid",
                            "The enclosing native user aperture is not a directory")),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                        Err(error) => return Err(corpus_io_error(error)),
                },
                None => None,
            };
            central_root
        };
        let mut admission = Self { project: None, central_root };
        // Check actual enclosing and selected-directory floors before any
        // Project metadata body; neither a directory nor a floor mints an ID.
        admission.check_floor(corpus)?;
        for route in [corpus, physical.as_path()] {
            match std::fs::metadata(route.join(".no-agent-retrieval")) {
                Ok(metadata) if metadata.is_file() => return Err(AikitError::new(
                    "knowledge.ingest_corpus_withheld", "The selected corpus root is withheld")),
                Ok(_) => {}
                Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {}
                Err(cause) => return Err(corpus_io_error(cause)),
            }
        }
        for root in physical.ancestors() {
            if let Some(deadline) = deadline { corpus_remaining(deadline)?; }
            match std::fs::symlink_metadata(root.join(PROJECT_MANIFEST_SOURCE)) {
                Ok(_) => {
                    let relative = physical.strip_prefix(root).map_err(|_|
                        AikitError::new("knowledge.ingest_origin_invalid", "Project floor mapping changed"))?;
                    if !relative.as_os_str().is_empty()
                        && !aikit_adapters::projectcentral::path_agent_readability(root, relative)
                            .map_err(corpus_io_error)?
                    {
                        return Err(AikitError::new("knowledge.ingest_corpus_withheld",
                            "The actual Project marker floor withholds the selected corpus"));
                    }
                    let binding = match deadline {
                        Some(deadline) => ProjectCentralFilesystemBinding::inspect_before(
                            root, admission.central_root.as_deref(), deadline, remaining_payload_bytes)?,
                        None => ProjectCentralFilesystemBinding::inspect(root, admission.central_root.as_deref())?,
                    };
                    admission.project = Some((root.to_path_buf(), binding));
                    break;
                }
                Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {}
                Err(cause) => return Err(corpus_io_error(cause)),
            }
        }
        admission.check_floor(corpus)?;
        if let Some(deadline) = deadline { corpus_remaining(deadline)?; }
        Ok(admission)
    }

    fn known_project_member(&self, physical: &Path) -> bool {
        self.project.as_ref().is_some_and(|(root, binding)| {
            physical.strip_prefix(root).ok().is_some_and(|relative| binding.semantic.sources.iter().any(|source| {
                source.kind != aikit_core::projectcentral::ProjectCentralSourceKind::NativeProjectRoot
                    && source.exists
                    && (relative == source.relative_path || (source.is_directory && relative.starts_with(&source.relative_path)))
            }))
        })
    }

    fn known_native_member(&self, physical: &Path) -> Result<bool> {
        if self.known_project_member(physical) { return Ok(true); }
        let Some(root) = &self.central_root else { return Ok(false); };
        let physical_root = std::fs::canonicalize(root).map_err(|error| {
            corpus_io_error(error)
        })?;
        let Ok(relative) = physical.strip_prefix(&physical_root) else { return Ok(false); };
        // These are the supplied Control root's native source apertures,
        // declared by Central source_horizon::CONTROL_TREE_BINDINGS. Their
        // tree-stamp relation is known even when the executable cannot be
        // reached; this guard neither mints a SourceRef nor recognises a
        // World from an arbitrary enclosing corpus directory.
        Ok([
            "Control/user",
            aikit_core::projectcentral::CENTRAL_ROOT_GOVERNANCE_ROOT,
            "Control/agents/wiki",
            "Control/agents/profiles",
            "Control/agents/expressions",
            "Control/agents/agent-sets",
        ].iter().any(|aperture| relative.starts_with(aperture)))
    }

    fn check_floor(&self, path: &Path) -> Result<()> {
        let physical = std::fs::canonicalize(path).map_err(|error| {
            corpus_io_error(error)
        })?;
        for floor in self.project.as_ref().map(|(root, _)| root).into_iter().chain(self.central_root.as_ref()) {
            let root = std::fs::canonicalize(floor).map_err(|error| {
                corpus_io_error(error)
            })?;
            // Both routes participate. Choosing only a canonical fallback can
            // lose a marker above an in-World lexical member alias.
            let lexical = path.strip_prefix(floor).or_else(|_| path.strip_prefix(&root)).ok();
            let canonical = physical.strip_prefix(&root).ok();
            for (route, relative) in lexical.map(|relative| (floor, relative)).into_iter()
                .chain(canonical.map(|relative| (&root, relative)))
            {
                if relative.as_os_str().is_empty() { continue; }
                let admitted = aikit_adapters::projectcentral::path_agent_readability(route, relative)
                    .map_err(corpus_io_error)?;
                if !admitted {
                    return Err(command_failure(AikitError::new("knowledge.ingest_corpus_withheld",
                        "The actual native source floor withholds this selected member"),
                        &[], "AIKit/SourcePool", path, "selection", "none"));
                }
            }
        }
        Ok(())
    }

    fn recheck_selected(&self, root: &Path, inputs: &[(String, String)], warnings: &mut Vec<String>, observations: &mut Vec<Value>) -> Result<()> {
        for (relative, body) in inputs {
            let path = root.join(relative);
            let failure = |error| command_failure(error, &[], "AIKit/SourcePool", &path, "origin-admission", "none");
            if !aikit_adapters::projectcentral::path_agent_readability(root, Path::new(relative)).map_err(|error| {
                failure(corpus_io_error(error))
            })? {
                return Err(failure(AikitError::new("knowledge.ingest_corpus_withheld", "Source admission changed after selection")));
            }
            self.check_floor(&path)?;
            self.check_native_target(&path, warnings, observations)?;
            let current = aikit_adapters::wiki_publication::material_bytes(&path, 16 * 1024 * 1024).map_err(failure)?;
            if current != body.as_bytes() {
                return Err(failure(AikitError::new("knowledge.ingest_origin_revision_conflict",
                    "Selected source content changed before publication; refresh explicitly")));
            }
            self.check_floor(&path)?;
            if !aikit_adapters::projectcentral::path_agent_readability(root, Path::new(relative)).map_err(|error| {
                failure(corpus_io_error(error))
            })? {
                return Err(failure(AikitError::new("knowledge.ingest_corpus_withheld", "Source admission changed during read")));
            }
        }
        Ok(())
    }

    fn check_native_target(&self, path: &Path, warnings: &mut Vec<String>, observations: &mut Vec<Value>) -> Result<()> {
        let physical = std::fs::canonicalize(path).map_err(|error| {
            corpus_io_error(error)
        })?;
        let known = self.known_native_member(&physical)?;
        let Some(root) = &self.central_root else {
            if known {
                return Err(command_failure(AikitError::new("knowledge.ingest_origin_unavailable",
                    "A bound native source needs its current owner route and target disclosure evidence"),
                    &[], "AIKit/SourcePool", path, "origin-admission", "none"));
            }
            return Ok(());
        };
        let runner = aikit_adapters::runner::SystemRunner::new().with_timeout(std::time::Duration::from_secs(15));
        match aikit_adapters::central_file_map::call(&runner, &aikit_adapters::central_file_map::executable(),
            root, "locate", &jval!({"path": path, "binding_only": true}))
        {
            Ok(owner) if owner["ownership"] == "unregistered" && owner["binding_only"] == true
                && owner["requested_path"] == jval!(path)
                && ["source", "world_ref", "project", "path", "kind", "revision", "content",
                    "content_encoding", "relation_revision", "material_metadata_basis"].iter()
                    .all(|key| owner.get(*key).is_none()) => {
                if known {
                    return Err(command_failure(AikitError::new("knowledge.ingest_origin_unavailable",
                        "Known native input is no longer registered by its current owner")
                        .with("native_reading", owner.to_string()),
                        &[], "AIKit/SourcePool", path, "origin-admission", "none"));
                }
                Ok(())
            }
            Ok(owner) if owner["ownership"] == "owned" && owner["binding_only"] == true
                && owner["source"]["agent_retrieval_allowed"] == true => {
                // Current selected local retrieval does not establish the
                // destination World/Project publication relation. Team describes
                // project eligibility, not permission for external egress.
                Err(command_failure(AikitError::new("knowledge.ingest_target_scope_unproven",
                    "Native source retrieval alone does not establish the destination World/Project publication relation")
                    .with("native_source", owner["source"]["ref"].to_string())
                    .with("native_world", owner["world_ref"].to_string())
                    .with("native_relation_revision", owner["relation_revision"].to_string()),
                    &[], "AIKit/SourcePool", path, "target-admission", "none"))
            }
            Ok(owner) => Err(command_failure(AikitError::new("central.file_map_invalid",
                "Native participation did not return a supported explicit binding disposition")
                .with("native_reading", owner.to_string()),
                &[], "AIKit/SourcePool", path, "origin-admission", "none")),
            Err(error) if !known && error.code() == "central.file_map_unavailable"
                && error.details().get("native_error_code").is_none_or(|code| code.is_empty()) => {
                let warning = "Native participation could not be observed; only the explicitly selected standalone authored corpus contract is used";
                if !warnings.iter().any(|held| held == warning) { warnings.push(warning.into()); }
                // Local invocation depth only: the actual transport/native
                // failure is not persisted in SourceBinding or Wiki provenance
                // and is not proof of nonparticipation or an audience grant.
                let observation = jval!({
                    "participation": "unavailable",
                    "owner_operation": "central.file-map.locate",
                    "original_error": {"code": error.code(), "message": error.message(), "details": error.details()},
                });
                if !observations.contains(&observation) { observations.push(observation); }
                Ok(())
            }
            Err(error) => Err(command_failure(error, &[], "AIKit/SourcePool", path, "origin-admission", "none")),
        }
    }
}

fn walk_corpus(root: &Path, extension: &str, admission: &CorpusAdmission) -> Result<WalkedCorpus> {
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
    // A selected excluded root is a refusal, not an empty successful refresh:
    // an empty apply would otherwise prune material retained from an earlier run.
    let check_root = || {
        let marker = match std::fs::metadata(root.join(aikit_core::projectcentral::NO_AGENT_RETRIEVAL_MARKER)) {
            Ok(metadata) => metadata.is_file(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(command_failure(
                corpus_io_error(error),
                &[], "AIKit/SourcePool", root, "selection", "none")),
        };
        if marker {
            Err(command_failure(
                AikitError::new(
                    "knowledge.ingest_corpus_withheld",
                    "the selected corpus root is withheld from agent retrieval",
                )
                .with("corpus", root.display().to_string()),
                &[],
                "AIKit/SourcePool",
                root,
                "selection",
                "none",
            ))
        } else {
            Ok(())
        }
    };
    check_root()?;
    admission.check_floor(root)?;
    // Reuse the native fallible policy at the explicit standalone boundary.
    // This does not introduce an above-root convention for an unbound corpus.
    let withheld = |path: &Path| -> Result<bool> {
        if path == root { check_root()?; return Ok(false); }
        let relative = path.strip_prefix(root).map_err(|_| {
            AikitError::new("knowledge.ingest_corpus_unreadable", "Corpus walk escaped its selected boundary")
        })?;
        aikit_adapters::projectcentral::path_agent_readability(root, relative)
            .map(|admitted| !admitted).map_err(|error| command_failure(
                corpus_io_error(error),
                &[], "AIKit/SourcePool", path, "selection", "none"))
    };
    let suffix = format!(".{extension}");
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    let mut skipped = Vec::new();
    let mut participation_warnings = Vec::new();
    let mut participation_observations = Vec::new();
    let mut traversal_error = None;
    let entries = walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_entry(|entry| {
        if traversal_error.is_some() { return false; }
        match withheld(entry.path()) {
            Ok(true) => false,
            Err(error) => { traversal_error = Some(error); false }
            Ok(false) => match admission.check_floor(entry.path()) {
                Ok(()) => true,
                Err(error) if error.code() == "knowledge.ingest_corpus_withheld" => false,
                Err(error) => { traversal_error = Some(error); false }
            },
        }
    });
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                if let Some(path) = error.path() {
                    // A marker may have arrived after entry admission. Its
                    // withheld name must not escape via traversal diagnostics.
                    if withheld(path)? { continue; }
                    match admission.check_floor(path) {
                        Ok(()) => {}
                        Err(failure) if failure.code() == "knowledge.ingest_corpus_withheld" => continue,
                        Err(failure) => return Err(failure),
                    }
                }
                skipped.push(format!("corpus walk could not read an entry: {error}"));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        // The owner's `.no-agent-retrieval` marker prunes a subtree before
        // any descendant is read — the same law the ProjectCentral binding
        // and the NOW-field reader honour. Ingest is a read; a room the
        // owner withheld from agent retrieval must not enter the wiki
        // through the back door of a corpus walk.
        if withheld(path)? {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !name.ends_with(&suffix) {
            continue;
        }
        // Admit capacity before retaining this member's name/path. We have
        // observed one more eligible file, not enumerated the remaining tree.
        if found.len() >= WIKI_INGEST_MAX_FILES {
            return Err(AikitError::new(
                "knowledge.ingest_corpus_too_large",
                format!(
                    "{} has at least {} eligible `.{extension}` files, past the \
                     {WIKI_INGEST_MAX_FILES}-file bound; point ingest at a narrower corpus root",
                    root.display(),
                    WIKI_INGEST_MAX_FILES + 1,
                ),
            )
            .with("corpus", root.display().to_string())
            .with("file_limit", WIKI_INGEST_MAX_FILES.to_string())
            .with("observed_files_lower_bound", (WIKI_INGEST_MAX_FILES + 1).to_string())
            .with("remaining_roster", "unknown"));
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        found.push((relative, path.to_path_buf()));
    }
    if let Some(error) = traversal_error { return Err(error); }
    found.sort_by(|left, right| left.0.cmp(&right.0));

    let mut selector = CorpusSelector::default();
    let mut capacity = CorpusReadCapacity::default();
    let mut files_read = 0usize;
    for (relative, path) in found {
        check_root()?;
        if withheld(&path)? {
            continue;
        }
        admission.check_floor(&path)?;
        admission.check_native_target(&path, &mut participation_warnings, &mut participation_observations)?;
        let read_limit = capacity.next_read_limit()
            .map_err(|error| capacity.annotate_failure(error, files_read))?;
        match aikit_adapters::wiki_publication::material_bytes(&path, read_limit as u64) {
            Ok(bytes) => {
                // Count every returned payload before decoding, including invalid
                // UTF-8. The physical owner's consistency reread is not measured
                // by this logical initial-payload counter.
                capacity.observed_payload_bytes = capacity.observed_payload_bytes.checked_add(bytes.len())
                    .ok_or_else(|| capacity.read_failure(None))?;
                match String::from_utf8(bytes) {
                    Ok(text) => {
                        check_root()?;
                        admission.check_floor(&path)?;
                        if !withheld(&path)? {
                            files_read += 1;
                            selector.push_owned_with(relative, text, |_, text| capacity.retain_selected(text))
                                .map_err(|error| capacity.annotate_failure(error, files_read))?;
                        }
                    }
                    Err(_) => skipped.push(format!(
                        "{relative}: not valid UTF-8; set aside, not ingested"
                    )),
                }
            }
            Err(error) if error.code() == "knowledge.wiki_publication_budget" => {
                // A physical byte-limit refusal cannot become an IO skip and
                // hence a falsely complete or empty refresh. Retain its cause.
                let failure = if read_limit < WIKI_INGEST_SOURCE_BYTES {
                    capacity.read_failure(Some(read_limit + 1))
                } else {
                    corpus_capacity_error("source_payload", WIKI_INGEST_SOURCE_BYTES, read_limit + 1)
                };
                return Err(capacity.annotate_failure(failure.with("original_error", jval!({
                    "code":error.code(), "message":error.message(), "details":error.details(),
                }).to_string()), files_read));
            }
            Err(error) => {
                // An unsuccessful physical observation has no byte-count receipt.
                // Charge its bounded attempt conservatively; do not call this
                // reservation observed bytes or erase the original IO failure.
                capacity.failed_payload_reserved_bytes = capacity.failed_payload_reserved_bytes.checked_add(read_limit)
                    .ok_or_else(|| capacity.read_failure(None))?;
                capacity.failed_read_causes.push(jval!({
                    "source_path":path.display().to_string(), "code":error.code(),
                    "message":error.message(), "details":error.details(),
                }));
                skipped.push(format!("{relative}: unreadable ({error}); set aside, not ingested"));
            }
        }
    }
    check_root()?;
    Ok(WalkedCorpus {
        selection: selector.finish(),
        files_read,
        observed_payload_bytes: capacity.observed_payload_bytes,
        failed_payload_reserved_bytes: capacity.failed_payload_reserved_bytes,
        selected_text_bytes: capacity.selected_text_bytes,
        skipped,
        participation_warnings,
        participation_observations,
    })
}

/// One expressly selected current read; no Wiki or SourcePool publication.
pub(crate) struct CurrentCorpusReading {
    pub material: Vec<aikit_core::knowledge_source_pool::SourceMaterial>,
    pub privacy: aikit_core::context_source::ContextSourcePrivacy,
}

pub(crate) fn read_current_corpus(
    cwd: &Path,
    selection: &crate::app::CurrentCorpusSelection,
    native_root: Option<&Path>,
    owner: Option<&aikit_adapters::central_file_map::CentralFileMapProvider<aikit_adapters::runner::SystemRunner>>,
    member: Option<&str>,
    target: aikit_core::context_source::RetrievalTarget,
    deadline: std::time::Instant,
) -> Result<CurrentCorpusReading> {
    use aikit_core::knowledge_ingest::compile_corpus_for_current_read;
    let cwd = if cwd.is_absolute() { cwd.to_path_buf() } else {
        std::env::current_dir().map_err(corpus_io_error)?.join(cwd)
    };
    let root = if selection.corpus.is_absolute() { selection.corpus.clone() } else {
        cwd.join(&selection.corpus)
    };
    let root_metadata = std::fs::metadata(&root).map_err(corpus_io_error)?;
    if !root_metadata.is_dir() {
        return Err(AikitError::new("knowledge.corpus_selection_invalid", "Current corpus root is not a directory"));
    }
    let root_basis = current_corpus_root_basis(&root)?;
    corpus_remaining(deadline)?;
    // One logical operation allowance includes returned native metadata and
    // both preliminary/final corpus observations. Physical consistency rereads
    // do not constitute a separately measured logical payload receipt.
    let mut remaining_payload_bytes = WIKI_INGEST_READ_BYTES;
    let admission = CorpusAdmission::new_with_read_budget(
        &cwd, &root, Some(native_root), Some(deadline), &mut remaining_payload_bytes,
    )?;
    if admission.central_root.is_some() && owner.is_none() {
        return Err(AikitError::new("knowledge.corpus_owner_unavailable",
            "The selected corpus needs its configured current native owner"));
    }
    corpus_remaining(deadline)?;
    let first = current_corpus_inputs(&root, &root_basis, &selection.extension, &admission, owner, member, target, deadline, &mut remaining_payload_bytes)?;
    let (compiled, privacy) = compile_corpus_for_current_read(&first.0.records, &first.0.sources,
        selection.room_depth, &first.1, &first.2, target)?;
    // Membership and all observed inputs are checked again, including inert
    // files that may have become new compiler inputs. No memo or old shard is
    // a current read witness. Both observations use this same operation budget.
    let second = current_corpus_inputs(&root, &root_basis, &selection.extension, &admission, owner, member, target, deadline, &mut remaining_payload_bytes)?;
    let (current, current_privacy) = compile_corpus_for_current_read(&second.0.records, &second.0.sources,
        selection.room_depth, &second.1, &second.2, target)?;
    corpus_remaining(deadline)?;
    if first.3 != second.3 || compiled.material != current.material || privacy != current_privacy {
        return Err(AikitError::new("knowledge.source_origin_revision_conflict",
            "The complete current corpus changed during compilation; refresh explicitly"));
    }
    // Reuse the existing serializer capacity check without publishing a shard.
    validate_current_corpus_root(&root, &root_basis)?;
    let rendered = render_source_pool(&compiled.material)?;
    drop(rendered);
    Ok(CurrentCorpusReading { material: compiled.material, privacy })
}

fn corpus_remaining(deadline: std::time::Instant) -> Result<std::time::Duration> {
    deadline.checked_duration_since(std::time::Instant::now()).filter(|time| !time.is_zero())
        .ok_or_else(|| AikitError::new("knowledge.corpus_read_incomplete",
            "The complete current corpus read exhausted its operation budget"))
}

type CurrentCorpusInputs = (
    aikit_core::knowledge_ingest::CorpusSelection,
    BTreeMap<String, IngestOriginBinding>,
    BTreeMap<String, aikit_core::context_source::ContextSourcePrivacy>,
    BTreeMap<String, (String, Option<aikit_core::knowledge_source_pool::SourceOrigin>, aikit_core::context_source::ContextSourcePrivacy)>,
);

// These are operation-local physical coordinates, never semantic ownership.
fn current_corpus_root_basis(root: &Path) -> Result<(PathBuf, (u64, u64))> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt;
        let canonical = std::fs::canonicalize(root).map_err(corpus_io_error)?;
        let metadata = std::fs::metadata(&canonical).map_err(corpus_io_error)?;
        if !metadata.is_dir() {
            return Err(AikitError::new("knowledge.corpus_read_incomplete", "Current corpus root changed physical form"));
        }
        Ok((canonical, (metadata.dev(), metadata.ino())))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = root;
        Err(AikitError::new("knowledge.wiki_publication_metadata_unsupported", "Current corpus held physical observation is unavailable on this platform"))
    }
}

fn validate_current_corpus_root(root: &Path, basis: &(PathBuf, (u64, u64))) -> Result<()> {
    if current_corpus_root_basis(root)? != *basis {
        return Err(AikitError::new("knowledge.source_origin_revision_conflict", "Current corpus root affiliation changed"));
    }
    Ok(())
}

fn current_corpus_inputs(
    root: &Path, root_basis: &(PathBuf, (u64, u64)), extension: &str, admission: &CorpusAdmission,
    owner: Option<&aikit_adapters::central_file_map::CentralFileMapProvider<aikit_adapters::runner::SystemRunner>>,
    member: Option<&str>, target: aikit_core::context_source::RetrievalTarget,
    deadline: std::time::Instant, remaining_payload_bytes: &mut usize,
) -> Result<CurrentCorpusInputs> {
    use aikit_core::context_source::ContextSourcePrivacy;
    use aikit_core::knowledge_source_pool::SourcePoolReading;
    validate_current_corpus_root(root, root_basis)?;
    admission.check_floor(root)?;
    match std::fs::metadata(root.join(".no-agent-retrieval")) {
        Ok(metadata) if metadata.is_file() => return Err(AikitError::new("knowledge.ingest_corpus_withheld",
            "The selected current corpus root is withheld")),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(corpus_io_error(error)),
    }
    let mut found = Vec::new();
    let mut visited = 0usize;
    let mut floor_failure = None;
    let mut entries = walkdir::WalkDir::new(root).follow_links(false).max_open(8).into_iter().filter_entry(|entry| {
        if floor_failure.is_some() { return false; }
        let result = corpus_remaining(deadline).and_then(|_| admission.check_floor(entry.path())).and_then(|_| {
            let relative = entry.path().strip_prefix(root).map_err(|_|
                AikitError::new("knowledge.corpus_read_incomplete", "Current corpus escaped its selected boundary"))?;
            if relative.as_os_str().is_empty() {
                // Existing per-member predicate does not test its own root.
                match std::fs::metadata(root.join(".no-agent-retrieval")) {
                    Ok(metadata) => Ok(!metadata.is_file()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
                    Err(error) => Err(corpus_io_error(error)),
                }
            } else {
                aikit_adapters::projectcentral::path_agent_readability(root, relative).map_err(corpus_io_error)
            }
        });
        match result {
            Ok(allowed) => allowed,
            Err(error) if error.code() == "knowledge.ingest_corpus_withheld" => false,
            Err(error) => { floor_failure = Some(error); false }
        }
    });
    for entry in &mut entries {
        corpus_remaining(deadline)?;
        let entry = entry.map_err(|error| {
            let message = error.to_string();
            match error.into_io_error() {
                Some(cause) => AikitError::new("knowledge.corpus_read_incomplete", message).with_io_source(cause),
                None => AikitError::new("knowledge.corpus_read_incomplete", message),
            }
        })?;
        visited += 1;
        if visited > WIKI_INGEST_MAX_FILES {
            return Err(corpus_capacity_error("candidate_entries", WIKI_INGEST_MAX_FILES, visited));
        }
        if entry.file_type().is_dir() { continue; }
        let relative = entry.path().strip_prefix(root).map_err(|_|
            AikitError::new("knowledge.corpus_read_incomplete", "Current corpus escaped its selection"))?;
        let relative = relative.to_str().ok_or_else(||
            AikitError::new("knowledge.corpus_read_incomplete", "Current corpus member has no exact text compiler coordinate"))?;
        if relative.contains('\\') {
            return Err(AikitError::new("knowledge.corpus_read_incomplete", "Current compiler key cannot alias a literal backslash"));
        }
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some(extension) { continue; }
        // Nonregular selected forms reach the SAME held reader's finite form
        // refusal. Do not let a FIFO become a skipped authoritative input.
        found.push((relative.to_owned(), entry.path().to_path_buf()));
    }
    drop(entries);
    if let Some(error) = floor_failure { return Err(error); }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    let mut selector = CorpusSelector::default();
    let mut capacity = CorpusReadCapacity {
        observed_payload_bytes: WIKI_INGEST_READ_BYTES - *remaining_payload_bytes,
        ..CorpusReadCapacity::default()
    };
    let mut origins = BTreeMap::new();
    let mut privacy = BTreeMap::new();
    let mut checkpoint = BTreeMap::new();
    for (relative, path) in found {
        validate_current_corpus_root(root, root_basis)?;
        admission.check_floor(&path)?;
        if !aikit_adapters::projectcentral::path_agent_readability(root, Path::new(&relative)).map_err(corpus_io_error)? {
            return Err(AikitError::new("knowledge.ingest_corpus_withheld", "Current corpus admission changed during observation"));
        }
        let limit = capacity.next_read_limit()?;
        let native = match owner {
            Some(owner) => match member {
                Some(member) => owner.for_project(member)?.read_selected_path_for(&path, target, corpus_remaining(deadline)?)?,
                None => owner.read_selected_path_for(&path, target, corpus_remaining(deadline)?)?,
            },
            None => None,
        };
        let (body, origin, visibility, owners, current_privacy) = match native {
            Some(reading) => {
                let current_privacy = reading.privacy;
                let material = reading.admit(target)?;
                let origin = material.binding.source_origin()?.ok_or_else(||
                    AikitError::new("knowledge.ingest_origin_invalid", "Actual native read has no native origin"))?;
                (material.body, origin, material.binding.visibility, material.binding.owners, current_privacy)
            }
            None => {
                let physical = std::fs::canonicalize(&path).map_err(corpus_io_error)?;
                if admission.known_native_member(&physical)? {
                    return Err(AikitError::new("knowledge.corpus_owner_unavailable",
                        "Known native input has no current participating owner reading"));
                }
                let current_privacy = ContextSourcePrivacy::default();
                SourcePoolReading::check_target(current_privacy, target)?;
                let bytes = aikit_adapters::wiki_publication::material_bytes_affiliated(root, root_basis.1, Path::new(&relative), limit as u64)?;
                let body = String::from_utf8(bytes).map_err(|_|
                    AikitError::new("knowledge.corpus_read_incomplete", "Selected current corpus input is not UTF-8"))?;
                (body, SourceOrigin::declared_corpus(), SourceVisibility::Team, Vec::new(), current_privacy)
            }
        };
        if body.len() > limit { return Err(corpus_capacity_error("source_payload", limit, body.len())); }
        capacity.observed_payload_bytes += body.len();
        *remaining_payload_bytes -= body.len();
        let revision = aikit_core::SourceRevision::parse(corpus_content_revision(body.as_bytes()))?;
        checkpoint.insert(relative.clone(), (revision.to_string(), Some(origin.clone()), current_privacy));
        admission.check_floor(&path)?;
        if !aikit_adapters::projectcentral::path_agent_readability(root, Path::new(&relative)).map_err(corpus_io_error)? {
            return Err(AikitError::new("knowledge.ingest_corpus_withheld", "Current corpus admission changed after observation"));
        }
        corpus_remaining(deadline)?;
        selector.push_owned_with(relative, body, |relative, body| {
            capacity.retain_selected(body)?;
            origins.insert(relative.to_owned(), IngestOriginBinding {
                origin, content_revision: revision, visibility, owners,
            });
            privacy.insert(relative.to_owned(), current_privacy);
            Ok(())
        })?;
    }
    let selection = selector.finish();
    if !selection.unparseable.is_empty() {
        return Err(AikitError::new("knowledge.corpus_read_incomplete", "Selected corpus has unreadable compiler metadata"));
    }
    corpus_remaining(deadline)?;
    validate_current_corpus_root(root, root_basis)?;
    Ok((selection, origins, privacy, checkpoint))
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
fn ingest(cwd: &Path, args: &WikiIngestArgs) -> Result<WikiOutcome> {
    let supplied_corpus = if args.corpus.is_absolute() { args.corpus.clone() } else { cwd.join(&args.corpus) };
    // Keep the selected lexical route and its native floor on the same
    // invocation basis, including when -C itself is relative. Canonicalising
    // here would erase an alias's withheld lexical ancestor.
    let invocation_cwd = if cwd.is_absolute() { cwd.to_path_buf() } else {
        std::env::current_dir().map_err(|error| {
            command_failure(corpus_io_error(error), &[], "AIKit/SourcePool", &supplied_corpus,
                "selection", "none")
        })?.join(cwd)
    };
    let corpus_root = if args.corpus.is_absolute() { args.corpus.clone() } else { invocation_cwd.join(&args.corpus) };
    let preeffect = |error: AikitError| {
        if error.details().contains_key("command_effect") { error } else {
            command_failure(error, &[], "AIKit/SourcePool", &corpus_root, "selection", "none")
        }
    };
    let admission = CorpusAdmission::new(&invocation_cwd, &corpus_root).map_err(preeffect)?;
    let walked = walk_corpus(&corpus_root, &args.extension, &admission).map_err(preeffect)?;
    let selection = walked.selection;
    let io_skipped = walked.skipped;
    let mut participation_observations = walked.participation_observations;
    let origins = selection.records.iter().chain(&selection.sources).map(|(relative, body)| {
        Ok((relative.clone(), IngestOriginBinding {
            origin: SourceOrigin::declared_corpus(),
            content_revision: aikit_core::SourceRevision::parse(corpus_content_revision(body.as_bytes()))?,
            visibility: SourceVisibility::Team,
            owners: Vec::new(),
        }))
    }).collect::<Result<BTreeMap<_, _>>>()?;
    let compiled = ingest_corpus_with_origins(&selection.records, &selection.sources, args.room_depth, &origins).map_err(preeffect)?;
    let (objects, material, absences) = (compiled.objects, compiled.material, compiled.absences);
    let pool_dir = source_pool_dir(args);

    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(io_skipped.iter().cloned());
    warnings.extend(walked.participation_warnings);
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
        "files_read": walked.files_read,
        "observed_payload_bytes": walked.observed_payload_bytes,
        "failed_payload_reserved_bytes": walked.failed_payload_reserved_bytes,
        "selected_text_bytes": walked.selected_text_bytes,
        "io_skipped": io_skipped.len(),
        "native_participation": participation_observations.clone(),
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

    // A proposal set that collides with itself cannot be written, whatever
    // the file holds. Report it here rather than letting `--apply` discover
    // it: a dry run that cannot predict its own apply is worse than none,
    // and this one used to promise "re-run with --apply to write these
    // objects" and then die on a duplicate ref against an empty file.
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for object in &objects {
        *seen.entry(object.ref_id().to_string()).or_default() += 1;
    }
    let colliding: Vec<String> = seen
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(reference, count)| {
            format!("{count} proposed objects claim `{reference}`; the apply would refuse")
        })
        .collect();
    summary["self_colliding_refs"] = jval!(colliding.len());
    warnings.extend(colliding.iter().cloned());

    // Dry-run success must admit the same complete, discoverable next material
    // as apply; serialization capacity is settled before either can succeed.
    let rendered_material = render_source_pool(&material)?;
    summary["rendered_material_bytes"] = jval!(rendered_material.iter().map(String::len).sum::<usize>());
    summary["prepared_source_pool_files"] = jval!(rendered_material.len());
    if !args.apply {
        let held = WikiDocument::parse(&read(&args.file)?)?;
        let already_held = objects
            .iter()
            .filter(|object| held.holds(object.ref_id()))
            .count();
        summary["applied"] = jval!(false);
        summary["already_held"] = jval!(already_held);
        summary["source_pool_files"] = jval!(0);
        summary["note"] = jval!(if colliding.is_empty() {
            "dry run; re-run with --apply to write these objects \
             (pass --update too if any are already held and should advance)"
        } else {
            "dry run; this proposal set collides with itself and --apply would \
             refuse — see the warnings naming each contested ref"
        });
        return Ok(WikiOutcome::reported(summary, warnings, json::EXIT_OK));
    }

    let update = args.update;
    let file_display = args.file.display().to_string();
    if !io_skipped.is_empty() {
        return Err(command_failure(AikitError::new("knowledge.ingest_corpus_incomplete",
            "corpus IO was incomplete; retained Wiki and SourcePool material were not refreshed")
            .with("skipped", jval!(io_skipped).to_string()), &[], "AIKit/SourcePool",
            &args.corpus, "read_corpus", "none"));
    }
    admission.recheck_selected(&corpus_root, &selection.records, &mut warnings, &mut participation_observations).map_err(preeffect)?;
    admission.recheck_selected(&corpus_root, &selection.sources, &mut warnings, &mut participation_observations).map_err(preeffect)?;
    summary["native_participation"] = jval!(participation_observations);
    let mut unchanged = 0usize;
    let receipt = mutate_file_receipt(&args.file, |doc, ledger| {
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
                // An unchanged corpus re-ingested must not masquerade as new
                // knowledge: identical content, revision aside, keeps the
                // held revision instead of advancing it.
                if doc.holds_equivalent(&object) {
                    unchanged += 1;
                    continue;
                }
                doc.update_object(object)?
            } else {
                doc.create_object(object)?
            };
            ledger.record(touched);
        }
        Ok(())
    })?;
    let written = write_source_pool(&pool_dir, &rendered_material)
        .map_err(|error| extend_command_failure(error, &receipt.completed_effects()))?;
    let outcome = receipt.outcome;
    summary["applied"] = jval!(true);
    summary["unchanged"] = jval!(unchanged);
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

/// An ordinary grouping target, measured from exact serialized material.
/// The existing discovery reader accepts at most4 MiB per candidate file.
const SOURCE_POOL_SHARD_BYTES: usize = 1024 * 1024;

struct BoundedMaterialJson {
    bytes: Vec<u8>,
    limit: usize,
    refused_lower_bound: Option<usize>,
}

impl std::io::Write for BoundedMaterialJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let Some(next) = self.bytes.len().checked_add(bytes.len()) else {
            self.refused_lower_bound = Some(usize::MAX);
            return Err(std::io::Error::other("serialized Source material capacity arithmetic overflowed"));
        };
        if next > self.limit {
            self.refused_lower_bound = Some(next);
            return Err(std::io::Error::other("serialized Source material exceeds its admitted capacity"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

fn append_rendered_fragment(buffer: &mut Vec<u8>, completed_bytes: usize, bytes: &[u8]) -> Result<()> {
    let next = completed_bytes.checked_add(buffer.len()).and_then(|used| used.checked_add(bytes.len()))
        .ok_or_else(|| corpus_capacity_error("rendered_material", WIKI_INGEST_RENDERED_BYTES, usize::MAX))?;
    if next > WIKI_INGEST_RENDERED_BYTES {
        return Err(corpus_capacity_error("rendered_material", WIKI_INGEST_RENDERED_BYTES, next));
    }
    buffer.extend_from_slice(bytes);
    Ok(())
}

fn finish_rendered_shard(buffer: &mut Vec<u8>, completed_bytes: &mut usize, rendered: &mut Vec<String>) -> Result<()> {
    if buffer.is_empty() { return Ok(()); }
    append_rendered_fragment(buffer, *completed_bytes, b"\n]")?;
    let bytes = std::mem::take(buffer);
    *completed_bytes = completed_bytes.checked_add(bytes.len()).ok_or_else(|| {
        corpus_capacity_error("rendered_material", WIKI_INGEST_RENDERED_BYTES, usize::MAX)
    })?;
    let text = String::from_utf8(bytes).map_err(|error| {
        AikitError::new("knowledge.ingest_source_pool_unwritable", error.to_string())
    })?;
    rendered.push(text);
    Ok(())
}

/// Render the complete next material set before dry-run success or any effect.
/// Singleton arrays use the real pretty serde writer, including escaping and
/// metadata. Grouping moves their exact interiors with the same array separators;
/// no body estimate, clipping or synthetic Source identity grants capacity.
fn render_source_pool(material: &[SourceMaterial]) -> Result<Vec<String>> {
    let prepared = (|| {
        let mut rendered = Vec::new();
        let mut shard = Vec::new();
        let mut completed_bytes = 0usize;
        for item in material {
            let mut singleton = BoundedMaterialJson {
                bytes: Vec::new(), limit: SOURCE_POOL_DISCOVERY_BYTES, refused_lower_bound: None,
            };
            if let Err(error) = serde_json::to_writer_pretty(&mut singleton, std::slice::from_ref(item)) {
                let failure = match singleton.refused_lower_bound {
                    Some(observed) => corpus_capacity_error("source_pool_material", SOURCE_POOL_DISCOVERY_BYTES, observed)
                        .with("source", item.binding.source.to_string()),
                    None => AikitError::new("knowledge.ingest_source_pool_unwritable",
                        format!("SourcePool material could not be rendered: {error}")),
                };
                return Err(failure.with("serialization_error", error.to_string())
                    .with_io_source(std::io::Error::other(error)));
            }
            // A nonempty singleton pretty array has exactly '[\n' and '\n]'.
            // Its own Source body and metadata remain serialized by serde.
            let interior = &singleton.bytes[2..singleton.bytes.len() - 2];
            let next_shard_bytes = shard.len().checked_add(2).and_then(|used| used.checked_add(interior.len()))
                .and_then(|used| used.checked_add(2)).ok_or_else(|| {
                    corpus_capacity_error("rendered_material", WIKI_INGEST_RENDERED_BYTES, usize::MAX)
                })?;
            if !shard.is_empty() && next_shard_bytes > SOURCE_POOL_SHARD_BYTES {
                finish_rendered_shard(&mut shard, &mut completed_bytes, &mut rendered)?;
            }
            if shard.is_empty() {
                append_rendered_fragment(&mut shard, completed_bytes, b"[\n")?;
            } else {
                append_rendered_fragment(&mut shard, completed_bytes, b",\n")?;
            }
            append_rendered_fragment(&mut shard, completed_bytes, interior)?;
        }
        finish_rendered_shard(&mut shard, &mut completed_bytes, &mut rendered)?;
        Ok(rendered)
    })();
    prepared.map_err(|error| command_failure(error, &[], "AIKit/SourcePool", Path::new("corpus"), "render", "none"))
}

/// Recognise only the exact names emitted by the native shard writer. No lossy
/// decoding or alternate zero padding may grant ownership of a foreign file.
fn corpus_shard_index(name: &std::ffi::OsStr) -> Option<usize> {
    let name = name.to_str()?;
    let index = name
        .strip_prefix("corpus-")?
        .strip_suffix(".json")?
        .parse::<usize>()
        .ok()?;
    (name == format!("corpus-{index:03}.json")).then_some(index)
}

/// Refresh discoverable material through the same physical publication owner.
/// The corpus remains source; these files remain a reconstructable material
/// projection. All required replacements precede stale removal. A multi-file
/// failure retains its exact acknowledgements, never promises a transaction.
fn write_source_pool(dir: &Path, rendered: &[String]) -> Result<usize> {
    let mut completed = Vec::new();
    let io_failure =
        |path: &Path, phase: &str, error: std::io::Error, effects: &[Value], effect: &str| {
            command_failure(
                AikitError::new(
                    "knowledge.ingest_source_pool_unwritable",
                    format!("{}: {error}", path.display()),
                )
                .with("cause_kind", format!("{:?}", error.kind()))
                .with(
                    "cause_raw_os_error",
                    jval!(error.raw_os_error()).to_string(),
                ),
                effects,
                "AIKit/SourcePool",
                path,
                phase,
                effect,
            )
        };
    if !dir.is_dir() {
        std::fs::create_dir_all(dir)
            .map_err(|error| io_failure(dir, "prepare_directory", error, &completed, "unknown"))?;
        completed.push(
            jval!({"owner":"AIKit/SourcePool", "action":"prepare_directory",
            "source_path":dir.display().to_string()}),
        );
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|error| io_failure(dir, "inventory", error, &completed, "none"))?;
    let mut previous = BTreeMap::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| io_failure(dir, "inventory", error, &completed, "none"))?;
        if let Some(index) = corpus_shard_index(&entry.file_name()) {
            let name = format!("corpus-{index:03}.json");
            let path = entry.path();
            let basis = aikit_adapters::projectcentral::publication::material_basis(&path)
                .map_err(|error| {
                    command_failure(
                        error,
                        &completed,
                        "AIKit/SourcePool",
                        &path,
                        "read_basis",
                        "none",
                    )
                })?;
            previous.insert(name, basis);
        }
    }
    for (index, text) in rendered.iter().enumerate() {
        let name = format!("corpus-{index:03}.json");
        let path = dir.join(&name);
        let changed = match previous.get(&name) {
            Some(basis) => {
                aikit_adapters::projectcentral::publication::publish_wiki(&path, text, basis)
            }
            None => {
                aikit_adapters::projectcentral::publication::publish_absent_material(&path, text)
            }
        }
        .map_err(|error| {
            command_failure(
                error,
                &completed,
                "AIKit/SourcePool",
                &path,
                "publication",
                "unknown",
            )
        })?;
        if changed {
            completed.push(
                jval!({"owner":"AIKit/SourcePool", "action":"publish_material",
                "source_path":path.display().to_string(), "base_hash":previous.get(&name),
                "published_hash":content_hash(text.as_bytes())}),
            );
        }
    }
    for (name, basis) in previous {
        if (0..rendered.len()).any(|index| name == format!("corpus-{index:03}.json")) {
            continue;
        }
        let path = dir.join(&name);
        aikit_adapters::projectcentral::publication::remove_material(&path, &basis).map_err(
            |error| {
                command_failure(
                    error,
                    &completed,
                    "AIKit/SourcePool",
                    &path,
                    "prune",
                    "unknown",
                )
            },
        )?;
        completed.push(
            jval!({"owner":"AIKit/SourcePool", "action":"remove_stale_material",
            "source_path":path.display().to_string(), "base_hash":basis}),
        );
    }
    Ok(rendered.len())
}

// ---------------------------------------------------------------------------
// query — read the semantic index over a Wiki file
// ---------------------------------------------------------------------------

/// Query only the named file's objects. Document validation preserves the
/// writer's federation contract and still refuses broken local reciprocity.
/// External refs are disclosed, never invented as local objects or persisted.
fn read_index(file: &Path) -> Result<(SemanticWikiIndex, Vec<String>)> {
    let document = WikiDocument::parse(&read(file)?)?;
    document.validate()?;
    let (index, repairs) = SemanticWikiIndex::rebuild_with_repairs(document.objects().to_vec())?;
    let warnings = repairs
        .into_iter()
        .map(|repair| {
            format!(
                "{} declares {} outside this Wiki file; query covers local objects only ({})",
                repair.subject, repair.other, repair.code,
            )
        })
        .collect();
    Ok((index, warnings))
}

fn query_search(args: &WikiQuerySearchArgs) -> Result<WikiOutcome> {
    let (index, warnings) = read_index(&args.file)?;
    let hits = index.search(&args.query, args.limit);
    Ok(WikiOutcome::reported(
        jval!({
            "command": "query.search",
            "file": args.file.display().to_string(),
            "query": args.query,
            "hits": serde_json::to_value(&hits).unwrap_or_default(),
        }),
        warnings,
        json::EXIT_OK,
    ))
}

/// Every object `--ref` points at, outgoing and incoming alike — the ordinary
/// traversal view. Tags and backlinks ride here exactly as any other edge:
/// nothing about a `tagged` relation or an ingested `references` edge is
/// special-cased.
fn query_neighbours(args: &WikiQueryRefArgs) -> Result<WikiOutcome> {
    let (index, mut warnings) = read_index(&args.file)?;
    let resource = ResourceRef::parse(&args.resource_ref)?;
    let mut neighbours: Vec<Value> = index
        .neighbours(&resource, args.limit)
        .into_iter()
        .map(|n| serde_json::to_value(n).unwrap_or_default())
        .collect();
    // The nodes citing an authored source are its neighbourhood, incoming.
    neighbours.extend(citations_of(&index, &resource));
    neighbours.truncate(args.limit);
    warnings.extend(absent_ref_warnings(&index, &resource));
    Ok(WikiOutcome::reported(
        jval!({
            "command": "query.neighbours",
            "file": args.file.display().to_string(),
            "ref": resource.to_string(),
            "neighbours": serde_json::to_value(&neighbours).unwrap_or_default(),
        }),
        warnings,
        json::EXIT_OK,
    ))
}

/// An empty traversal has two very different causes: a curated object with
/// nothing attached, and a ref the field does not hold at all. Reported the
/// same way they are indistinguishable, and a caller reads "no relations"
/// where the truth is "not here". Say which.
///
/// A cited authored source lands in the second case by design — it is
/// findable through search without ever being a curated object.
fn absent_ref_warnings(index: &SemanticWikiIndex, resource: &ResourceRef) -> Vec<String> {
    if index.contains(resource) || !citations_of(index, resource).is_empty() {
        return Vec::new();
    }
    vec![format!(
        "{resource} is not in this Wiki file: an empty result here means absent, not unrelated"
    )]
}

/// The curated nodes citing `resource`, when `resource` names an authored
/// source rather than a curated object.
///
/// A citation is a real, directional, authored fact: these nodes point at this
/// source. Reporting it costs the source nothing — it still does not resolve,
/// is not in `all_refs`, and is not discoverable as an object. What it does buy
/// is an answer where there used to be an empty list and an apology.
///
/// The entries carry no `edge_ref`, because no edge object exists: the claim
/// lives in each node's `source_refs`. Saying so is more honest than inventing
/// an edge to make the shape uniform.
fn citations_of(index: &SemanticWikiIndex, resource: &ResourceRef) -> Vec<Value> {
    if index.contains(resource) {
        return Vec::new();
    }
    let Ok(source) = SourceRef::parse(resource.as_str()) else {
        return Vec::new();
    };
    index
        .citing_nodes(&source)
        .into_iter()
        .map(|node| {
            jval!({
                "resource": node.to_string(),
                "relation": "cites",
                "direction": "incoming",
                "origin": "authored",
                "via": "source_refs",
            })
        })
        .collect()
}

/// Every object that points *at* `--ref` — what cites it. First-class over
/// an ingested corpus: an argument's authored citations and a tag's members
/// are both ordinary backlinks here, not a derived view bolted on after.
fn query_backlinks(args: &WikiQueryRefArgs) -> Result<WikiOutcome> {
    let (index, mut warnings) = read_index(&args.file)?;
    let resource = ResourceRef::parse(&args.resource_ref)?;
    let mut backlinks: Vec<Value> = index
        .backlinks(&resource)
        .into_iter()
        .map(|n| serde_json::to_value(n).unwrap_or_default())
        .collect();
    // An authored source's backlinks are the nodes that cite it. First-class
    // here, not a footnote pointing somewhere else.
    backlinks.extend(citations_of(&index, &resource));
    backlinks.truncate(args.limit);
    warnings.extend(absent_ref_warnings(&index, &resource));
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
/// validate the whole, render, atomic rename. A pre-publication refusal leaves
/// the source byte-identical; lost readback after rename is an uncertain effect.
/// The
/// hash of what was read travels to `persist` as the compare-and-swap base, so
/// a peer's write landing between this read and the rename is refused rather
/// than silently overwritten.
fn mutate_file<F>(path: &Path, mutate: F) -> Result<WikiMutationOutcome>
where
    F: FnOnce(&mut WikiDocument, &mut WikiMutationLedger) -> Result<()>,
{
    Ok(mutate_file_receipt(path, mutate)?.outcome)
}

/// A local acknowledgement of the existing owner's actual publication, not
/// another store or operation identity. A semantic no-op is not a new write.
struct WikiFileReceipt {
    outcome: WikiMutationOutcome,
    publication: Option<Value>,
}

impl WikiFileReceipt {
    fn completed_effects(&self) -> Vec<Value> {
        self.publication.iter().cloned().collect()
    }
}

fn mutate_file_receipt<F>(path: &Path, mutate: F) -> Result<WikiFileReceipt>
where
    F: FnOnce(&mut WikiDocument, &mut WikiMutationLedger) -> Result<()>,
{
    let before_publication =
        |error, phase| command_failure(error, &[], "AIKit/Wiki", path, phase, "none");
    let input = read(path).map_err(|error| before_publication(error, "read"))?;
    let physical_path = std::fs::canonicalize(path).map_err(|error| {
        before_publication(
            AikitError::new("knowledge.wiki_file_unreadable", error.to_string())
                .with("path", path.display().to_string()),
            "resolve_source",
        )
    })?;
    let base_hash = content_hash(input.as_bytes());
    let (rendered, outcome) =
        apply_wiki_mutation(&input, mutate).map_err(|error| before_publication(error, "plan"))?;
    let changed =
        aikit_adapters::projectcentral::publication::publish_wiki(path, &rendered, &base_hash)
            .map_err(|error| {
                command_failure(error, &[], "AIKit/Wiki", path, "publication", "unknown")
            })?;
    let publication = changed.then(|| {
        jval!({
            "owner": "AIKit/Wiki", "action": "publish_wiki",
            "source_path": physical_path.display().to_string(),
            "base_hash": base_hash, "published_hash": content_hash(rendered.as_bytes()),
            "touched": &outcome.touched,
        })
    });
    Ok(WikiFileReceipt {
        outcome,
        publication,
    })
}

/// Preserve the native cause verbatim while describing this invocation's
/// phases. JSON-valued details remain strings in the public schema-1 envelope.
fn command_failure(
    error: AikitError,
    completed: &[Value],
    owner: &str,
    path: &Path,
    phase: &str,
    failed_effect: &str,
) -> AikitError {
    let original = error
        .details()
        .get("original_error")
        .cloned()
        .unwrap_or_else(|| {
            jval!({"code":error.code(), "message":error.message(), "details":error.details()})
                .to_string()
        });
    let mut all_completed = completed.to_vec();
    if let Some(previous) = error.details().get("completed_effects") {
        if let Ok(effects) = serde_json::from_str::<Vec<Value>>(previous) {
            all_completed.extend(effects);
        }
    }
    let uncertain = failed_effect != "none"
        || error
            .details()
            .get("published")
            .is_some_and(|value| value == "true")
        || error
            .details()
            .get("outcome")
            .is_some_and(|value| value == "unknown");
    let effect = if uncertain {
        "unknown"
    } else if all_completed.is_empty() {
        "none"
    } else {
        "present"
    };
    let failure = jval!({"owner":owner, "source_path":path.display().to_string(),
        "phase":phase, "effect":failed_effect});
    let error = error
        .with("original_error", original)
        .with("command_effect", effect)
        .with("completed_effects", jval!(all_completed).to_string())
        .with("failed_effect", failure.to_string());
    if effect == "none" {
        error
    } else {
        error
            .with("outcome", if uncertain { "unknown" } else { "partial" })
            .with("automatic_retry", "false")
    }
}

fn extend_command_failure(error: AikitError, completed: &[Value]) -> AikitError {
    // The child owner already classified its actual phase. Preserve that leg,
    // adding earlier acknowledged effects instead of replacing its cause.
    let mut all_completed = completed.to_vec();
    if let Some(previous) = error.details().get("completed_effects") {
        if let Ok(effects) = serde_json::from_str::<Vec<Value>>(previous) {
            all_completed.extend(effects);
        }
    }
    let child_effect = error.details().get("command_effect").map(String::as_str);
    let known = matches!(child_effect, Some("none") | Some("present"));
    let effect = if !known {
        "unknown"
    } else if child_effect == Some("none") && all_completed.is_empty() {
        "none"
    } else {
        "present"
    };
    let error = error
        .with("command_effect", effect)
        .with("completed_effects", jval!(all_completed).to_string());
    if effect == "none" {
        error
    } else {
        error
            .with("outcome", if known { "partial" } else { "unknown" })
            .with("automatic_retry", "false")
    }
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_file_unreadable",
            format!("could not read {}: {error}", path.display()),
        )
        .with("path", path.display().to_string())
        .with("cause_kind", format!("{:?}", error.kind()))
        .with(
            "cause_raw_os_error",
            jval!(error.raw_os_error()).to_string(),
        )
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

/// Every native Wiki writer delegates exact-basis, metadata-preserving
/// publication to the shared physical-file lock protocol.
fn persist(path: &Path, rendered: &str, base_hash: &str) -> Result<()> {
    aikit_adapters::projectcentral::publication::publish_wiki(path, rendered, base_hash)?;
    Ok(())
}

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

// ---------------------------------------------------------------------------
// maintenance
// ---------------------------------------------------------------------------

/// The maintenance request body: reviewed upserts plus the observations the
/// plan needs. Shape mirrors [`plan_agent_wiki_maintenance`]'s request minus
/// `current_objects`, which this verb always reads through the binding itself
/// — the base-hash contract requires the plan and the compare-and-swap base
/// to come from one and the same read, so a caller-supplied snapshot would
/// undercut the concurrency guarantee.
#[derive(Debug, serde::Deserialize)]
struct WikiMaintenanceRequest {
    #[serde(default)]
    upserts: Vec<Value>,
    #[serde(default)]
    human_source_proposals: Vec<HumanSourceRevisionProposal>,
    #[serde(default)]
    observed_source_revisions: BTreeMap<SourceRef, SemanticRevision>,
}

/// `aikit wiki maintenance` — the one CLI owner verb for the canonical Agent
/// Wiki maintenance contract (`ProjectCentral/agents/wiki/wiki.json`). The
/// order is the contract: read the canonical wiki and capture the exact base
/// hash; plan through `plan_agent_wiki_maintenance` (object validation,
/// revision advancement, provenance, whole-document rebuild); persist through
/// the binding's compare-and-swap write; then read back to prove the file
/// holds exactly what the plan committed. Human source is never written here:
/// proposals ride the receipt as decision pressure only, and a refused write
/// leaves the file exactly as a peer left it.
fn maintenance(cwd: &Path, args: &WikiMaintenanceArgs) -> Result<WikiOutcome> {
    maintenance_command(cwd, args).map_err(|error| {
        if error.details().contains_key("command_effect") {
            error
        } else {
            command_failure(
                error,
                &[],
                "AIKit/Wiki",
                &cwd.join(PROJECTCENTRAL_WIKI_SOURCE),
                "prepare",
                "none",
            )
        }
    })
}

fn maintenance_command(cwd: &Path, args: &WikiMaintenanceArgs) -> Result<WikiOutcome> {
    let binding = ProjectCentralFilesystemBinding::inspect(cwd, None)?;
    let (current_objects, base_hash) = binding.load_project_wiki_for_maintenance()?;
    let raw = if args.request.as_os_str() == "-" {
        read_stdin()?
    } else {
        std::fs::read_to_string(&args.request).map_err(|error| {
            AikitError::new(
                "cli.usage",
                format!(
                    "could not read maintenance request {}: {error}",
                    args.request.display()
                ),
            )
        })?
    };
    let request: WikiMaintenanceRequest = serde_json::from_str(&raw).map_err(|error| {
        AikitError::new(
            "cli.usage",
            format!("invalid maintenance request JSON: {error}"),
        )
    })?;
    let upserts = request
        .upserts
        .iter()
        .map(WikiObject::parse)
        .collect::<Result<Vec<_>>>()?;
    let plan = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
        current_objects,
        upserts,
        observed_source_revisions: request.observed_source_revisions,
        human_source_proposals: request.human_source_proposals,
    })?;
    let source_path = binding.project_root().join(PROJECTCENTRAL_WIKI_SOURCE);
    let changed = binding
        .persist_agent_wiki(&plan, &base_hash)
        .map_err(|error| {
            let absent = error
                .details()
                .get("command_effect")
                .is_some_and(|value| value == "none");
            command_failure(
                error,
                &[],
                "AIKit/Wiki",
                &source_path,
                "publication",
                if absent { "none" } else { "unknown" },
            )
        })?;
    let completed = maintenance_completed(&binding, &plan, &base_hash, changed);
    let persisted = maintenance_readback(&binding, &plan, &completed)?;
    let stale_resources = plan
        .stale_resources
        .iter()
        .map(|resource| resource.to_string())
        .collect::<Vec<_>>();
    let human_source_proposals =
        serde_json::to_value(&plan.human_source_proposals).map_err(|error| {
            command_failure(
                AikitError::new("knowledge.wiki_write_failed", error.to_string()),
                &completed,
                "AIKit/Wiki",
                &source_path,
                "return",
                "none",
            )
        })?;
    Ok(WikiOutcome {
        data: jval!({
            "state": "maintained",
            "changed": changed,
            "wiki": PROJECTCENTRAL_WIKI_SOURCE,
            "objects": persisted.len(),
            "current_index_revision": plan.current_index_revision,
            "stale_resources": stale_resources,
            "human_source_proposals": human_source_proposals,
        }),
        warnings: vec![],
        exit_code: json::EXIT_OK,
    })
}

fn maintenance_completed(
    binding: &ProjectCentralFilesystemBinding,
    plan: &AgentWikiMaintenancePlan,
    base_hash: &str,
    changed: bool,
) -> Vec<Value> {
    if changed {
        vec![jval!({
            "owner":"AIKit/Wiki", "action":"publish_wiki",
            "source_path":binding.project_root().join(PROJECTCENTRAL_WIKI_SOURCE).display().to_string(),
            "source_ref":PROJECTCENTRAL_WIKI_SOURCE, "base_hash":base_hash,
            "plan_index_revision":plan.current_index_revision,
        })]
    } else {
        vec![]
    }
}

fn maintenance_readback(
    binding: &ProjectCentralFilesystemBinding,
    plan: &AgentWikiMaintenancePlan,
    completed: &[Value],
) -> Result<Vec<WikiObject>> {
    let path = binding.project_root().join(PROJECTCENTRAL_WIKI_SOURCE);
    let lost_readback =
        |error| command_failure(error, completed, "AIKit/Wiki", &path, "readback", "none");
    let persisted = binding.load_project_wiki().map_err(lost_readback)?;
    // Container order is not object identity. Compare complete objects by
    // native ref while keeping every inner ordered value and extension exact.
    let persisted_by_ref = persisted
        .iter()
        .map(|object| (object.ref_id(), object))
        .collect::<BTreeMap<_, _>>();
    let planned_by_ref = plan
        .next_objects
        .iter()
        .map(|object| (object.ref_id(), object))
        .collect::<BTreeMap<_, _>>();
    if persisted.len() != plan.next_objects.len()
        || persisted_by_ref.len() != persisted.len()
        || planned_by_ref.len() != plan.next_objects.len()
        || persisted_by_ref != planned_by_ref
    {
        return Err(lost_readback(
            AikitError::new(
                "knowledge.wiki_concurrent_write",
                "readback after persist does not match the committed plan; a peer write landed in \
             the window between the write and the readback — re-read and reconcile",
            )
            .with("wiki", PROJECTCENTRAL_WIKI_SOURCE),
        ));
    }
    Ok(persisted)
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
            .filter(|entry| {
                entry.file_name() != "wiki.json"
                    && entry.file_name() != ".wiki.json.publication.lock"
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "a refused write must not leave a temp file behind: {leftovers:?}"
        );
        assert!(
            fs::metadata(dir.path().join(".wiki.json.publication.lock"))
                .unwrap()
                .is_file(),
            "the shared lock inode persists across publications and process restarts"
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

// ---------------------------------------------------------------------------
// maintenance tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod maintenance_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const MANIFEST: &str = r#"{
      "schema":"central.project/v1",
      "project_id":"epilogos/demo",
      "human_source":"ProjectCentral/user",
      "wiki":{
        "profile":"okf-wiki/v1",
        "source":"ProjectCentral/agents/wiki/wiki.json"
      }
    }"#;

    /// One space plus one provenance-carrying node, in the same object form
    /// the Wiki file and the maintenance request both speak.
    fn wiki_document() -> String {
        r#"{"profile":"okf-wiki/v1","objects":[
          {"object":"space","profile":"okf-wiki/v1","ref":"wiki:space:project","revision":1,
           "provenance":[],"title":"Project","parent_space_refs":[],
           "child_space_refs":[],"node_refs":["wiki:node:purpose"]},
          {"object":"node","profile":"okf-wiki/v1","ref":"wiki:node:purpose","revision":1,
           "provenance":[{"source_ref":"central:project-source:epilogos/demo:purpose",
                          "source_revision":"r1"}],
           "type":"ProjectKnowledge","title":"Purpose",
           "space_refs":["wiki:space:project"],
           "source_refs":["central:project-source:epilogos/demo:purpose"]}
        ]}"#
        .to_string()
    }

    fn revision_two_upsert(title: &str) -> String {
        format!(
            r#"{{
              "object":"node","profile":"okf-wiki/v1","ref":"wiki:node:purpose","revision":2,
              "provenance":[{{"source_ref":"central:project-source:epilogos/demo:purpose",
                             "source_revision":"r1",
                             "producer_ref":"agent:test",
                             "generation_ref":"run:test"}}],
              "type":"ProjectKnowledge","title":"{title}",
              "space_refs":["wiki:space:project"],
              "source_refs":["central:project-source:epilogos/demo:purpose"]
            }}"#
        )
    }

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// A minimal ProjectCentral project: manifest plus canonical Agent Wiki.
    fn fixture() -> (TempDir, PathBuf) {
        let temporary = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        fs::create_dir_all(&temporary).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("native-wiki-maintenance-")
            .tempdir_in(&temporary)
            .unwrap();
        let project = temp.path().join("Work/demo");
        write(&project.join("ProjectCentral/project.json"), MANIFEST);
        write(
            &project.join("ProjectCentral/agents/wiki/wiki.json"),
            &wiki_document(),
        );
        (temp, project)
    }

    fn request_file(upserts_json: &str) -> (TempDir, PathBuf) {
        let temporary = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        fs::create_dir_all(&temporary).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("native-wiki-maintenance-request-")
            .tempdir_in(&temporary)
            .unwrap();
        let path = dir.path().join("request.json");
        write(&path, &format!(r#"{{"upserts":[{upserts_json}]}}"#));
        (dir, path)
    }

    #[test]
    fn actual_peer_write_after_maintenance_publication_keeps_its_acknowledgement() {
        let (_temp, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, None).unwrap();
        let (current_objects, basis) = binding.load_project_wiki_for_maintenance().unwrap();
        let upsert = WikiObject::parse(
            &serde_json::from_str(&revision_two_upsert("Acknowledged owner plan")).unwrap(),
        )
        .unwrap();
        let plan = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
            current_objects,
            upserts: vec![upsert],
            observed_source_revisions: binding.observed_source_revisions(),
            human_source_proposals: vec![],
        })
        .unwrap();
        let changed = binding.persist_agent_wiki(&plan, &basis).unwrap();
        assert!(changed);
        let completed = maintenance_completed(&binding, &plan, &basis, changed);
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let peer = WikiObject::parse(
            &serde_json::from_str(
                &revision_two_upsert("Actual later peer")
                    .replace("\"revision\":2", "\"revision\":3"),
            )
            .unwrap(),
        )
        .unwrap();
        mutate_file(&path, |doc, ledger| {
            ledger.record(doc.update_object(peer)?);
            Ok(())
        })
        .unwrap();
        let failure = maintenance_readback(&binding, &plan, &completed).unwrap_err();
        assert_eq!(failure.code(), "knowledge.wiki_concurrent_write");
        assert_eq!(failure.details()["command_effect"], "present");
        assert_eq!(failure.details()["outcome"], "partial");
        let retained: Value =
            serde_json::from_str(&failure.details()["completed_effects"]).unwrap();
        assert_eq!(retained, jval!(completed));
        let index = SemanticWikiIndex::rebuild(binding.load_project_wiki().unwrap()).unwrap();
        assert_eq!(
            index
                .node(&ResourceRef::parse("wiki:node:purpose").unwrap())
                .unwrap()
                .revision,
            3
        );
    }

    #[test]
    fn no_op_maintenance_then_real_read_failure_does_not_claim_publication() {
        let (_temp, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, None).unwrap();
        let (current_objects, basis) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
            current_objects,
            upserts: vec![],
            observed_source_revisions: binding.observed_source_revisions(),
            human_source_proposals: vec![],
        })
        .unwrap();
        let changed = binding.persist_agent_wiki(&plan, &basis).unwrap();
        assert!(!changed);
        let completed = maintenance_completed(&binding, &plan, &basis, changed);
        assert!(completed.is_empty());
        fs::remove_file(project.join(PROJECTCENTRAL_WIKI_SOURCE)).unwrap();
        let original = binding.load_project_wiki().unwrap_err();
        let failure = maintenance_readback(&binding, &plan, &completed).unwrap_err();
        assert_eq!(failure.code(), original.code());
        assert_eq!(failure.message(), original.message());
        assert_eq!(failure.details()["command_effect"], "none");
        assert_eq!(failure.details()["completed_effects"], "[]");
        assert!(!failure.details().contains_key("published"));
    }

    #[test]
    fn maintenance_applies_reviewed_upserts_through_the_contract() {
        let (_temp, project) = fixture();
        let (_request_dir, request) = request_file(&revision_two_upsert("Purpose returned"));
        let outcome = maintenance(&project, &WikiMaintenanceArgs { request }).unwrap();
        assert_eq!(outcome.exit_code, json::EXIT_OK);
        assert_eq!(outcome.data["state"], "maintained");
        assert_eq!(outcome.data["objects"], 2);

        let wiki_path = project.join("ProjectCentral/agents/wiki/wiki.json");
        let persisted = binding_objects(&wiki_path);
        let index = SemanticWikiIndex::rebuild(persisted).unwrap();
        assert_eq!(
            index
                .node(&ResourceRef::parse("wiki:node:purpose").unwrap())
                .unwrap()
                .revision,
            2
        );
        assert_eq!(
            index
                .node(&ResourceRef::parse("wiki:node:purpose").unwrap())
                .unwrap()
                .title
                .as_deref(),
            Some("Purpose returned")
        );
    }

    #[test]
    fn maintenance_replay_with_no_upserts_keeps_the_document_whole() {
        let (_temp, project) = fixture();
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let mut original: Value = serde_json::from_str(&wiki_document()).unwrap();
        original["owner_header"] = jval!({"retained": true});
        original["objects"][1]["owner_extension"] =
            jval!({"ordered": ["first", "second"], "retained": true});
        write(&path, &serde_json::to_string(&original).unwrap());
        let before = fs::read(&path).unwrap();
        let before_metadata = fs::metadata(&path).unwrap();
        let (_request_dir, request) = request_file("");
        let outcome = maintenance(&project, &WikiMaintenanceArgs { request }).unwrap();
        assert_eq!(outcome.data["objects"], 2);
        assert_eq!(outcome.data["changed"], false);
        assert_eq!(fs::read(&path).unwrap(), before);
        let after_metadata = fs::metadata(&path).unwrap();
        assert_eq!(
            after_metadata.modified().unwrap(),
            before_metadata.modified().unwrap()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(after_metadata.ino(), before_metadata.ino());
        }
        // The retained source starts with the Space, whereas the plan's native
        // identity order starts with the Node. Neither rewrites this no-op.
        let persisted = binding_objects(&path);
        assert_eq!(persisted[0].ref_id().as_str(), "wiki:space:project");
        let index = SemanticWikiIndex::rebuild(persisted).unwrap();
        let node = index
            .node(&ResourceRef::parse("wiki:node:purpose").unwrap())
            .unwrap();
        assert_eq!(
            node.extensions["owner_extension"]["ordered"],
            jval!(["first", "second"])
        );
    }

    #[test]
    fn maintenance_readback_accepts_actual_top_level_reordering_only() {
        let (_temp, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, None).unwrap();
        let (current_objects, basis) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
            current_objects,
            upserts: vec![],
            observed_source_revisions: binding.observed_source_revisions(),
            human_source_proposals: vec![],
        })
        .unwrap();
        let changed = binding.persist_agent_wiki(&plan, &basis).unwrap();
        assert!(!changed);
        let completed = maintenance_completed(&binding, &plan, &basis, changed);
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let mut peer: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        peer["objects"].as_array_mut().unwrap().reverse();
        write(&path, &serde_json::to_string(&peer).unwrap());
        let readback = maintenance_readback(&binding, &plan, &completed).unwrap();
        assert_eq!(readback[0].ref_id().as_str(), "wiki:node:purpose");
        assert_eq!(readback.len(), 2);
        assert!(completed.is_empty());
    }

    #[test]
    fn maintenance_readback_rejects_actual_inner_changes_loss_and_duplicate_identity() {
        for alteration in ["inner-order", "extension", "missing", "duplicate"] {
            let (_temp, project) = fixture();
            let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
            let mut original: Value = serde_json::from_str(&wiki_document()).unwrap();
            original["objects"][1]["owner_extension"] =
                jval!({"ordered": ["first", "second"], "retained": true});
            write(&path, &serde_json::to_string(&original).unwrap());
            let binding = ProjectCentralFilesystemBinding::inspect(&project, None).unwrap();
            let (current_objects, basis) = binding.load_project_wiki_for_maintenance().unwrap();
            let plan = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
                current_objects,
                upserts: vec![],
                observed_source_revisions: binding.observed_source_revisions(),
                human_source_proposals: vec![],
            })
            .unwrap();
            let changed = binding.persist_agent_wiki(&plan, &basis).unwrap();
            assert!(!changed);
            let completed = maintenance_completed(&binding, &plan, &basis, changed);
            let mut peer: Value =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            match alteration {
                "inner-order" => peer["objects"][1]["owner_extension"]["ordered"]
                    .as_array_mut()
                    .unwrap()
                    .reverse(),
                "extension" => peer["objects"][1]["owner_extension"]["retained"] = jval!(false),
                "missing" => {
                    peer["objects"].as_array_mut().unwrap().remove(1);
                }
                "duplicate" => {
                    let node = peer["objects"][1].clone();
                    peer["objects"][0] = node;
                }
                _ => unreachable!(),
            }
            let actual_peer = serde_json::to_string(&peer).unwrap();
            write(&path, &actual_peer);
            let failure = maintenance_readback(&binding, &plan, &completed).unwrap_err();
            assert_eq!(
                failure.code(),
                "knowledge.wiki_concurrent_write",
                "{alteration}"
            );
            assert_eq!(failure.details()["command_effect"], "none", "{alteration}");
            assert_eq!(failure.details()["completed_effects"], "[]", "{alteration}");
            let original_error: Value =
                serde_json::from_str(&failure.details()["original_error"]).unwrap();
            assert_eq!(
                original_error["code"], "knowledge.wiki_concurrent_write",
                "{alteration}"
            );
            assert_eq!(
                read(&path).unwrap(),
                actual_peer,
                "readback does not undo the actual peer's {alteration}"
            );
        }
    }

    #[test]
    fn maintenance_refuses_an_upsert_that_does_not_advance_the_revision() {
        let (_temp, project) = fixture();
        let stale_upsert =
            revision_two_upsert("Purpose returned").replace("\"revision\":2", "\"revision\":1");
        let (_request_dir, request) = request_file(&stale_upsert);
        let Err(error) = maintenance(&project, &WikiMaintenanceArgs { request }) else {
            panic!("an upsert that does not advance the revision must be refused");
        };
        assert_eq!(error.code(), "projectcentral.wiki_revision_not_advanced");
        // The refusal leaves the on-disk wiki untouched.
        let index = SemanticWikiIndex::rebuild(binding_objects(
            &project.join("ProjectCentral/agents/wiki/wiki.json"),
        ))
        .unwrap();
        assert_eq!(
            index
                .node(&ResourceRef::parse("wiki:node:purpose").unwrap())
                .unwrap()
                .revision,
            1
        );
    }

    #[test]
    fn maintenance_requires_a_projectcentral_ground() {
        let temp = TempDir::new().unwrap();
        let (_request_dir, request) = request_file("");
        let Err(error) = maintenance(temp.path(), &WikiMaintenanceArgs { request }) else {
            panic!("maintenance outside a ProjectCentral ground must be refused");
        };
        assert_eq!(error.code(), "projectcentral.manifest_read");
    }

    fn binding_objects(wiki_path: &Path) -> Vec<WikiObject> {
        let text = fs::read_to_string(wiki_path).unwrap();
        aikit_core::parse_wiki_objects(&text).unwrap()
    }

    #[test]
    fn material_rendering_keeps_exact_pretty_array_and_metadata_escape_boundaries() {
        let input = vec![("actual-source.md".to_owned(), "---\nsource_id: actual-render-source\ntitle_full: Actual rendering source\n---\n\n# Actual source\n".to_owned())];
        let mut material = aikit_core::knowledge_ingest::ingest_corpus(&[], &input, 0).unwrap().material;
        material[0].binding.metadata.insert("actual_escaped_metadata".into(), jval!("\u{0}\t\n\"\\"));
        let complete = serde_json::to_string_pretty(&material).unwrap();
        let rendered = render_source_pool(&material).unwrap();
        assert_eq!(rendered, vec![complete]);
        let decoded: Vec<SourceMaterial> = serde_json::from_str(&rendered[0]).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), serde_json::to_value(&material).unwrap());

        material[0].body.clear();
        material[0].binding.revision = aikit_core::SourceRevision::parse(corpus_content_revision(b"")).unwrap();
        let overhead = serde_json::to_string_pretty(&material).unwrap().len();
        material[0].body = "x".repeat(SOURCE_POOL_DISCOVERY_BYTES - overhead);
        material[0].binding.revision = aikit_core::SourceRevision::parse(corpus_content_revision(material[0].body.as_bytes())).unwrap();
        let boundary = render_source_pool(&material).unwrap();
        assert_eq!(boundary[0].len(), SOURCE_POOL_DISCOVERY_BYTES);
        assert_eq!(serde_json::to_string_pretty(&material).unwrap(), boundary[0]);
        material[0].body.push('x');
        material[0].binding.revision = aikit_core::SourceRevision::parse(corpus_content_revision(material[0].body.as_bytes())).unwrap();
        let error = render_source_pool(&material).unwrap_err();
        assert_eq!(error.code(), "knowledge.ingest_corpus_capacity");
        assert_eq!(error.details()["dimension"], "source_pool_material");
        assert_eq!(error.details()["command_effect"], "none");
        assert!(error.details().contains_key("serialization_error"));
    }

}


#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod r4_metadata_corpus_tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::time::{Duration, Instant};

    // Retention is the default on assertion/unwind. Only an accepted, entirely
    // synchronous filesystem case removes its own admitted scratch directory.
    struct Fixture(Option<tempfile::TempDir>);
    impl Fixture {
        fn new() -> Self {
            let scratch = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent().unwrap().parent().unwrap().join("ProjectCentral/now/tmp");
            fs::create_dir_all(&scratch).unwrap();
            Self(Some(tempfile::Builder::new().prefix("r4-corpus-metadata-")
                .tempdir_in(scratch).unwrap()))
        }
        fn root(&self) -> &Path { self.0.as_ref().unwrap().path() }
        fn setup(&self) -> (PathBuf, Vec<u8>) {
            let root = self.root().join("native-project");
            fs::create_dir_all(root.join("ProjectCentral")).unwrap();
            let bytes = serde_json::to_vec(&jval!({
                "schema": "central.project/v1", "project_id": "literal-native-corpus-id",
                "human_source": "ProjectCentral/user",
                "wiki": {"profile": "okf-wiki/v1", "source": "ProjectCentral/agents/wiki/wiki.json"},
                "native_extension": {"preserved": true}
            })).unwrap();
            fs::write(root.join(PROJECT_MANIFEST_SOURCE), &bytes).unwrap();
            fs::create_dir_all(root.join("corpus")).unwrap();
            (root, bytes)
        }
        fn finish(mut self) {
            let path = self.0.take().unwrap().keep();
            fs::remove_dir_all(&path).unwrap_or_else(|cause|
                panic!("Owned corpus metadata fixture cleanup failed at {}: {cause:?}", path.display()));
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(directory) = self.0.take() {
                eprintln!("Retained actual corpus metadata failure fixture: {}", directory.keep().display());
            }
        }
    }
    struct RestoreMode(PathBuf, fs::Permissions);
    impl Drop for RestoreMode {
        fn drop(&mut self) {
            if let Err(cause) = fs::set_permissions(&self.0, self.1.clone()) {
                if std::thread::panicking() {
                    eprintln!("Corpus fixture permission restoration failed at {}: {cause:?}", self.0.display());
                } else {
                    panic!("Corpus fixture permission restoration failed at {}: {cause:?}", self.0.display());
                }
            }
        }
    }
    fn fifo(path: &Path) {
        let mut command = std::process::Command::new("/usr/bin/mkfifo");
        command.arg(path);
        let output = aikit_adapters::runner::SystemRunner::new()
            .with_timeout(Duration::from_secs(2)).with_output_limit_bytes(4096)
            .with_strict_utf8().capture_command(&mut command).unwrap();
        assert_eq!(output.status, 0, "actual FIFO prerequisite: {output:?}");
        assert!(fs::symlink_metadata(path).unwrap().file_type().is_fifo());
    }

    #[test]
    fn original_corpus_deadline_refuses_before_fifo_metadata_body() {
        let fixture = Fixture::new(); let (root, bytes) = fixture.setup();
        let manifest = root.join(PROJECT_MANIFEST_SOURCE);
        let retained = root.join("retained-manifest.json"); fs::rename(&manifest, &retained).unwrap();
        fifo(&manifest);
        let mut remaining = WIKI_INGEST_READ_BYTES;
        let begun = Instant::now();
        let failure = CorpusAdmission::new_with_read_budget(&root, &root.join("corpus"),
            Some(None), Some(Instant::now()), &mut remaining).err().expect("expired actual deadline must refuse");
        assert_eq!(failure.code(), "knowledge.corpus_read_incomplete");
        assert_eq!(remaining, WIKI_INGEST_READ_BYTES);
        assert!(begun.elapsed() < Duration::from_secs(2));
        assert!(fs::symlink_metadata(&manifest).unwrap().file_type().is_fifo());
        assert_eq!(fs::read(&retained).unwrap(), bytes);
        fixture.finish();
    }

    #[test]
    fn corpus_prerequisite_fifo_and_oversize_retain_actual_native_refusals() {
        let fixture = Fixture::new(); let (root, bytes) = fixture.setup();
        let manifest = root.join(PROJECT_MANIFEST_SOURCE);
        let retained = root.join("retained-manifest.json"); fs::rename(&manifest, &retained).unwrap();
        fifo(&manifest);
        let begun = Instant::now(); let mut remaining = WIKI_INGEST_READ_BYTES;
        let failure = CorpusAdmission::new_with_read_budget(&root, &root.join("corpus"),
            Some(None), Some(Instant::now() + Duration::from_secs(2)), &mut remaining)
            .err().expect("actual FIFO metadata must refuse");
        assert_eq!(failure.code(), "knowledge.wiki_publication_identity");
        assert!(begun.elapsed() < Duration::from_secs(2));
        assert_eq!(remaining, WIKI_INGEST_READ_BYTES);
        fs::remove_file(&manifest).unwrap(); fs::rename(&retained, &manifest).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&manifest).unwrap();
        file.set_len((WIKI_INGEST_SOURCE_BYTES + 1) as u64).unwrap(); drop(file);
        let before = fs::metadata(&manifest).unwrap();
        let failure = CorpusAdmission::new_with_read_budget(&root, &root.join("corpus"),
            Some(None), Some(Instant::now() + Duration::from_secs(2)), &mut remaining)
            .err().expect("actual oversized metadata must refuse");
        assert_eq!(failure.code(), "knowledge.wiki_publication_budget");
        let after = fs::metadata(&manifest).unwrap();
        assert_eq!((after.dev(), after.ino(), after.len()), (before.dev(), before.ino(), before.len()));
        assert_eq!(remaining, WIKI_INGEST_READ_BYTES);
        // The test's admitted operation restores its own original metadata.
        fs::write(&manifest, &bytes).unwrap();
        assert_eq!(fs::read(&manifest).unwrap(), bytes);
        fixture.finish();
    }

    #[test]
    fn metadata_payload_uses_same_remaining_allowance_and_literal_project_identity() {
        let fixture = Fixture::new(); let (root, bytes) = fixture.setup();
        let corpus = root.join("corpus"); let mut remaining = WIKI_INGEST_READ_BYTES;
        let deadline = Instant::now() + Duration::from_secs(2);
        let first = CorpusAdmission::new_with_read_budget(&root, &corpus, Some(None),
            Some(deadline), &mut remaining).unwrap();
        assert_eq!(first.project.as_ref().unwrap().1.semantic.project_id, "literal-native-corpus-id");
        assert_eq!(remaining, WIKI_INGEST_READ_BYTES - bytes.len());
        let second = CorpusAdmission::new_with_read_budget(&root, &corpus, Some(None),
            Some(deadline), &mut remaining).unwrap();
        assert_eq!(second.project.as_ref().unwrap().1.semantic.project_id, "literal-native-corpus-id");
        assert_eq!(remaining, WIKI_INGEST_READ_BYTES - 2 * bytes.len());
        assert_eq!(fs::read(root.join(PROJECT_MANIFEST_SOURCE)).unwrap(), bytes);
        fixture.finish();
    }

    #[test]
    fn project_and_enclosing_corpus_floors_precede_invalid_manifest_parse() {
        let fixture = Fixture::new(); let (root, _) = fixture.setup();
        let corpus = root.join("corpus"); let manifest = root.join(PROJECT_MANIFEST_SOURCE);
        fs::write(&manifest, b"actual-invalid-native-metadata").unwrap();
        let enclosing = fixture.root().to_path_buf();
        for floor in [&root, &enclosing] {
            let marker = floor.join(".no-agent-retrieval"); fs::write(&marker, b"actual withheld floor").unwrap();
            let mut remaining = WIKI_INGEST_READ_BYTES;
            let failure = CorpusAdmission::new_with_read_budget(&root, &corpus,
                Some(Some(&enclosing)), Some(Instant::now() + Duration::from_secs(2)), &mut remaining)
                .err().expect("actual floor must refuse before metadata parse");
            assert_eq!(failure.code(), "knowledge.ingest_corpus_withheld");
            assert_eq!(remaining, WIKI_INGEST_READ_BYTES);
            assert_eq!(fs::read(&manifest).unwrap(), b"actual-invalid-native-metadata");
            fs::remove_file(marker).unwrap();
        }
        let mut remaining = WIKI_INGEST_READ_BYTES;
        assert_eq!(CorpusAdmission::new_with_read_budget(&root, &corpus, Some(None),
            Some(Instant::now() + Duration::from_secs(2)), &mut remaining)
            .err().expect("actually selected malformed metadata must fail").code(), "projectcentral.manifest_invalid");
        fixture.finish();
    }

    #[test]
    fn actual_failed_reads_then_reduced_capacity_distinguish_observed_bytes_from_reservations() {
        let fixture = Fixture::new(); let (root, bytes) = fixture.setup();
        let corpus = root.join("corpus"); let mut restored = Vec::new();
        let mut actual_causes = Vec::new();
        for index in 0..15 {
            let path = corpus.join(format!("a-unreadable-{index:02}.md"));
            fs::write(&path, b"actual retained unreadable source").unwrap();
            restored.push(RestoreMode(path.clone(), fs::metadata(&path).unwrap().permissions()));
            fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
            let oracle = fs::File::open(&path).err()
                .expect("real EACCES prerequisite requires a nonroot native test host");
            assert_eq!(oracle.kind(), std::io::ErrorKind::PermissionDenied);
            actual_causes.push(oracle);
        }
        fs::write(corpus.join("b-observed.md"), b"x").unwrap();
        let next = corpus.join("z-next.md");
        let file = fs::File::create(&next).unwrap();
        file.set_len(WIKI_INGEST_SOURCE_BYTES as u64).unwrap(); drop(file);
        let before = fs::metadata(&next).unwrap();
        let admission = CorpusAdmission::new_with_selected_root(&root, &corpus, Some(None)).unwrap();
        let failure = walk_corpus(&corpus, "md", &admission).err()
            .expect("actual unreadable reservations plus next physical bound must refuse");
        assert_eq!(failure.code(), "knowledge.ingest_corpus_capacity");
        assert_eq!(failure.details()["files_read"], "1");
        assert_eq!(failure.details()["observed_payload_bytes"], "1");
        assert_eq!(failure.details()["failed_payload_reserved_bytes"], (15 * WIKI_INGEST_SOURCE_BYTES).to_string());
        assert_eq!(failure.details()["next_payload_observed_lower_bound"], WIKI_INGEST_SOURCE_BYTES.to_string());
        assert_eq!(failure.details()["observed_lower_bound"], (WIKI_INGEST_SOURCE_BYTES + 1).to_string());
        assert_eq!(failure.details()["budget_charge_lower_bound"], (WIKI_INGEST_READ_BYTES + 1).to_string());
        let causes: Vec<Value> = serde_json::from_str(&failure.details()["failed_read_causes"]).unwrap();
        assert_eq!(causes.len(), actual_causes.len());
        for (cause, oracle) in causes.iter().zip(&actual_causes) {
            assert_eq!(cause["details"]["cause_kind"], format!("{:?}", oracle.kind()));
            assert_eq!(cause["details"]["cause_raw_os_error"], jval!(oracle.raw_os_error()).to_string());
        }
        let original: Value = serde_json::from_str(&failure.details()["original_error"]).unwrap();
        assert_eq!(original["code"], "knowledge.wiki_publication_budget");
        drop(restored);
        for index in 0..15 {
            assert_eq!(fs::read(corpus.join(format!("a-unreadable-{index:02}.md"))).unwrap(),
                b"actual retained unreadable source");
        }
        assert_eq!(fs::read(root.join(PROJECT_MANIFEST_SOURCE)).unwrap(), bytes);
        let after = fs::metadata(&next).unwrap();
        assert_eq!((after.dev(), after.ino(), after.len()), (before.dev(), before.ino(), before.len()));
        fixture.finish();
    }
}
