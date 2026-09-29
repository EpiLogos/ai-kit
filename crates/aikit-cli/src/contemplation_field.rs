//! Native `aikit.contemplation-field/v1` assembly — the continuation of
//! `scripts/jev-redis/assemble_contemplation.py`, now native and revision-
//! bound. Same lineage: telos anchor, UX spine (stories/practices/coverage),
//! practice→Skill bindings, capability matrix, changed subject, GitNexus code
//! lens, deterministic cross-lens joins, tests/evidence and NOW/Redis state.
//!
//! Every document carries `source_ref` + content revision (the same
//! `blake3:` digest convention `jev_now.rs` already uses); every relation
//! carries `basis: explicit | derived(<provider>)` and the source it came
//! from. Nothing here invents a second matrix/spine/code registry: the
//! matrix reader is `jev_now::read_matrix`, extended in place to carry
//! `code_refs`/`test_refs`; the code lens is the existing
//! `GitNexusCodeIndexProvider`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aikit_adapters::central_file_map;
use aikit_adapters::gitnexus::GitNexusCodeIndexProvider;
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_core::context_source::{AgentVisibility, ExternalEgress};
use aikit_core::knowledge_code::{CodeIndexProvider, CodeIndexStatus, CodeReference};
use aikit_core::resource::SourceRevision;
use aikit_core::{AikitError, ResourceRef, Result, SourceRef};
use aikit_store::now_context::{RedisNowConfig, RedisNowStore};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app::Service;
use crate::cli::{
    KnowledgeCodeChangesArgs, KnowledgeCodeImpactArgs, KnowledgeCodeRepoArgs,
    KnowledgeCodeSearchArgs, KnowledgeCodeSub, KnowledgeCodeSymbolArgs, KnowledgeCodeTraceArgs,
    NowFieldArgs,
};
use crate::jev_now::{
    bounded_text, central_action, extract_now_source_refs, fail, file_digest, read_json,
    read_matrix, resolve_secret, MatrixCapabilityRow, MatrixPrepare,
};

pub const FIELD_REQUEST_SCHEMA: &str = "aikit.contemplation-field-request/v1";
pub const FIELD_SCHEMA: &str = "aikit.contemplation-field/v1";
const SPINE_TRACE_FORMAT: &str = "ql.ux-spine-trace/1";
const MAX_SPINE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TELOS_BYTES: usize = 512 * 1024;
const MAX_RETURN_BYTES: usize = 1024 * 1024;
const MAX_EXPERIENCE_READING_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_CODE_SYMBOLS: usize = 8;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

/// The typed request, whether it arrived as `--request-file` JSON or was
/// built from individual flags. Field names mirror the CLI flags.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FieldRequest {
    #[allow(dead_code)]
    schema: Option<String>,
    projectcentral: Option<PathBuf>,
    matrix_manifest: Option<PathBuf>,
    matrix_csv: Option<PathBuf>,
    spine_trace: Option<PathBuf>,
    spine_repo_root: Option<PathBuf>,
    ai_kit_repo_root: Option<PathBuf>,
    telos_goal_dir: Option<PathBuf>,
    serving_track: Option<String>,
    now_ref: Option<String>,
    central_root: Option<PathBuf>,
    ctrl_bin: Option<PathBuf>,
    redis_config: Option<PathBuf>,
    redis_participant_ref: Option<String>,
    allow_env_import: bool,
    #[serde(default)]
    wiki_queries: Vec<String>,
    pass: Option<String>,
    return_file: Option<PathBuf>,
    repo: Option<PathBuf>,
    repo_name: Option<String>,
    base: Option<String>,
    head: Option<String>,
    max_code_symbols: Option<usize>,
    gitnexus_binary: Option<String>,
    experience_reading: Option<PathBuf>,
}

fn args_to_request(args: NowFieldArgs) -> Result<FieldRequest> {
    if let Some(path) = &args.request_file {
        return read_json(path, "contemplation field request", 1024 * 1024);
    }
    Ok(FieldRequest {
        schema: Some(FIELD_REQUEST_SCHEMA.into()),
        projectcentral: args.projectcentral,
        matrix_manifest: args.matrix_manifest,
        matrix_csv: args.matrix_csv,
        spine_trace: args.spine_trace,
        spine_repo_root: args.spine_repo_root,
        ai_kit_repo_root: args.ai_kit_repo_root,
        telos_goal_dir: args.telos_goal_dir,
        serving_track: args.serving_track,
        now_ref: args.now_ref,
        central_root: args.central_root,
        ctrl_bin: args.ctrl_bin,
        redis_config: args.redis_config,
        redis_participant_ref: args.redis_participant_ref,
        allow_env_import: args.allow_env_import,
        wiki_queries: args.wiki_queries,
        pass: Some(args.pass),
        return_file: args.return_file,
        repo: args.repo,
        repo_name: args.repo_name,
        base: args.base,
        head: Some(args.head),
        max_code_symbols: Some(args.max_code_symbols),
        gitnexus_binary: args.gitnexus_binary,
        experience_reading: args.experience_reading,
    })
}

pub fn now_field(cwd: &Path, args: NowFieldArgs) -> Result<Value> {
    let request = args_to_request(args)?;
    assemble(cwd, &request)
}

