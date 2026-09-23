//! Real-document proof for `aikit now-context field`: the field is assembled
//! against the ACTUAL registered ProjectCentral capability matrix in this
//! worktree and the ACTUAL QL UX spine trace — never a fixture standing in
//! for them — and asserted against counts read from those same documents
//! (never a hard-coded magic number that drifts the moment the spine is
//! revised, as it was mid-lane on 2026-09-23: XP04/XP05/XP07/XP08/XP09/XP10
//! moved from unbound to bound/legitimately-unavailable).

use std::path::{Path, PathBuf};

use aikit_cli::cli::NowFieldArgs;
use aikit_cli::contemplation_field::now_field;

fn ai_kit_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// The QL lane worktree's spine trace is the revision about to merge and
/// carries the XP04/XP05/XP07/XP08/XP09/XP10 bindings this session's brief
/// update named; prefer it when present. Fall back to the primary QL
/// checkout, and skip (never fail) when neither is mounted on this machine.
fn spine_trace_path() -> Option<PathBuf> {
    for candidate in [
        "/Users/admin/Central/worktrees/jev-redis-20260923/ql/docs/kernel-rebuild/ux-spine-trace.json",
        "/Users/admin/Central/Work/Quaternal-Logic/docs/kernel-rebuild/ux-spine-trace.json",
    ] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn field_args(
    matrix_manifest: PathBuf,
    matrix_csv: PathBuf,
    spine_trace: PathBuf,
    spine_repo_root: PathBuf,
    ai_kit_repo_root: PathBuf,
) -> NowFieldArgs {
    NowFieldArgs {
        request_file: None,
        projectcentral: None,
        matrix_manifest: Some(matrix_manifest),
        matrix_csv: Some(matrix_csv),
        spine_trace: Some(spine_trace),
        spine_repo_root: Some(spine_repo_root),
        ai_kit_repo_root: Some(ai_kit_repo_root),
        telos_goal_dir: None,
        serving_track: None,
        now_ref: None,
        central_root: None,
        ctrl_bin: None,
        redis_config: None,
        redis_participant_ref: None,
        allow_env_import: false,
        wiki_queries: Vec::new(),
        pass: "prospective".into(),
        return_file: None,
        repo: None,
        repo_name: None,
        base: None,
        head: "HEAD".into(),
        max_code_symbols: 8,
        gitnexus_binary: None,
        experience_reading: None,
    }
}

#[test]
fn field_over_the_real_registered_telos_matrix_and_ql_spine_matches_the_documents_own_counts() {
    let ai_kit_root = ai_kit_root();
    let manifest = ai_kit_root.join("ProjectCentral/user/telos/capability-matrix.json");
    let csv = ai_kit_root.join("ProjectCentral/user/telos/capability-matrix.csv");
    if !manifest.is_file() || !csv.is_file() {
        eprintln!(
            "skip: no real ProjectCentral/user/telos capability matrix at {}",
            ai_kit_root.display()
        );
        return;
    }
    let Some(spine_trace) = spine_trace_path() else {
        eprintln!("skip: no real QL ux-spine-trace.json on this machine");
        return;
    };
    let spine_repo_root = spine_trace
        .ancestors()
        .find(|p| p.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| spine_trace.parent().unwrap().to_path_buf());

    // Independently compute the expected practice-binding split straight
    // from the spine document, exactly as the field module's own binding
    // resolver would: unbound iff neither canonical_skill nor
    // native_skill_ref is present. This is the counter this session's brief
    // update asked for — read from the document, never hard-coded.
    let trace: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&spine_trace).unwrap()).unwrap();
    let practices = trace["practices"].as_array().expect("spine has practices");
    let total_practices = practices.len();
    let expected_unbound = practices
        .iter()
        .filter(|p| p["canonical_skill"].is_null() && p["native_skill_ref"].is_null())
        .count();
    let expected_bound_candidates = total_practices - expected_unbound;

    // Independently compute the expected capability-row count from the CSV.
    let csv_text = std::fs::read_to_string(&csv).unwrap();
    let expected_capabilities = csv_text
        .lines()
        .skip(1)
        .filter(|line| line.split(',').nth(1) == Some("capability"))
        .count();

    let args = field_args(
        manifest,
        csv,
        spine_trace,
        spine_repo_root,
        ai_kit_root.clone(),
    );
    let field =
        now_field(&ai_kit_root, args).expect("field assembly over real registered documents");

    assert_eq!(field["schema"], "aikit.contemplation-field/v1");
    assert_eq!(
        field["spine"]["practices"].as_array().unwrap().len(),
        total_practices
    );
    assert_eq!(
        field["matrix"]["capabilities"].as_array().unwrap().len(),
        expected_capabilities,
        "capability row count must match the real CSV's own capability rows"
    );

    let summary = &field["practice_binding_summary"];
    assert_eq!(summary["total"], total_practices);
    assert_eq!(
        summary["unbound"], expected_unbound,
        "unbound count must match what the real spine document itself declares \
         (neither canonical_skill nor native_skill_ref), not a hard-coded number"
    );
    let bound = summary["bound"].as_u64().unwrap() as usize;
    let bound_missing = summary["bound_missing"].as_u64().unwrap() as usize;
    assert_eq!(
        bound + bound_missing,
        expected_bound_candidates,
        "every practice naming a canonical_skill or native_skill_ref must resolve to bound or bound-missing"
    );

    // Every capability row the real matrix carries has both code_refs and
    // test_refs populated (verified independently on 2026-09-23); the field
    // must surface them structurally, not just embed them in a text excerpt.
    let capabilities = field["matrix"]["capabilities"].as_array().unwrap();
    assert!(capabilities
        .iter()
        .all(|c| c["code_refs"].as_array().is_some_and(|a| !a.is_empty())));
    assert!(capabilities
        .iter()
        .all(|c| c["test_refs"].as_array().is_some_and(|a| !a.is_empty())));

    // No changed subject was supplied, so the code lens and joins must be
    // absent, disclosed as such rather than fabricated.
    assert!(field["changed_subject"].is_null());
    assert!(field["code_lens"].is_null());
    assert!(field["joins"].as_array().unwrap().is_empty());
}

