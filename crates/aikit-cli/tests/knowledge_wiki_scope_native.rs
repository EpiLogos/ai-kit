//! Explicit native integration: real Central Project identities through the
//! production AIKit CLI. No manufactured owner replies, Wiki objects or model
//! activity. Run all three cases with --ignored after building the pinned owner.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use aikit_adapters::runner::{Output, SystemRunner};
use aikit_core::resource::{ResourceKind, SourceAuthority};
use aikit_core::{KnowledgeAddress, KnowledgeProviderStatus, KnowledgeSearchResult};
use aikit_store::knowledge_wiki::SQLITE_WIKI_PROVIDER;
use serde_json::{json, Value};
use tempfile::TempDir;

const EDITOR: &str = "central:wiki:project:editor-walk";
const OTHER: &str = "central:wiki:project:editor";
const CAPTURE_BYTES: u64 = 1024 * 1024;

struct Ground {
    owned: Option<TempDir>,
    ctrl: PathBuf,
    world: PathBuf,
    home: PathBuf,
    native_roots: Vec<PathBuf>,
    native_projects: Vec<PathBuf>,
    deadline: Instant,
    commands: usize,
}

impl Ground {
    fn new() -> Self {
        let ctrl = fs::canonicalize(
            std::env::var_os("AIKIT_CENTRAL_REAL_BIN")
                .expect("selected native gate requires its built/pinned actual Central owner"),
        )
        .expect("the required native owner must exist; never an absent-owner green skip");
        assert!(ctrl.is_file());
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        fs::create_dir_all(&scratch).unwrap();
        let owned = tempfile::Builder::new()
            .prefix("knowledge-wiki-scope-native-")
            .tempdir()
            .unwrap();
        let private = fs::canonicalize(owned.path()).unwrap();
        let world = private.join("world");
        let home = private.join("aikit-home");
        for directory in [&world, &home, &private.join("home"), &private.join("tmp")] {
            fs::create_dir_all(directory).unwrap();
        }
        let mut ground = Self {
            owned: Some(owned),
            ctrl,
            world,
            home,
            native_roots: Vec::new(),
            native_projects: Vec::new(),
            deadline: Instant::now() + Duration::from_secs(180),
            commands: 0,
        };
        let world = ground.world.clone();
        ground.owner(&world, "central.init", json!({}));
        ground.project(&world, "Editor", "editor-walk");
        ground.project(&world, "Other", "editor");
        ground
    }

    fn command(&self, executable: &Path, cwd: &Path) -> Command {
        let private = fs::canonicalize(self.owned.as_ref().unwrap().path()).unwrap();
        let mut command = Command::new(executable);
        // Private child-only environment; no native token, personal AIKit
        // context, profile, Redis binding or provider credential is inherited.
        command
            .env_clear()
            .env(
                "PATH",
                std::env::var_os("PATH").expect("native host PATH required"),
            )
            .env("HOME", private.join("home"))
            .env("TMPDIR", private.join("tmp"))
            .env("TMP", private.join("tmp"))
            .env("TEMP", private.join("tmp"))
            .env("AIKIT_HOME", &self.home)
            .env("CENTRAL_ROOT", &self.world)
            .env("CENTRAL_CTRL_BIN", &self.ctrl)
            .current_dir(cwd);
        command
    }

    fn capture(&mut self, command: &mut Command) -> Output {
        assert!(
            self.commands < 40,
            "native scenario command budget exceeded"
        );
        self.commands += 1;
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .expect("native scenario absolute deadline exceeded before another launch");
        let start = Instant::now();
        let result = SystemRunner::new()
            .with_timeout(remaining.min(Duration::from_secs(30)))
            .with_output_limit_bytes(CAPTURE_BYTES)
            .with_strict_utf8()
            .capture_command(command);
        match result {
            Ok(output) => {
                eprintln!("native command {}: status={} elapsed={:?} stdout_bytes={} stdout_blake3={} stderr_bytes={}",
                    self.commands, output.status, start.elapsed(), output.stdout.len(),
                    blake3::hash(output.stdout.as_bytes()), output.stderr.len());
                output
            }
            Err(error) => {
                // The production runner owns finite pipes and group retirement.
                // Preserve private state if it could not produce a complete
                // lifecycle receipt; deleting it cannot resolve that failure.
                let retained = self.owned.take().unwrap().keep();
                panic!(
                    "native capture failed; retained {}: {error}",
                    retained.display()
                );
            }
        }
    }

