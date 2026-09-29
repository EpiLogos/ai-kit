//! `aikit set package` end to end, driving the real binary against a
//! temporary AIKIT_HOME.
//!
//! The fixture: a personal registry with three small Skill capsules — one
//! plain, one `METHOD:`, one `METHODOLOGY:` — and a home SkillSet that carries
//! the plain Skill directly, the Method through a contained child directory
//! and the Methodology through a referenced child set. The set's `[package]`
//! table declares an MCP dependency, a session-start hook and an environment
//! name.
//!
//! Tests that call a vendor binary (`claude`, `pi`, `codex`) skip with a
//! printed reason when the binary is not on PATH.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn skill(home: &Path, id: &str, name: &str, description: &str) {
    let base = home.join(format!("registries/personal/capsules/{id}"));
    write(
        &base.join("manifest.toml"),
        &format!(
            "schema = 1\nid = \"{id}\"\nkind = \"skill\"\nname = \"{name}\"\ndescription = \"{description}\"\n\n[skill]\nroot = \"payload\"\n"
        ),
    );
    write(
        &base.join("payload/SKILL.md"),
        &format!("---\nname: {name}\ndescription: \"{description}\"\n---\n\n# {name}\n\nBody.\n"),
    );
    write(
        &base.join("payload/references/detail.md"),
        &format!("# {name} detail\n"),
    );
}

const PACKAGE_TOML: &str = r#"description = "Praxis demo repertoire."

[package]
version = "1.0.0"
license = "MIT"
keywords = ["demo"]

[package.author]
name = "AIKit Test"

[[package.mcp]]
name = "demo-server"
command = "npx"
args = ["-y", "@demo/server"]
env = ["DEMO_TOKEN"]

[[package.hooks]]
event = "session-start"
purpose = "announce the repertoire"
command = "echo ready"

[[package.environment]]
name = "DEMO_TOKEN"
purpose = "demo server auth"
"#;

struct Fixture {
    home: TempDir,
    project: TempDir,
    out: TempDir,
}

fn fixture() -> Fixture {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    let h = home.path();
    skill(h, "skill/demo/plain", "demo-plain", "Plain demo skill.");
    skill(
        h,
        "skill/demo/method",
        "demo-method",
        "METHOD: Carry the demo act end to end.",
    );
    skill(
        h,
        "skill/demo/field",
        "demo-field",
        "METHODOLOGY: Orient in the demo field.",
    );
    // Referenced child: its own home set.
    write(&h.join("skillsets/orient/members"), "skill/demo/field\n");
    // Parent: plain member, contained child with the Method, referenced child.
    write(
        &h.join("skillsets/praxis-demo/members"),
        "skill/demo/plain\n",
    );
    write(
        &h.join("skillsets/praxis-demo/set.toml"),
        &format!("children = [\"orient\"]\n{PACKAGE_TOML}"),
    );
    write(
        &h.join("skillsets/praxis-demo/inner/members"),
        "skill/demo/method\n",
    );
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    Fixture { home, project, out }
}