fn assemble(cwd: &Path, request: &FieldRequest) -> Result<Value> {
    let pass = match request.pass.as_deref() {
        Some("retrospective") => "retrospective",
        Some("prospective") | None => "prospective",
        Some(other) => {
            return Err(fail(
                "contemplation_field.invalid_pass",
                format!("pass must be prospective or retrospective, got `{other}`"),
            ))
        }
    };

    let telos = match &request.telos_goal_dir {
        Some(dir) => read_telos(dir, request.serving_track.as_deref())?,
        None => None,
    };

    // A UX-spine trace is an optional lens: the field's code, capability and
    // test joins stand on their own, and no one product's spine is assumed.
    let ai_kit_repo_root = match &request.ai_kit_repo_root {
        Some(p) => p.clone(),
        None => discover_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf()),
    };
    let mut alias_roots = BTreeMap::new();
    alias_roots.insert("ai-kit".to_string(), ai_kit_repo_root.clone());
    let (spine, practice_binding_summary) = match request.spine_trace.as_ref() {
        None => (
            Value::Null,
            json!({"disclosure": "no UX-spine trace supplied; practice bindings not assessed"}),
        ),
        Some(spine_trace_path) => {
            let (trace, spine_digest) = read_spine(spine_trace_path)?;
            let spine_repo_root = match &request.spine_repo_root {
                Some(p) => p.clone(),
                None => discover_repo_root(spine_trace_path.parent().unwrap_or(Path::new(".")))
                    .ok_or_else(|| {
                        fail(
                            "contemplation_field.spine_repo_root",
                            "could not discover the spine trace's Git repository root; pass --spine-repo-root",
                        )
                    })?,
            };
            let stories = trace["stories"].as_array().cloned().unwrap_or_default();
            let coverage_cells = trace["m_capability_coverage"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let (practices, bound, bound_missing, unbound) =
                build_practices(&trace, &spine_repo_root, &alias_roots)?;
            (
                json!({
                    "source": spine_trace_path.display().to_string(),
                    "source_revision": spine_digest,
                    "spine_repo_root": spine_repo_root.display().to_string(),
                    "format": SPINE_TRACE_FORMAT,
                    "stories": stories,
                    "practices": practices,
                    "coverage_cells": coverage_cells,
                }),
                json!({
                    "bound": bound,
                    "bound_missing": bound_missing,
                    "unbound": unbound,
                    "total": bound + bound_missing + unbound,
                }),
            )
        }
    };

    let matrix_config = resolve_matrix_config(request)?;
    let (_items, matrix_evidence) = read_matrix(&matrix_config)?;

    let (changed_subject, code_lens, mut joins, touched_capability_ids) =
        match (&request.repo, &request.base) {
            (Some(repo), Some(base)) => {
                let head = request.head.as_deref().unwrap_or("HEAD");
                let runner = SystemRunner::new();
                let repo_root = repo.canonicalize().map_err(|e| {
                    fail(
                        "contemplation_field.repo_invalid",
                        format!("{}: {e}", repo.display()),
                    )
                })?;
                let base_sha = git_rev_parse(&runner, &repo_root, base)?;
                let head_sha = git_rev_parse(&runner, &repo_root, head)?;
                let changed_paths = git_diff_name_only(&runner, &repo_root, &base_sha, &head_sha)?;
                let repo_name = request.repo_name.clone().unwrap_or_else(|| {
                    repo_root
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("repo")
                        .to_string()
                });
                let source_ref = SourceRef::parse(format!("source:git/{repo_name}"))?;
                let subject = json!({
                    "repo": repo_root.display().to_string(),
                    "repo_name": repo_name,
                    "source_ref": source_ref,
                    "base_revision": format!("git:{base_sha}"),
                    "head_revision": format!("git:{head_sha}"),
                    "changed_paths": changed_paths,
                });
                let (lens, gitnexus_paths) = code_lens_readings(
                    &repo_root,
                    &repo_name,
                    &source_ref,
                    CompareRevisions {
                        base: &base_sha,
                        head: &head_sha,
                    },
                    &changed_paths,
                    request.max_code_symbols.unwrap_or(DEFAULT_MAX_CODE_SYMBOLS),
                    request.gitnexus_binary.as_deref(),
                );
                let (joins, touched) = cross_lens_joins(
                    &matrix_evidence.capability_rows,
                    &changed_paths,
                    &gitnexus_paths,
                );
                (Some(subject), Some(lens), joins, touched)
            }
            (None, None) => (None, None, Vec::new(), BTreeSet::new()),
            _ => {
                return Err(fail(
                    "contemplation_field.changed_subject_incomplete",
                    "the changed subject requires both repo and base",
                ))
            }
        };

    let tests_evidence_out = match (&request.repo, &changed_subject) {
        (Some(repo), Some(subject)) => {
            let repo_root = repo.canonicalize().unwrap_or_else(|_| repo.clone());
            let head_revision = subject["head_revision"].as_str().unwrap_or("");
            let head_sha = head_revision.strip_prefix("git:").unwrap_or(head_revision);
            tests_evidence(
                &repo_root,
                head_sha,
                &matrix_evidence.capability_rows,
                &touched_capability_ids,
            )
        }
        _ => Vec::new(),
    };

    let (experience_joins, experience_disclosure, experience_reading) =
        match &request.experience_reading {
            Some(path) => {
                let (bytes, digest) = file_digest(
                    path,
                    "experience coverage reading",
                    MAX_EXPERIENCE_READING_BYTES,
                )?;
                let doc: Value = serde_json::from_slice(&bytes).map_err(|e| {
                    fail(
                        "contemplation_field.experience_reading_invalid",
                        e.to_string(),
                    )
                })?;
                let (joins, disclosure) = experience_joins(&doc);
                (
                    joins,
                    disclosure,
                    Some(json!({
                        "source": path.display().to_string(),
                        "source_revision": digest,
                    })),
                )
            }
            None => (
                Vec::new(),
                Some(
                    "no --experience-reading was supplied; capability→story/practice relations \
                     are not fabricated here (only an explicit reading makes them explicit, and \
                     only Jev may propose them as candidates)"
                        .to_string(),
                ),
                None,
            ),
        };
    joins.extend(experience_joins);

    let now_section = read_now(request)?;
    let redis_section = read_redis(request)?;
    let knowledge_frames = read_knowledge_frames(cwd, &request.wiki_queries)?;

    let return_document = match &request.return_file {
        Some(path) => {
            let (bytes, digest) = file_digest(path, "Return/evidence document", MAX_RETURN_BYTES)?;
            Some(json!({
                "source": path.display().to_string(),
                "source_revision": digest,
                "excerpt": bounded_text(&String::from_utf8_lossy(&bytes), 4096),
            }))
        }
        None => None,
    };

    Ok(json!({
        "schema": FIELD_SCHEMA,
        "pass": pass,
        "assembled_at_unix_ms": now_ms(),
        "telos": telos,
        "spine": spine,
        "practice_binding_summary": practice_binding_summary,
        "matrix": {
            "source_manifest": matrix_config.manifest.display().to_string(),
            "source_csv": matrix_config.csv.display().to_string(),
            "manifest_digest": matrix_evidence.manifest_digest,
            "csv_digest": matrix_evidence.csv_digest,
            "matrix_id": matrix_evidence.matrix_id,
            "whole_account_ref": matrix_evidence.whole_account_ref,
            "view_id": matrix_evidence.view_id,
            "capabilities": matrix_evidence.capability_rows.iter().map(capability_row_json).collect::<Vec<_>>(),
            "grid_relations": matrix_evidence.grid_relations,
            "all_view_relations": matrix_evidence.all_view_relations,
        },
        "changed_subject": changed_subject,
        "code_lens": code_lens,
        "joins": joins,
        "tests_evidence": tests_evidence_out,
        "experience_reading": experience_reading,
        "experience_reading_disclosure": experience_disclosure,
        "now": now_section,
        "redis": redis_section,
        "return_document": return_document,
        "knowledge_frames": knowledge_frames,
    }))
}

/// Optional bounded Wiki/source-pool queries over the shared Knowledge
/// application — the same seam `jev_now::now_prepare_request` uses, kept
/// off by default (empty query list, no AIKit context discovery cost).
fn read_knowledge_frames(cwd: &Path, queries: &[String]) -> Result<Vec<Value>> {
    if queries.is_empty() {
        return Ok(Vec::new());
    }
    let mut service = Service::discover(cwd)?;
    let mut frames = Vec::new();
    for query in queries {
        let found = service.knowledge_search(query, 32)?;
        let addresses = found
            .hits
            .iter()
            .map(|hit| hit.address.clone())
            .collect::<Vec<_>>();
        let frame = service.knowledge_frame(Some(query.as_str()), &addresses)?;
        frames.push(
            serde_json::to_value(frame)
                .map_err(|e| fail("contemplation_field.knowledge_frame_encode", e.to_string()))?,
        );
    }
    Ok(frames)
}

// ---------------------------------------------------------------------
// Telos anchor
// ---------------------------------------------------------------------

fn read_telos(goal_dir: &Path, serving_track: Option<&str>) -> Result<Option<Value>> {
    let goal_md = goal_dir.join("goal.md");
    if !goal_md.is_file() {
        return Ok(None);
    }
    let (bytes, digest) = file_digest(&goal_md, "telos goal", MAX_TELOS_BYTES)?;
    let text = String::from_utf8_lossy(&bytes);
    let title = text
        .lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l[2..].trim().to_string());
    let tracks_dir = goal_dir.join("tracks");
    let mut tracks = Vec::new();
    if tracks_dir.is_dir() {
        for entry in std::fs::read_dir(&tracks_dir)
            .map_err(|e| fail("contemplation_field.telos_unavailable", e.to_string()))?
        {
            let entry =
                entry.map_err(|e| fail("contemplation_field.telos_unavailable", e.to_string()))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    tracks.push(stem.to_string());
                }
            }
        }
        tracks.sort();
    }
    let mut anchor = json!({
        "goal": title,
        "tracks": tracks,
        "source": goal_dir.display().to_string(),
        "source_revision": digest,
    });
    if let Some(track) = serving_track {
        anchor["serving_track"] = json!(track);
        let track_file = tracks_dir.join(format!("{track}.md"));
        if track_file.is_file() {
            let excerpt = std::fs::read_to_string(&track_file)
                .map_err(|e| fail("contemplation_field.telos_unavailable", e.to_string()))?;
            anchor["serving_track_excerpt"] = json!(bounded_text(&excerpt, 600));
        }
    }
    Ok(Some(anchor))
}

