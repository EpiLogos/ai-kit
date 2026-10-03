use std::collections::BTreeMap;

use aikit_adapters::bkmr::BkmrSourcePoolProvider;
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_core::knowledge_source_pool::{
    material_for_actor, SourceBinding, SourceMaterial, SourcePool, SourcePoolProvider,
    SourceSearchMode, SourceVisibility, BKMR_GLADE_CONFORMANCE_VERSION,
};
use aikit_core::resource::{SourceRef, SourceRevision};
use tempfile::TempDir;

fn native_tempdir() -> TempDir {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ProjectCentral/now/tmp");
    std::fs::create_dir_all(&root).expect("actual product Run-space scratch");
    tempfile::Builder::new()
        .prefix("bkmr-real-")
        .tempdir_in(root)
        .expect("exclusive owned bkmr fixture")
}

fn source(
    id: &str,
    title: &str,
    body: &str,
    tags: &[&str],
    visibility: SourceVisibility,
    owners: &[&str],
) -> SourceMaterial {
    SourceMaterial {
        binding: SourceBinding {
            source: SourceRef::parse(id).expect("fixture source ref"),
            revision: SourceRevision::parse(format!("revision:{id}")).expect("fixture revision"),
            title: title.into(),
            tags: tags.iter().map(|tag| (*tag).into()).collect(),
            visibility,
            owners: owners.iter().map(|owner| (*owner).into()).collect(),
            media_type: "text/markdown".into(),
            locator: None,
            metadata: BTreeMap::new(),
        },
        body: body.into(),
    }
}

