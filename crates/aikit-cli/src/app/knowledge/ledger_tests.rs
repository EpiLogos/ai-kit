//! The discovery ledger against a real filesystem: a kept walk answers the
//! next discovery exactly while nothing it observed changed, and any change
//! it observed — a file's bytes, a marker, an alias — makes the next
//! discovery walk again and answer what the bare walk answers.

use super::*;
use std::os::unix::fs::symlink;

fn scratch() -> tempfile::TempDir {
    let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
    fs::create_dir_all(&scratch).unwrap();
    tempfile::Builder::new()
        .prefix("knowledge-ledger-")
        .tempdir_in(&scratch)
        .unwrap()
}

fn source(reference: &str, body: &str) -> String {
    serde_json::json!({
        "binding": {
            "source": reference,
            "revision": "rev-1",
            "title": reference,
            "tags": [],
            "visibility": "public",
            "owners": [],
            "media_type": "text/markdown",
            "metadata": {}
        },
        "body": body,
    })
    .to_string()
}

fn wiki(reference: &str, title: &str) -> String {
    serde_json::json!({"objects": [{
        "profile": "okf-wiki/v1", "object": "node", "ref": reference,
        "type": "Concept", "title": title, "space_refs": [], "source_refs": [],
        "provenance": [],
    }]})
    .to_string()
}

/// Past the racy window, so the walk over this tree may be kept.
fn settle() {
    std::thread::sleep(std::time::Duration::from_millis(2_100));
}

fn walks() -> usize {
    ledger::WALKS.with(std::cell::Cell::get)
}

struct Reading {
    wiki: Vec<WikiObject>,
    sources: BTreeMap<SourceRef, SourceMaterial>,
    absences: Vec<String>,
}

fn through_ledger(root: &Path, home: &Path) -> Reading {
    let mut absences = Vec::new();
    let discovered = discover_material_through_ledger(
        root,
        home,
        &home.join("cache"),
        &mut absences,
        true,
        None,
        None,
    )
    .unwrap();
    Reading {
        wiki: discovered.wiki,
        sources: discovered.sources,
        absences,
    }
}

fn bare(root: &Path, home: &Path) -> Reading {
    let mut absences = Vec::new();
    let discovered = discover_material(root, home, &mut absences, true, None).unwrap();
    Reading {
        wiki: discovered.wiki,
        sources: discovered.sources,
        absences,
    }
}

fn assert_same(left: &Reading, right: &Reading) {
    assert_eq!(left.wiki, right.wiki);
    assert_eq!(left.sources, right.sources);
    assert_eq!(left.absences, right.absences);
}

fn kept_ledgers(home: &Path) -> usize {
    fs::read_dir(home.join("cache/knowledge/discovery"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
                .count()
        })
        .unwrap_or(0)
}

fn body_of(reading: &Reading, reference: &str) -> Option<String> {
    reading
        .sources
        .get(&SourceRef::parse(reference).unwrap())
        .map(|item| item.body.clone())
}

#[test]
fn each_directory_entry_is_checked_once() {
    let root = scratch();
    for n in 0..5 {
        fs::write(root.path().join(format!("entry-{n}.txt")), b"entry").unwrap();
    }
    let boundary = DiscoveryBoundary::capture(root.path(), None).unwrap();
    let mut budget = DirectoryBudget::new(DiscoveryLimits::default().entries);
    let visit = boundary
        .visit_entries(
            root.path(),
            &mut budget,
            |_| Ok(()),
            |pending| {
                pending?;
                Ok(DirectoryControl::Continue)
            },
        )
        .unwrap();
    assert!(matches!(visit, DirectoryVisit::Complete));
    // One checkpoint before listing, one per observed entry, one at the end.
    assert_eq!(boundary.checkpoints.get(), 5 + 2);
}

#[test]
fn a_kept_walk_answers_the_next_discovery_while_nothing_it_observed_changed() {
    let root = scratch();
    let home = scratch();
    fs::create_dir_all(root.path().join("notes/deep")).unwrap();
    fs::write(
        root.path().join("notes/source.json"),
        source("source:ledger:one", "first body"),
    )
    .unwrap();
    fs::write(
        root.path().join("notes/deep/wiki.json"),
        wiki("wiki:node:ledger", "Ledger"),
    )
    .unwrap();
    fs::write(root.path().join("notes/readme.txt"), b"not material").unwrap();
    settle();

    let started = walks();
    let first = through_ledger(root.path(), home.path());
    assert_eq!(walks(), started + 1, "the first discovery walks");
    assert_eq!(kept_ledgers(home.path()), 1, "and keeps its ledger");
    assert_same(&first, &bare(root.path(), home.path()));
    assert_eq!(
        body_of(&first, "source:ledger:one").as_deref(),
        Some("first body")
    );
    assert_eq!(first.wiki.len(), 1);

    let walked = walks();
    let second = through_ledger(root.path(), home.path());
    assert_eq!(walks(), walked, "the ledger answered without a walk");
    assert_same(&second, &first);
}

#[test]
fn a_changed_file_is_never_read_stale() {
    let root = scratch();
    let home = scratch();
    fs::create_dir(root.path().join("notes")).unwrap();
    let path = root.path().join("notes/source.json");
    fs::write(&path, source("source:ledger:two", "before")).unwrap();
    settle();
    let first = through_ledger(root.path(), home.path());
    assert_eq!(kept_ledgers(home.path()), 1);

    // Same length, same directory listing: only the file itself changed.
    fs::write(&path, source("source:ledger:two", "after!")).unwrap();
    let walked = walks();
    let second = through_ledger(root.path(), home.path());
    assert_eq!(walks(), walked + 1, "a changed file makes discovery walk");
    assert_eq!(
        body_of(&first, "source:ledger:two").as_deref(),
        Some("before")
    );
    assert_eq!(
        body_of(&second, "source:ledger:two").as_deref(),
        Some("after!")
    );
    assert_same(&second, &bare(root.path(), home.path()));
}