// ---------------------------------------------------------------------
// UX spine + practice→Skill bindings
// ---------------------------------------------------------------------

fn read_spine(path: &Path) -> Result<(Value, String)> {
    let (bytes, digest) = file_digest(path, "UX spine trace", MAX_SPINE_BYTES)?;
    let trace: Value = serde_json::from_slice(&bytes)
        .map_err(|e| fail("contemplation_field.spine_invalid", e.to_string()))?;
    if trace["format"] != SPINE_TRACE_FORMAT {
        return Err(fail(
            "contemplation_field.spine_invalid",
            format!("UX spine trace must declare format {SPINE_TRACE_FORMAT}"),
        ));
    }
    Ok((trace, digest))
}

/// Resolve one practice's Skill binding. `canonical_skill` and
/// `native_skill_ref` are mutually exclusive in practice: a `native_skill_ref`
/// (e.g. `ai-kit:registry/capsules/skill/aikit/<name>/payload/SKILL.md`) is
/// resolved by splitting on the first `:` into an alias and a path relative
/// to that alias's repo root. `classification`, when the spine carries it, is
/// always disclosed regardless of binding status.
struct Binding {
    status: &'static str,
    kind: Option<&'static str>,
    skill_ref: Option<String>,
    resolved_path: Option<String>,
    resolved_against: Option<String>,
    note: Option<String>,
}

fn resolve_binding(
    practice: &Value,
    spine_repo_root: &Path,
    alias_roots: &BTreeMap<String, PathBuf>,
) -> Value {
    let canonical_skill = practice["canonical_skill"].as_str();
    let native_skill_ref = practice["native_skill_ref"].as_str();
    let classification = practice["classification"].as_str();

    let binding = if let Some(cs) = canonical_skill {
        let p = spine_repo_root.join(cs);
        Binding {
            status: if p.is_file() {
                "bound"
            } else {
                "bound-missing"
            },
            kind: Some("canonical_skill"),
            skill_ref: Some(cs.to_string()),
            resolved_path: Some(p.display().to_string()),
            resolved_against: Some(spine_repo_root.display().to_string()),
            note: None,
        }
    } else if let Some(nsr) = native_skill_ref {
        match nsr.split_once(':') {
            Some((alias, rel)) if alias_roots.contains_key(alias) => {
                let root = &alias_roots[alias];
                let p = root.join(rel);
                Binding {
                    status: if p.is_file() {
                        "bound"
                    } else {
                        "bound-missing"
                    },
                    kind: Some("native_skill_ref"),
                    skill_ref: Some(nsr.to_string()),
                    resolved_path: Some(p.display().to_string()),
                    resolved_against: Some(root.display().to_string()),
                    note: None,
                }
            }
            Some((alias, _)) => Binding {
                status: "bound-missing",
                kind: Some("native_skill_ref"),
                skill_ref: Some(nsr.to_string()),
                resolved_path: None,
                resolved_against: None,
                note: Some(format!(
                    "native_skill_ref alias `{alias}` has no known repo root"
                )),
            },
            None => Binding {
                status: "bound-missing",
                kind: Some("native_skill_ref"),
                skill_ref: Some(nsr.to_string()),
                resolved_path: None,
                resolved_against: None,
                note: Some("native_skill_ref has no `alias:path` shape".to_string()),
            },
        }
    } else {
        Binding {
            status: "unbound",
            kind: None,
            skill_ref: None,
            resolved_path: None,
            resolved_against: None,
            note: None,
        }
    };

    json!({
        "status": binding.status,
        "basis": "explicit",
        "kind": binding.kind,
        "skill_ref": binding.skill_ref,
        "resolved_path": binding.resolved_path,
        "resolved_against": binding.resolved_against,
        "classification": classification,
        "gap_owner": practice["gap_owner"].clone(),
        "note": binding.note,
    })
}