#[test]
fn real_bkmr_767_preserves_refs_capabilities_and_privacy_membrane() {
    let dir = native_tempdir();
    let shared = source(
        "source:astronomy",
        "Astronomy",
        "Astronomy uses a telescope to observe distant galaxies and quasars.",
        &["astronomy", "science"],
        SourceVisibility::Team,
        &[],
    );
    let private = source(
        "source:private",
        "Private",
        "The private obsidian narwhal phrase belongs only to Alex.",
        &["private"],
        SourceVisibility::Personal,
        &["alex"],
    );
    let pool = SourcePool::new(
        "source-pool:real-bkmr",
        vec![shared.binding.clone(), private.binding.clone()],
    )
    .expect("valid source pool");
    let material = vec![shared.clone(), private.clone()];

    let mut provider =
        BkmrSourcePoolProvider::new(SystemRunner::new(), dir.path().join("bkmr.db"), false);
    let status = provider.status();
    if !status.available {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_BKMR_REAL").is_none(),
            "AIKIT_REQUIRE_BKMR_REAL is set but bkmr is unavailable: {}",
            status.detail
        );
        return;
    }

    // Detection truth: the reported version must be what the installed CLI
    // itself reports. A real-environment test never mandates a pinned version;
    // it proves detection is truthful on whatever is installed.
    let probe = SystemRunner::new()
        .run(&["bkmr".into(), "--version".into()])
        .expect("bkmr --version probe");
    let reported = format!("{} {}", probe.stdout, probe.stderr);
    let version = status
        .version
        .as_deref()
        .expect("status reports the installed bkmr version");
    assert!(
        reported.contains(version),
        "detected version {version} must come from the CLI's own --version output"
    );
    // The conformance pin is tested-version metadata, not a mandate: drift is
    // reported, never enforced.
    assert_eq!(
        status.tested_version.as_deref(),
        Some(BKMR_GLADE_CONFORMANCE_VERSION)
    );
    assert_eq!(
        status.version_drift,
        version != BKMR_GLADE_CONFORMANCE_VERSION
    );

    // The behavioural contract below runs fulltext JSON search with tag
    // filtering; both are detected from the installed CLI. When either is
    // absent there is nothing honest to verify, so the test skips like any
    // unavailable provider. The rest of the capability matrix is a detected
    // report, not a law.
    let missing: Vec<&str> = [
        ("fulltext", status.capabilities.fulltext),
        ("tags", status.capabilities.tags),
    ]
    .iter()
    .filter(|(_, present)| !*present)
    .map(|(faculty, _)| *faculty)
    .collect();
    if !missing.is_empty() {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_BKMR_REAL").is_none(),
            "AIKIT_REQUIRE_BKMR_REAL is set but the installed bkmr lacks required faculties {missing:?}: {}",
            status.detail
        );
        return;
    }

    // The strict conformance matrix is opt-in: set AIKIT_BKMR_CONFORMANCE in an
    // environment pinned to exactly the tested version (e.g. CI).
    if std::env::var_os("AIKIT_BKMR_CONFORMANCE").is_some() {
        assert_eq!(version, BKMR_GLADE_CONFORMANCE_VERSION);
        assert!(!status.version_drift);
        assert!(status.capabilities.fuzzy_interactive);
        assert!(!status.capabilities.semantic);
        assert!(!status.capabilities.hybrid);
    }

    let frank_material = material_for_actor(&pool, &material, Some("frank"), true)
        .expect("privacy-filtered provider material");
    assert_eq!(frank_material.len(), 1);
    assert_eq!(
        frank_material[0].binding.source.as_str(),
        "source:astronomy"
    );
    provider
        .rebuild(&frank_material)
        .expect("build frank provider view");

    let hits = provider
        .search("quasars", SourceSearchMode::Fulltext, &[], 20)
        .expect("fulltext search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].source.as_str(), "source:astronomy");
    assert_eq!(hits[0].provider.as_str(), "provider/source-pool/bkmr");
    assert!(hits[0].provider_binding.is_some());
    assert!(provider
        .search("narwhal", SourceSearchMode::Fulltext, &[], 20)
        .expect("private search")
        .is_empty());

    let tagged = provider
        .search(
            "telescope",
            SourceSearchMode::Fulltext,
            &["astronomy".into()],
            20,
        )
        .expect("tagged search");
    assert_eq!(tagged.len(), 1);
    assert_eq!(tagged[0].source.as_str(), "source:astronomy");
    assert!(provider
        .search(
            "telescope",
            SourceSearchMode::Fulltext,
            &["private".into()],
            20,
        )
        .expect("non-matching tag search")
        .is_empty());

    provider
        .rebuild(&frank_material)
        .expect("rebuild provider view");
    let rebuilt = provider
        .search("quasars", SourceSearchMode::Fulltext, &[], 20)
        .expect("search after rebuild");
    assert_eq!(rebuilt[0].source.as_str(), "source:astronomy");

    let alex_material =
        material_for_actor(&pool, &material, Some("alex"), true).expect("alex provider material");
    assert_eq!(alex_material.len(), 2);
    provider
        .rebuild(&alex_material)
        .expect("build alex provider view");
    let private_hits = provider
        .search("narwhal", SourceSearchMode::Fulltext, &[], 20)
        .expect("alex private search");
    assert_eq!(private_hits.len(), 1);
    assert_eq!(private_hits[0].source.as_str(), "source:private");
}


// These cases exercise the authored shell capsule with the actual upstream
// binary. They do not establish registry promotion, a loaded projection or the
// separate CLI capture/Return contract.
#[cfg(unix)]
mod project_text_capsule {
    use super::*;
    use aikit_adapters::runner::Output;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    fn runner(root: &Path) -> SystemRunner {
        SystemRunner::new()
            .with_cwd(root)
            .with_env("HOME", root.to_string_lossy())
            .with_env("XDG_CONFIG_HOME", root.join("config").to_string_lossy())
            .with_timeout(Duration::from_secs(30))
            .with_output_limit_bytes(1024 * 1024)
            .with_strict_utf8()
    }