fn aikit(f: &Fixture, args: &[&str]) -> (bool, Value) {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = std::process::Command::new(&bin)
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", f.home.path())
        .env("HOME", f.home.path())
        .current_dir(f.project.path())
        .output()
        .unwrap_or_else(|e| panic!("aikit {args:?}: {e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?} must emit JSON; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), envelope)
}

/// Every file under `root`, relative path → bytes.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
        if entry.file_type().is_file() {
            out.insert(
                entry.path().strip_prefix(root).unwrap().to_path_buf(),
                fs::read(entry.path()).unwrap(),
            );
        }
    }
    out
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn json_file(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
        .unwrap()
}

fn export(f: &Fixture, target: &str, extra: &[&str]) -> (bool, Value, PathBuf) {
    let dir = f.out.path().join(target);
    let dir_s = dir.display().to_string();
    let mut args = vec![
        "set",
        "package",
        "export",
        "praxis-demo",
        "--target",
        target,
        "--out",
        &dir_s,
    ];
    args.extend_from_slice(extra);
    let (ok, envelope) = aikit(f, &args);
    (ok, envelope, dir)
}

#[test]
fn inspect_resolves_members_through_both_kinds_of_child() {
    let f = fixture();
    let (ok, envelope) = aikit(&f, &["set", "package", "inspect", "praxis-demo"]);
    assert!(ok, "{envelope}");
    let data = &envelope["data"];
    assert_eq!(data["schema"], "aikit.portable-skill-package/v1");
    assert_eq!(data["identity"]["name"], "praxis-demo");
    assert_eq!(data["version"], "1.0.0");
    let members = data["members"].as_array().unwrap();
    let forms: BTreeMap<&str, &str> = members
        .iter()
        .map(|m| (m["name"].as_str().unwrap(), m["form"].as_str().unwrap()))
        .collect();
    assert_eq!(forms.get("demo-plain"), Some(&"skill"));
    assert_eq!(forms.get("demo-method"), Some(&"method"), "contained child");
    assert_eq!(
        forms.get("demo-field"),
        Some(&"methodology"),
        "referenced child"
    );
    assert!(members
        .iter()
        .all(|m| m["revision"].as_str().is_some_and(|r| !r.is_empty())));
    assert!(data["source_revision"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    let files: Vec<&str> = members[0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert!(files.contains(&"SKILL.md") && files.contains(&"references/detail.md"));
}

#[test]
fn export_writes_native_trees_and_never_touches_the_source() {
    let f = fixture();
    let sets_before = snapshot(&f.home.path().join("skillsets"));
    let registry_before = snapshot(&f.home.path().join("registries/personal"));
    let (_, inspected) = aikit(&f, &["set", "package", "inspect", "praxis-demo"]);
    let revisions: BTreeMap<String, String> = inspected["data"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["id"].as_str().unwrap().to_string(),
                m["revision"].as_str().unwrap().to_string(),
            )
        })
        .collect();

    // claude
    let (ok, envelope, dir) = export(&f, "claude", &[]);
    assert!(ok, "claude export: {envelope}");
    let receipt = &envelope["data"];
    assert_eq!(receipt["schema"], "aikit.skillset-package-receipt/v1");
    assert_eq!(receipt["source_unchanged"], true);
    assert_eq!(receipt["validation"]["structural"], "passed");
    let manifest = json_file(&dir.join(".claude-plugin/plugin.json"));
    assert_eq!(manifest["name"], "praxis-demo");
    assert_eq!(manifest["version"], "1.0.0");
    for name in ["demo-plain", "demo-method", "demo-field"] {
        assert!(dir.join(format!("skills/{name}/SKILL.md")).is_file());
        assert!(dir
            .join(format!("skills/{name}/references/detail.md"))
            .is_file());
    }
    assert!(dir.join(".mcp.json").is_file());
    assert_eq!(
        json_file(&dir.join("hooks/hooks.json"))["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        "echo ready"
    );
    assert!(!dir.join("commands").exists() && !dir.join("agents").exists());
    let provenance = json_file(&dir.join("aikit-package.json"));
    assert_eq!(provenance["skillset_ref"], "praxis-demo");
    assert_eq!(
        provenance["source_revision"],
        inspected["data"]["source_revision"]
    );
    for member in provenance["members"].as_array().unwrap() {
        assert_eq!(
            Some(member["revision"].as_str().unwrap()),
            revisions
                .get(member["id"].as_str().unwrap())
                .map(String::as_str),
            "provenance carries the exact member revision"
        );
    }
    // Skill bytes are carried verbatim.
    assert_eq!(
        fs::read(dir.join("skills/demo-method/SKILL.md")).unwrap(),
        fs::read(
            f.home
                .path()
                .join("registries/personal/capsules/skill/demo/method/payload/SKILL.md")
        )
        .unwrap()
    );

    // openai
    let (ok, envelope, dir) = export(&f, "openai", &[]);
    assert!(ok, "openai export: {envelope}");
    let manifest = json_file(&dir.join("plugin.json"));
    assert_eq!(
        manifest["$schema"],
        "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"
    );
    assert_eq!(
        manifest["extensions"]["com.openai"]["hooks"],
        "./hooks/hooks.json"
    );
    assert_eq!(
        json_file(&dir.join("mcp.json"))["mcpServers"]["demo-server"]["type"],
        "stdio"
    );
    assert!(!dir.join(".codex-plugin").exists());
    assert!(dir.join("skills/demo-field/SKILL.md").is_file());

    // codex overlay
    let (ok, envelope, dir) = export(&f, "codex", &[]);
    assert!(ok, "codex export: {envelope}");
    assert!(dir.join(".codex-plugin/plugin.json").is_file());

    // pi
    let (ok, envelope, dir) = export(&f, "pi", &[]);
    assert!(ok, "pi export: {envelope}");
    let package = json_file(&dir.join("package.json"));
    assert_eq!(package["pi"]["skills"][0], "./skills");
    assert!(package["keywords"]
        .as_array()
        .unwrap()
        .contains(&Value::from("pi-package")));
    assert!(dir.join("extensions/praxis-demo-aikit.ts").is_file());
    let unsupported = envelope["data"]["unsupported"].as_array().unwrap();
    assert!(
        unsupported
            .iter()
            .any(|u| u["relation"] == "mcp:demo-server"
                && u["reason"].as_str().unwrap().contains("no MCP host")),
        "pi MCP is recorded unsupported, not dropped: {unsupported:?}"
    );
    assert!(envelope["data"]["target_additions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["relation"] == "hook:session-start"));
    assert!(!dir.join("mcp.json").exists() && !dir.join(".mcp.json").exists());

    // The canonical SkillSet and capsules are byte-identical.
    assert_eq!(snapshot(&f.home.path().join("skillsets")), sets_before);
    assert_eq!(
        snapshot(&f.home.path().join("registries/personal")),
        registry_before
    );

    // Re-export over a previous export of the same package is allowed…
    let (ok, envelope, _) = export(&f, "pi", &[]);
    assert!(ok, "re-export: {envelope}");
    // …but a foreign non-empty directory is refused.
    let foreign = f.out.path().join("foreign");
    write(&foreign.join("keep.txt"), "mine");
    let foreign_s = foreign.display().to_string();
    let (ok, envelope) = aikit(
        &f,
        &[
            "set",
            "package",
            "export",
            "praxis-demo",
            "--target",
            "pi",
            "--out",
            &foreign_s,
        ],
    );
    assert!(!ok);
    assert_eq!(envelope["error"]["code"], "skillset.package.out_not_empty");
    assert!(foreign.join("keep.txt").is_file());
}

#[test]
fn plan_and_verify_and_diff() {
    let f = fixture();
    let (ok, envelope) = aikit(
        &f,
        &["set", "package", "plan", "praxis-demo", "--target", "pi"],
    );
    assert!(ok, "{envelope}");
    let entries = envelope["data"]["entries"].as_array().unwrap();
    assert!(entries
        .iter()
        .any(|e| e["relation"] == "mcp:demo-server" && e["class"] == "unsupported"));
    assert_eq!(
        entries.iter().filter(|e| e["class"] == "portable").count(),
        3
    );

    let (ok, _, dir) = export(&f, "claude", &[]);
    assert!(ok);
    let dir_s = dir.display().to_string();
    let (ok, envelope) = aikit(
        &f,
        &[
            "set",
            "package",
            "verify",
            "praxis-demo",
            "--target",
            "claude",
            "--out",
            &dir_s,
        ],
    );
    assert!(ok, "verify: {envelope}");
    let (ok, envelope) = aikit(
        &f,
        &[
            "set",
            "package",
            "diff",
            "praxis-demo",
            "--target",
            "claude",
            "--out",
            &dir_s,
        ],
    );
    assert!(ok);
    assert_eq!(envelope["data"]["current"], true, "{envelope}");

    // Revise one member at the source: the export is now stale, by member.
    write(
        &f.home
            .path()
            .join("registries/personal/capsules/skill/demo/plain/payload/SKILL.md"),
        "---\nname: demo-plain\ndescription: \"Plain demo skill.\"\n---\n\n# demo-plain\n\nRevised.\n",
    );
    let (ok, envelope) = aikit(
        &f,
        &[
            "set",
            "package",
            "diff",
            "praxis-demo",
            "--target",
            "claude",
            "--out",
            &dir_s,
        ],
    );
    assert!(ok);
    let data = &envelope["data"];
    assert_eq!(data["current"], false);
    assert_eq!(data["changed"][0]["id"], "skill/demo/plain");
    assert!(data["files"]["changed"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p == "skills/demo-plain/SKILL.md"));
    let (ok, envelope) = aikit(
        &f,
        &[
            "set",
            "package",
            "verify",
            "praxis-demo",
            "--target",
            "claude",
            "--out",
            &dir_s,
        ],
    );
    assert!(!ok, "a stale tree does not verify");
    assert_eq!(
        envelope["error"]["code"],
        "skillset.package.validation_failed"
    );
}

#[test]
fn unresolved_members_refuse_export_unless_partial() {
    let f = fixture();
    write(
        &f.home.path().join("skillsets/broken/members"),
        "skill/demo/plain\nskill/demo/absent\n",
    );
    write(
        &f.home.path().join("skillsets/broken/set.toml"),
        "[package.author]\nname = \"AIKit Test\"\n",
    );
    let out = f.out.path().join("broken");
    let out_s = out.display().to_string();
    let (ok, envelope) = aikit(
        &f,
        &[
            "set", "package", "export", "broken", "--target", "claude", "--out", &out_s,
        ],
    );
    assert!(!ok);
    assert_eq!(envelope["error"]["code"], "skillset.package.unresolved");
    assert!(!out.exists(), "nothing is written when export refuses");

    let (ok, envelope) = aikit(
        &f,
        &[
            "set",
            "package",
            "export",
            "broken",
            "--target",
            "claude",
            "--out",
            &out_s,
            "--allow-partial",
        ],
    );
    assert!(ok, "{envelope}");
    assert!(envelope["data"]["unsupported"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["relation"] == "member:skill/demo/absent"));
}

#[test]
fn secret_values_in_package_metadata_are_refused() {
    let f = fixture();
    write(
        &f.home.path().join("skillsets/leaky/members"),
        "skill/demo/plain\n",
    );
    write(
        &f.home.path().join("skillsets/leaky/set.toml"),
        "[[package.mcp]]\nname = \"srv\"\ncommand = \"srv\"\nargs = [\"--key\", \"sk-live-abcdefghijklmnopqrstuvwxyz\"]\n",
    );
    let (ok, envelope) = aikit(&f, &["set", "package", "inspect", "leaky"]);
    assert!(!ok);
    assert_eq!(envelope["error"]["code"], "skillset.package.secret_value");
}

// ---------------------------------------------------------------------------
// Native validation (skips cleanly when the vendor binary is absent)
// ---------------------------------------------------------------------------

#[test]
fn native_claude_validate() {
    if !on_path("claude") {
        eprintln!("skip: `claude` not on PATH");
        return;
    }
    let f = fixture();
    let (ok, envelope, _) = export(&f, "claude", &["--native"]);
    assert!(ok, "{envelope}");
    let native = &envelope["data"]["validation"]["native"];
    assert_eq!(native["status"], "passed", "{native}");
    assert_eq!(native["exit"], 0);
}

#[test]
fn native_pi_discovers_every_skill() {
    if !on_path("pi") {
        eprintln!("skip: `pi` not on PATH");
        return;
    }
    let f = fixture();
    let (ok, envelope, _) = export(&f, "pi", &["--native"]);
    assert!(ok, "{envelope}");
    let discovery = &envelope["data"]["discovery"];
    assert_eq!(discovery["status"], "passed", "{envelope}");
    let found: Vec<&str> = discovery["discovered_skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for name in ["demo-plain", "demo-method", "demo-field"] {
        assert!(found.contains(&name), "{found:?}");
    }
}

#[test]
fn native_codex_disposable_load() {
    if !on_path("codex") {
        eprintln!("skip: `codex` not on PATH");
        return;
    }
    let f = fixture();
    let (ok, envelope, _) = export(&f, "codex", &["--native"]);
    assert!(ok, "{envelope}");
    let native = &envelope["data"]["validation"]["native"];
    assert_eq!(native["status"], "passed", "{native}");
    assert!(native["summary"]
        .as_str()
        .unwrap()
        .contains("untouched=true"));
}