fn build_practices(
    trace: &Value,
    spine_repo_root: &Path,
    alias_roots: &BTreeMap<String, PathBuf>,
) -> Result<(Vec<Value>, usize, usize, usize)> {
    let stories = trace["stories"].as_array().cloned().unwrap_or_default();
    let coverage = trace["m_capability_coverage"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let practices = trace["practices"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::with_capacity(practices.len());
    let (mut bound, mut bound_missing, mut unbound) = (0usize, 0usize, 0usize);
    for practice in &practices {
        let pid = practice["id"].as_str().unwrap_or_default().to_string();
        let served: Vec<String> = stories
            .iter()
            .filter(|s| {
                s["practices"]
                    .as_array()
                    .map(|arr| arr.iter().any(|p| p.as_str() == Some(pid.as_str())))
                    .unwrap_or(false)
            })
            .filter_map(|s| s["id"].as_str().map(str::to_owned))
            .collect();
        let served_set: BTreeSet<&str> = served.iter().map(String::as_str).collect();
        let cells: Vec<Value> = coverage
            .iter()
            .filter(|c| {
                c["stories"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .any(|s| s.as_str().map(|s| served_set.contains(s)).unwrap_or(false))
                    })
                    .unwrap_or(false)
            })
            .map(|c| json!({"capability_ref": c["capability_ref"], "stories": c["stories"]}))
            .collect();

        let binding = resolve_binding(practice, spine_repo_root, alias_roots);
        match binding["status"].as_str() {
            Some("bound") => bound += 1,
            Some("bound-missing") => bound_missing += 1,
            _ => unbound += 1,
        }

        out.push(json!({
            "id": pid,
            "purpose": practice["purpose"],
            "stories_served": served,
            "implementation_owner": practice["implementation_owner"],
            "support": practice["support"],
            "capability_coverage_cells": cells,
            "binding": binding,
        }));
    }
    Ok((out, bound, bound_missing, unbound))
}

// ---------------------------------------------------------------------
// Capability matrix (reuses jev_now::read_matrix)
// ---------------------------------------------------------------------

fn discover_matrix_carriers(projectcentral: &Path) -> Option<(PathBuf, PathBuf)> {
    for base in [
        projectcentral.join("user").join("telos"),
        projectcentral.join("user"),
        projectcentral.join("telos"),
    ] {
        let manifest = base.join("capability-matrix.json");
        let csv = base.join("capability-matrix.csv");
        if manifest.is_file() && csv.is_file() {
            return Some((manifest, csv));
        }
    }
    None
}

/// `MatrixCapabilityRow` serializes camelCase (the pre-existing convention of
/// `MatrixEvidence`, shared with `now-context prepare`'s own reply). The
/// field's own schema follows its python-assembler lineage and the request
/// side's convention instead (snake_case throughout), so capability rows are
/// re-mapped here rather than embedded as-is.
fn capability_row_json(row: &MatrixCapabilityRow) -> Value {
    json!({
        "id": row.id,
        "need": row.need,
        "operation": row.operation,
        "outcome": row.outcome,
        "implementation_status": row.implementation_status,
        "standing": row.standing,
        "source_refs": row.source_refs,
        "code_refs": row.code_refs,
        "test_refs": row.test_refs,
        "account_ref": row.account_ref,
    })
}

fn resolve_matrix_config(request: &FieldRequest) -> Result<MatrixPrepare> {
    let (manifest, csv) =
        if let (Some(m), Some(c)) = (&request.matrix_manifest, &request.matrix_csv) {
            (m.clone(), c.clone())
        } else if let Some(pc) = &request.projectcentral {
            discover_matrix_carriers(pc).ok_or_else(|| {
                fail(
                    "contemplation_field.matrix_missing",
                    format!(
                    "no capability-matrix.json+csv under {} (searched user/telos/, user/, telos/)",
                    pc.display()
                ),
                )
            })?
        } else {
            return Err(fail(
                "contemplation_field.matrix_missing",
                "provide matrix_manifest/matrix_csv or projectcentral",
            ));
        };
    Ok(MatrixPrepare {
        manifest,
        csv,
        view_id: None,
        capability_refs: Vec::new(),
        full_scope: true,
        agent_visibility: AgentVisibility::Payload,
        external_egress: ExternalEgress::Denied,
    })
}

// ---------------------------------------------------------------------
// Changed subject (Git)
// ---------------------------------------------------------------------

fn git_rev_parse(runner: &SystemRunner, repo: &Path, rev: &str) -> Result<String> {
    let argv = vec![
        "git".to_string(),
        "-C".into(),
        repo.display().to_string(),
        "rev-parse".into(),
        rev.into(),
    ];
    let out = runner
        .run(&argv)?
        .require(&argv, "contemplation_field.git_rev_parse_failed")?;
    Ok(out.stdout.trim().to_string())
}

fn git_diff_name_only(
    runner: &SystemRunner,
    repo: &Path,
    base: &str,
    head: &str,
) -> Result<Vec<String>> {
    let argv = vec![
        "git".to_string(),
        "-C".into(),
        repo.display().to_string(),
        "diff".into(),
        "--name-only".into(),
        base.into(),
        head.into(),
    ];
    let out = runner
        .run(&argv)?
        .require(&argv, "contemplation_field.git_diff_failed")?;
    Ok(out
        .stdout
        .lines()
        .map(str::to_owned)
        .filter(|l| !l.trim().is_empty())
        .collect())
}

fn discover_repo_root(start: &Path) -> Option<PathBuf> {
    let runner = SystemRunner::new();
    let dir = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };
    let argv = vec![
        "git".to_string(),
        "-C".into(),
        dir.display().to_string(),
        "rev-parse".into(),
        "--show-toplevel".into(),
    ];
    let out = runner.run(&argv).ok()?;
    if !out.ok() {
        return None;
    }
    let root = out.stdout.trim();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

fn exists_at_revision(runner: &SystemRunner, repo: &Path, revision: &str, path: &str) -> bool {
    let argv = vec![
        "git".to_string(),
        "-C".into(),
        repo.display().to_string(),
        "cat-file".into(),
        "-e".into(),
        format!("{revision}:{path}"),
    ];
    runner.run(&argv).map(|o| o.ok()).unwrap_or(false)
}

// ---------------------------------------------------------------------
// GitNexus code lens
// ---------------------------------------------------------------------

/// The changed subject's compare pair: GitNexus compares against `base`;
/// readings are stamped with `head`.
#[derive(Clone, Copy)]
struct CompareRevisions<'a> {
    base: &'a str,
    head: &'a str,
}

fn code_lens_readings(
    repo_root: &Path,
    repo_name: &str,
    source_ref: &SourceRef,
    revisions: CompareRevisions<'_>,
    changed_paths: &[String],
    max_symbols: usize,
    gitnexus_binary: Option<&str>,
) -> (Value, Vec<String>) {
    let CompareRevisions {
        base: base_sha,
        head: head_sha,
    } = revisions;
    let revision = SourceRevision::parse(format!("git:{head_sha}")).ok();
    let mut provider = match gitnexus_binary {
        Some(bin) => GitNexusCodeIndexProvider::with_binary_memoised(
            SystemRunner::new(),
            bin.to_string(),
            repo_name.to_string(),
            source_ref.clone(),
            revision.clone(),
        ),
        None => GitNexusCodeIndexProvider::new(
            SystemRunner::new(),
            repo_name.to_string(),
            source_ref.clone(),
            revision.clone(),
        ),
    };
    let status = provider.status();
    if !status.available {
        return (
            json!({
                "provider": status.provider,
                "available": false,
                "indexed": false,
                "disclosure": status.detail,
                "readings": [],
            }),
            Vec::new(),
        );
    }
    // The index is derived and rebuildable: when an incremental refresh fails
    // (e.g. an inconsistent upstream FTS index), rebuild once from source and
    // disclose that the reading stands on a forced rebuild.
    let mut index_disclosure: Option<String> = None;
    let indexed_status = match provider.index(repo_root, false) {
        Ok(s) => s,
        Err(first) => match provider.index(repo_root, true) {
            Ok(s) => {
                index_disclosure = Some(format!(
                    "incremental GitNexus index failed ({first}); rebuilt with --force"
                ));
                s
            }
            Err(e) => {
                return (
                    json!({
                        "provider": status.provider,
                        "available": true,
                        "indexed": false,
                        "disclosure": format!("GitNexus index failed: {first}; forced rebuild also failed: {e}"),
                        "readings": [],
                    }),
                    Vec::new(),
                )
            }
        },
    };

    let mut readings = Vec::new();
    let mut derived_paths = BTreeSet::new();

    match provider.detect_changes("compare", Some(base_sha)) {
        Ok(changes) => readings.push(json!({
            "kind": "detect_changes",
            "provider": changes.provider,
            "scope": changes.scope,
            "base_ref": changes.base_ref,
            "detail": changes.detail,
            "basis": "derived(gitnexus)",
        })),
        Err(e) => readings.push(json!({
            "kind": "detect_changes",
            "error": e.to_string(),
            "basis": "derived(gitnexus)",
        })),
    }

    let mut symbol_count = 0usize;
    'paths: for path in changed_paths {
        if symbol_count >= max_symbols {
            break;
        }
        let query = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(path.as_str());
        let hits = provider.search(query, 16).unwrap_or_default();
        for hit in hits {
            if symbol_count >= max_symbols {
                break 'paths;
            }
            if hit.reference.path != *path {
                continue;
            }
            let context = provider.context(&hit.reference);
            let impact = provider.impact(&hit.reference, "upstream");
            if let Ok(c) = &context {
                derived_paths.extend(extract_paths_from_json(&c.detail));
            }
            if let Ok(i) = &impact {
                derived_paths.extend(extract_paths_from_json(&i.detail));
            }
            readings.push(json!({
                "kind": "symbol",
                "reference": hit.reference,
                "resource": hit.resource,
                "provider": hit.provider,
                "context": context.as_ref().ok().map(|c| c.detail.clone()),
                "context_error": context.as_ref().err().map(|e| e.to_string()),
                "impact_upstream": impact.as_ref().ok().map(|c| c.detail.clone()),
                "impact_error": impact.as_ref().err().map(|e| e.to_string()),
                "basis": "derived(gitnexus)",
            }));
            symbol_count += 1;
        }
    }

    (
        json!({
            "provider": indexed_status.provider,
            "version": indexed_status.version,
            "tested_version": indexed_status.tested_version,
            "version_drift": indexed_status.version_drift,
            "available": true,
            "indexed": indexed_status.indexed,
            "source_ref": source_ref,
            "source_revision": revision,
            "index_disclosure": index_disclosure,
            "readings": readings,
        }),
        derived_paths.into_iter().collect(),
    )
}

