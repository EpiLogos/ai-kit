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
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
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

    pub(super) fn runner(root: &Path) -> SystemRunner {
        SystemRunner::new()
            .with_cwd(root)
            .with_env("HOME", root.to_string_lossy())
            .with_env("XDG_CONFIG_HOME", root.join("config").to_string_lossy())
            .with_timeout(Duration::from_secs(30))
            .with_output_limit_bytes(1024 * 1024)
            .with_strict_utf8()
    }

    pub(super) fn fixture() -> Option<TempDir> {
        let dir = native_tempdir();
        let required = std::env::var_os("AIKIT_REQUIRE_BKMR_REAL").is_some();
        let probe = runner(dir.path()).run(&["bkmr".into(), "--version".into()]);
        let probe = match probe {
            Ok(probe) if probe.ok() => probe,
            other => {
                assert!(
                    !required,
                    "selected real bkmr prerequisite failed: {other:?}"
                );
                eprintln!("unavailable optional capsule proof: {other:?}");
                return None;
            }
        };
        let provider =
            BkmrSourcePoolProvider::new(runner(dir.path()), dir.path().join("probe.db"), false);
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
            format!("{} {}", probe.stdout, probe.stderr).contains(BKMR_GLADE_CONFORMANCE_VERSION),
            "the actual version command must confirm the selected native version"
        );
        // No database is created by prerequisite observation.
        assert!(!dir.path().join("probe.db").exists());
        Some(dir)
    }

    fn seed(root: &Path, name: &str, label: &str) -> PathBuf {
        seed_with(
            root,
            name,
            &[(
                label,
                &format!("capsulequasar {label} native selected content"),
            )],
        )
    }

    /// [`seed`] with one bookmark for each label and body of `items`.
    fn seed_with(root: &Path, name: &str, items: &[(&str, &str)]) -> PathBuf {
        let path = root.join(name);
        let items: Vec<_> = items
            .iter()
            .map(|(label, body)| {
                source(
                    &format!("source:capsule:{label}"),
                    label,
                    body,
                    &["capsule-proof"],
                    SourceVisibility::Team,
                    &[],
                )
            })
            .collect();
        let mut provider = BkmrSourcePoolProvider::new(runner(root), &path, false);
        provider
            .rebuild(&items)
            .expect("real controlled native database seed");
        path
    }

    pub(super) fn capture(root: &Path, label: &str, command: &mut Command) -> Output {
        let output = runner(root)
            .capture_command(command)
            .expect("same finite native capture; timeout/capacity/refusal is not success");
        eprintln!(
            "{label}: status={} stdout={:?} stderr={:?}",
            output.status, output.stdout, output.stderr
        );
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
        command
            .env_remove("AIKIT_BKMR_DB")
            .env_remove("AIKIT_BKMR_DB_SET")
            .env_remove("BKMR_DB_URL");
        command.args(["search", "--np", "--limit", "10"]);
        if !raw {
            command.arg("--json");
        }
        command.arg(query).env("BKMR_DB_URL", db);
        capture(root, "actual upstream", &mut command)
    }

    fn capsule(root: &Path, primary: Option<&Path>, args: &[&str]) -> Output {
        capsule_environment(root, primary, None, None, args)
    }

    fn capsule_environment(
        root: &Path,
        primary: Option<&Path>,
        fallback: Option<&Path>,
        declared: Option<&str>,
        args: &[&str],
    ) -> Output {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contrib/bkmr/capsules/script/search/project-text/payload/search-text.sh");
        let mut command = Command::new("/bin/sh");
        command
            .env_remove("AIKIT_BKMR_DB")
            .env_remove("AIKIT_BKMR_DB_SET")
            .env_remove("BKMR_DB_URL");
        command.arg(script).args(args);
        if let Some(primary) = primary {
            command.env("AIKIT_BKMR_DB", primary);
        }
        if let Some(fallback) = fallback {
            command.env("BKMR_DB_URL", fallback);
        }
        if let Some(declared) = declared {
            command.env("AIKIT_BKMR_DB_SET", declared);
        }
        capture(root, "authored capsule", &mut command)
    }

    fn assert_failure_return(native: &Output, wrapped: &Output, raw: bool) {
        assert_ne!(
            native.status, 0,
            "the control must be a genuine native failure"
        );
        assert_eq!(wrapped.status, native.status, "native status was changed");
        let diagnostic = if raw { &native.stdout } else { &native.stderr };
        let final_line = diagnostic
            .lines()
            .rfind(|line| !line.is_empty())
            .expect("the actual native failure must supply a diagnostic");
        let returned = if raw {
            &wrapped.stdout
        } else {
            &wrapped.stderr
        };
        assert!(
            returned.contains(final_line),
            "native diagnostic lost: {wrapped:?}"
        );
        if !raw {
            assert_eq!(wrapped.stdout, native.stdout);
        }
    }

    // Native7.6.7 Bookmark tags are HashSet membership; its JSON view
    // iterates that set. Only tag order is normalized: store/record order,
    // duplicate store entries, every other field, status and stderr remain exact.
    fn normalized_json_sections(stdout: &str) -> Vec<(Option<&str>, serde_json::Value)> {
        let sections: Vec<_> = if let Some(headed) = stdout.strip_prefix("### ") {
            headed
                .split("\n### ")
                .map(|section| {
                    let (header, body) = section
                        .split_once('\n')
                        .expect("each actual store header must precede its native JSON");
                    assert!(!header.is_empty(), "actual store header must not be empty");
                    (Some(header), body)
                })
                .collect()
        } else {
            vec![(None, stdout)]
        };
        sections
            .into_iter()
            .map(|(header, body)| {
                let mut value: serde_json::Value = serde_json::from_str(body)
                    .expect("actual successful native JSON must remain complete");
                for record in value
                    .as_array_mut()
                    .expect("native JSON retains ordered bookmark records")
                {
                    let tags = record
                        .get_mut("tags")
                        .and_then(serde_json::Value::as_array_mut)
                        .expect("each native bookmark retains its complete tags array");
                    assert!(
                        tags.iter().all(serde_json::Value::is_string),
                        "native tags must remain strings"
                    );
                    tags.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
                }
                (header, value)
            })
            .collect()
    }

    fn assert_json_return_eq(actual: &Output, expected: &Output) {
        assert_eq!(actual.status, expected.status, "native status was changed");
        assert_eq!(actual.stderr, expected.stderr, "native stderr was changed");
        assert_eq!(
            normalized_json_sections(&actual.stdout),
            normalized_json_sections(&expected.stdout)
        );
    }

    pub(super) fn database_bytes(path: &Path) -> Vec<Option<Vec<u8>>> {
        ["", "-wal", "-shm", "-journal"]
            .iter()
            .map(|suffix| {
                let name = PathBuf::from(format!("{}{suffix}", path.display()));
                match fs::read(name) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => panic!("actual database observation failed: {error}"),
                }
            })
            .collect()
    }

    #[test]
    fn real_767_capsule_preserves_single_json_raw_status_and_diagnostics() {
        let Some(dir) = fixture() else {
            return;
        };
        let db = seed(dir.path(), "single.db", "single-native");
        let native = direct(dir.path(), &db, "capsulequasar", false);
        let wrapped = capsule(dir.path(), Some(&db), &["capsulequasar"]);
        assert_eq!(native.status, 0);
        assert_json_return_eq(&wrapped, &native);
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
        let Some(dir) = fixture() else {
            return;
        };
        let first = seed(dir.path(), "first.db", "first-native");
        let last = seed(dir.path(), "last.db", "last-native");
        let corrupt = dir.path().join("corrupt.db");
        fs::write(&corrupt, b"not a SQLite database\n").unwrap();
        let missing = dir.path().join("missing.db");
        let actual_error = direct(dir.path(), &corrupt, "capsulequasar", false);
        assert_ne!(actual_error.status, 0);
        assert_ne!(
            actual_error.status, 78,
            "native error must distinguish selected-missing78"
        );
        let set = format!(
            "{}:{}:{}:{}",
            first.display(),
            corrupt.display(),
            missing.display(),
            last.display()
        );
        let output = capsule(
            dir.path(),
            Some(&first),
            &["--all", "--set", &set, "capsulequasar"],
        );
        assert_eq!(output.status, actual_error.status);
        assert!(output.stdout.contains("first-native"));
        assert!(output.stdout.contains("last-native"));
        let first_header = format!("### {}\n", first.display());
        let last_header = format!("### {}\n", last.display());
        assert!(
            output.stdout.find(&first_header).unwrap() < output.stdout.find(&last_header).unwrap()
        );
        assert!(output.stderr.contains(&missing.display().to_string()));
        let diagnostic = actual_error
            .stderr
            .lines()
            .rfind(|line| !line.is_empty())
            .expect("actual corrupt database diagnostic");
        assert!(output.stderr.contains(diagnostic));
        let reverse = format!(
            "{}:{}:{}",
            missing.display(),
            corrupt.display(),
            last.display()
        );
        let output = capsule(
            dir.path(),
            Some(&first),
            &["--all", "--set", &reverse, "capsulequasar"],
        );
        assert_eq!(output.status, 78);
        assert!(output.stdout.contains("last-native"));
        assert!(output.stderr.contains(diagnostic));
        assert_eq!(fs::read(corrupt).unwrap(), b"not a SQLite database\n");
        assert!(!missing.exists());
    }

    #[test]
    fn real_767_capsule_retains_declared_grammar_and_unselected_primary() {
        let Some(dir) = fixture() else {
            return;
        };
        let first = seed(dir.path(), "space [one]*.db", "selected-one");
        let second = seed(dir.path(), "second.db", "selected-two");
        let backup = seed(dir.path(), "first_backup_20261003.db", "unselected-backup");
        let missing_primary = dir.path().join("unselected-primary.db");
        // The existing translation treats both colons and actual newlines as
        // separators. Quoted reads retain literal spaces/globs and duplicates.
        let set = format!(
            ":\n{}::\n{}:\n{}:\n",
            first.display(),
            second.display(),
            first.display()
        );
        let output = capsule(
            dir.path(),
            Some(&missing_primary),
            &["--all", "--set", &set, "capsulequasar"],
        );
        assert_eq!(output.status, 0);
        assert_eq!(
            output
                .stdout
                .matches(&format!("### {}\n", first.display()))
                .count(),
            2
        );
        assert_eq!(
            output
                .stdout
                .matches(&format!("### {}\n", second.display()))
                .count(),
            1
        );
        assert!(!output.stdout.contains("unselected-backup"));
        assert!(!output
            .stdout
            .contains(&format!("### {}\n", backup.display())));
        assert!(!missing_primary.exists());
        let single = capsule(dir.path(), Some(&missing_primary), &["capsulequasar"]);
        assert_eq!(single.status, 78);
        assert!(single
            .stderr
            .contains(&missing_primary.display().to_string()));
        let fallback =
            capsule_environment(dir.path(), None, Some(&first), None, &["capsulequasar"]);
        assert_eq!(fallback.status, 0);
        assert!(fallback.stdout.contains("selected-one"));
        let priority = capsule_environment(
            dir.path(),
            Some(&missing_primary),
            Some(&first),
            None,
            &["capsulequasar"],
        );
        assert_eq!(priority.status, 78);
        let environment_set = capsule_environment(
            dir.path(),
            Some(&missing_primary),
            None,
            Some(&set),
            &["--all", "capsulequasar"],
        );
        assert_json_return_eq(&environment_set, &output);
        // Genuine native queries establish that the comparison does not erase
        // the declared ordering or a repeated selected store.
        let reordered_set = format!(
            "{}:{}:{}",
            second.display(),
            first.display(),
            first.display()
        );
        let reordered = capsule(
            dir.path(),
            Some(&missing_primary),
            &["--all", "--set", &reordered_set, "capsulequasar"],
        );
        assert_eq!(reordered.status, 0);
        assert_eq!(reordered.stderr, output.stderr);
        assert_ne!(
            normalized_json_sections(&reordered.stdout),
            normalized_json_sections(&output.stdout)
        );
        let without_repeat = format!("{}:{}", first.display(), second.display());
        let without_repeat = capsule(
            dir.path(),
            Some(&missing_primary),
            &["--all", "--set", &without_repeat, "capsulequasar"],
        );
        assert_eq!(without_repeat.status, 0);
        assert_eq!(without_repeat.stderr, output.stderr);
        assert_ne!(
            normalized_json_sections(&without_repeat.stdout),
            normalized_json_sections(&output.stdout)
        );
        let only_second = second.display().to_string();
        let explicit_set = capsule_environment(
            dir.path(),
            Some(&missing_primary),
            None,
            Some(&set),
            &["--all", "--set", &only_second, "capsulequasar"],
        );
        assert_eq!(explicit_set.status, 0);
        assert!(explicit_set.stdout.contains("selected-two"));
        assert!(!explicit_set.stdout.contains("selected-one"));
        let unbound = capsule(dir.path(), None, &["--all", "--set", &set, "capsulequasar"]);
        assert_eq!(unbound.status, 78);
        let empty = capsule(
            dir.path(),
            Some(&missing_primary),
            &["--all", "--set", "::\n:\n", "capsulequasar"],
        );
        assert_eq!(empty.status, 0);
        assert!(empty.stdout.is_empty());
    }

    #[test]
    fn real_767_capsule_search_preserves_actual_database_and_sidecar_bytes() {
        let Some(dir) = fixture() else {
            return;
        };
        let db = seed(dir.path(), "readonly.db", "readonly-native");
        let before = database_bytes(&db);
        let output = capsule(dir.path(), Some(&db), &["capsulequasar"]);
        assert_eq!(output.status, 0);
        assert!(output.stdout.contains("readonly-native"));
        assert_eq!(
            database_bytes(&db),
            before,
            "a search must not silently mutate native database or sidecar bytes"
        );
    }

    /// The capsule over `primary`, with `env` set as well (the folder its private copy is made in, a PATH).
    fn capsule_in(
        root: &Path,
        primary: &Path,
        env: &[(&str, std::ffi::OsString)],
        args: &[&str],
    ) -> Output {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contrib/bkmr/capsules/script/search/project-text/payload/search-text.sh");
        let mut command = Command::new("/bin/sh");
        command
            .env_remove("AIKIT_BKMR_DB_SET")
            .env_remove("BKMR_DB_URL")
            .env("AIKIT_BKMR_DB", primary)
            .envs(env.iter().map(|(name, value)| (*name, value)))
            .arg(script)
            .args(args);
        capture(root, "authored capsule", &mut command)
    }

    /// Round 55 item 9 (DLW build ruling 2026-11-09): bkmr 7.x opens a database it searches for writing, and first switches it
    /// to WAL journal mode. A database kept in rollback-journal mode, as bkmr 6.x made them, is left as it was by a search, and
    /// the search's private copy is gone after it.
    #[test]
    fn real_767_capsule_search_leaves_a_rollback_journal_database_as_it_was() {
        let Some(dir) = fixture() else {
            return;
        };
        let db = seed(dir.path(), "rollback.db", "rollback-native");
        let connection = rusqlite::Connection::open(&db).unwrap();
        let mode: String = connection
            .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
        drop(connection);
        let copies = dir.path().join("copies");
        fs::create_dir(&copies).unwrap();
        let before = database_bytes(&db);
        assert_eq!(
            before[1..],
            [None, None, None],
            "the seed leaves no sidecar"
        );
        let output = capsule_in(
            dir.path(),
            &db,
            &[("TMPDIR", copies.clone().into())],
            &["capsulequasar"],
        );
        assert_eq!(output.status, 0);
        assert!(output.stdout.contains("rollback-native"));
        assert_eq!(
            database_bytes(&db),
            before,
            "a search must not switch the database's journal mode"
        );
        assert_eq!(fs::read_dir(&copies).unwrap().count(), 0, "no copy is left");
    }

    /// The same item: a search whose bkmr is killed before it closes the database leaves the bound database and its sidecars
    /// as they were. #611's CI failure had this shape: after a search an empty `-wal` and a `-shm` stood beside the unchanged
    /// database, where none stood before. A `bkmr` first on PATH runs the real one and kills it while the database it was given
    /// is open, so its close never runs. (A runner's timeout kills the whole search, its shell too. The folder that leaves is
    /// removed by a later search: `a_search_removes_this_users_own_copies_left_more_than_an_hour_ago`.)
    ///
    /// The real bkmr is held at its open database, not caught there (round 55 items 9c and 9d, DLW build ruling 2026-11-11).
    /// What it finds is larger than any pipe holds, and the shim gives it a pipe for its output and reads one byte of it: bkmr
    /// opens the database, finds the bookmarks and blocks writing them, with the database still open, for as long as the shim
    /// leaves it. The shim's wait is that one byte, so it waits on bkmr itself and on no file's coming and going: a `-shm` alone
    /// says too little, since bkmr was once seen to drop it and go on (a kill then came between). The shim kills bkmr and leaves
    /// its mark only when it read the byte, bkmr died of that kill, and the `-shm` stands beside the database afterwards. So
    /// the mark means: bkmr had searched, had the database open, and never closed it, since a bkmr that closes its database
    /// removes the `-shm`, and one that had already exited does not die of the kill.
    ///
    /// Before, the shim polled for the `-shm` of a bkmr that ran free. A small search opens and closes its database between two
    /// polls, and a shim that missed it could not tell that bkmr had already finished, so it polled on to its ten-second bound
    /// and the try was spent: five such misses failed the test on Linux twice in three runs. Nothing here waits on a moment that
    /// passes, so there is one try. A whole search still proves nothing: without the mark the test fails.
    #[test]
    fn real_767_capsule_search_cut_short_leaves_the_database_and_its_sidecars_as_they_were() {
        let Some(dir) = fixture() else {
            return;
        };
        // Six bookmarks of about 95 KB each, all found by the one query, under the capsule's limit of ten: bkmr's result is some
        // 570 KB, and a pipe holds 64 KB, so bkmr cannot finish writing it. Each body is one argument of the `bkmr add` that
        // seeds it, and Linux refuses an argument of 128 KB or more (9c's single 400 KB body failed there for that).
        // Each body differs by its label: bkmr keeps one bookmark for one content.
        let held = "held at its output ".repeat(5_000);
        let labels: Vec<String> = (1..=6).map(|index| format!("cut-native-{index}")).collect();
        let bodies: Vec<String> = labels
            .iter()
            .map(|label| format!("capsulequasar {label} native selected content {held}"))
            .collect();
        let items: Vec<(&str, &str)> = labels
            .iter()
            .zip(&bodies)
            .map(|(label, body)| (label.as_str(), body.as_str()))
            .collect();
        let db = seed_with(dir.path(), "cut.db", &items);
        let real = on_path("bkmr");
        let shim = dir.path().join("shim");
        // bkmr writes into a pipe the shim alone reads, one byte and no more, and keeps open, so bkmr's write blocks once the
        // pipe is full and is not refused. A bkmr that exits with nothing written ends the read at once. One that neither
        // writes nor exits is killed after ten seconds by the clock, so the test fails at its mark and not at the runner's
        // timeout.
        tool(
            &shim,
            "bkmr",
            "#!/bin/sh\nhold=\"$AIKIT_TEST_SHIM_CUT.hold\"\nmkfifo \"$hold\" || exit 97\n\
             \"$AIKIT_TEST_REAL_BKMR\" \"$@\" > \"$hold\" &\npid=$!\n\
             ( sleep 10; kill -KILL \"$pid\" ) < /dev/null > /dev/null 2>&1 &\ndog=$!\n\
             exec 3< \"$hold\"\ngot=$(dd bs=1 count=1 <&3 2>/dev/null | wc -c | tr -d ' ')\n\
             kill -KILL \"$pid\" 2>/dev/null\nwait \"$pid\"\nstatus=$?\nkill \"$dog\" 2>/dev/null\n\
             if [ \"$got\" -eq 1 ] && [ \"$status\" -eq 137 ] && [ -e \"$BKMR_DB_URL-shm\" ]; then\n\
             : > \"$AIKIT_TEST_SHIM_CUT\"\nfi\nexit \"$status\"\n",
        );
        let cut = dir.path().join("cut");
        let path = path_before(&[&shim]);
        let copies = dir.path().join("copies");
        fs::create_dir(&copies).unwrap();
        let before = database_bytes(&db);
        assert_eq!(
            before[1..],
            [None, None, None],
            "the seed leaves no sidecar"
        );
        let env = [
            ("PATH", path),
            ("AIKIT_TEST_REAL_BKMR", real.into()),
            ("AIKIT_TEST_SHIM_CUT", cut.clone().into()),
            ("TMPDIR", copies.clone().into()),
        ];
        let output = capsule_in(dir.path(), &db, &env, &["capsulequasar"]);
        assert!(
            cut.exists(),
            "the shim held bkmr at its open database and cut it short: {output:?}"
        );
        assert_eq!(
            (output.status, output.stdout.as_str()),
            (137, ""),
            "the search ends as its bkmr did, killed, and answers nothing"
        );
        assert_eq!(
            database_bytes(&db),
            before,
            "a search cut short must not leave sidecars beside the database"
        );
        assert_eq!(fs::read_dir(&copies).unwrap().count(), 0, "no copy is left");
    }

    /// The same item: the search's private copy is taken with the database's `-wal`, where a writer that has not checkpointed yet
    /// keeps what it committed. A bookmark changed by such a writer is found by its new words, and the database and its sidecars,
    /// the writer's own, are left as they were.
    #[test]
    fn real_767_capsule_search_reads_what_still_stands_in_the_wal() {
        let Some(dir) = fixture() else {
            return;
        };
        let db = seed(dir.path(), "wal.db", "wal-native");
        let writer = rusqlite::Connection::open(&db).unwrap();
        let mode: String = writer
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        // One small change stays under the automatic checkpoint's threshold, and the writer stays open, so it stays in the -wal.
        writer
            .execute(
                "UPDATE bookmarks SET \"desc\" = \"desc\" || ' walquasar'",
                [],
            )
            .unwrap();
        let before = database_bytes(&db);
        assert!(
            before[1].as_ref().is_some_and(|wal| !wal.is_empty()),
            "the change stands in the -wal"
        );
        let copies = dir.path().join("copies");
        fs::create_dir(&copies).unwrap();
        let output = capsule_in(
            dir.path(),
            &db,
            &[("TMPDIR", copies.clone().into())],
            &["walquasar"],
        );
        assert_eq!(output.status, 0);
        assert!(output.stdout.contains("wal-native"), "{}", output.stdout);
        assert_eq!(
            database_bytes(&db),
            before,
            "a search must not change the database or the writer's sidecars"
        );
        assert_eq!(fs::read_dir(&copies).unwrap().count(), 0, "no copy is left");
        drop(writer);
    }

    /// The real `name` on PATH, which a shim of the same name stands in front of.
    fn on_path(name: &str) -> PathBuf {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|folder| folder.join(name))
            .find(|tool| tool.is_file())
            .unwrap_or_else(|| panic!("the real {name} on PATH"))
    }

    /// An executable `name` in `folder`, holding `text`.
    fn tool(folder: &Path, name: &str, text: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(folder).unwrap();
        fs::write(folder.join(name), text).unwrap();
        fs::set_permissions(folder.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// PATH with `folders` first.
    fn path_before(folders: &[&Path]) -> std::ffi::OsString {
        let mut all: Vec<PathBuf> = folders.iter().map(|folder| folder.to_path_buf()).collect();
        all.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::join_paths(all).unwrap()
    }

    /// The names in `folder`, sorted.
    fn names(folder: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A `bkmr` first on PATH for the tests that need no real one: it keeps the `BKMR_DB_URL` it was given in
    /// `$AIKIT_TEST_SHIM_SEEN` and answers an empty search.
    const RECORDING_BKMR: &str =
        "#!/bin/sh\nprintf '%s\\n' \"$BKMR_DB_URL\" >> \"$AIKIT_TEST_SHIM_SEEN\"\nprintf '[]\\n'\n";

    /// A `cp` first on PATH. It copies as the real one does. After a copy of the watched database it changes that database as a
    /// writer would: it writes `$AIKIT_TEST_CP_NEXT` over it in place, or adds a byte to it where none is named. It does so after
    /// the first copy, or after every one when `$AIKIT_TEST_CP_CHANGE` is `every`, and counts the copies in `$AIKIT_TEST_CP_COUNT`.
    /// Where `$AIKIT_TEST_CP_TIME` names a file, it puts the watched file's modification time back after the change, as a
    /// change inside one tick of a coarse clock leaves it.
    const CHANGING_CP: &str = "#!/bin/sh\n\"$AIKIT_TEST_REAL_CP\" \"$@\" || exit \"$?\"\n\
        [ \"$1\" -ef \"$AIKIT_TEST_CP_WATCHED\" ] || exit 0\n\
        n=0\n[ ! -f \"$AIKIT_TEST_CP_COUNT\" ] || n=$(cat \"$AIKIT_TEST_CP_COUNT\")\nn=$((n + 1))\n\
        printf '%s\\n' \"$n\" > \"$AIKIT_TEST_CP_COUNT\"\n\
        [ \"$AIKIT_TEST_CP_CHANGE\" = every ] || [ \"$n\" -eq 1 ] || exit 0\n\
        [ -z \"${AIKIT_TEST_CP_TIME:-}\" ] || touch -r \"$1\" \"$AIKIT_TEST_CP_TIME\"\n\
        if [ -n \"${AIKIT_TEST_CP_NEXT:-}\" ]; then cat \"$AIKIT_TEST_CP_NEXT\" > \"$1\"; else printf x >> \"$1\"; fi\n\
        [ -z \"${AIKIT_TEST_CP_TIME:-}\" ] || touch -r \"$AIKIT_TEST_CP_TIME\" \"$1\"\n";

    /// A search the tests that need no real bkmr run: a bound database of a few bytes in a folder of its own, the folder TMPDIR
    /// names, and the recording `bkmr` first on PATH.
    struct Offline {
        dir: TempDir,
        db: PathBuf,
        copies: PathBuf,
        shims: PathBuf,
        seen: PathBuf,
    }

    impl Offline {
        fn new() -> Self {
            let dir = native_tempdir();
            let bound = dir.path().join("bound");
            fs::create_dir(&bound).unwrap();
            let db = bound.join("bound.db");
            fs::write(&db, b"state").unwrap();
            fs::create_dir(dir.path().join("copies")).unwrap();
            let copies = fs::canonicalize(dir.path().join("copies")).unwrap();
            let shims = dir.path().join("shims");
            tool(&shims, "bkmr", RECORDING_BKMR);
            let seen = dir.path().join("seen");
            Self {
                dir,
                db,
                copies,
                shims,
                seen,
            }
        }

        fn env(&self) -> Vec<(&'static str, std::ffi::OsString)> {
            vec![
                ("PATH", path_before(&[&self.shims])),
                ("TMPDIR", self.copies.clone().into()),
                ("AIKIT_TEST_SHIM_SEEN", self.seen.clone().into()),
            ]
        }

        fn search(&self, env: &[(&'static str, std::ffi::OsString)]) -> Output {
            capsule_in(self.dir.path(), &self.db, env, &["capsulequasar"])
        }
    }

    /// Round 55 item 9.1 (DLW build ruling 2026-11-10; the review of #613, F6): bkmr is handed a copy in a folder of the search's
    /// own under TMPDIR, never one beside the bound database, and nothing is written beside the database.
    #[test]
    fn the_capsule_hands_bkmr_a_copy_in_a_folder_of_its_own_under_tmpdir() {
        let run = Offline::new();
        let beside = run.db.parent().unwrap();
        let before = names(beside);
        let output = run.search(&run.env());
        assert_eq!(output.status, 0, "{output:?}");
        let seen = fs::read_to_string(&run.seen).unwrap();
        let url = Path::new(seen.trim_end());
        let folder = url.parent().unwrap();
        assert_eq!(
            folder.parent(),
            Some(run.copies.as_path()),
            "the copy lies in a folder under TMPDIR: {seen}"
        );
        assert!(
            folder
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("aikit-bkmr-search.")),
            "{seen}"
        );
        assert_eq!(
            names(beside),
            before,
            "nothing is written beside the database"
        );
        assert!(names(&run.copies).is_empty(), "no copy is left");
    }

    /// The same item (F1): a database that moves on every one of three copies is refused with status 75, in words naming it, and
    /// is never searched. The bound database is changed only by the test's own writer, and no copy is left.
    #[test]
    fn a_database_that_moves_on_every_copy_is_refused_and_never_searched() {
        let run = Offline::new();
        tool(&run.shims, "cp", CHANGING_CP);
        let count = run.dir.path().join("cp-count");
        let mut env = run.env();
        env.extend([
            ("AIKIT_TEST_REAL_CP", on_path("cp").into()),
            ("AIKIT_TEST_CP_WATCHED", run.db.clone().into()),
            ("AIKIT_TEST_CP_COUNT", count.clone().into()),
            ("AIKIT_TEST_CP_CHANGE", "every".into()),
        ]);
        let output = run.search(&env);
        assert_eq!(output.status, 75, "{output:?}");
        assert!(
            output.stderr.contains(&format!(
                "the database changed on every try while it was copied (3 tries): {}\n",
                run.db.display()
            )),
            "{}",
            output.stderr
        );
        assert_eq!(fs::read_to_string(&count).unwrap(), "3\n");
        assert_eq!(
            fs::read(&run.db).unwrap(),
            b"statexxx",
            "only the writer changed it"
        );
        assert_eq!(database_bytes(&run.db)[1..], [None, None, None]);
        assert!(!run.seen.exists(), "bkmr never ran");
        assert!(names(&run.copies).is_empty(), "no copy is left");
    }

    /// Round 55 item 9b, item 7 (DLW build ruling 2026-11-10; the re-check of 9.1, R1): a change that keeps the size, and the
    /// modification time, as a change inside one tick of a coarse clock does, is still seen, and the copy is taken again. A
    /// writer rewrites the database in place, or restarts its `-wal`, after the first copy, at the same size and with the time
    /// put back: the first 32 bytes, the header, moved, and the search reads the state after the change.
    #[test]
    fn a_change_that_keeps_the_size_and_the_time_is_seen_and_copied_again() {
        use std::os::unix::fs::MetadataExt;
        let rows: [(&str, &str, &[u8], &[u8]); 2] = [
            (
                "the database",
                "",
                b"SQLite format 3\0state one, page 1",
                b"SQLite format 3\0state two, page 1",
            ),
            (
                "the -wal",
                "-wal",
                b"WAL header, salt one: frame 1 and 2",
                b"WAL header, salt two: frame 1 and 2",
            ),
        ];
        for (row, suffix, first, next) in rows {
            let run = Offline::new();
            let watched = PathBuf::from(format!("{}{suffix}", run.db.display()));
            fs::write(&watched, first).unwrap();
            let next_file = run.dir.path().join("next");
            fs::write(&next_file, next).unwrap();
            let time = run.dir.path().join("time");
            let count = run.dir.path().join("cp-count");
            tool(&run.shims, "cp", CHANGING_CP);
            let before = fs::metadata(&watched).unwrap();
            let mut env = run.env();
            env.extend([
                ("AIKIT_TEST_REAL_CP", on_path("cp").into()),
                ("AIKIT_TEST_CP_WATCHED", watched.clone().into()),
                ("AIKIT_TEST_CP_COUNT", count.clone().into()),
                ("AIKIT_TEST_CP_NEXT", next_file.clone().into()),
                ("AIKIT_TEST_CP_CHANGE", "once".into()),
                ("AIKIT_TEST_CP_TIME", time.clone().into()),
            ]);
            let output = run.search(&env);
            assert_eq!(output.status, 0, "{row}: {output:?}");
            let after = fs::metadata(&watched).unwrap();
            assert_eq!(
                fs::read(&watched).unwrap(),
                next,
                "{row}: the writer changed it"
            );
            assert_eq!(
                (after.ino(), after.len(), after.modified().unwrap()),
                (before.ino(), before.len(), before.modified().unwrap()),
                "{row}: the change kept the inode, the size and the time"
            );
            assert_eq!(
                fs::read_to_string(&count).unwrap(),
                "2\n",
                "{row}: the change was seen and the copy taken again"
            );
            assert!(run.seen.exists(), "{row}: bkmr searched the copy");
            assert!(names(&run.copies).is_empty(), "{row}: no copy is left");
        }
    }

    /// The same item (3): a copy that cannot be made, looked at or read is refused with status 74, in words naming the database.
    #[test]
    fn a_copy_that_cannot_be_made_looked_at_or_read_is_refused_naming_the_database() {
        let refused = |run: &Offline, env: &[(&'static str, std::ffi::OsString)], words: &str| {
            let output = run.search(env);
            assert_eq!(output.status, 74, "{words}: {output:?}");
            assert!(
                output
                    .stderr
                    .contains(&format!("{words}: {}\n", run.db.display())),
                "{}",
                output.stderr
            );
            assert!(!run.seen.exists(), "{words}: bkmr never ran");
            assert!(names(&run.copies).is_empty(), "{words}: no copy is left");
        };
        let run = Offline::new();
        let mut env = run.env();
        env.retain(|(name, _)| *name != "TMPDIR");
        env.push(("TMPDIR", run.dir.path().join("missing").into()));
        refused(
            &run,
            &env,
            "could not make a private folder to search a copy of the database in",
        );
        let run = Offline::new();
        tool(&run.shims, "stat", "#!/bin/sh\nexit 1\n");
        refused(
            &run,
            &run.env(),
            "could not look at the database to tell whether it moved while it was copied",
        );
        let run = Offline::new();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&run.db, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&run.db).is_ok() {
            eprintln!("skipped the unreadable case: this user reads a file of mode 000");
        } else {
            refused(&run, &run.env(), "could not copy the database to search it");
        }
        fs::set_permissions(&run.db, fs::Permissions::from_mode(0o644)).unwrap();
    }

    /// The same item (4, the review's F3): a search stopped by SIGQUIT, or by SIGPIPE when what reads it has gone, removes its
    /// copy as it ends.
    #[test]
    fn a_search_stopped_by_quit_or_a_closed_pipe_removes_its_copy() {
        for (signal, status) in [("QUIT", 131), ("PIPE", 141)] {
            let run = Offline::new();
            tool(
                &run.shims,
                "bkmr",
                &format!("#!/bin/sh\nkill -{signal} \"$PPID\"\n"),
            );
            let output = run.search(&run.env());
            assert_eq!(output.status, status, "{signal}: {output:?}");
            assert!(names(&run.copies).is_empty(), "{signal}: no copy is left");
        }
    }

    /// The same item (4): a search killed outright, as the runner's timeout kills one (SIGKILL), cannot remove its folder. So a
    /// search first removes this user's own folders that are more than an hour old: an old one goes, and one half an hour old,
    /// a fresh one, a folder of another name and a file of the same name stay. Round 55 item 9b, item 8 (DLW build ruling
    /// 2026-11-10; the re-check of 9.1, R2): the age is an hour, not ten minutes, so a live search held up by a stalled reader
    /// keeps its folder; the runner stops a search after 30 s, so an hour still takes every folder a killed one left.
    #[test]
    fn a_search_removes_this_users_own_copies_left_more_than_an_hour_ago() {
        let run = Offline::new();
        let old = run.copies.join("aikit-bkmr-search.old123");
        let held = run.copies.join("aikit-bkmr-search.mid123");
        let fresh = run.copies.join("aikit-bkmr-search.new123");
        let other = run.copies.join("other.old123");
        let file = run.copies.join("aikit-bkmr-search.file12");
        for folder in [&old, &held, &fresh, &other] {
            fs::create_dir(folder).unwrap();
            fs::write(folder.join("search.db"), b"left").unwrap();
        }
        fs::write(&file, b"left").unwrap();
        let now = std::time::SystemTime::now();
        for (stale, ago) in [(&old, 7200), (&held, 1800), (&other, 7200), (&file, 7200)] {
            fs::File::open(stale)
                .unwrap()
                .set_modified(now - Duration::from_secs(ago))
                .unwrap();
        }
        let output = run.search(&run.env());
        assert_eq!(output.status, 0, "{output:?}");
        assert_eq!(
            names(&run.copies),
            [
                "aikit-bkmr-search.file12",
                "aikit-bkmr-search.mid123",
                "aikit-bkmr-search.new123",
                "other.old123"
            ]
        );
    }

    /// The same item (F1): a database that moved while it was copied is copied again, and the search reads it as it stands
    /// after the move: a bookmark the writer changed is found by its new words.
    #[test]
    fn real_767_capsule_search_copies_again_a_database_that_moved_while_it_was_copied() {
        let Some(dir) = fixture() else {
            return;
        };
        let db = seed(dir.path(), "moved.db", "moved-native");
        let next = dir.path().join("next.db");
        fs::copy(&db, &next).unwrap();
        let writer = rusqlite::Connection::open(&next).unwrap();
        writer
            .execute(
                "UPDATE bookmarks SET \"desc\" = \"desc\" || ' movedquasar'",
                [],
            )
            .unwrap();
        drop(writer);
        assert_eq!(database_bytes(&next)[1..], [None, None, None]);
        let shims = dir.path().join("shims");
        tool(&shims, "cp", CHANGING_CP);
        let count = dir.path().join("cp-count");
        let copies = dir.path().join("copies");
        fs::create_dir(&copies).unwrap();
        let output = capsule_in(
            dir.path(),
            &db,
            &[
                ("PATH", path_before(&[&shims])),
                ("AIKIT_TEST_REAL_CP", on_path("cp").into()),
                ("AIKIT_TEST_CP_WATCHED", db.clone().into()),
                ("AIKIT_TEST_CP_COUNT", count.clone().into()),
                ("AIKIT_TEST_CP_NEXT", next.clone().into()),
                ("AIKIT_TEST_CP_CHANGE", "once".into()),
                ("TMPDIR", copies.clone().into()),
            ],
            &["movedquasar"],
        );
        assert_eq!(output.status, 0);
        assert!(output.stdout.contains("moved-native"), "{}", output.stdout);
        assert_eq!(fs::read_to_string(&count).unwrap(), "2\n", "copied twice");
        assert_eq!(
            database_bytes(&db),
            database_bytes(&next),
            "only the writer changed it"
        );
        assert!(names(&copies).is_empty(), "no copy is left");
    }

    /// The same item (F1): a database kept in rollback-journal mode whose writer crashed mid-transaction keeps its `-journal`,
    /// and the half-written pages stand in the database file. The copy takes the journal too, so bkmr rolls the transaction back
    /// in the copy, as it would have in the database: the search sees none of the half-written rows, and the database and its
    /// journal are left as they were.
    #[test]
    fn real_767_capsule_search_rolls_back_a_crashed_writers_journal_in_its_copy() {
        let Some(dir) = fixture() else {
            return;
        };
        let live = seed(dir.path(), "live.db", "crashed-native");
        let connection = rusqlite::Connection::open(&live).unwrap();
        let mode: String = connection
            .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
        connection
            .execute_batch("CREATE TABLE ballast(b BLOB)")
            .unwrap();
        // A cache of two pages spills the changed bookmark to the database file before the ballast is written, as a crash
        // mid-transaction leaves it; the -journal keeps the pages as they were.
        connection
            .execute_batch(
                "PRAGMA cache_size = 2; BEGIN; \
                 UPDATE bookmarks SET \"desc\" = \"desc\" || ' halfquasar'; \
                 INSERT INTO ballast VALUES (randomblob(4000000));",
            )
            .unwrap();
        let db = dir.path().join("crashed.db");
        fs::copy(&live, &db).unwrap();
        fs::copy(
            format!("{}-journal", live.display()),
            format!("{}-journal", db.display()),
        )
        .unwrap();
        drop(connection);
        let bare = dir.path().join("bare.db");
        fs::copy(&db, &bare).unwrap();
        let half: i64 = rusqlite::Connection::open(&bare)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM bookmarks WHERE \"desc\" LIKE '%halfquasar%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            half, 1,
            "the half-written bookmark stands in the database file"
        );
        let before = database_bytes(&db);
        assert!(before[3].is_some(), "the crashed writer's -journal stands");
        let copies = dir.path().join("copies");
        fs::create_dir(&copies).unwrap();
        let output = capsule_in(
            dir.path(),
            &db,
            &[("TMPDIR", copies.clone().into())],
            &["capsulequasar"],
        );
        assert_eq!(output.status, 0, "{output:?}");
        assert!(
            output.stdout.contains("crashed-native"),
            "{}",
            output.stdout
        );
        assert!(!output.stdout.contains("halfquasar"), "{}", output.stdout);
        assert_eq!(
            database_bytes(&db),
            before,
            "a search must not roll back the database itself"
        );
        assert!(names(&copies).is_empty(), "no copy is left");
    }

    /// The same item (F2): SQLite keeps a database's `-wal` beside the file its path leads to, every link followed. A search of a
    /// database bound through two links, one relative and one absolute, reads the `-wal` beside the real file, where a writer
    /// keeps a commit it has not checkpointed, and finds the bookmark by its new words.
    #[test]
    fn real_767_capsule_search_through_links_reads_the_wal_beside_the_real_file() {
        let Some(dir) = fixture() else {
            return;
        };
        for folder in ["real", "middle", "link"] {
            fs::create_dir(dir.path().join(folder)).unwrap();
        }
        let db = seed(dir.path(), "real/linked.db", "linked-native");
        let writer = rusqlite::Connection::open(&db).unwrap();
        let mode: String = writer
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        writer
            .execute(
                "UPDATE bookmarks SET \"desc\" = \"desc\" || ' linkquasar'",
                [],
            )
            .unwrap();
        let middle = dir.path().join("middle/linked.db");
        std::os::unix::fs::symlink("../real/linked.db", &middle).unwrap();
        let bound = dir.path().join("link/linked.db");
        std::os::unix::fs::symlink(&middle, &bound).unwrap();
        let before = database_bytes(&db);
        assert!(
            before[1].as_ref().is_some_and(|wal| !wal.is_empty()),
            "the change stands in the -wal"
        );
        let copies = dir.path().join("copies");
        fs::create_dir(&copies).unwrap();
        let output = capsule_in(
            dir.path(),
            &bound,
            &[("TMPDIR", copies.clone().into())],
            &["linkquasar"],
        );
        assert_eq!(output.status, 0);
        assert!(output.stdout.contains("linked-native"), "{}", output.stdout);
        assert_eq!(database_bytes(&db), before);
        assert_eq!(names(&dir.path().join("link")), ["linked.db"]);
        assert!(names(&copies).is_empty(), "no copy is left");
        drop(writer);
    }

    /// What a search says, once, when bkmr migrated its copy of `db` (round 55 item 9b, the review of #613's F5).
    pub(super) fn migrated_words(db: &Path) -> String {
        format!(
            "bkmr migrated the private copy this search made of {db}, which is kept in an older schema. The backup bkmr names \
             was made of that copy and is removed with it. Each search migrates its copy again until the database itself is \
             migrated, once: bkmr --db {db} search --np --limit 1 \"\"\n",
            db = db.display()
        )
    }

    /// Round 55 item 9b (DLW build ruling 2026-11-10; the review of #613, F5): bkmr migrates the search's copy of a database kept
    /// in an older schema, never the database, and its words name a backup it made of that copy, which is gone when the search
    /// ends. The capsule says so, once, naming the database and how to migrate the database itself, and says nothing of it for
    /// a database that needed no migration, though both are searched in one run. The databases and their folder are as they were.
    #[test]
    fn real_767_capsule_search_of_an_older_schema_says_once_that_it_migrated_its_copy() {
        let Some(dir) = fixture() else {
            return;
        };
        let folder = dir.path().join("older");
        fs::create_dir(&folder).unwrap();
        let older = seed(dir.path(), "older/older.db", "older-native");
        let current = seed(dir.path(), "older/current.db", "current-native");
        // Revert the 2026-04-03 migration, as bkmr_store_read_only.rs does, so bkmr 7.6.7 migrates the database when it opens it.
        rusqlite::Connection::open(&older)
            .unwrap()
            .execute_batch(
                "PRAGMA journal_mode=DELETE; ALTER TABLE bookmarks DROP COLUMN accessed_at;
                DELETE FROM __diesel_schema_migrations WHERE version >= '20260403100000';
                CREATE TRIGGER UpdateLastTime AFTER UPDATE ON bookmarks FOR EACH ROW
                  WHEN NEW.last_update_ts <= OLD.last_update_ts BEGIN
                  UPDATE bookmarks SET last_update_ts=CURRENT_TIMESTAMP WHERE id=OLD.id; END;",
            )
            .unwrap();
        let beside = names(&folder);
        let before = (database_bytes(&older), database_bytes(&current));
        let copies = dir.path().join("copies");
        fs::create_dir(&copies).unwrap();
        let set = format!("{}:{}", older.display(), current.display());
        let output = capsule_in(
            dir.path(),
            &older,
            &[("TMPDIR", copies.clone().into())],
            &["--all", "--set", &set, "capsulequasar"],
        );
        assert_eq!(output.status, 0, "{output:?}");
        assert!(output.stdout.contains("older-native") && output.stdout.contains("current-native"));
        assert!(
            output.stderr.contains("Backup created at"),
            "bkmr migrated the copy: {}",
            output.stderr
        );
        assert_eq!(
            output.stderr.matches(&migrated_words(&older)).count(),
            1,
            "{}",
            output.stderr
        );
        assert!(
            !output.stderr.contains(&migrated_words(&current)),
            "{}",
            output.stderr
        );
        assert_eq!((database_bytes(&older), database_bytes(&current)), before);
        assert_eq!(
            names(&folder),
            beside,
            "no backup is made beside the database"
        );
        assert!(names(&copies).is_empty(), "no copy is left");
    }
}

/// Round 55 item 9b (DLW build ruling 2026-11-10): the semantic capsule and the bkmr doctor read a private copy of the bound
/// database, as the text capsule does (item 9). bkmr is never given the bound database, and neither is sqlite3: bkmr 7.x leaves
/// its databases in WAL mode, which `sqlite3 -readonly` cannot open once their `-shm` is gone, and a reader beside a live writer
/// rewrites the `-shm`.
mod project_semantic_capsule {
    use super::project_text_capsule::{capture, database_bytes, fixture, migrated_words, runner};
    use super::*;
    use aikit_adapters::runner::Output;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const TEXT: &str = "script/search/project-text/payload/search-text.sh";
    const SEMANTIC: &str = "script/search/project-semantic/payload/search-semantic.sh";
    const DOCTOR: &str = "tool/search/bkmr/payload/doctor.sh";

    fn payload(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contrib/bkmr/capsules")
            .join(relative)
    }

    /// The payload `script`, run by /bin/sh over `primary`, with `env` set as well.
    fn run(
        root: &Path,
        script: &str,
        primary: &Path,
        env: &[(&str, OsString)],
        args: &[&str],
    ) -> Output {
        let mut command = Command::new("/bin/sh");
        command
            .env_remove("AIKIT_BKMR_DB_SET")
            .env_remove("BKMR_DB_URL")
            .env_remove("FASTEMBED_CACHE_DIR")
            .env("AIKIT_BKMR_DB", primary)
            .envs(env.iter().map(|(name, value)| (*name, value)))
            .arg(payload(script))
            .args(args);
        capture(root, script, &mut command)
    }

    /// A `bkmr` first on PATH. It answers `--version` as 7.6.7. For any other command it keeps, in a folder of its own under
    /// `$AIKIT_TEST_SHIM_SEEN`, the BKMR_DB_URL it was given, the names beside it and a copy of that database (its `-wal` too);
    /// then it makes the `-wal` and `-shm` bkmr 7.x makes beside a database it opens, and answers an empty result. Where
    /// `$AIKIT_TEST_SHIM_BACKUP` is set it also makes, and names, the backup bkmr makes beside a database it migrates.
    const SHIM: &str = "#!/bin/sh\n\
        [ \"$1\" != --version ] || { echo 'bkmr 7.6.7'; exit 0; }\n\
        seen=$(mktemp -d \"$AIKIT_TEST_SHIM_SEEN/run.XXXXXX\")\n\
        printf '%s\\n' \"$BKMR_DB_URL\" > \"$seen/url\"\n\
        ls -a \"$(dirname \"$BKMR_DB_URL\")\" > \"$seen/beside\"\n\
        cp \"$BKMR_DB_URL\" \"$seen/seen.db\"\n\
        [ ! -f \"$BKMR_DB_URL-wal\" ] || cp \"$BKMR_DB_URL-wal\" \"$seen/seen.db-wal\"\n\
        [ -e \"$BKMR_DB_URL-wal\" ] || : > \"$BKMR_DB_URL-wal\"\n\
        [ -e \"$BKMR_DB_URL-shm\" ] || : > \"$BKMR_DB_URL-shm\"\n\
        if [ -n \"${AIKIT_TEST_SHIM_BACKUP:-}\" ]; then\n\
        backup=$(dirname \"$BKMR_DB_URL\")/search_backup_20261110.db\n\
        : > \"$backup\"\nprintf 'Backup created at: %s\\n' \"$backup\" >&2\nfi\n\
        printf '[]\\n'\n";

    /// The [`SHIM`] in `root`: the PATH that finds it first, and the folder it keeps what it saw in.
    fn shim(root: &Path) -> (OsString, PathBuf) {
        let folder = root.join("shim");
        let seen = root.join("seen");
        fs::create_dir_all(&folder).unwrap();
        fs::create_dir_all(&seen).unwrap();
        fs::write(folder.join("bkmr"), SHIM).unwrap();
        fs::set_permissions(folder.join("bkmr"), fs::Permissions::from_mode(0o755)).unwrap();
        let mut path = OsString::from(&folder);
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        (path, seen)
    }

    /// What the [`SHIM`] saw at its one run: the database it was given, the names beside that database, and the rows of
    /// `vec_bookmarks` in it.
    fn seen_once(seen: &Path) -> (PathBuf, Vec<String>, i64) {
        let runs: Vec<_> = fs::read_dir(seen)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(runs.len(), 1, "bkmr ran once: {runs:?}");
        let run = &runs[0];
        let url = PathBuf::from(fs::read_to_string(run.join("url")).unwrap().trim_end());
        let beside = fs::read_to_string(run.join("beside"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        let rows = rusqlite::Connection::open(run.join("seen.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM vec_bookmarks", [], |row| row.get(0))
            .unwrap();
        fs::remove_dir_all(run).unwrap();
        (url, beside, rows)
    }

    /// The states a bound database is found in: bkmr 7.x's own once it has closed (WAL mode, no sidecar), bkmr 6.x's (rollback
    /// journal), and a live writer's (WAL, with what it committed still in the `-wal`).
    const CLEAN_WAL: &str = "a clean WAL database";
    const ROLLBACK: &str = "a rollback-journal database";
    const WRITER: &str = "a writer's change in the -wal";

    /// A database at `root/name` with a `vec_bookmarks` table of one row, in `state`. For [`WRITER`] a second row is committed by
    /// a writer that is kept open, and answered, so that its change stays in the `-wal`.
    fn vector_database(
        root: &Path,
        name: &str,
        state: &str,
    ) -> (PathBuf, Option<rusqlite::Connection>) {
        let db = root.join(name);
        let connection = rusqlite::Connection::open(&db).unwrap();
        let mode = if state == ROLLBACK { "delete" } else { "wal" };
        let set: String = connection
            .query_row(&format!("PRAGMA journal_mode = {mode}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(set, mode);
        connection
            .execute_batch(
                "CREATE TABLE vec_bookmarks (id INTEGER); INSERT INTO vec_bookmarks VALUES (1);",
            )
            .unwrap();
        if state == WRITER {
            connection
                .execute("INSERT INTO vec_bookmarks VALUES (2)", [])
                .unwrap();
            return (db, Some(connection));
        }
        drop(connection);
        (db, None)
    }

    /// Round 55 item 9b: in either mode and whatever state the bound database is in, the semantic capsule hands bkmr a private
    /// copy of it, holding what its `-wal` holds, and checks for vectors in that copy. The database and its sidecars are as they
    /// were after, and no copy is left. Before 9b the capsule gave bkmr the bound database, and its `sqlite3 -readonly` check
    /// refused a clean WAL database outright, as one with no vectors.
    #[test]
    fn the_semantic_capsule_hands_bkmr_a_private_copy_of_the_bound_database() {
        for state in [CLEAN_WAL, ROLLBACK, WRITER] {
            for mode in ["hybrid", "semantic"] {
                let case = format!("{state}, {mode}");
                let dir = native_tempdir();
                let (db, writer) = vector_database(dir.path(), "bound.db", state);
                if state == CLEAN_WAL {
                    assert_eq!(
                        database_bytes(&db)[1..],
                        [None, None, None],
                        "{case}: no sidecar"
                    );
                }
                let (path, seen) = shim(dir.path());
                let copies = dir.path().join("copies");
                fs::create_dir(&copies).unwrap();
                let before = database_bytes(&db);
                let output = run(
                    dir.path(),
                    SEMANTIC,
                    &db,
                    &[
                        ("PATH", path),
                        ("TMPDIR", copies.clone().into()),
                        ("AIKIT_TEST_SHIM_SEEN", seen.clone().into()),
                    ],
                    &["--mode", mode, "lighthouse"],
                );
                assert_eq!(output.status, 0, "{case}: {}", output.stderr);
                assert!(
                    !output.stderr.contains("bkmr migrated"),
                    "{case}: {}",
                    output.stderr
                );
                let (url, beside, rows) = seen_once(&seen);
                assert_ne!(url, db, "{case}: bkmr is never given the bound database");
                assert!(url.starts_with(&copies), "{case}: {url:?}");
                assert!(
                    beside.iter().any(|name| name == "search.db"),
                    "{case}: {beside:?}"
                );
                let committed = if writer.is_some() { 2 } else { 1 };
                assert_eq!(
                    rows, committed,
                    "{case}: the copy holds what the -wal holds"
                );
                assert_eq!(
                    database_bytes(&db),
                    before,
                    "{case}: the database and its sidecars are as they were"
                );
                assert_eq!(
                    fs::read_dir(&copies).unwrap().count(),
                    0,
                    "{case}: no copy is left"
                );
                drop(writer);
            }
        }
    }

    /// The same item: a bound database the capsule cannot copy is refused with status 74, in words that name it, and bkmr never
    /// runs. Before 9b its `sqlite3 -readonly` check failed first, and the capsule said the database had no vectors (78).
    #[test]
    fn a_bound_database_the_semantic_capsule_cannot_copy_is_refused_naming_it() {
        let dir = native_tempdir();
        let (db, _) = vector_database(dir.path(), "unreadable.db", ROLLBACK);
        fs::set_permissions(&db, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&db).is_ok() {
            eprintln!("skipped: this process reads a file with no permissions (a superuser)");
            return;
        }
        let (path, seen) = shim(dir.path());
        let copies = dir.path().join("copies");
        fs::create_dir(&copies).unwrap();
        let output = run(
            dir.path(),
            SEMANTIC,
            &db,
            &[
                ("PATH", path),
                ("TMPDIR", copies.clone().into()),
                ("AIKIT_TEST_SHIM_SEEN", seen.clone().into()),
            ],
            &["lighthouse"],
        );
        fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(output.status, 74, "{}", output.stderr);
        assert!(
            output.stderr.contains(&format!(
                "could not copy the database to search it: {}",
                db.display()
            )),
            "{}",
            output.stderr
        );
        assert_eq!(fs::read_dir(&seen).unwrap().count(), 0, "bkmr never ran");
        assert_eq!(fs::read_dir(&copies).unwrap().count(), 0, "no copy is left");
    }

    /// The same item: the bkmr doctor counts a clean WAL database from a private copy, so it reports its rows and its vectors and
    /// warns of nothing, and leaves the database as it was. Before 9b its `sqlite3 -readonly` reads could not open the database:
    /// it printed rows "?" and warned that the database was not migrated.
    #[test]
    fn the_doctor_reports_a_clean_wal_database_from_a_private_copy() {
        if Command::new("sqlite3").arg("--version").output().is_err() {
            eprintln!("skipped: no sqlite3 on PATH, so the doctor counts nothing");
            return;
        }
        let dir = native_tempdir();
        let db = dir.path().join("doctor.db");
        let connection = rusqlite::Connection::open(&db).unwrap();
        let set: String = connection
            .query_row("PRAGMA journal_mode = wal", [], |row| row.get(0))
            .unwrap();
        assert_eq!(set, "wal");
        connection
            .execute_batch(
                "CREATE TABLE bookmarks (id INTEGER PRIMARY KEY, embedding BLOB);
                 INSERT INTO bookmarks (embedding) VALUES (NULL), (NULL), (NULL);
                 CREATE TABLE vec_bookmarks (id INTEGER);",
            )
            .unwrap();
        drop(connection);
        let before = database_bytes(&db);
        assert_eq!(before[1..], [None, None, None], "no sidecar");
        let (path, seen) = shim(dir.path());
        let copies = dir.path().join("copies");
        fs::create_dir(&copies).unwrap();
        let output = run(
            dir.path(),
            DOCTOR,
            &db,
            &[
                ("PATH", path),
                ("TMPDIR", copies.clone().into()),
                ("AIKIT_TEST_SHIM_SEEN", seen.clone().into()),
            ],
            &[],
        );
        assert_eq!(output.status, 0, "{}", output.stderr);
        assert!(
            output.stdout.contains("rows          : 3\n"),
            "{}",
            output.stdout
        );
        assert!(
            output
                .stdout
                .contains("embeddings    : vec_bookmarks present\n"),
            "{}",
            output.stdout
        );
        assert_eq!(output.stderr, "", "the doctor warns of nothing");
        assert_eq!(database_bytes(&db), before, "the database is as it was");
        assert_eq!(fs::read_dir(&copies).unwrap().count(), 0, "no copy is left");
        assert_eq!(
            fs::read_dir(&seen).unwrap().count(),
            0,
            "bkmr only told its version"
        );
    }

    /// The same item: the three payloads hold one private copy, word for word. A capsule ships only its own payload, so they
    /// cannot share one file.
    #[test]
    fn the_text_and_semantic_capsules_and_the_doctor_hold_one_private_copy() {
        let block = |relative: &str| {
            let text = fs::read_to_string(payload(relative)).unwrap();
            let start = text
                .find("# --- the private copy:")
                .unwrap_or_else(|| panic!("{relative} holds the private copy"));
            let end = text[start..]
                .find("# --- end of the private copy ---")
                .unwrap_or_else(|| panic!("{relative} ends the private copy"));
            text[start..start + end].to_owned()
        };
        let text = block(TEXT);
        assert_eq!(block(SEMANTIC), text, "{SEMANTIC}");
        assert_eq!(block(DOCTOR), text, "{DOCTOR}");
    }

    /// The model cache to look in: `AIKIT_TEST_BKMR_MODEL_CACHE` where a run names one (so no bkmr the tests start inherits a
    /// pointer to it), else bkmr 7.6.7's own: `FASTEMBED_CACHE_DIR`, else the platform's cache folder's `bkmr/models` (its
    /// `FastEmbedProvider::cache_dir`, through `dirs::cache_dir`).
    fn model_cache() -> Option<PathBuf> {
        for named in ["AIKIT_TEST_BKMR_MODEL_CACHE", "FASTEMBED_CACHE_DIR"] {
            if let Some(folder) = std::env::var_os(named) {
                return Some(folder.into());
            }
        }
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let cache = if cfg!(target_os = "macos") {
            home.join("Library/Caches")
        } else {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cache"))
        };
        Some(cache.join("bkmr/models"))
    }

    /// A model bkmr can load from `cache` with no network: its bkmr name and its folder, when every file fastembed 5.17.2 fetches
    /// for it (hf-hub 0.5.0 answers a cached file without the network) stands in the snapshot its `refs/main` names.
    fn cached_model(cache: &Path) -> Option<(&'static str, PathBuf)> {
        const MODELS: [(&str, &str, &str); 3] = [
            (
                "NomicEmbedTextV15",
                "models--nomic-ai--nomic-embed-text-v1.5",
                "onnx/model.onnx",
            ),
            (
                "BGESmallENV15",
                "models--Xenova--bge-small-en-v1.5",
                "onnx/model.onnx",
            ),
            (
                "AllMiniLML6V2",
                "models--Qdrant--all-MiniLM-L6-v2-onnx",
                "model.onnx",
            ),
        ];
        MODELS.iter().find_map(|(name, folder, model)| {
            let folder = cache.join(folder);
            let commit = fs::read_to_string(folder.join("refs/main")).ok()?;
            let snapshot = folder.join("snapshots").join(commit.trim());
            [
                *model,
                "tokenizer.json",
                "config.json",
                "special_tokens_map.json",
                "tokenizer_config.json",
            ]
            .iter()
            .all(|file| snapshot.join(file).is_file())
            .then_some((*name, folder))
        })
    }

    /// `from` made again at `to`: folders and links as they are, each file a copy (a clone on APFS).
    fn clone_tree(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            let target = to.join(entry.file_name());
            if kind.is_symlink() {
                std::os::unix::fs::symlink(fs::read_link(entry.path()).unwrap(), &target).unwrap();
            } else if kind.is_dir() {
                clone_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    /// The same item, on real bkmr 7.6.7 and a real model, where one is already cached: a database embedded on that model is
    /// found by meaning in both modes, and is left as it was, with no copy left. The cached model is cloned into the test's own
    /// folder and bkmr is pointed at the clone (`FASTEMBED_CACHE_DIR`, and the model pinned in the test HOME's config), so the
    /// cache is only read. Without a cached model the test skips, naming why; it never downloads one.
    #[test]
    fn real_767_semantic_capsule_search_on_a_cached_model_leaves_the_database_as_it_was() {
        let Some(cache) = model_cache() else {
            eprintln!("skipped: no HOME, so no bkmr model cache to look in");
            return;
        };
        let Some((model, folder)) = cached_model(&cache) else {
            eprintln!(
                "skipped: no bkmr model is cached whole in {} (bkmr 7.6.7 looks in FASTEMBED_CACHE_DIR, else the platform cache \
                 folder's bkmr/models), and this test never downloads one",
                cache.display()
            );
            return;
        };
        let Some(dir) = fixture() else {
            return;
        };
        let models = dir.path().join("models");
        clone_tree(&folder, &models.join(folder.file_name().unwrap()));
        fs::create_dir_all(dir.path().join(".config/bkmr")).unwrap();
        fs::write(
            dir.path().join(".config/bkmr/config.toml"),
            format!("[embeddings]\nmodel = \"{model}\"\n"),
        )
        .unwrap();
        let db = dir.path().join("semantic.db");
        let item = source(
            "source:capsule:semantic-native",
            "semantic-native",
            "a lighthouse keeper tends the lamp through the storm",
            &["capsule-proof"],
            SourceVisibility::Team,
            &[],
        );
        let mut provider = BkmrSourcePoolProvider::new(
            runner(dir.path()).with_env("FASTEMBED_CACHE_DIR", models.to_string_lossy()),
            &db,
            true,
        );
        provider
            .rebuild(&[item])
            .expect("real native database seed, embedded on the cached model");
        let before = database_bytes(&db);
        assert_eq!(
            before[1..],
            [None, None, None],
            "the seed leaves no sidecar"
        );
        let copies = dir.path().join("copies");
        fs::create_dir(&copies).unwrap();
        for mode in ["hybrid", "semantic"] {
            let output = run(
                dir.path(),
                SEMANTIC,
                &db,
                &[
                    ("TMPDIR", copies.clone().into()),
                    ("FASTEMBED_CACHE_DIR", models.clone().into()),
                ],
                &["--mode", mode, "lighthouse"],
            );
            assert_eq!(output.status, 0, "{mode} on {model}: {}", output.stderr);
            assert!(
                format!("{}{}", output.stdout, output.stderr).contains("semantic-native"),
                "{mode}: {}",
                output.stdout
            );
            assert!(
                !output.stderr.contains("Downloading embedding model"),
                "{mode}: {}",
                output.stderr
            );
            assert_eq!(
                database_bytes(&db),
                before,
                "{mode}: the database is as it was"
            );
            assert_eq!(
                fs::read_dir(&copies).unwrap().count(),
                0,
                "{mode}: no copy is left"
            );
        }
    }

    /// The same item (F5): the semantic capsule says, once, that bkmr migrated its copy, in either mode.
    #[test]
    fn the_semantic_capsule_says_once_that_bkmr_migrated_its_copy() {
        for mode in ["hybrid", "semantic"] {
            let dir = native_tempdir();
            let (db, _) = vector_database(dir.path(), "older.db", CLEAN_WAL);
            let (path, seen) = shim(dir.path());
            let copies = dir.path().join("copies");
            fs::create_dir(&copies).unwrap();
            let output = run(
                dir.path(),
                SEMANTIC,
                &db,
                &[
                    ("PATH", path),
                    ("TMPDIR", copies.clone().into()),
                    ("AIKIT_TEST_SHIM_SEEN", seen.clone().into()),
                    ("AIKIT_TEST_SHIM_BACKUP", "1".into()),
                ],
                &["--mode", mode, "lighthouse"],
            );
            assert_eq!(output.status, 0, "{mode}: {}", output.stderr);
            assert_eq!(
                output.stderr.matches(&migrated_words(&db)).count(),
                1,
                "{mode}: {}",
                output.stderr
            );
            assert_eq!(
                fs::read_dir(&copies).unwrap().count(),
                0,
                "{mode}: no copy is left"
            );
        }
    }
}