    fn fixture() -> Option<TempDir> {
        let dir = native_tempdir();
        let required = std::env::var_os("AIKIT_REQUIRE_BKMR_REAL").is_some();
        let probe = runner(dir.path()).run(&["bkmr".into(), "--version".into()]);
        let probe = match probe {
            Ok(probe) if probe.ok() => probe,
            other => {
                assert!(!required, "selected real bkmr prerequisite failed: {other:?}");
                eprintln!("unavailable optional capsule proof: {other:?}");
                return None;
            }
        };
        let provider = BkmrSourcePoolProvider::new(
            runner(dir.path()), dir.path().join("probe.db"), false,
        );
        let status = provider.status();
        let ready = status.available
            && status.version.as_deref() == Some(BKMR_GLADE_CONFORMANCE_VERSION)
            && status.capabilities.fulltext
            && status.capabilities.tags;
        if !ready {
            assert!(
                !required,
                "selected capsule proof requires actual7.6.7/fulltext/tags: {status:?}; {probe:?}"
            );
            eprintln!("unavailable optional7.6.7 capsule proof: {status:?}");
            return None;
        }
        assert!(
            format!("{} {}", probe.stdout, probe.stderr)
                .contains(BKMR_GLADE_CONFORMANCE_VERSION),
            "the actual version command must confirm the selected native version"
        );
        // No database is created by prerequisite observation.
        assert!(!dir.path().join("probe.db").exists());
        Some(dir)
    }

    fn seed(root: &Path, name: &str, label: &str) -> PathBuf {
        let path = root.join(name);
        let item = source(
            &format!("source:capsule:{label}"), label,
            &format!("capsulequasar {label} native selected content"),
            &["capsule-proof"], SourceVisibility::Team, &[],
        );
        let mut provider = BkmrSourcePoolProvider::new(runner(root), &path, false);
        provider.rebuild(&[item]).expect("real controlled native database seed");
        path
    }

    fn capture(root: &Path, label: &str, command: &mut Command) -> Output {
        let output = runner(root).capture_command(command)
            .expect("same finite native capture; timeout/capacity/refusal is not success");
        eprintln!("{label}: status={} stdout={:?} stderr={:?}",
            output.status, output.stdout, output.stderr);
        output
    }