/// Heuristic path extraction from an opaque GitNexus JSON reading (the CLI's
/// own upstream schema is not guaranteed stable across versions). Only
/// strings that look like repo-relative source paths are kept, and every
/// path taken from here is disclosed with `basis: derived(gitnexus)` — it is
/// candidate structural intelligence, never treated as an authored relation.
fn extract_paths_from_json(value: &Value) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => {
                if s.contains('/') && !s.contains(' ') && !s.starts_with("http") {
                    if let Some(ext) = Path::new(s).extension().and_then(|e| e.to_str()) {
                        if matches!(
                            ext,
                            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "md" | "json"
                        ) {
                            out.push(s.clone());
                        }
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            Value::Object(map) => map.values().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, &mut out);
    out
}

// ---------------------------------------------------------------------
// Cross-lens joins + tests/evidence
// ---------------------------------------------------------------------

fn path_match(changed: &str, code_ref: &str) -> bool {
    changed == code_ref
        || changed.starts_with(&format!("{code_ref}/"))
        || code_ref.starts_with(&format!("{changed}/"))
}

fn cross_lens_joins(
    capabilities: &[MatrixCapabilityRow],
    changed_paths: &[String],
    gitnexus_paths: &[String],
) -> (Vec<Value>, BTreeSet<String>) {
    let mut joins = Vec::new();
    let mut touched = BTreeSet::new();
    let changed_set: BTreeSet<&str> = changed_paths.iter().map(String::as_str).collect();
    for cap in capabilities {
        let mut cap_touched = false;
        for changed in changed_paths {
            if cap.code_refs.iter().any(|c| path_match(changed, c)) {
                joins.push(json!({
                    "from": changed,
                    "to": cap.id,
                    "relation": "changed-path-implements-capability",
                    "basis": "explicit",
                }));
                cap_touched = true;
            }
        }
        for derived in gitnexus_paths {
            if changed_set.contains(derived.as_str()) {
                continue; // already carried as explicit above
            }
            if cap.code_refs.iter().any(|c| path_match(derived, c)) {
                joins.push(json!({
                    "from": derived,
                    "to": cap.id,
                    "relation": "gitnexus-impacted-path-implicates-capability",
                    "basis": "derived(gitnexus)",
                }));
                cap_touched = true;
            }
        }
        if cap_touched {
            touched.insert(cap.id.clone());
            for test_ref in &cap.test_refs {
                joins.push(json!({
                    "from": cap.id,
                    "to": test_ref,
                    "relation": "capability-requires-test",
                    "basis": "explicit",
                }));
            }
        }
    }
    (joins, touched)
}

fn tests_evidence(
    repo_root: &Path,
    head_sha: &str,
    capabilities: &[MatrixCapabilityRow],
    touched: &BTreeSet<String>,
) -> Vec<Value> {
    let runner = SystemRunner::new();
    let mut out = Vec::new();
    for cap in capabilities {
        if !touched.contains(&cap.id) {
            continue;
        }
        for test_ref in &cap.test_refs {
            let exists = exists_at_revision(&runner, repo_root, head_sha, test_ref);
            out.push(json!({
                "capability_id": cap.id,
                "test_ref": test_ref,
                "exists_at_head": exists,
                "head_revision": format!("git:{head_sha}"),
            }));
        }
    }
    out
}

fn experience_joins(doc: &Value) -> (Vec<Value>, Option<String>) {
    if let Some(arr) = doc.get("coverage").and_then(Value::as_array) {
        let mut joins = Vec::new();
        for entry in arr {
            let cap = entry.get("capability_ref").and_then(Value::as_str);
            let stories = entry.get("stories").and_then(Value::as_array);
            if let (Some(cap), Some(stories)) = (cap, stories) {
                for story in stories {
                    if let Some(story) = story.as_str() {
                        joins.push(json!({
                            "from": cap,
                            "to": story,
                            "relation": "capability-covers-story",
                            "basis": "explicit",
                        }));
                    }
                }
            }
        }
        (joins, None)
    } else {
        (
            Vec::new(),
            Some(
                "the experience reading did not carry a recognised `coverage` array; no \
                 capability→story relation was fabricated"
                    .to_string(),
            ),
        )
    }
}

// ---------------------------------------------------------------------
// NOW / Redis (optional, read-only)
// ---------------------------------------------------------------------

fn read_now(request: &FieldRequest) -> Result<Option<Value>> {
    let (Some(now_ref), Some(root)) = (&request.now_ref, &request.central_root) else {
        return Ok(None);
    };
    let ctrl = request
        .ctrl_bin
        .clone()
        .unwrap_or_else(central_file_map::executable);
    let runner = SystemRunner::new().with_timeout(std::time::Duration::from_secs(20));
    let data = central_action(
        &runner,
        &ctrl,
        root,
        "central.now.read",
        &match request
            .projectcentral
            .as_deref()
            .and_then(|pc| pc.parent())
            .and_then(|project| project.file_name())
            .and_then(|name| name.to_str())
        {
            // A Project-scope NOW is read in its Project.
            Some(project) if now_ref.starts_with("central:now:project:") => {
                json!({"now_ref": now_ref, "project": project})
            }
            _ => json!({"now_ref": now_ref}),
        },
    )?;
    let source_refs = extract_now_source_refs(&data);
    Ok(Some(json!({
        "now_ref": now_ref,
        "concern": data["record"]["purpose"],
        "task_ref": data["record"]["task_ref"],
        "source_refs": source_refs,
        "revision": data["revision"],
    })))
}

fn read_redis(request: &FieldRequest) -> Result<Option<Value>> {
    let (Some(config_path), Some(participant_ref)) =
        (&request.redis_config, &request.redis_participant_ref)
    else {
        return Ok(None);
    };
    let config: RedisNowConfig = read_json(config_path, "Redis NOW config", 256 * 1024)?;
    let participant = ResourceRef::parse(participant_ref)?;
    let secret = resolve_secret(&config, request.allow_env_import)?;
    let store = RedisNowStore::new(config)?;
    let ack = store.ack_cursor(&participant, secret.as_ref())?;
    let last_delivery = store.last_delivery(&participant, secret.as_ref())?;
    let prepared = store.read_prepared(&participant, false, secret.as_ref())?;
    let change_cursor = ack.max(last_delivery.as_ref().map(|r| r.change_cursor).unwrap_or(0));
    Ok(Some(json!({
        "participant_ref": participant_ref,
        "ack_cursor": ack,
        "last_delivery": last_delivery,
        "neighbours": prepared.as_ref().map(|p| p.neighbours.clone()).unwrap_or_default(),
        "change_cursor": change_cursor,
    })))
}

// ---------------------------------------------------------------------
// `aikit knowledge code <verb>` — the direct code-lens CLI
// ---------------------------------------------------------------------

fn status_summary(status: &CodeIndexStatus) -> Value {
    json!({
        "provider": status.provider,
        "version": status.version,
        "tested_version": status.tested_version,
        "version_drift": status.version_drift,
        "indexed": status.indexed,
        "capabilities": status.capabilities,
        "detail": status.detail,
    })
}

type CodeProvider = (
    GitNexusCodeIndexProvider<SystemRunner>,
    PathBuf,
    SourceRef,
    Option<SourceRevision>,
);

fn code_provider(repo: &KnowledgeCodeRepoArgs) -> Result<CodeProvider> {
    let repo_root = repo.repo.canonicalize().map_err(|e| {
        AikitError::new(
            "knowledge.code_repo_invalid",
            format!("{}: {e}", repo.repo.display()),
        )
    })?;
    let repo_name = repo.repo_name.clone().unwrap_or_else(|| {
        repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo")
            .to_string()
    });
    let runner = SystemRunner::new();
    let revision = match &repo.revision {
        Some(r) => Some(SourceRevision::parse(format!("git:{r}"))?),
        None => git_rev_parse(&runner, &repo_root, "HEAD")
            .ok()
            .and_then(|sha| SourceRevision::parse(format!("git:{sha}")).ok()),
    };
    let source = SourceRef::parse(format!("source:git/{repo_name}"))?;
    let provider = match &repo.gitnexus_binary {
        Some(bin) => GitNexusCodeIndexProvider::with_binary_memoised(
            SystemRunner::new(),
            bin.clone(),
            repo_name,
            source.clone(),
            revision.clone(),
        ),
        None => GitNexusCodeIndexProvider::new(
            SystemRunner::new(),
            repo_name,
            source.clone(),
            revision.clone(),
        ),
    };
    Ok((provider, repo_root, source, revision))
}

fn symbol_reference(
    source: &SourceRef,
    revision: &Option<SourceRevision>,
    a: &KnowledgeCodeSymbolArgs,
) -> CodeReference {
    CodeReference {
        source: source.clone(),
        revision: revision.clone(),
        path: a.file.clone(),
        symbol: Some(a.symbol.clone()),
        kind: a.kind.clone(),
        line: None,
    }
}

pub fn knowledge_code(cmd: KnowledgeCodeSub) -> Result<Value> {
    match cmd {
        KnowledgeCodeSub::Status(repo) => {
            let (provider, _root, _source, _revision) = code_provider(&repo)?;
            let status = provider.status();
            Ok(json!({
                "schema": "aikit.code-lens-reading/v1",
                "operation": "status",
                "basis": "observed",
                "available": status.available,
                "reading": status_summary(&status),
            }))
        }
        KnowledgeCodeSub::Index(a) => {
            let (mut provider, root, _source, _revision) = code_provider(&a.repo)?;
            let status = provider.index(&root, a.force)?;
            Ok(json!({
                "schema": "aikit.code-lens-reading/v1",
                "operation": "index",
                "basis": "observed",
                "reading": status_summary(&status),
            }))
        }
        KnowledgeCodeSub::Search(a) => code_search(a),
        KnowledgeCodeSub::Context(a) => code_context(a),
        KnowledgeCodeSub::Impact(a) => code_impact(a),
        KnowledgeCodeSub::Trace(a) => code_trace(a),
        KnowledgeCodeSub::Changes(a) => code_changes(a),
        KnowledgeCodeSub::Check(repo) => {
            let (mut provider, root, _source, _revision) = code_provider(&repo)?;
            provider.index(&root, false)?;
            let check = provider.structural_check()?;
            Ok(json!({
                "schema": "aikit.code-lens-reading/v1",
                "operation": "structural_check",
                "basis": "derived(gitnexus)",
                "status": status_summary(&provider.status()),
                "reading": check,
            }))
        }
    }
}

fn code_search(a: KnowledgeCodeSearchArgs) -> Result<Value> {
    let (mut provider, root, _source, _revision) = code_provider(&a.repo)?;
    provider.index(&root, false)?;
    let hits = provider.search(&a.query, a.limit)?;
    Ok(json!({
        "schema": "aikit.code-lens-reading/v1",
        "operation": "search",
        "basis": "derived(gitnexus)",
        "status": status_summary(&provider.status()),
        "hits": hits,
    }))
}

fn code_context(a: KnowledgeCodeSymbolArgs) -> Result<Value> {
    let (mut provider, root, source, revision) = code_provider(&a.repo)?;
    provider.index(&root, false)?;
    let reference = symbol_reference(&source, &revision, &a);
    let context = provider.context(&reference)?;
    Ok(json!({
        "schema": "aikit.code-lens-reading/v1",
        "operation": "context",
        "basis": "derived(gitnexus)",
        "status": status_summary(&provider.status()),
        "reading": context,
    }))
}

fn code_impact(a: KnowledgeCodeImpactArgs) -> Result<Value> {
    let (mut provider, root, source, revision) = code_provider(&a.symbol.repo)?;
    provider.index(&root, false)?;
    let reference = symbol_reference(&source, &revision, &a.symbol);
    let impact = provider.impact(&reference, &a.direction)?;
    Ok(json!({
        "schema": "aikit.code-lens-reading/v1",
        "operation": "impact",
        "direction": a.direction,
        "basis": "derived(gitnexus)",
        "status": status_summary(&provider.status()),
        "reading": impact,
    }))
}

fn code_trace(a: KnowledgeCodeTraceArgs) -> Result<Value> {
    let (mut provider, root, source, revision) = code_provider(&a.repo)?;
    provider.index(&root, false)?;
    let from = CodeReference {
        source: source.clone(),
        revision: revision.clone(),
        path: a.from_file,
        symbol: Some(a.from_symbol),
        kind: None,
        line: None,
    };
    let to = CodeReference {
        source,
        revision,
        path: a.to_file,
        symbol: Some(a.to_symbol),
        kind: None,
        line: None,
    };
    let trace = provider.trace(&from, &to)?;
    Ok(json!({
        "schema": "aikit.code-lens-reading/v1",
        "operation": "trace",
        "basis": "derived(gitnexus)",
        "status": status_summary(&provider.status()),
        "reading": trace,
    }))
}

fn code_changes(a: KnowledgeCodeChangesArgs) -> Result<Value> {
    let (mut provider, root, _source, _revision) = code_provider(&a.repo)?;
    provider.index(&root, false)?;
    let changes = provider.detect_changes(&a.scope, a.base_ref.as_deref())?;
    Ok(json!({
        "schema": "aikit.code-lens-reading/v1",
        "operation": "detect_changes",
        "basis": "derived(gitnexus)",
        "status": status_summary(&provider.status()),
        "reading": changes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git fixture command");
        assert!(status.success(), "git fixture command failed: {args:?}");
    }

    fn write_matrix(dir: &Path, code_refs: &str, test_refs: &str) -> (PathBuf, PathBuf) {
        let manifest = dir.join("capability-matrix.json");
        let csv = dir.join("capability-matrix.csv");
        std::fs::write(
            &manifest,
            serde_json::to_vec_pretty(&json!({
                "protocol":"ql-capability-matrix/1",
                "matrix_id":"matrix.fixture",
                "anchor_ref":"fixture:account:whole",
                "default_view":"product-field",
                "views":[{
                    "id":"product-field",
                    "title":"Fixture",
                    "semantics":"Fixture semantics",
                    "row_axis":{"id":"seed","label":"Seed","members":[{"id":"q0","label":"Why?"}]},
                    "column_axis":{"id":"field","label":"Field","members":[{"id":"S2","label":"AIKit"}]}
                }]
            })).unwrap(),
        ).unwrap();
        std::fs::write(
            &csv,
            format!(
                "id,record_type,view_id,row_id,column_id,capability_refs,need,operation,outcome,implementation_status,standing,source_refs,code_refs,test_refs,account_ref,relation,coverage,extensions,question\n\
                 cap.fixture,capability,,,,[],Need,operate,outcome,implemented,implementation-fact,source/one,{code_refs},{test_refs},account.html#q1,,,,\n"
            ),
        ).unwrap();
        (manifest, csv)
    }

    fn write_spine(dir: &Path) -> PathBuf {
        let path = dir.join("ux-spine-trace.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "format": "ql.ux-spine-trace/1",
                "stories": [
                    {"id":"UX01","practices":["XP01","XP02"]}
                ],
                "practices": [
                    {"id":"XP01","purpose":"p1","canonical_skill":"skills/one/SKILL.md","implementation_owner":"owner"},
                    {"id":"XP02","purpose":"p2","native_skill_ref":"ai-kit:registry/capsules/skill/aikit/two/SKILL.md","classification":"composed-skillset-aikit-owned"},
                    {"id":"XP03","purpose":"p3"}
                ],
                "m_capability_coverage": [
                    {"capability_ref":"CAP-A","stories":["UX01"]}
                ]
            })).unwrap(),
        ).unwrap();
        path
    }

    #[test]
    fn practice_bindings_resolve_canonical_skill_native_skill_ref_and_unbound() {
        let spine_dir = tempfile::tempdir().unwrap();
        let ai_kit_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(spine_dir.path().join("skills/one")).unwrap();
        std::fs::write(spine_dir.path().join("skills/one/SKILL.md"), "# one").unwrap();
        std::fs::create_dir_all(ai_kit_dir.path().join("registry/capsules/skill/aikit/two"))
            .unwrap();
        std::fs::write(
            ai_kit_dir
                .path()
                .join("registry/capsules/skill/aikit/two/SKILL.md"),
            "# two",
        )
        .unwrap();

        let spine_path = write_spine(spine_dir.path());
        let (trace, _digest) = read_spine(&spine_path).unwrap();
        let mut alias_roots = BTreeMap::new();
        alias_roots.insert("ai-kit".to_string(), ai_kit_dir.path().to_path_buf());
        let (practices, bound, bound_missing, unbound) =
            build_practices(&trace, spine_dir.path(), &alias_roots).unwrap();
        assert_eq!(practices.len(), 3);
        assert_eq!(
            bound, 2,
            "XP01 canonical_skill and XP02 native_skill_ref both exist"
        );
        assert_eq!(bound_missing, 0);
        assert_eq!(unbound, 1, "XP03 carries neither field");
        let xp01 = practices.iter().find(|p| p["id"] == "XP01").unwrap();
        assert_eq!(xp01["binding"]["status"], "bound");
        assert_eq!(xp01["binding"]["kind"], "canonical_skill");
        let xp02 = practices.iter().find(|p| p["id"] == "XP02").unwrap();
        assert_eq!(xp02["binding"]["status"], "bound");
        assert_eq!(xp02["binding"]["kind"], "native_skill_ref");
        assert_eq!(
            xp02["binding"]["classification"],
            "composed-skillset-aikit-owned"
        );
        let xp03 = practices.iter().find(|p| p["id"] == "XP03").unwrap();
        assert_eq!(xp03["binding"]["status"], "unbound");
        assert_eq!(xp03["stories_served"], Value::Array(Vec::new()));
    }

    #[test]
    fn a_named_but_absent_canonical_skill_is_bound_missing_not_unbound() {
        let spine_dir = tempfile::tempdir().unwrap();
        let path = spine_dir.path().join("ux-spine-trace.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "format": "ql.ux-spine-trace/1",
                "stories": [],
                "practices": [
                    {"id":"XP99","purpose":"p","canonical_skill":"skills/absent/SKILL.md"}
                ],
                "m_capability_coverage": []
            }))
            .unwrap(),
        )
        .unwrap();
        let (trace, _digest) = read_spine(&path).unwrap();
        let (practices, bound, bound_missing, unbound) =
            build_practices(&trace, spine_dir.path(), &BTreeMap::new()).unwrap();
        assert_eq!(bound, 0);
        assert_eq!(bound_missing, 1);
        assert_eq!(unbound, 0);
        assert_eq!(practices[0]["binding"]["status"], "bound-missing");
    }

    #[test]
    fn matrix_scope_change_and_git_wrong_subject_are_disclosed_not_erroring() {
        // base == head must disclose an empty changed set, never error.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.txt"), "one").unwrap();
        git(root, &["init", "-q"]);
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.email=a@b.invalid",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "init",
            ],
        );
        let runner = SystemRunner::new();
        let sha = git_rev_parse(&runner, root, "HEAD").unwrap();
        let changed = git_diff_name_only(&runner, root, &sha, &sha).unwrap();
        assert!(changed.is_empty());
    }

    #[test]
    fn cross_lens_join_matches_by_explicit_code_ref_prefix_and_ties_tests() {
        let cap = MatrixCapabilityRow {
            id: "cap.one".into(),
            need: "n".into(),
            operation: "o".into(),
            outcome: "out".into(),
            implementation_status: "implemented".into(),
            standing: "implementation-fact".into(),
            source_refs: vec![],
            code_refs: vec!["crates/aikit-cli/src/jev_now.rs".into()],
            test_refs: vec!["crates/aikit-cli/tests/jev_now.rs".into()],
            account_ref: "a.html#q".into(),
        };
        let changed = vec!["crates/aikit-cli/src/jev_now.rs".to_string()];
        let (joins, touched) = cross_lens_joins(&[cap], &changed, &[]);
        assert!(touched.contains("cap.one"));
        assert!(joins
            .iter()
            .any(|j| j["relation"] == "changed-path-implements-capability"
                && j["basis"] == "explicit"));
        assert!(joins
            .iter()
            .any(|j| j["relation"] == "capability-requires-test" && j["basis"] == "explicit"));
    }

    #[test]
    fn experience_reading_without_recognised_shape_discloses_instead_of_fabricating() {
        let (joins, disclosure) = experience_joins(&json!({"unexpected": true}));
        assert!(joins.is_empty());
        assert!(disclosure.is_some());
    }

    #[test]
    fn experience_reading_with_recognised_coverage_produces_explicit_joins() {
        let (joins, disclosure) = experience_joins(&json!({
            "coverage": [{"capability_ref": "cap.one", "stories": ["UX01", "UX02"]}]
        }));
        assert_eq!(joins.len(), 2);
        assert!(disclosure.is_none());
        assert!(joins.iter().all(|j| j["basis"] == "explicit"));
    }

    #[test]
    fn matrix_read_carries_code_refs_and_test_refs_through_capability_rows() {
        let dir = tempfile::tempdir().unwrap();
        let (manifest, csv) =
            write_matrix(dir.path(), "crates/one.rs;crates/two.rs", "tests/one.rs");
        let config = MatrixPrepare {
            manifest,
            csv,
            view_id: None,
            capability_refs: Vec::new(),
            full_scope: true,
            agent_visibility: AgentVisibility::Payload,
            external_egress: ExternalEgress::Denied,
        };
        let (_items, evidence) = read_matrix(&config).unwrap();
        assert_eq!(evidence.capability_rows.len(), 1);
        assert_eq!(
            evidence.capability_rows[0].code_refs,
            vec!["crates/one.rs".to_string(), "crates/two.rs".to_string()]
        );
        assert_eq!(
            evidence.capability_rows[0].test_refs,
            vec!["tests/one.rs".to_string()]
        );
    }

    #[test]
    fn field_assembly_over_a_minimal_real_fixture_discloses_no_code_lens_without_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        let spine_path = write_spine(dir.path());
        let (manifest, csv) = write_matrix(dir.path(), "crates/one.rs", "tests/one.rs");
        let request = FieldRequest {
            schema: Some(FIELD_REQUEST_SCHEMA.into()),
            spine_trace: Some(spine_path),
            spine_repo_root: Some(dir.path().to_path_buf()),
            ai_kit_repo_root: Some(dir.path().to_path_buf()),
            matrix_manifest: Some(manifest),
            matrix_csv: Some(csv),
            pass: Some("prospective".into()),
            ..Default::default()
        };
        let field = assemble(dir.path(), &request).unwrap();
        assert_eq!(field["schema"], FIELD_SCHEMA);
        assert_eq!(field["pass"], "prospective");
        assert!(field["changed_subject"].is_null());
        assert!(field["code_lens"].is_null());
        assert_eq!(field["matrix"]["capabilities"].as_array().unwrap().len(), 1);
        assert_eq!(field["practice_binding_summary"]["total"], 3);
        assert!(field["experience_reading_disclosure"].is_string());
    }

    #[test]
    fn field_assembly_without_a_spine_trace_discloses_it_and_keeps_every_other_lens() {
        let dir = tempfile::tempdir().unwrap();
        let (manifest, csv) = write_matrix(dir.path(), "crates/one.rs", "tests/one.rs");
        let request = FieldRequest {
            schema: Some(FIELD_REQUEST_SCHEMA.into()),
            ai_kit_repo_root: Some(dir.path().to_path_buf()),
            matrix_manifest: Some(manifest),
            matrix_csv: Some(csv),
            pass: Some("prospective".into()),
            ..Default::default()
        };
        let field = assemble(dir.path(), &request).unwrap();
        assert!(field["spine"].is_null(), "no product's spine is assumed");
        assert!(field["practice_binding_summary"]["disclosure"].is_string());
        assert_eq!(field["matrix"]["capabilities"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn field_assembly_refuses_a_changed_subject_missing_its_base() {
        let dir = tempfile::tempdir().unwrap();
        let spine_path = write_spine(dir.path());
        let (manifest, csv) = write_matrix(dir.path(), "crates/one.rs", "tests/one.rs");
        let request = FieldRequest {
            spine_trace: Some(spine_path),
            spine_repo_root: Some(dir.path().to_path_buf()),
            matrix_manifest: Some(manifest),
            matrix_csv: Some(csv),
            repo: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let error = assemble(dir.path(), &request).unwrap_err();
        assert_eq!(
            error.code(),
            "contemplation_field.changed_subject_incomplete"
        );
    }

    #[test]
    fn wrong_subject_base_equals_head_discloses_an_empty_changed_set_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.txt"), "one").unwrap();
        git(root, &["init", "-q"]);
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.email=a@b.invalid",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "init",
            ],
        );
        let spine_path = write_spine(root);
        let (manifest, csv) = write_matrix(root, "crates/one.rs", "tests/one.rs");
        let request = FieldRequest {
            spine_trace: Some(spine_path),
            spine_repo_root: Some(root.to_path_buf()),
            matrix_manifest: Some(manifest),
            matrix_csv: Some(csv),
            repo: Some(root.to_path_buf()),
            base: Some("HEAD".into()),
            head: Some("HEAD".into()),
            gitnexus_binary: Some("aikit-field-test-no-such-gitnexus-binary".into()),
            ..Default::default()
        };
        let field = assemble(root, &request).expect("base == head must not error");
        assert!(field["changed_subject"].is_object());
        assert_eq!(
            field["changed_subject"]["changed_paths"],
            Value::Array(Vec::new()),
            "an identical base/head must disclose an empty changed set, never error"
        );
        assert_eq!(
            field["changed_subject"]["base_revision"],
            field["changed_subject"]["head_revision"]
        );
        assert!(field["joins"].as_array().unwrap().is_empty());
    }

    #[test]
    fn gitnexus_unavailable_is_disclosed_and_the_field_still_assembles_with_explicit_relations() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("crates")).unwrap();
        std::fs::write(root.join("crates/one.rs"), "// one").unwrap();
        git(root, &["init", "-q"]);
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.email=a@b.invalid",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "init",
            ],
        );
        std::fs::write(root.join("crates/one.rs"), "// one changed").unwrap();
        git(root, &["add", "crates/one.rs"]);
        git(
            root,
            &[
                "-c",
                "user.email=a@b.invalid",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "change",
            ],
        );
        let spine_path = write_spine(root);
        let (manifest, csv) = write_matrix(root, "crates/one.rs", "tests/one.rs");
        let request = FieldRequest {
            spine_trace: Some(spine_path),
            spine_repo_root: Some(root.to_path_buf()),
            matrix_manifest: Some(manifest),
            matrix_csv: Some(csv),
            repo: Some(root.to_path_buf()),
            base: Some("HEAD~1".into()),
            head: Some("HEAD".into()),
            // A binary name that cannot exist: proves the disclosed-unavailable
            // path rather than depending on whether GitNexus happens to be
            // installed on the machine running this unit test.
            gitnexus_binary: Some("aikit-field-test-no-such-gitnexus-binary".into()),
            ..Default::default()
        };
        let field =
            assemble(root, &request).expect("GitNexus unavailability must not error the field");
        let code_lens = &field["code_lens"];
        assert_eq!(code_lens["available"], false);
        assert!(
            code_lens["disclosure"].is_string()
                && !code_lens["disclosure"].as_str().unwrap().is_empty(),
            "unavailability must be disclosed explicitly, never silently empty: {code_lens}"
        );
        assert_eq!(code_lens["readings"], Value::Array(Vec::new()));
        // Explicit relations (changed path -> capability -> test) never depend
        // on GitNexus: they come straight from the authored code_refs/test_refs.
        let joins = field["joins"].as_array().unwrap();
        assert!(joins.iter().any(|j| j["from"] == "crates/one.rs"
            && j["to"] == "cap.fixture"
            && j["relation"] == "changed-path-implements-capability"
            && j["basis"] == "explicit"));
        assert!(joins.iter().any(|j| j["from"] == "cap.fixture"
            && j["to"] == "tests/one.rs"
            && j["relation"] == "capability-requires-test"
            && j["basis"] == "explicit"));
    }

    #[test]
    fn each_call_reads_the_matrix_fresh_so_output_never_mixes_two_readings() {
        let dir = tempfile::tempdir().unwrap();
        let spine_path = write_spine(dir.path());
        let (manifest, csv) = write_matrix(dir.path(), "crates/one.rs", "tests/one.rs");
        let request = FieldRequest {
            spine_trace: Some(spine_path),
            spine_repo_root: Some(dir.path().to_path_buf()),
            matrix_manifest: Some(manifest),
            matrix_csv: Some(csv.clone()),
            ..Default::default()
        };
        let field1 = assemble(dir.path(), &request).unwrap();
        let digest1 = field1["matrix"]["csv_digest"].clone();
        assert_eq!(
            field1["matrix"]["capabilities"][0]["code_refs"],
            json!(["crates/one.rs"])
        );

        // Mutate the matrix on disk after the first read.
        write_matrix(dir.path(), "crates/one.rs;crates/two.rs", "tests/one.rs");
        let field2 = assemble(dir.path(), &request).unwrap();
        let digest2 = field2["matrix"]["csv_digest"].clone();

        assert_ne!(
            digest1, digest2,
            "the digest must move when the content moves"
        );
        assert_eq!(
            field2["matrix"]["capabilities"][0]["code_refs"],
            json!(["crates/one.rs", "crates/two.rs"]),
            "the second call's capability rows must match the second call's own read, never the first"
        );
        assert_eq!(
            field1["matrix"]["capabilities"][0]["code_refs"],
            json!(["crates/one.rs"]),
            "the first call's already-returned output must not retroactively change"
        );
    }
}
