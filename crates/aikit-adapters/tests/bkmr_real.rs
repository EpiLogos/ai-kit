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
        let path = root.join(name);
        let item = source(
            &format!("source:capsule:{label}"),
            label,
            &format!("capsulequasar {label} native selected content"),
            &["capsule-proof"],
            SourceVisibility::Team,
            &[],
        );
        let mut provider = BkmrSourcePoolProvider::new(runner(root), &path, false);
        provider
            .rebuild(&[item])
            .expect("real controlled native database seed");
        path
    }

    fn capture(root: &Path, label: &str, command: &mut Command) -> Output {
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

    fn database_bytes(path: &Path) -> Vec<Option<Vec<u8>>> {
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
    /// database, where none stood before. A `bkmr` first on PATH runs the real one and kills it once the database it was given
    /// is open, so its close never runs. (A runner's timeout kills the whole search, its shell too. The folder that leaves is
    /// removed by a later search: `a_search_removes_this_users_own_copies_left_more_than_ten_minutes_ago`.)
    #[test]
    fn real_767_capsule_search_cut_short_leaves_the_database_and_its_sidecars_as_they_were() {
        let Some(dir) = fixture() else {
            return;
        };
        let db = seed(dir.path(), "cut.db", "cut-native");
        let real = on_path("bkmr");
        let shim = dir.path().join("shim");
        // The wait is bounded by time, ten seconds by the clock, and the shim leaves a mark when it saw the database open and
        // cut bkmr short (round 55 item 9.1, the review's F7). bkmr holds the copy open only briefly, and the shim's poll can
        // miss it (once in 30 runs here), so a bkmr that finished first is searched again, five tries in all. A whole search
        // proves nothing here; a wait that ran out, or five bkmrs that all finished first, fail the test.
        tool(
            &shim,
            "bkmr",
            "#!/bin/sh\n\"$AIKIT_TEST_REAL_BKMR\" \"$@\" &\npid=$!\nend=$(($(date +%s) + 10))\n\
             while [ ! -e \"$BKMR_DB_URL-shm\" ] && [ \"$(date +%s)\" -lt \"$end\" ]; do\n\
             n=0\nwhile [ ! -e \"$BKMR_DB_URL-shm\" ] && [ \"$n\" -lt 1000 ]; do n=$((n + 1)); done\ndone\n\
             if [ -e \"$BKMR_DB_URL-shm\" ] && kill -KILL \"$pid\" 2>/dev/null; then : > \"$AIKIT_TEST_SHIM_CUT\"; fi\n\
             wait \"$pid\"\n",
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
        let mut tries = 0;
        let output = loop {
            tries += 1;
            let output = capsule_in(dir.path(), &db, &env, &["capsulequasar"]);
            if cut.exists() || tries == 5 {
                break output;
            }
            assert_eq!(
                output.status, 0,
                "a bkmr not cut short searched whole: {output:?}"
            );
            assert_eq!(
                database_bytes(&db),
                before,
                "a whole search leaves the database as it was"
            );
        };
        assert!(
            cut.exists(),
            "the shim saw bkmr open the database and cut it short, in {tries} tries"
        );
        if tries > 1 {
            eprintln!("the shim cut bkmr short on try {tries}");
        }
        assert_ne!(output.status, 0, "the search was cut short");
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
    const CHANGING_CP: &str = "#!/bin/sh\n\"$AIKIT_TEST_REAL_CP\" \"$@\" || exit \"$?\"\n\
        [ \"$1\" -ef \"$AIKIT_TEST_CP_WATCHED\" ] || exit 0\n\
        n=0\n[ ! -f \"$AIKIT_TEST_CP_COUNT\" ] || n=$(cat \"$AIKIT_TEST_CP_COUNT\")\nn=$((n + 1))\n\
        printf '%s\\n' \"$n\" > \"$AIKIT_TEST_CP_COUNT\"\n\
        [ \"$AIKIT_TEST_CP_CHANGE\" = every ] || [ \"$n\" -eq 1 ] || exit 0\n\
        if [ -n \"${AIKIT_TEST_CP_NEXT:-}\" ]; then cat \"$AIKIT_TEST_CP_NEXT\" > \"$1\"; else printf x >> \"$1\"; fi\n";

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
    /// search first removes this user's own folders that are more than ten minutes old: an old one goes, and a fresh one, a
    /// folder of another name and a file of the same name stay.
    #[test]
    fn a_search_removes_this_users_own_copies_left_more_than_ten_minutes_ago() {
        let run = Offline::new();
        let old = run.copies.join("aikit-bkmr-search.old123");
        let fresh = run.copies.join("aikit-bkmr-search.new123");
        let other = run.copies.join("other.old123");
        let file = run.copies.join("aikit-bkmr-search.file12");
        for folder in [&old, &fresh, &other] {
            fs::create_dir(folder).unwrap();
            fs::write(folder.join("search.db"), b"left").unwrap();
        }
        fs::write(&file, b"left").unwrap();
        let hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
        for stale in [&old, &other, &file] {
            fs::File::open(stale)
                .unwrap()
                .set_modified(hour_ago)
                .unwrap();
        }
        let output = run.search(&run.env());
        assert_eq!(output.status, 0, "{output:?}");
        assert_eq!(
            names(&run.copies),
            [
                "aikit-bkmr-search.file12",
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
}
