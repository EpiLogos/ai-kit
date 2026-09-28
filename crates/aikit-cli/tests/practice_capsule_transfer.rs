//! A practice crosses between Worlds as a proven capsule: `praxis read` proves
//! the files against AIKit's revision, `praxis skill export` writes the
//! portable archive, and `system source add-capsule` registers it in another
//! home — original id and revision retained, provenance in the registration,
//! activation only through sync → promote.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;

fn run(home: &Path, cwd: &Path, args: &[&str]) -> (bool, Value) {
    let output = Command::cargo_bin("aikit")
        .unwrap()
        .env("AIKIT_HOME", home)
        .env("HOME", home.join("user-home"))
        .current_dir(cwd)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?} printed no JSON\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), value)
}

fn aikit(home: &Path, cwd: &Path, args: &[&str]) -> Value {
    let (ok, value) = run(home, cwd, args);
    assert!(ok, "aikit {args:?} failed: {value}");
    value["data"].clone()
}

fn refused(home: &Path, cwd: &Path, args: &[&str]) -> String {
    let (ok, value) = run(home, cwd, args);
    assert!(!ok, "aikit {args:?} was expected to refuse: {value}");
    value["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

const SCRIPT: &str = "#!/usr/bin/env python3\nprint('darshana')\n";

/// A multi-file practice: SKILL.md plus an executable payload script.
fn practice(root: &Path, marker: &str) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: darshana\ndescription: METHOD: See before acting.\n---\n\n{marker}\n"),
    )
    .unwrap();
    fs::write(root.join("scripts/darshana.py"), SCRIPT).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.join("scripts/darshana.py"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
}

fn file<'a>(reading: &'a Value, path: &str) -> &'a Value {
    reading["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == path)
        .unwrap_or_else(|| panic!("{path} missing from {reading}"))
}

struct World {
    _temp: tempfile::TempDir,
    home: std::path::PathBuf,
    cwd: std::path::PathBuf,
    root: std::path::PathBuf,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    let root = temp.path().to_path_buf();
    fs::create_dir_all(&cwd).unwrap();
    World {
        _temp: temp,
        home,
        cwd,
        root,
    }
}

/// An offering World with `skill/ql/darshana` promoted from a directory.
fn offering() -> World {
    let w = world();
    let source = w.root.join("ql-source");
    practice(&source.join("darshana"), "first");
    aikit(
        &w.home,
        &w.cwd,
        &["source", "add-directory", "ql", source.to_str().unwrap()],
    );
    aikit(&w.home, &w.cwd, &["source", "sync", "ql"]);
    aikit(&w.home, &w.cwd, &["source", "promote", "ql"]);
    w
}

#[test]
fn read_proves_the_active_revision_and_refuses_one_it_does_not_retain() {
    let w = offering();
    let reading = aikit(&w.home, &w.cwd, &["praxis", "read", "skill/ql/darshana"]);
    let explained = aikit(&w.home, &w.cwd, &["explain", "skill/ql/darshana"]);
    assert_eq!(reading["schema"], "aikit.practice-reading/v1");
    assert_eq!(reading["revision"], explained["revision"]);
    assert_eq!(reading["form"], "method");
    assert_eq!(reading["source_id"], "ql");
    assert!(reading["snapshot"].is_string());
    let script = file(&reading, "payload/scripts/darshana.py");
    assert_eq!(script["text"], SCRIPT);
    #[cfg(unix)]
    assert_eq!(script["mode"], 0o755);
    assert!(file(&reading, "manifest.toml")["text"].is_string());

    // A new revision promoted over it: the old one is still retained and
    // readable by name; an unknown revision is refused.
    let first = reading["revision"].as_str().unwrap().to_string();
    practice(&w.root.join("ql-source/darshana"), "second");
    aikit(&w.home, &w.cwd, &["source", "sync", "ql"]);
    aikit(&w.home, &w.cwd, &["source", "promote", "ql"]);
    let now = aikit(&w.home, &w.cwd, &["praxis", "read", "skill/ql/darshana"]);
    assert_ne!(now["revision"], first.as_str());
    let old = aikit(
        &w.home,
        &w.cwd,
        &["praxis", "read", "skill/ql/darshana", "--revision", &first],
    );
    assert_eq!(old["revision"], first.as_str());
    assert!(file(&old, "payload/SKILL.md")["text"]
        .as_str()
        .unwrap()
        .contains("first"));
    assert_eq!(
        refused(
            &w.home,
            &w.cwd,
            &["praxis", "read", "skill/ql/darshana", "--revision", "0000"],
        ),
        "praxis.revision_not_retained"
    );
}