    fn owner(&mut self, root: &Path, operation: &str, input: Value) -> Value {
        let mut command = self.command(&self.ctrl, root);
        command
            .args(["--json", "--root"])
            .arg(root)
            .args(["action", "run", operation])
            .arg(input.to_string());
        let output = self.capture(&mut command);
        let envelope: Value = serde_json::from_str(&output.stdout).unwrap();
        assert!(
            output.ok() && envelope["ok"] == true,
            "{envelope}; stderr={}",
            output.stderr
        );
        if operation == "central.init" {
            self.native_roots.push(root.to_path_buf());
        }
        envelope["data"].clone()
    }

    fn project(&mut self, root: &Path, name: &str, id: &str) -> PathBuf {
        let path = root.join("Work").join(name);
        fs::create_dir_all(&path).unwrap();
        let result = self.owner(
            root,
            "projectcentral.init",
            json!({"project":name,"project_id":id}),
        );
        assert_eq!(result["project_id"], id);
        assert_eq!(
            result["wiki_space_ref"],
            format!("central:wiki:project:{id}")
        );
        assert_eq!(Path::new(result["project_root"].as_str().unwrap()), path);
        assert_eq!(
            result["wiki_source"],
            "ProjectCentral/agents/wiki/wiki.json"
        );
        let manifest: Value =
            serde_json::from_slice(&fs::read(path.join("ProjectCentral/project.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["project_id"], id);
        self.native_projects.push(path.clone());
        path
    }

    fn cli(&mut self, cwd: &Path, arguments: &[&str]) -> (Output, Value) {
        let mut command = self.command(Path::new(env!("CARGO_BIN_EXE_aikit")), &self.world);
        command.args(["--json", "-C"]).arg(cwd).args(arguments);
        let output = self.capture(&mut command);
        let envelope: Value = serde_json::from_str(&output.stdout).unwrap();
        (output, envelope)
    }

    fn success(&mut self, cwd: &Path, arguments: &[&str]) -> Value {
        let (output, envelope) = self.cli(cwd, arguments);
        assert!(
            output.ok() && envelope["ok"] == true,
            "{envelope}; stderr={}",
            output.stderr
        );
        envelope["data"].clone()
    }

    fn search(&mut self, cwd: &Path, operation: &str, query: &str) -> KnowledgeSearchResult {
        serde_json::from_value(self.success(cwd, &["knowledge", operation, query, "--limit", "50"]))
            .unwrap()
    }

    fn assert_no_use(&mut self, cwd: &Path) {
        assert_eq!(
            self.success(cwd, &["knowledge", "history"]),
            json!([]),
            "Search/Resolve must not manufacture a successful use or run effects"
        );
    }

    fn assert_wiki_registers(&mut self) {
        let world = self.world.clone();
        let status: KnowledgeProviderStatus =
            serde_json::from_value(self.success(&world, &["knowledge", "status"])).unwrap();
        let wiki = status
            .wiki
            .expect("actual canonical Wiki provider required");
        assert!(wiki.available);
        for (name, reference) in [("Editor", EDITOR), ("Other", OTHER)] {
            let bytes = fs::read(
                self.world
                    .join("Work")
                    .join(name)
                    .join("ProjectCentral/agents/wiki/wiki.json"),
            )
            .unwrap();
            let revision = format!("blake3:{}", blake3::hash(&bytes));
            assert!(
                wiki.registers
                    .iter()
                    .any(|register| register.register.as_str() == reference
                        && register.revision == revision),
                "native canonical register/basis absent: {wiki:?}"
            );
        }
    }

    fn source_bytes(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut bytes = BTreeMap::new();
        for root in &self.native_roots {
            let path = root.join("Control/agents/wiki/wiki.json");
            assert!(fs::metadata(&path).unwrap().len() <= 1024 * 1024);
            bytes.insert(path.clone(), fs::read(path).unwrap());
        }
        for project in &self.native_projects {
            for member in ["project.json", "agents/wiki/wiki.json", "provenance.json"] {
                let path = project.join("ProjectCentral").join(member);
                let metadata = fs::metadata(&path).unwrap();
                assert!(metadata.len() <= 1024 * 1024);
                bytes.insert(path.clone(), fs::read(path).unwrap());
            }
        }
        bytes
    }
}

fn admitted(result: &KnowledgeSearchResult, reference: &str) -> bool {
    result.hits.iter().any(|hit| {
        hit.resource.as_str() == reference
            && matches!(&hit.address, KnowledgeAddress::Wiki(address) if address.as_str() == reference)
            && hit.provider.as_str() == SQLITE_WIKI_PROVIDER
            && hit.kind == ResourceKind::KnowledgeSpace
            && hit.authority == SourceAuthority::Authored
    })
}

#[test]
#[ignore = "explicit native gate: built/pinned AIKIT_CENTRAL_REAL_BIN; execute --ignored"]
fn actual_native_wiki_id_differs_from_folder_and_sibling_label_cannot_grant_scope() {
    let mut ground = Ground::new();
    ground.assert_wiki_registers();
    let before = ground.source_bytes();
    let world = ground.world.clone();
    let editor = world.join("Work/Editor");
    let other = world.join("Work/Other");
    for operation in ["search", "resolve"] {
        let root = ground.search(&world, operation, "editor");
        assert!(
            admitted(&root, EDITOR) && admitted(&root, OTHER),
            "both genuine native candidates must exist before testing scope: {root:?}"
        );
        for query in ["editor-walk", "editor"] {
            let selected = ground.search(&editor, operation, query);
            assert!(
                admitted(&selected, EDITOR),
                "own canonical Wiki is missing: {selected:?}"
            );
            assert!(
                !selected
                    .hits
                    .iter()
                    .any(|hit| hit.resource.as_str() == OTHER),
                "a sibling canonical ID matching Editor's folder was disclosed: {selected:?}"
            );
        }
        let sibling = ground.search(&other, operation, "editor");
        assert!(
            admitted(&sibling, OTHER),
            "Other remains independently addressable: {sibling:?}"
        );
        assert!(
            !sibling
                .hits
                .iter()
                .any(|hit| hit.resource.as_str() == EDITOR),
            "{sibling:?}"
        );
    }
    ground.assert_no_use(&editor);
    ground.assert_no_use(&other);
    assert_eq!(
        ground.source_bytes(),
        before,
        "read-only native queries changed owner source"
    );
}

#[test]
#[ignore = "explicit native gate: built/pinned AIKIT_CENTRAL_REAL_BIN; execute --ignored"]
fn actual_owner_created_external_identity_missing_or_ambiguous_refuses_broad_query() {
    let mut ground = Ground::new();
    let private = fs::canonicalize(ground.owned.as_ref().unwrap().path()).unwrap();
    let external = private.join("independent-native-world");
    fs::create_dir(&external).unwrap();
    ground.owner(&external, "central.init", json!({}));
    // These declarations are created by the actual owner too. Selecting this
    // external native checkout against the configured World exercises the
    // existing production Project identity resolver, without forged JSON.
    let selected = ground.project(&external, "Selected", "editor-walk");
    let missing = ground.project(&external, "Missing", "no-native-match");
    for operation in ["search", "resolve"] {
        let original = ground.search(&selected, operation, "editor-walk");
        assert!(
            admitted(&original, EDITOR),
            "native selected identity failed: {original:?}"
        );
        let (output, refused) = ground.cli(
            &missing,
            &["knowledge", operation, "editor-walk", "--limit", "50"],
        );
        assert!(!output.ok() && refused["ok"] == false, "{refused}");
        assert_eq!(
            refused["error"]["code"],
            "knowledge.project_scope_unresolved"
        );
        assert!(refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("matches 0 discovered Work Projects"));
        assert!(
            refused.get("data").is_none_or(Value::is_null),
            "refusal returned a broad reading: {refused}"
        );
    }
    let world = ground.world.clone();
    ground.project(&world, "Duplicate", "editor-walk");
    let before = ground.source_bytes();
    for operation in ["search", "resolve"] {
        let (output, refused) = ground.cli(
            &selected,
            &["knowledge", operation, "editor-walk", "--limit", "50"],
        );
        assert!(!output.ok() && refused["ok"] == false, "{refused}");
        assert_eq!(
            refused["error"]["code"],
            "knowledge.project_scope_unresolved"
        );
        assert!(refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("matches 2 discovered Work Projects"));
        assert!(
            refused.get("data").is_none_or(Value::is_null),
            "refusal returned a broad reading: {refused}"
        );
    }
    ground.assert_no_use(&selected);
    assert_eq!(ground.source_bytes(), before);
}

#[test]
#[ignore = "explicit native gate: built/pinned AIKIT_CENTRAL_REAL_BIN; execute --ignored"]
fn actual_native_duplicate_id_is_withheld_from_every_internal_project_scope() {
    let mut ground = Ground::new();
    let world = ground.world.clone();
    let editor = world.join("Work/Editor");
    assert!(admitted(
        &ground.search(&editor, "search", "editor-walk"),
        EDITOR
    ));
    ground.project(&world, "Duplicate", "editor-walk");
    ground.project(&world, "Third", "editor-walk");
    let before = ground.source_bytes();
    for name in ["Editor", "Duplicate", "Third"] {
        let selected = world.join("Work").join(name);
        for operation in ["search", "resolve"] {
            let reading = ground.search(&selected, operation, "editor-walk");
            assert!(
                !reading
                    .hits
                    .iter()
                    .any(|hit| hit.resource.as_str() == EDITOR),
                "an ambiguous native ID was assigned to a first/last folder: {reading:?}"
            );
        }
    }
    ground.assert_no_use(&editor);
    assert_eq!(ground.source_bytes(), before);
}
