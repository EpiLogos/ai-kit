//! Copied native guard Source fixtures qualify real owned Git/filesystem effects.
//! Public fixture retention is not protected Control installation or acceptance.
//! A qualification host can run this module in the existing aikit-adapters
//! integration-test harness, using its SAME SystemRunner and supplied exact
//! hook candidates. No mock Git, Node, receipt or process supervisor is used.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use aikit_adapters::runner::{CommandRunner, Output, SystemRunner};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    error::Error,
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const PRECOMMIT_SOURCE: &str = "4e1a2db887e8e742225c9bb2185724bc27c454d313a0be510cda1aaba68c527c";
const PREPUSH_SOURCE: &str = "ca3a8c3d747fcf85e5e34f6ac61c41f4224cff1c88ef6d90f5f529dc9b243dee";

fn digest_file(path: &Path) -> std::io::Result<String> {
    let mut source = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut bytes = [0_u8; 64 * 1024];
    loop {
        let count = source.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        digest.update(&bytes[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn native_error(error: &aikit_core::AikitError) -> Value {
    let io = error
        .source()
        .and_then(|source| source.downcast_ref::<std::io::Error>());
    json!({"code":error.code(),"message":error.message(),"details":error.details(),
        "original_io":io.map(|cause| json!({"kind":format!("{:?}",cause.kind()),
            "raw_os_error":cause.raw_os_error(),"message":cause.to_string()}))})
}

fn required_directory(variable: &str) -> PathBuf {
    let path =
        PathBuf::from(std::env::var_os(variable).unwrap_or_else(|| {
            panic!("requires provisioned actual tool/cache directory: {variable}")
        }));
    assert!(
        path.is_absolute() && path.is_dir(),
        "actual {variable} must be an existing absolute directory"
    );
    path
}

struct Fixture {
    root: PathBuf,
    evidence: PathBuf,
    cleanup_allowed: Cell<bool>,
}
impl Fixture {
    fn new() -> Self {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        Self::new_in_scratch(&scratch)
    }
    fn new_in_project(requested: &Path) -> Self {
        // The full owner clone must not inherit the AIKit Cargo workspace.
        // All actual source/ignore checks precede full fixture allocation.
        assert!(
            requested.is_absolute() && requested.is_dir(),
            "full fixture requires the actual absolute admitted producer root"
        );
        let project = fs::canonicalize(requested).unwrap();
        let aikit = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
        assert!(
            !project.starts_with(&aikit),
            "full producer must be outside the enclosing AIKit Cargo workspace"
        );
        let expected_tip = std::env::var("SEAT_GUARD_GATE_PRODUCT_REVISION")
            .expect("Root-admitted exact actual product tip is required");
        let expected_ignore = std::env::var("SEAT_GUARD_GATE_IGNORE_SHA256")
            .expect("exact admitted native Run-space ignore source is required");
        let mut runner = SystemRunner::new()
            .with_cwd(&project)
            .with_strict_utf8()
            .with_timeout(Duration::from_secs(10))
            .with_output_limit_bytes(1024 * 1024)
            .with_env("GIT_CONFIG_NOSYSTEM", "1")
            .with_env("GIT_CONFIG_GLOBAL", "/dev/null");
        for variable in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_CONFIG_COUNT",
            "LANE",
            "CENTRAL_ROOT",
        ] {
            runner = runner.with_env_removed(variable);
        }
        let mut observations = Vec::new();
        let mut check = |arguments: &[&str]| {
            let argv: Vec<String> = arguments.iter().map(|value| (*value).to_owned()).collect();
            let output = runner
                .run_with_limits(&argv, Duration::from_secs(10), 1024 * 1024, true)
                .unwrap_or_else(|error| {
                    panic!(
                        "actual producer preflight failed before allocation: {}",
                        native_error(&error)
                    )
                });
            assert!(
                output.ok(),
                "actual producer preflight {arguments:?} refused before allocation: {output:?}"
            );
            observations.push(json!({"cwd":project,"argv":argv,"status":output.status,
                "stdout":output.stdout,"stderr":output.stderr}));
            output
        };
        let top = check(&["git", "rev-parse", "--show-toplevel"]);
        assert_eq!(
            fs::canonicalize(top.line()).unwrap(),
            project,
            "actual producer top-level"
        );
        assert_eq!(check(&["git", "rev-parse", "HEAD"]).line(), expected_tip);
        assert!(
            check(&["git", "status", "--porcelain", "--untracked-files=all"])
                .stdout
                .is_empty(),
            "actual producer must be clean before native Run-space allocation"
        );
        assert_eq!(
            check(&["git", "ls-files", "--error-unmatch", "--", ".gitignore"]).line(),
            ".gitignore"
        );
        assert_eq!(
            digest_file(&project.join(".gitignore")).unwrap(),
            expected_ignore,
            "actual tracked owner ignore source, not an external Git exclude"
        );
        const MEMBER: &str = "ProjectCentral/now/tmp/native-seat-guard-ignore-preflight";
        let ignored = check(&["git", "check-ignore", "-v", "--no-index", "--", MEMBER]);
        let (rule, member) = ignored
            .line()
            .split_once('\t')
            .expect("actual Git ignore rule and member");
        assert!(
            rule.starts_with(".gitignore:"),
            "native scratch must use tracked owner .gitignore: {rule}"
        );
        assert_eq!(member, MEMBER);
        let scratch = project.join("ProjectCentral/now/tmp");
        let fixture = Self::new_in_scratch(&scratch);
        fixture.save_json("actual-producer-preflight", &json!({"requested_root":requested,
            "project_root":project,"scratch":scratch,"head":expected_tip,
            "gitignore_sha256":expected_ignore,"observations":observations,
            "standing":"actual source prerequisites observed before native full-case material allocation"}));
        fixture
    }
    fn new_in_scratch(scratch: &Path) -> Self {
        fs::create_dir_all(scratch).unwrap();
        let root = scratch.join(format!(
            "native-seat-guard-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let evidence_root = scratch.join("native-seat-guard-evidence");
        fs::create_dir_all(&evidence_root).unwrap();
        let evidence = evidence_root.join(root.file_name().unwrap());
        fs::create_dir(&evidence).unwrap();
        let fixture = Self {
            root,
            evidence,
            cleanup_allowed: Cell::new(true),
        };
        let hooks = fixture.hooks();
        fs::create_dir_all(&hooks).unwrap();
        for (name, variable, expected) in [
            (
                "pre-commit",
                "SEAT_GUARD_PRECOMMIT_CANDIDATE",
                PRECOMMIT_SOURCE,
            ),
            ("pre-push", "SEAT_GUARD_PREPUSH_CANDIDATE", PREPUSH_SOURCE),
        ] {
            let source = PathBuf::from(
                std::env::var_os(variable)
                    .unwrap_or_else(|| panic!("requires exact admitted hook source: {variable}")),
            );
            assert!(fs::metadata(&source).unwrap().is_file());
            assert_eq!(
                digest_file(&source).unwrap(),
                expected,
                "exact source candidate {name}"
            );
            fs::copy(&source, hooks.join(name)).unwrap();
            assert_eq!(
                digest_file(&hooks.join(name)).unwrap(),
                expected,
                "exact copied source {name}"
            );
            fs::set_permissions(hooks.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::create_dir_all(fixture.root.join("Control/machines")).unwrap();
        fs::create_dir(fixture.root.join("home")).unwrap();
        fs::write(fixture.root.join("global-git-config"), b"").unwrap();
        let executable = std::env::current_exe().unwrap();
        fixture.save_json(
            "test-basis",
            &json!({
                "schema":"central.seat-guard-qualification-basis/v1",
                "standing":"controlled native test evidence; not protected source acceptance",
                "os":std::env::consts::OS,"arch":std::env::consts::ARCH,
                "test_binary":executable,"test_binary_sha256":digest_file(&executable).unwrap(),
                "precommit_sha256":PRECOMMIT_SOURCE,"prepush_sha256":PREPUSH_SOURCE,
                "owned_fixture":fixture.root,"evidence_directory":fixture.evidence,
                "cargo_home":std::env::var("CARGO_HOME").ok(),
                "rustup_home":std::env::var("RUSTUP_HOME").ok(),
                "playwright_browsers_path":std::env::var("PLAYWRIGHT_BROWSERS_PATH").ok()
            }),
        );
        fixture
    }
    fn save_json(&self, label: &str, value: &Value) {
        fs::write(
            self.evidence.join(format!("{label}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap_or_else(|error| {
            panic!(
                "retain actual evidence {}: {error}",
                self.evidence.display()
            )
        });
    }
    fn retain_output(&self, label: &str, cwd: &Path, argv: &[String], output: &Output) {
        self.save_json(
            label,
            &json!({"cwd":cwd,"argv":argv,"status":output.status,
            "stdout":output.stdout,"stderr":output.stderr}),
        );
    }
    fn retain_gate_files(&self, product: &Path) {
        // Only actual producer output in this owned fresh clone is retained.
        // Refuse unknown material rather than upload the repository or clip logs.
        let receipts = product.join("gates/receipts");
        if !receipts.exists() {
            return;
        }
        let destination = self.evidence.join("actual-gate-receipts");
        fs::create_dir(&destination).unwrap();
        let mut queued = vec![(receipts, destination, 0_usize)];
        let mut entries = 0_usize;
        let mut total = 0_u64;
        while let Some((source, target, depth)) = queued.pop() {
            for entry in fs::read_dir(source).unwrap() {
                let entry = entry.unwrap();
                entries += 1;
                assert!(
                    entries <= 256,
                    "actual receipt evidence capacity exceeded; fixture retained"
                );
                let material = entry.path();
                let metadata = fs::symlink_metadata(&material).unwrap();
                let copied = target.join(entry.file_name());
                if metadata.is_dir() {
                    assert!(
                        depth < 2,
                        "unexpected receipt directory depth; fixture retained"
                    );
                    fs::create_dir(&copied).unwrap();
                    queued.push((material, copied, depth + 1));
                } else {
                    assert!(
                        metadata.is_file() && !metadata.file_type().is_symlink(),
                        "actual receipt evidence must be regular material"
                    );
                    assert!(
                        metadata.len() <= 16 * 1024 * 1024,
                        "receipt evidence file capacity exceeded"
                    );
                    total = total.checked_add(metadata.len()).unwrap();
                    assert!(
                        total <= 64 * 1024 * 1024,
                        "receipt evidence total capacity exceeded"
                    );
                    assert_eq!(fs::copy(&material, &copied).unwrap(), metadata.len());
                }
            }
        }
        self.save_json("actual-gate-evidence-retention", &json!({"entries":entries,
            "bytes":total,"standing":"copies of real producer outputs; not another receipt authority"}));
    }
    fn hooks(&self) -> PathBuf {
        self.root.join("Control/user/seat-guard/hooks")
    }
    fn register(&self) -> PathBuf {
        self.root.join("Control/machines/workcells.register.json")
    }
    fn runner(&self, cwd: &Path) -> SystemRunner {
        let mut runner = SystemRunner::new()
            .with_cwd(cwd)
            .with_strict_utf8()
            .with_timeout(Duration::from_secs(10))
            .with_output_limit_bytes(1024 * 1024)
            .with_env("HOME", self.root.join("home").to_str().unwrap())
            .with_env("GIT_CONFIG_NOSYSTEM", "1")
            .with_env(
                "GIT_CONFIG_GLOBAL",
                self.root.join("global-git-config").to_str().unwrap(),
            );
        for variable in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_CONFIG_COUNT",
            "LANE",
            "CENTRAL_ROOT",
        ] {
            runner = runner.with_env_removed(variable);
        }
        runner
    }
    fn run(&self, cwd: &Path, arguments: &[&str]) -> Output {
        self.cleanup_allowed.set(false);
        let arguments: Vec<String> = arguments.iter().map(|value| (*value).to_owned()).collect();
        match self.runner(cwd).run_with_limits(
            &arguments,
            Duration::from_secs(10),
            1024 * 1024,
            true,
        ) {
            Ok(output) => {
                self.retain_output(
                    &format!("command-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
                    cwd,
                    &arguments,
                    &output,
                );
                self.cleanup_allowed.set(true);
                output
            }
            Err(error) => {
                self.save_json(
                    "actual-native-command-error",
                    &json!({"cwd":cwd,"argv":arguments,
                    "native_error":native_error(&error)}),
                );
                panic!(
                    "actual native command failure; retain owned fixture {}: {error}",
                    self.root.display()
                )
            }
        }
    }
    fn ok(&self, cwd: &Path, arguments: &[&str]) -> Output {
        let output = self.run(cwd, arguments);
        assert!(
            output.ok(),
            "{arguments:?}: {} {}",
            output.stdout,
            output.stderr
        );
        output
    }
    fn repo(&self, relative: &str, branch: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(&path).unwrap();
        self.ok(&path, &["git", "init", "--initial-branch", branch]);
        self.ok(
            &path,
            &[
                "git",
                "config",
                "user.name",
                "Controlled native hook fixture",
            ],
        );
        self.ok(
            &path,
            &[
                "git",
                "config",
                "user.email",
                "native-fixture@example.invalid",
            ],
        );
        fs::write(path.join("seed"), b"actual fixture seed\n").unwrap();
        self.ok(&path, &["git", "add", "seed"]);
        self.ok(&path, &["git", "commit", "-m", "Initial native fixture"]);
        self.ok(
            &path,
            &[
                "git",
                "config",
                "core.hooksPath",
                self.hooks().to_str().unwrap(),
            ],
        );
        path
    }
    fn commit(&self, path: &Path) -> Output {
        let name = format!("case-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        fs::write(path.join(&name), b"actual selected fixture bytes\n").unwrap();
        self.ok(path, &["git", "add", &name]);
        self.run(path, &["git", "commit", "-m", &name])
    }
    fn registration(&self) -> Value {
        json!({"schema":"central.workcell-register/v1","unknown_fixture_field":"preserved",
            "workcells":[{"name":"env-3","path":self.root.join("worktrees/env-3"),
                "products":{"central":{"path":"central","state":"occupied-foreign-lane",
                    "claim":{"branch":"a retained label is not live Git truth"}}}}]})
    }
    fn write_registration(&self, value: &Value) {
        fs::write(self.register(), serde_json::to_vec(value).unwrap()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.cleanup_allowed.get() {
            eprintln!(
                "retain exact fixture after native command uncertainty: {}",
                self.root.display()
            );
            return;
        }
        if let Err(error) = fs::remove_dir_all(&self.root) {
            let detail = format!(
                "owned native hook fixture cleanup {}: {error}",
                self.root.display()
            );
            if std::thread::panicking() {
                eprintln!("{detail}");
            } else {
                panic!("{detail}");
            }
        }
    }
}

#[test]
#[ignore = "requires admitted exact hook candidates, real Git/Python and nonroot permission prerequisite"]
fn real_precommit_qualifies_register_and_refuses_material_or_shape_failure() {
    assert!(
        !rustix::process::geteuid().is_root(),
        "actual EACCES prerequisite is unavailable under root"
    );
    let fixture = Fixture::new();
    let repo = fixture.repo("worktrees/env-3/central", "agent/native-fixture");
    let valid = fixture.registration();
    fixture.write_registration(&valid);
    let bytes = fs::read(fixture.register()).unwrap();
    assert!(fixture.commit(&repo).ok());
    assert_eq!(fs::read(fixture.register()).unwrap(), bytes);

    fs::remove_file(fixture.register()).unwrap();
    let absent = fixture.commit(&repo);
    assert!(
        !absent.ok() && absent.stderr.contains("FileNotFoundError"),
        "{absent:?}"
    );
    fs::create_dir(fixture.register()).unwrap();
    let directory = fixture.commit(&repo);
    assert!(
        !directory.ok() && directory.stderr.contains("not a regular file"),
        "{directory:?}"
    );
    fs::remove_dir(fixture.register()).unwrap();
    let fifo = fixture.register();
    fixture.ok(
        &fixture.root,
        &["mkfifo", "-m", "600", fifo.to_str().unwrap()],
    );
    let fifo_output = fixture.commit(&repo);
    assert!(
        !fifo_output.ok() && fifo_output.stderr.contains("not a regular file"),
        "{fifo_output:?}"
    );
    fs::remove_file(fifo).unwrap();
    fs::write(fixture.register(), [0xff]).unwrap();
    let invalid_utf8 = fixture.commit(&repo);
    assert!(
        !invalid_utf8.ok() && invalid_utf8.stderr.contains("UnicodeDecodeError"),
        "{invalid_utf8:?}"
    );
    fs::write(fixture.register(), b"{not-json").unwrap();
    let invalid_json = fixture.commit(&repo);
    assert!(
        !invalid_json.ok() && invalid_json.stderr.contains("JSONDecodeError"),
        "{invalid_json:?}"
    );

    let mut malformed_after_valid = valid.clone();
    malformed_after_valid["workcells"]
        .as_array_mut()
        .unwrap()
        .push(json!({"products":{}}));
    for value in [
        json!({}),
        json!({"schema":"wrong","workcells":[]}),
        json!({"schema":"central.workcell-register/v1","workcells":{}}),
        json!({"schema":"central.workcell-register/v1","workcells":[null]}),
        json!({"schema":"central.workcell-register/v1","workcells":[{"path":"","products":{"central":{"path":""}}}]}),
        json!({"schema":"central.workcell-register/v1","workcells":[{"path":"x","products":[]}]}),
        json!({"schema":"central.workcell-register/v1","workcells":[{"path":"x","products":{"central":{}}}]}),
        malformed_after_valid,
    ] {
        fixture.write_registration(&value);
        let output = fixture.commit(&repo);
        assert!(
            !output.ok() && output.stderr.contains("unavailable or malformed"),
            "{value}: {output:?}"
        );
    }

    fixture.write_registration(&valid);
    let mode = fs::metadata(fixture.register()).unwrap().permissions();
    fs::set_permissions(fixture.register(), fs::Permissions::from_mode(0o0)).unwrap();
    let actual = fs::read(fixture.register()).unwrap_err();
    let permission = fixture.commit(&repo);
    fs::set_permissions(fixture.register(), mode).unwrap();
    assert_eq!(actual.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(
        actual.raw_os_error(),
        Some(rustix::io::Errno::ACCESS.raw_os_error())
    );
    assert!(
        !permission.ok() && permission.stderr.contains("PermissionError"),
        "{permission:?}"
    );
    assert!(fixture.commit(&repo).ok());
    assert_eq!(fs::read(fixture.register()).unwrap(), bytes);

    let original = fixture.root.join("Control/machines/original-register.json");
    fs::rename(fixture.register(), &original).unwrap();
    std::os::unix::fs::symlink(&original, fixture.register()).unwrap();
    assert!(
        fixture.commit(&repo).ok(),
        "unchanged symlink-to-regular support"
    );
    assert_eq!(fs::read(original).unwrap(), bytes);
}

#[test]
#[ignore = "requires admitted exact hook candidates and real Git/Python"]
fn real_precommit_preserves_primary_nested_foreign_legacy_and_stray_scope() {
    let fixture = Fixture::new();
    let registered = fixture.repo("worktrees/env-3/central", "agent/native-fixture");
    let mut valid = fixture.registration();
    // Historical names/unknown state do not become a second authority.
    valid["workcells"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"retained-legacy",
        "path":fixture.root.join("worktrees/legacy"),"products":{"old":{"path":"old"}}}));
    fixture.write_registration(&valid);
    assert!(fixture.commit(&registered).ok());
    let legacy = fixture.repo("worktrees/legacy/old", "agent/retained-legacy");
    assert!(fixture.commit(&legacy).ok());
    let unregistered = fixture.repo("worktrees/env-3/another-checkout", "agent/stray");
    let rejected = fixture.commit(&unregistered);
    assert!(
        !rejected.ok() && rejected.stderr.contains("not a registered seat"),
        "{rejected:?}"
    );
    let primary = fixture.repo("Work/project", "main");
    assert!(fixture.commit(&primary).ok());
    fixture.ok(&primary, &["git", "switch", "-c", "agent/wrong-primary"]);
    assert!(!fixture.commit(&primary).ok());
    fixture.ok(&primary, &["git", "switch", "--detach"]);
    assert!(!fixture.commit(&primary).ok());
    let nested = fixture.repo("Work/another/nested", "main");
    assert!(
        fixture.commit(&nested).ok(),
        "retain actual existing nested scope; no new adoption"
    );
    let external = fixture.repo("outside-ground", "main");
    assert!(fixture.commit(&external).ok());

    // A real linked test worktree of the owned fixture (never a development
    // seat, product or personal checkout) exercises the actual common-dir seam.
    let stray = fixture.root.join("outside-linked-fixture");
    fixture.ok(
        &registered,
        &[
            "git",
            "worktree",
            "add",
            "-b",
            "agent/linked-fixture",
            stray.to_str().unwrap(),
        ],
    );
    let stray_output = fixture.commit(&stray);
    assert!(
        !stray_output.ok() && stray_output.stderr.contains("stray worktree"),
        "{stray_output:?}"
    );
    fixture.ok(
        &registered,
        &[
            "git",
            "worktree",
            "remove",
            "--force",
            stray.to_str().unwrap(),
        ],
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(fixture.register()).unwrap()).unwrap(),
        valid
    );
}

#[test]
#[ignore = "requires exact hooks and admitted actual product repository; runs real full owner landing gates before normal local-bare push"]
fn real_prepush_uses_third_destination_token_and_actual_full_tip_receipt() {
    // HOME is isolated below; preserve actual provisioned tool and browser homes.
    let cargo_home = required_directory("CARGO_HOME");
    let rustup_home = required_directory("RUSTUP_HOME");
    let browser_home = required_directory("PLAYWRIGHT_BROWSERS_PATH");
    assert!(
        !rustix::process::geteuid().is_root(),
        "nonroot native qualification prerequisite"
    );
    let source = PathBuf::from(
        std::env::var_os("SEAT_GUARD_GATE_PRODUCT_REPOSITORY")
            .expect("actual product repository is required; no synthetic full-pass fixture"),
    );
    let expected_tip = std::env::var("SEAT_GUARD_GATE_PRODUCT_REVISION")
        .expect("Root-admitted exact actual product tip is required");
    let fixture = Fixture::new_in_project(&source);
    fixture.save_json(
        "actual-full-prerequisites",
        &json!({"cargo_home":cargo_home,
        "rustup_home":rustup_home,"playwright_browsers_path":browser_home,
        "actual_product_root":source}),
    );
    let original_tip = fixture
        .ok(&source, &["git", "rev-parse", "HEAD"])
        .line()
        .to_owned();
    assert_eq!(
        original_tip, expected_tip,
        "Root admitted exact committed product source"
    );
    let product = fixture.root.join("gate-product-fixture");
    fixture.ok(
        &fixture.root,
        &[
            "git",
            "clone",
            "--local",
            "--no-hardlinks",
            "--no-checkout",
            source.to_str().unwrap(),
            product.to_str().unwrap(),
        ],
    );
    fixture.ok(&product, &["git", "checkout", "--detach", &original_tip]);
    assert!(
        product.join("gates/run.mjs").is_file(),
        "actual committed product runner required"
    );
    assert!(product.join("gates/manifest.json").is_file());
    for (member, variable) in [
        ("gates/run.mjs", "SEAT_GUARD_GATE_RUN_SHA256"),
        ("gates/manifest.json", "SEAT_GUARD_GATE_MANIFEST_SHA256"),
    ] {
        let expected = std::env::var(variable).expect("exact Root-admitted production source hash");
        assert_eq!(digest_file(&product.join(member)).unwrap(), expected);
    }
    assert!(fixture
        .ok(&product, &["git", "status", "--porcelain"])
        .stdout
        .is_empty());
    let manifest: Value =
        serde_json::from_slice(&fs::read(product.join("gates/manifest.json")).unwrap()).unwrap();
    fixture.save_json(
        "actual-product-source",
        &json!({"head":original_tip,
        "gate_run_sha256":digest_file(&product.join("gates/run.mjs")).unwrap(),
        "gate_manifest_sha256":digest_file(&product.join("gates/manifest.json")).unwrap()}),
    );
    for arguments in [
        &["git", "--version"][..],
        &["python3", "--version"][..],
        &["node", "--version"][..],
        &["cargo", "--version"][..],
        &["rustc", "-Vv"][..],
    ] {
        fixture.ok(&product, arguments);
    }
    fixture.ok(
        &product,
        &[
            "git",
            "config",
            "core.hooksPath",
            fixture.hooks().to_str().unwrap(),
        ],
    );
    let bare = fixture.root.join("owned-local-destination.git");
    fixture.ok(
        &fixture.root,
        &["git", "init", "--bare", bare.to_str().unwrap()],
    );
    let receipt = product.join("gates/receipts/latest-landing.json");
    assert!(!receipt.exists(), "no transported or invented old receipt");

    // Execute the existing product's actual full landing tier. No --only,
    // --skip, fake run.mjs, receipt write or qualification upgrade is used.
    fixture.cleanup_allowed.set(false);
    let output = fixture
        .runner(&product)
        .with_timeout(Duration::from_secs(5400))
        .with_output_limit_bytes(8 * 1024 * 1024)
        .run_with_limits(
            &["node".into(), "gates/run.mjs".into(), "landing".into()],
            Duration::from_secs(5400),
            8 * 1024 * 1024,
            true,
        )
        .unwrap_or_else(|error| {
            fixture.save_json("actual-gate-runner-error", &native_error(&error));
            panic!(
                "actual gate failure/uncertainty; retain {}: {error}",
                fixture.root.display()
            )
        });
    fixture.retain_output(
        "actual-full-gate-output",
        &product,
        &["node".into(), "gates/run.mjs".into(), "landing".into()],
        &output,
    );
    if receipt.is_file() {
        fs::copy(
            &receipt,
            fixture.evidence.join("actual-owner-latest-landing.json"),
        )
        .unwrap();
    }
    fixture.retain_gate_files(&product);
    fixture.cleanup_allowed.set(true);
    assert!(
        output.ok(),
        "full owner gates failed; not a successful hook prerequisite: {output:?}"
    );
    let owner_bytes = fs::read(&receipt).unwrap();
    let owner: Value = serde_json::from_slice(&owner_bytes).unwrap();
    assert_eq!(owner["schema"], "oi.gate-run/v1");
    assert_eq!(owner["head"], original_tip);
    assert_eq!(owner["scope"], "full");
    assert_eq!(owner["dirty"], false);
    assert_eq!(owner["tier"], "landing");
    assert_eq!(owner["pass"], true);
    let selected = owner["selected"].as_array().unwrap();
    let gates = owner["gates"].as_array().unwrap();
    assert!(!selected.is_empty());
    let expected: Vec<Value> = manifest["gates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|gate| gate["tier"] == "landing")
        .map(|gate| gate["name"].clone())
        .collect();
    assert_eq!(
        expected.len(),
        19,
        "exact paired owner manifest has nineteen landing gates"
    );
    assert_eq!(
        selected, &expected,
        "all actual selected landing definitions, never a filtered substitute"
    );
    assert_eq!(gates.len(), selected.len());
    let result_names: Vec<Value> = gates.iter().map(|gate| gate["name"].clone()).collect();
    assert_eq!(
        result_names, expected,
        "every actual selected result is present in order"
    );
    for gate in gates {
        assert_eq!(gate["ok"], true);
        assert_eq!(gate["exit"], 0);
    }

    let push = |destination: &str| {
        fixture.run(
            &product,
            &[
                "git",
                "push",
                bare.to_str().unwrap(),
                &format!("HEAD:{destination}"),
            ],
        )
    };
    // Empty valid Node stdout must not reject the actual owner-produced receipt.
    assert!(push("refs/heads/main").ok());
    assert_eq!(
        fixture
            .ok(&bare, &["git", "rev-parse", "refs/heads/main"])
            .line(),
        original_tip
    );
    assert_eq!(fs::read(&receipt).unwrap(), owner_bytes);

    // Now exercise a genuine existing remote-object token. A new actual
    // fixture commit has no fresh qualification; the prior real receipt is
    // stale. The normal fast-forward must be refused before remote mutation.
    fixture.ok(
        &product,
        &[
            "git",
            "config",
            "user.name",
            "Controlled native hook fixture",
        ],
    );
    fixture.ok(
        &product,
        &[
            "git",
            "config",
            "user.email",
            "native-fixture@example.invalid",
        ],
    );
    fs::write(
        product.join("actual-stale-tip-proof"),
        b"new actual fixture commit\n",
    )
    .unwrap();
    fixture.ok(&product, &["git", "add", "actual-stale-tip-proof"]);
    fixture.ok(
        &product,
        &["git", "commit", "-m", "Actual unqualified successor tip"],
    );
    let stale = push("refs/heads/main");
    assert!(
        !stale.ok()
            && format!("{}{}", stale.stdout, stale.stderr)
                .contains("does not cover this exact tip"),
        "{stale:?}"
    );
    assert_eq!(
        fixture
            .ok(&bare, &["git", "rev-parse", "refs/heads/main"])
            .line(),
        original_tip
    );
    // Only this disposable cloned test repository is restored; no source
    // input, personal World or development checkout is reset.
    fixture.ok(&product, &["git", "reset", "--hard", &original_tip]);

    // Each negative is a deliberately corrupted derivative of the actual
    // receipt, never reported as owner qualification. Remove remote Main so
    // normal Git generates a real Main update and reaches the hook each time.
    for (field, value) in [
        ("head", json!("not-this-tip")),
        ("pass", json!(false)),
        ("tier", json!("walks")),
        ("scope", json!("partial")),
    ] {
        fixture.ok(&bare, &["git", "update-ref", "-d", "refs/heads/main"]);
        let mut corrupt = owner.clone();
        corrupt[field] = value;
        fs::write(&receipt, serde_json::to_vec(&corrupt).unwrap()).unwrap();
        let rejected = push("refs/heads/main");
        assert!(
            !rejected.ok()
                && format!("{}{}", rejected.stdout, rejected.stderr)
                    .contains("does not cover this exact tip"),
            "{rejected:?}"
        );
        assert!(!fixture
            .run(&bare, &["git", "show-ref", "--verify", "refs/heads/main"])
            .ok());
    }
    fs::write(&receipt, b"{broken-json").unwrap();
    assert!(!push("refs/heads/main").ok());
    fs::remove_file(&receipt).unwrap();
    assert!(!push("refs/heads/main").ok());
    // Non-Main retains its existing scope even without a Main receipt.
    assert!(push("refs/heads/fixture-feature").ok());
    fs::write(&receipt, &owner_bytes).unwrap();
    assert!(push("refs/heads/main").ok());
    assert_eq!(fs::read(receipt).unwrap(), owner_bytes);
}