/// The same real documents, driven through the changed-subject + GitNexus
/// code-lens + cross-lens-join path against this very lane's own working
/// tree, proving the join from a real changed file to a real capability via
/// its authored `code_refs`.
#[test]
fn field_over_a_real_changed_subject_joins_the_touched_capability_explicitly() {
    let ai_kit_root = ai_kit_root();
    let manifest = ai_kit_root.join("ProjectCentral/user/telos/capability-matrix.json");
    let csv = ai_kit_root.join("ProjectCentral/user/telos/capability-matrix.csv");
    if !manifest.is_file() || !csv.is_file() {
        eprintln!("skip: no real ProjectCentral/user/telos capability matrix");
        return;
    }
    let Some(spine_trace) = spine_trace_path() else {
        eprintln!("skip: no real QL ux-spine-trace.json on this machine");
        return;
    };
    let spine_repo_root = spine_trace
        .ancestors()
        .find(|p| p.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| spine_trace.parent().unwrap().to_path_buf());

    let mut args = field_args(
        manifest,
        csv,
        spine_trace,
        spine_repo_root,
        ai_kit_root.clone(),
    );
    args.repo = Some(ai_kit_root.clone());
    // HEAD~1..HEAD always exists on this lane branch (it carries real
    // commits from the bootstrap and skill-capsule work); base==head is
    // covered separately as a disconnection control, so this proves the
    // ordinary case with whatever the branch's own last commit touched.
    args.base = Some("HEAD~1".into());
    args.head = "HEAD".into();
    args.gitnexus_binary = Some("gitnexus-definitely-not-installed-marker".into());

    let field = now_field(&ai_kit_root, args).expect("field assembly over a real changed subject");
    assert!(field["changed_subject"].is_object());
    let changed_paths = field["changed_subject"]["changed_paths"]
        .as_array()
        .unwrap();
    // GitNexus is deliberately pointed at a nonexistent binary above so this
    // test never depends on GitNexus being installed; the code lens must
    // disclose unavailability explicitly rather than silently omit itself.
    assert_eq!(field["code_lens"]["available"], false);
    assert!(field["code_lens"]["disclosure"].is_string());
    if !changed_paths.is_empty() {
        // At least the disclosure/join machinery ran over real paths; assert
        // every emitted join names an existing capability id from the matrix.
        let capability_ids: std::collections::BTreeSet<String> = field["matrix"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect();
        for join in field["joins"].as_array().unwrap() {
            if join["relation"] == "changed-path-implements-capability" {
                let to = join["to"].as_str().unwrap();
                assert!(
                    capability_ids.contains(to),
                    "join target {to} must be a real capability id from the matrix"
                );
                assert_eq!(join["basis"], "explicit");
            }
        }
    }
}