#[test]
fn an_exported_capsule_is_adopted_at_its_original_identity_and_revision() {
    let offer = offering();
    let archive = offer.root.join("darshana.capsule.json");
    let exported = aikit(
        &offer.home,
        &offer.cwd,
        &[
            "praxis",
            "skill",
            "export",
            "skill/ql/darshana",
            "--out",
            archive.to_str().unwrap(),
        ],
    );
    let revision = exported["revision"].as_str().unwrap().to_string();
    assert_eq!(exported["schema"], "aikit.practice-capsule/v1");
    assert_eq!(exported["exported_from"]["source_id"], "ql");

    let visitor = world();
    let added = aikit(
        &visitor.home,
        &visitor.cwd,
        &[
            "system",
            "source",
            "add-capsule",
            archive.to_str().unwrap(),
            "--world-ref",
            "world:offerer",
            "--upstream-ref",
            "entry:offerer/darshana",
        ],
    );
    let id = added["id"].as_str().unwrap().to_string();
    assert_eq!(added["kind"], "capsule");
    assert_eq!(added["already_registered"], false);
    assert_eq!(added["capsule"]["revision"], revision.as_str());

    // Registered, not active: publication alone never reaches the catalogue.
    let shown = aikit(&visitor.home, &visitor.cwd, &["source", "show", &id]);
    assert!(shown["active_snapshot"].is_null());
    assert_eq!(shown["upstream"]["world_ref"], "world:offerer");
    assert_eq!(shown["upstream"]["entry_ref"], "entry:offerer/darshana");
    assert_eq!(shown["upstream"]["practice_id"], "skill/ql/darshana");
    assert_eq!(shown["upstream"]["revision"], revision.as_str());
    assert_eq!(shown["upstream"]["exported_from"]["source_id"], "ql");
    assert_eq!(
        refused(
            &visitor.home,
            &visitor.cwd,
            &["praxis", "read", "skill/ql/darshana"]
        ),
        "praxis.unknown"
    );

    // Idempotent re-add.
    let again = aikit(
        &visitor.home,
        &visitor.cwd,
        &[
            "source",
            "add-capsule",
            archive.to_str().unwrap(),
            "--world-ref",
            "world:offerer",
            "--upstream-ref",
            "entry:offerer/darshana",
        ],
    );
    assert_eq!(again["id"], id.as_str());
    assert_eq!(again["already_registered"], true);

    aikit(&visitor.home, &visitor.cwd, &["source", "sync", &id]);
    aikit(&visitor.home, &visitor.cwd, &["source", "promote", &id]);
    let adopted = aikit(
        &visitor.home,
        &visitor.cwd,
        &["praxis", "read", "skill/ql/darshana"],
    );
    let original = aikit(
        &offer.home,
        &offer.cwd,
        &["praxis", "read", "skill/ql/darshana"],
    );
    assert_eq!(adopted["revision"], revision.as_str());
    assert_eq!(adopted["source_id"], id.as_str());
    assert_eq!(adopted["files"], original["files"]);
    let explained = aikit(
        &visitor.home,
        &visitor.cwd,
        &["explain", "skill/ql/darshana"],
    );
    assert_eq!(explained["revision"], revision.as_str());

    // The payload script arrived byte-identical with its mode.
    let script = fs::read_dir(visitor.home.join("sources").join(&id).join("snapshots"))
        .unwrap()
        .flatten()
        .map(|entry| {
            entry
                .path()
                .join("registry/capsules/skill/ql/darshana/payload/scripts/darshana.py")
        })
        .find(|path| path.is_file())
        .unwrap();
    assert_eq!(fs::read_to_string(&script).unwrap(), SCRIPT);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&script).unwrap().permissions().mode() & 0o7777,
            0o755
        );
    }

    // A different revision registers side by side, but cannot be promoted
    // while the first speaks for the same practice id.
    practice(&offer.root.join("ql-source/darshana"), "second");
    aikit(&offer.home, &offer.cwd, &["source", "sync", "ql"]);
    aikit(&offer.home, &offer.cwd, &["source", "promote", "ql"]);
    let second = offer.root.join("darshana-2.capsule.json");
    aikit(
        &offer.home,
        &offer.cwd,
        &[
            "praxis",
            "skill",
            "export",
            "skill/ql/darshana",
            "--out",
            second.to_str().unwrap(),
        ],
    );
    let beside = aikit(
        &visitor.home,
        &visitor.cwd,
        &["source", "add-capsule", second.to_str().unwrap()],
    );
    assert_ne!(beside["id"], id.as_str());
    assert_ne!(beside["capsule"]["revision"], revision.as_str());
    let beside_id = beside["id"].as_str().unwrap();
    aikit(&visitor.home, &visitor.cwd, &["source", "sync", beside_id]);
    assert_eq!(
        refused(
            &visitor.home,
            &visitor.cwd,
            &["source", "promote", beside_id]
        ),
        "source.capsule_identity_active"
    );
}

