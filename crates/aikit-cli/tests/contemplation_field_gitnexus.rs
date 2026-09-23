//! Real GitNexus proof for the contemplation field's code lens, on a
//! disposable temp Git fixture — reusing the gating pattern of
//! `crates/aikit-adapters/tests/gitnexus_real.rs` (real `gitnexus` binary,
//! same `AIKIT_REQUIRE_GITNEXUS_REAL` escape hatch, graceful skip otherwise).
//!
//! Proves the whole chain the design asks for: a changed file becomes a
//! `CodeReference` with provenance through the code lens, ties to a
//! capability through its authored `code_refs`, and the capability's
//! `test_refs` are checked for existence at the changed subject's head.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use aikit_cli::cli::NowFieldArgs;
use aikit_cli::contemplation_field::now_field;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("git fixture command");
    assert!(status.success(), "git fixture command failed: {args:?}");
}

fn write_matrix(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    let manifest = dir.join("capability-matrix.json");
    let csv = dir.join("capability-matrix.csv");
    fs::write(
        &manifest,
        serde_json::to_vec_pretty(&serde_json::json!({
            "protocol":"ql-capability-matrix/1",
            "matrix_id":"matrix.gitnexus-fixture",
            "anchor_ref":"fixture:account:whole",
            "default_view":"product-field",
            "views":[{
                "id":"product-field",
                "title":"Fixture",
                "semantics":"Fixture semantics",
                "row_axis":{"id":"seed","label":"Seed","members":[{"id":"q0","label":"Why?"}]},
                "column_axis":{"id":"field","label":"Field","members":[{"id":"S2","label":"AIKit"}]}
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &csv,
        "id,record_type,view_id,row_id,column_id,capability_refs,need,operation,outcome,implementation_status,standing,source_refs,code_refs,test_refs,account_ref,relation,coverage,extensions,question\n\
         cap.auth,capability,,,,[],Authenticate,validate and log in,session,implemented,implementation-fact,README.md,src/auth.ts,tests/auth.test.ts,account.html#q1,,,,\n",
    )
    .unwrap();
    (manifest, csv)
}

fn write_spine(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("ux-spine-trace.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "ql.ux-spine-trace/1",
            "stories": [],
            "practices": [],
            "m_capability_coverage": []
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

#[test]
fn changed_file_becomes_a_code_reference_ties_to_its_capability_and_checks_its_test_ref() {
    let fixture = tempfile::tempdir().expect("temp git fixture");
    let root = fixture.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("src/auth.ts"),
        r#"export function validate(token: string): boolean {
  return token.length > 3;
}

export function login(token: string): boolean {
  return validate(token);
}
"#,
    )
    .unwrap();
    fs::write(root.join("tests/auth.test.ts"), "// placeholder\n").unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"aikit-field-gitnexus-fixture"}"#,
    )
    .unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.email=aikit@example.invalid",
            "-c",
            "user.name=AIKit CI",
            "commit",
            "-qm",
            "base",
        ],
    );

    // Change the subject: extend auth.ts with a new symbol.
    fs::write(
        root.join("src/auth.ts"),
        r#"export function validate(token: string): boolean {
  return token.length > 3;
}

export function login(token: string): boolean {
  return validate(token);
}

export function logout(): boolean {
  return true;
}
"#,
    )
    .unwrap();
    git(root, &["add", "src/auth.ts"]);
    git(
        root,
        &[
            "-c",
            "user.email=aikit@example.invalid",
            "-c",
            "user.name=AIKit CI",
            "commit",
            "-qm",
            "extend auth",
        ],
    );

    let (manifest, csv) = write_matrix(root);
    let spine_trace = write_spine(root);

    let args = NowFieldArgs {
        request_file: None,
        projectcentral: None,
        matrix_manifest: Some(manifest),
        matrix_csv: Some(csv),
        spine_trace: Some(spine_trace),
        spine_repo_root: Some(root.to_path_buf()),
        ai_kit_repo_root: Some(root.to_path_buf()),
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
        repo: Some(root.to_path_buf()),
        repo_name: None,
        base: Some("HEAD~1".into()),
        head: "HEAD".into(),
        max_code_symbols: 8,
        gitnexus_binary: None,
        experience_reading: None,
    };

    let field = now_field(root, args).expect("field assembly over the GitNexus fixture");

    assert!(field["changed_subject"].is_object());
    assert_eq!(
        field["changed_subject"]["changed_paths"],
        serde_json::json!(["src/auth.ts"])
    );

    let code_lens = &field["code_lens"];
    if code_lens["available"] != true || code_lens["indexed"] != true {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_GITNEXUS_REAL").is_none(),
            "AIKIT_REQUIRE_GITNEXUS_REAL is set but GitNexus is unavailable or unindexed: {code_lens}"
        );
        eprintln!("skip: GitNexus unavailable on this machine ({code_lens})");
        return;
    }

    // A real symbol-level reading with provenance: provider, source_ref,
    // CodeReference (never a bare provider node id), and basis derived(gitnexus).
    let readings = code_lens["readings"].as_array().unwrap();
    let symbol_readings: Vec<_> = readings
        .iter()
        .filter(|r| r["kind"] == "symbol" && r["reference"]["path"] == "src/auth.ts")
        .collect();
    assert!(
        !symbol_readings.is_empty(),
        "expected at least one real GitNexus symbol reading for the changed file: {readings:?}"
    );
    for reading in &symbol_readings {
        assert_eq!(reading["basis"], "derived(gitnexus)");
        assert!(reading["reference"]["symbol"].is_string());
        assert!(reading["reference"]["source"]
            .as_str()
            .is_some_and(|s| s.starts_with("source:git/")));
    }

    // Cross-lens join: the changed file ties to the capability whose
    // code_refs names it, explicitly, and the capability requires its test.
    let joins = field["joins"].as_array().unwrap();
    assert!(joins.iter().any(|j| j["from"] == "src/auth.ts"
        && j["to"] == "cap.auth"
        && j["relation"] == "changed-path-implements-capability"
        && j["basis"] == "explicit"));
    assert!(joins.iter().any(|j| j["from"] == "cap.auth"
        && j["to"] == "tests/auth.test.ts"
        && j["relation"] == "capability-requires-test"
        && j["basis"] == "explicit"));

    // Tests/evidence: the real test file exists at head (it was committed).
    let tests_evidence = field["tests_evidence"].as_array().unwrap();
    let auth_test = tests_evidence
        .iter()
        .find(|t| t["capability_id"] == "cap.auth" && t["test_ref"] == "tests/auth.test.ts")
        .expect("tests_evidence must carry the capability's test_ref");
    assert_eq!(auth_test["exists_at_head"], true);
}
