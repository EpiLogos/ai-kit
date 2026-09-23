//! O:I #65 native-owner repair: the managed actor bootstrap named `Project: O-I`
//! and `ContextSources: 0 total` for a session standing at the Central root — a
//! Project it was not standing in, with no governance named at all. This drives
//! the real resolution (`aikit compose`, the same seam `apply` and the managed
//! `aikit-context` skill ride) against a temporary Central-shaped fixture root —
//! never a hand-built `ActorBootstrap` struct — and proves both halves of the
//! repair: root `Control/agents/governance/**` and a Project's own
//! `ProjectCentral/agents/governance/**` both surface as named ContextSources,
//! `authorship-and-return/responsibility.md` specifically among them, and the
//! Project identity resolved for each context is the context actually stood in
//! (`control:root` at the Central root, the Project itself inside `Work/`) —
//! never a leftover from somewhere else. Every AIKit write in this test targets
//! a disposable `AIKIT_HOME`; `aikit apply` is deliberately not exercised here
//! (it also writes native client config outside `AIKIT_HOME`, e.g.
//! `~/.claude.json`, which this test must not touch) — `aikit compose` reads
//! the same resolution and previews the same plan without writing anything.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

/// A Central-shaped fixture: root governance under `Control/agents/governance`,
/// and one Work member with its own `ProjectCentral/agents/governance`. The
/// governance reading itself is filesystem scanning against `CENTRAL_ROOT`,
/// but `compose` also lists agent profiles through the native `ctrl` owner, so
/// these end-to-end tests need the real `ctrl` on PATH (see `native_ctrl`).
struct Fixture {
    _temp: tempfile::TempDir,
    central: PathBuf,
    project: PathBuf,
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let central = temp.path().join("Central");
    fs::create_dir_all(central.join("Work")).unwrap();
    // `process_central_root` compares this path against the resolved
    // context's own (canonicalised) project root with `starts_with`; a
    // tempdir under a symlinked prefix (`/var` -> `/private/var` on macOS)
    // must be canonicalised the same way here or the two never match.
    let central = fs::canonicalize(&central).unwrap();

    // Root governance: mirrors the real shape closely enough to prove the
    // exact statement the task names, plus a sibling directory, so a listing
    // proves it walks the tree rather than reading one hardcoded file.
    write(
        &central.join("Control/agents/governance/authorship-and-return/responsibility.md"),
        "# Responsibility\n\nOwn the gaps you find.\n",
    );
    write(
        &central.join("Control/agents/governance/attention/consult-authored-ground.md"),
        "# Consult authored ground\n",
    );
    // A withheld root subtree must never surface as a ContextSource.
    write(
        &central.join("Control/agents/governance/withheld/.no-agent-retrieval"),
        "",
    );
    write(
        &central.join("Control/agents/governance/withheld/secret.md"),
        "not for agents",
    );

    let project = central.join("Work/demo-project");
    write(
        &project.join("ProjectCentral/project.json"),
        &json!({
            "schema": "central.project/v1",
            "project_id": "demo-project",
            "human_source": "ProjectCentral/user",
            "wiki": {
                "profile": "okf-wiki/v1",
                "source": "ProjectCentral/agents/wiki/wiki.json"
            }
        })
        .to_string(),
    );
    write(
        &project.join("ProjectCentral/agents/governance/repo-content.md"),
        "# Repo content\n",
    );

    Fixture {
        _temp: temp,
        central,
        project,
    }
}

/// `compose` asks the native Central owner (`ctrl … agent-profile.list`), so
/// this end-to-end test runs against the real `ctrl` or not at all — never a
/// stand-in. Without it the test skips with a reason, unless
/// `AIKIT_REQUIRE_NATIVE_CTRL` is set, in which case absence fails loudly.
fn native_ctrl() -> bool {
    let present = Command::new("ctrl")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !present {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_NATIVE_CTRL").is_none(),
            "AIKIT_REQUIRE_NATIVE_CTRL is set but no native `ctrl` is on PATH"
        );
        eprintln!("skip: native `ctrl` is not on PATH; compose cannot list agent profiles");
    }
    present
}

fn compose(central: &Path, home: &Path, cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aikit"))
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .env("CENTRAL_ROOT", central)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .current_dir(cwd)
        .args(["--json", "compose"])
        .output()
        .unwrap()
}

fn succeeded(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");
    value
}

#[test]
fn a_project_context_names_both_root_and_project_governance_including_responsibility() {
    if !native_ctrl() {
        return;
    }
    let fixture = fixture();
    let home = tempfile::tempdir().unwrap();
    let value = succeeded(&compose(&fixture.central, home.path(), &fixture.project));
    let plan = &value["data"]["plan"];

    // The Project actually stood in, not a leftover from anywhere else.
    assert_eq!(plan["project"]["project"], "demo-project");

    let governance = plan["governance_sources"]
        .as_array()
        .expect("governance_sources must be a disclosed array, not silently absent");
    let refs: Vec<&str> = governance.iter().map(|v| v.as_str().unwrap()).collect();

    assert!(
        refs.contains(
            &"central:source:control:root:Control/agents/governance/authorship-and-return/responsibility.md"
        ),
        "responsibility.md must be named explicitly: {refs:?}"
    );
    assert!(
        refs.contains(
            &"central:source:control:root:Control/agents/governance/attention/consult-authored-ground.md"
        ),
        "a second root governance file proves the tree is walked, not one file read: {refs:?}"
    );
    assert!(
        refs.iter()
            .any(|r| r.contains("demo-project") && r.contains("governance")),
        "the Project's own ProjectCentral/agents/governance must be named too: {refs:?}"
    );
    assert!(
        !refs
            .iter()
            .any(|r| r.contains("secret") || r.contains("withheld")),
        ".no-agent-retrieval must withhold its subtree from governance disclosure: {refs:?}"
    );

    // context_sources.total must actually include these, not just a side
    // channel nothing else sees — this is the concrete "0 total" defect.
    let total = plan["context_sources"]["total"].as_u64().unwrap();
    assert!(
        total >= governance.len() as u64,
        "context_sources.total ({total}) must cover the named governance sources ({})",
        governance.len()
    );
}

#[test]
fn a_root_context_names_root_governance_and_claims_the_root_project_not_a_leftover() {
    if !native_ctrl() {
        return;
    }
    let fixture = fixture();
    let home = tempfile::tempdir().unwrap();
    let value = succeeded(&compose(&fixture.central, home.path(), &fixture.central));
    let plan = &value["data"]["plan"];

    // Standing at the Central root must never claim an unrelated Project
    // (e.g. a stale "O-I" from some earlier composition elsewhere) — it is
    // either the root World identity or explicitly no Project.
    assert_eq!(
        plan["project"]["project"], "control:root",
        "a Central-root context must claim the root World identity, not a leftover Project"
    );

    let governance = plan["governance_sources"]
        .as_array()
        .expect("governance_sources must be a disclosed array");
    let refs: Vec<&str> = governance.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        refs.contains(
            &"central:source:control:root:Control/agents/governance/authorship-and-return/responsibility.md"
        ),
        "responsibility.md must be named at the Central root too: {refs:?}"
    );
    // No Project governance at the root itself — there is no Project here.
    assert!(
        !refs
            .iter()
            .any(|r| r.starts_with("source:central:demo-project:governance:")),
        "the root context must not name a Project's own governance: {refs:?}"
    );
}