#[test]
fn a_tampered_archive_or_contradicting_provenance_is_refused_and_registers_nothing() {
    let offer = offering();
    let archive = offer.root.join("darshana.capsule.json");
    aikit(
        &offer.home,
        &offer.cwd,
        &[
            "praxis",
            "skill",
            "export",
            "skill/ql/darshana",
            "--out",
            archive.to_str().unwrap(),
        ],
    );
    let mut value: Value = serde_json::from_slice(&fs::read(&archive).unwrap()).unwrap();
    let revision = value["revision"].as_str().unwrap().to_string();

    // The script's bytes changed and its digest recomputed to match: only
    // the revision can catch it.
    for row in value["files"].as_array_mut().unwrap() {
        if row["path"] == "payload/scripts/darshana.py" {
            let evil = "#!/usr/bin/env python3\nimport os\n";
            row["text"] = Value::from(evil);
            row["bytes"] = Value::from(evil.len());
            row["sha256"] = Value::from(format!(
                "sha256:{:x}",
                <sha2::Sha256 as sha2::Digest>::digest(evil.as_bytes())
            ));
        }
    }
    let tampered = offer.root.join("tampered.capsule.json");
    fs::write(&tampered, serde_json::to_vec(&value).unwrap()).unwrap();

    let visitor = world();
    assert_eq!(
        refused(
            &visitor.home,
            &visitor.cwd,
            &["source", "add-capsule", tampered.to_str().unwrap()],
        ),
        "capsule.revision_mismatch"
    );
    let provenance = format!(r#"{{"practice_id":"skill/ql/darshana","revision":"{revision}x"}}"#);
    assert_eq!(
        refused(
            &visitor.home,
            &visitor.cwd,
            &[
                "source",
                "add-capsule",
                archive.to_str().unwrap(),
                "--provenance",
                &provenance,
            ],
        ),
        "source.provenance_mismatch"
    );
    assert!(
        !visitor.home.join("sources").exists()
            || fs::read_dir(visitor.home.join("sources")).unwrap().count() == 0
    );
}