    fn direct(root: &Path, db: &Path, query: &str, raw: bool) -> Output {
        let mut command = if raw {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "exec bkmr \"$@\" 2>&1", "--"]);
            command
        } else {
            Command::new("bkmr")
        };
        command.env_remove("AIKIT_BKMR_DB")
            .env_remove("AIKIT_BKMR_DB_SET")
            .env_remove("BKMR_DB_URL");
        command.args(["search", "--np", "--limit", "10"]);
        if !raw { command.arg("--json"); }
        command.arg(query).env("BKMR_DB_URL", db);
        capture(root, "actual upstream", &mut command)
    }

    fn capsule(root: &Path, primary: Option<&Path>, args: &[&str]) -> Output {
        capsule_environment(root, primary, None, None, args)
    }

    fn capsule_environment(
        root: &Path, primary: Option<&Path>, fallback: Option<&Path>,
        declared: Option<&str>, args: &[&str],
    ) -> Output {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contrib/bkmr/capsules/script/search/project-text/payload/search-text.sh");
        let mut command = Command::new("/bin/sh");
        command.env_remove("AIKIT_BKMR_DB")
            .env_remove("AIKIT_BKMR_DB_SET")
            .env_remove("BKMR_DB_URL");
        command.arg(script).args(args);
        if let Some(primary) = primary { command.env("AIKIT_BKMR_DB", primary); }
        if let Some(fallback) = fallback { command.env("BKMR_DB_URL", fallback); }
        if let Some(declared) = declared { command.env("AIKIT_BKMR_DB_SET", declared); }
        capture(root, "authored capsule", &mut command)
    }

    fn assert_failure_return(native: &Output, wrapped: &Output, raw: bool) {
        assert_ne!(native.status, 0, "the control must be a genuine native failure");
        assert_eq!(wrapped.status, native.status, "native status was changed");
        let diagnostic = if raw { &native.stdout } else { &native.stderr };
        let final_line = diagnostic.lines().filter(|line| !line.is_empty()).next_back()
            .expect("the actual native failure must supply a diagnostic");
        let returned = if raw { &wrapped.stdout } else { &wrapped.stderr };
        assert!(returned.contains(final_line), "native diagnostic lost: {wrapped:?}");
        if !raw { assert_eq!(wrapped.stdout, native.stdout); }
    }

    fn database_bytes(path: &Path) -> Vec<Option<Vec<u8>>> {
        ["", "-wal", "-shm"].iter().map(|suffix| {
            let name = PathBuf::from(format!("{}{suffix}", path.display()));
            match fs::read(name) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("actual database observation failed: {error}"),
            }
        }).collect()
    }

    #[test]
    fn real_767_capsule_preserves_single_json_raw_status_and_diagnostics() {
        let Some(dir) = fixture() else { return; };
        let db = seed(dir.path(), "single.db", "single-native");
        let native = direct(dir.path(), &db, "capsulequasar", false);
        let wrapped = capsule(dir.path(), Some(&db), &["capsulequasar"]);
        assert_eq!(native.status, 0);
        assert_eq!(wrapped, native);
        assert!(wrapped.stdout.contains("single-native"));
        let native_raw = direct(dir.path(), &db, "capsulequasar", true);
        let wrapped_raw = capsule(dir.path(), Some(&db), &["--raw", "capsulequasar"]);
        assert_eq!(native_raw.status, 0);
        assert_eq!(wrapped_raw, native_raw);
        // Raw FTS syntax is deliberately unchanged; this is a real upstream
        // parse error, not a simulated exit or an ordinary-language quote fix.
        let native_error = direct(dir.path(), &db, "(", false);
        let wrapped_error = capsule(dir.path(), Some(&db), &["("]);
        assert_failure_return(&native_error, &wrapped_error, false);
        let native_raw_error = direct(dir.path(), &db, "(", true);
        let wrapped_raw_error = capsule(dir.path(), Some(&db), &["--raw", "("]);
        assert_failure_return(&native_raw_error, &wrapped_raw_error, true);
        let corrupt = dir.path().join("corrupt.db");
        fs::write(&corrupt, b"not a SQLite database\n").unwrap();
        let old = fs::read(&corrupt).unwrap();
        let native_corrupt = direct(dir.path(), &corrupt, "capsulequasar", false);
        let wrapped_corrupt = capsule(dir.path(), Some(&corrupt), &["capsulequasar"]);
        assert_failure_return(&native_corrupt, &wrapped_corrupt, false);
        assert_eq!(fs::read(corrupt).unwrap(), old);
    }

    #[test]
    fn real_767_capsule_continues_declared_siblings_and_keeps_first_failure() {
        let Some(dir) = fixture() else { return; };
        let first = seed(dir.path(), "first.db", "first-native");
        let last = seed(dir.path(), "last.db", "last-native");
        let corrupt = dir.path().join("corrupt.db");
        fs::write(&corrupt, b"not a SQLite database\n").unwrap();
        let missing = dir.path().join("missing.db");
        let actual_error = direct(dir.path(), &corrupt, "capsulequasar", false);
        assert_ne!(actual_error.status, 0);
        assert_ne!(actual_error.status, 78, "native error must distinguish selected-missing78");
        let set = format!("{}:{}:{}:{}", first.display(), corrupt.display(),
            missing.display(), last.display());
        let output = capsule(dir.path(), Some(&first), &["--all", "--set", &set, "capsulequasar"]);
        assert_eq!(output.status, actual_error.status);
        assert!(output.stdout.contains("first-native"));
        assert!(output.stdout.contains("last-native"));
        let first_header = format!("### {}\n", first.display());
        let last_header = format!("### {}\n", last.display());
        assert!(output.stdout.find(&first_header).unwrap() < output.stdout.find(&last_header).unwrap());
        assert!(output.stderr.contains(&missing.display().to_string()));
        let diagnostic = actual_error.stderr.lines().filter(|line| !line.is_empty())
            .next_back().expect("actual corrupt database diagnostic");
        assert!(output.stderr.contains(diagnostic));
        let reverse = format!("{}:{}:{}", missing.display(), corrupt.display(), last.display());
        let output = capsule(dir.path(), Some(&first), &["--all", "--set", &reverse, "capsulequasar"]);
        assert_eq!(output.status, 78);
        assert!(output.stdout.contains("last-native"));
        assert!(output.stderr.contains(diagnostic));
        assert_eq!(fs::read(corrupt).unwrap(), b"not a SQLite database\n");
        assert!(!missing.exists());
    }

    #[test]
    fn real_767_capsule_retains_declared_grammar_and_unselected_primary() {
        let Some(dir) = fixture() else { return; };
        let first = seed(dir.path(), "space [one]*.db", "selected-one");
        let second = seed(dir.path(), "second.db", "selected-two");
        let backup = seed(dir.path(), "first_backup_20261003.db", "unselected-backup");
        let missing_primary = dir.path().join("unselected-primary.db");
        // The existing translation treats both colons and actual newlines as
        // separators. Quoted reads retain literal spaces/globs and duplicates.
        let set = format!(":\n{}::\n{}:\n{}:\n", first.display(), second.display(), first.display());
        let output = capsule(dir.path(), Some(&missing_primary),
            &["--all", "--set", &set, "capsulequasar"]);
        assert_eq!(output.status, 0);
        assert_eq!(output.stdout.matches(&format!("### {}\n", first.display())).count(), 2);
        assert_eq!(output.stdout.matches(&format!("### {}\n", second.display())).count(), 1);
        assert!(!output.stdout.contains("unselected-backup"));
        assert!(!output.stdout.contains(&format!("### {}\n", backup.display())));
        assert!(!missing_primary.exists());
        let single = capsule(dir.path(), Some(&missing_primary), &["capsulequasar"]);
        assert_eq!(single.status, 78);
        assert!(single.stderr.contains(&missing_primary.display().to_string()));
        let fallback = capsule_environment(dir.path(), None, Some(&first), None,
            &["capsulequasar"]);
        assert_eq!(fallback.status, 0);
        assert!(fallback.stdout.contains("selected-one"));
        let priority = capsule_environment(dir.path(), Some(&missing_primary), Some(&first),
            None, &["capsulequasar"]);
        assert_eq!(priority.status, 78);
        let environment_set = capsule_environment(dir.path(), Some(&missing_primary), None,
            Some(&set), &["--all", "capsulequasar"]);
        assert_eq!(environment_set, output);
        let only_second = second.display().to_string();
        let explicit_set = capsule_environment(dir.path(), Some(&missing_primary), None,
            Some(&set), &["--all", "--set", &only_second, "capsulequasar"]);
        assert_eq!(explicit_set.status, 0);
        assert!(explicit_set.stdout.contains("selected-two"));
        assert!(!explicit_set.stdout.contains("selected-one"));
        let unbound = capsule(dir.path(), None, &["--all", "--set", &set, "capsulequasar"]);
        assert_eq!(unbound.status, 78);
        let empty = capsule(dir.path(), Some(&missing_primary),
            &["--all", "--set", "::\n:\n", "capsulequasar"]);
        assert_eq!(empty.status, 0);
        assert!(empty.stdout.is_empty());
    }

    #[test]
    fn real_767_capsule_search_preserves_actual_database_and_sidecar_bytes() {
        let Some(dir) = fixture() else { return; };
        let db = seed(dir.path(), "readonly.db", "readonly-native");
        let before = database_bytes(&db);
        let output = capsule(dir.path(), Some(&db), &["capsulequasar"]);
        assert_eq!(output.status, 0);
        assert!(output.stdout.contains("readonly-native"));
        assert_eq!(database_bytes(&db), before,
            "a search must not silently mutate native database or sidecar bytes");
    }
}