#[test]
fn a_new_marker_withholds_through_the_ledger() {
    let root = scratch();
    let home = scratch();
    fs::create_dir(root.path().join("private")).unwrap();
    fs::write(
        root.path().join("private/source.json"),
        source("source:ledger:three", "withheld later"),
    )
    .unwrap();
    settle();
    let first = through_ledger(root.path(), home.path());
    assert!(body_of(&first, "source:ledger:three").is_some());

    fs::write(
        root.path()
            .join("private")
            .join(aikit_core::NO_AGENT_RETRIEVAL_MARKER),
        b"",
    )
    .unwrap();
    let walked = walks();
    let second = through_ledger(root.path(), home.path());
    assert_eq!(walks(), walked + 1, "the marker's directory changed");
    assert!(
        body_of(&second, "source:ledger:three").is_none(),
        "a marker refuses through the ledger exactly as through a walk"
    );
    assert_same(&second, &bare(root.path(), home.path()));
}

#[test]
fn a_retargeted_alias_is_seen() {
    let root = scratch();
    let home = scratch();
    for (room, reference) in [
        ("one", "source:ledger:room-one"),
        ("two", "source:ledger:room-two"),
    ] {
        fs::create_dir(root.path().join(room)).unwrap();
        fs::write(
            root.path().join(room).join("source.json"),
            source(reference, room),
        )
        .unwrap();
    }
    let alias = root.path().join("alias");
    symlink(root.path().join("one"), &alias).unwrap();
    settle();
    let first = through_ledger(root.path(), home.path());
    assert_eq!(kept_ledgers(home.path()), 1);

    fs::remove_file(&alias).unwrap();
    symlink(root.path().join("two"), &alias).unwrap();
    let walked = walks();
    let second = through_ledger(root.path(), home.path());
    assert_eq!(walks(), walked + 1, "the alias was retargeted");
    assert_same(&second, &bare(root.path(), home.path()));
    assert!(body_of(&first, "source:ledger:room-one").is_some());
    assert!(body_of(&second, "source:ledger:room-two").is_some());
}

#[test]
fn a_walk_inside_the_racy_window_is_not_kept() {
    let root = scratch();
    let home = scratch();
    fs::write(
        root.path().join("source.json"),
        source("source:ledger:racy", "just written"),
    )
    .unwrap();
    let started = walks();
    let first = through_ledger(root.path(), home.path());
    assert_eq!(kept_ledgers(home.path()), 0, "a racy walk is not kept");
    let second = through_ledger(root.path(), home.path());
    assert_eq!(walks(), started + 2, "so the next discovery walks again");
    assert_same(&first, &second);
}

/// A search's discovery deadline stops the walk with an absence naming where
/// it stopped. The stopped walk is kept for the next search only: an operation
/// without a deadline walks in full and answers what the bare walk answers.
#[test]
fn a_walk_stopped_at_a_search_deadline_answers_only_another_search() {
    let root = scratch();
    let home = scratch();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(
        root.path().join("docs/one.json"),
        source("source:demo:one", "one"),
    )
    .unwrap();
    settle();
    let passed = Some(DiscoveryDeadline {
        at: std::time::Instant::now(),
        budget: std::time::Duration::from_secs(5),
    });
    let discover = |deadline: Option<DiscoveryDeadline>| {
        let mut absences = Vec::new();
        let discovered = discover_material_through_ledger(
            root.path(),
            home.path(),
            &home.path().join("cache"),
            &mut absences,
            true,
            None,
            deadline,
        )
        .unwrap();
        (discovered, absences)
    };
    let before = walks();
    let (stopped, absences) = discover(passed);
    assert_eq!(walks(), before + 1);
    assert!(stopped.deadline_stopped);
    // On disk the stopped walk carries its own schema, so an aikit that
    // predates the flag (it reads only v1) walks again instead of reusing it.
    fn schemas(dir: &std::path::Path, found: &mut Vec<String>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                schemas(&path, found);
            } else if let Ok(bytes) = fs::read(&path) {
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if let Some(schema) = value["schema"].as_str() {
                        found.push(schema.to_owned());
                    }
                }
            }
        }
    }
    let mut written = Vec::new();
    schemas(&home.path().join("cache"), &mut written);
    assert!(
        written
            .iter()
            .any(|schema| schema == "aikit.knowledge-discovery-ledger/deadline-stopped-v1")
            && !written
                .iter()
                .any(|schema| schema == "aikit.knowledge-discovery-ledger/v1"),
        "{written:?}"
    );
    assert!(stopped.sources.is_empty());
    assert!(
        absences
            .iter()
            .any(|absence| absence.contains("5.0 s discovery deadline")),
        "{absences:?}"
    );
    let (replayed, replayed_absences) = discover(passed);
    assert_eq!(
        walks(),
        before + 1,
        "another search replays the stopped walk"
    );
    assert!(replayed.deadline_stopped);
    assert_eq!(replayed_absences, absences);
    let (full, full_absences) = discover(None);
    assert_eq!(
        walks(),
        before + 2,
        "an operation without a deadline walks in full"
    );
    assert!(!full.deadline_stopped);
    let expected = bare(root.path(), home.path());
    assert_eq!(full.sources, expected.sources);
    assert_eq!(full_absences, expected.absences);
    assert!(body_of(
        &Reading {
            wiki: full.wiki,
            sources: full.sources,
            absences: full_absences,
        },
        "source:demo:one"
    )
    .is_some());
}
