//! Collate: answering "which version of a skill is actually running, where".
//!
//! This is the survey Phase E exists to prove. It is **read-only** — it indexes
//! foreign roots and reports; it never edits them, because a foreign root is
//! indexed, not owned (Spec II §3). Ambiguity goes to the inbox as a
//! `VersionConflict` rather than being resolved by guesswork (Spec II §7:
//! automate the provable, queue the ambiguous).

use std::fs;
use std::path::Path;

use aikit_cli::collate::{self, ForeignRootRef};
use aikit_store::channel::{InboxChannel, InboxKind};
use aikit_store::index::Index;

fn index(dir: &Path) -> Index {
    Index::open(&dir.join("state/aikit.sqlite3")).unwrap()
}

/// Every file under `root`, relative path and bytes, sorted — for proving a tree
/// is byte-for-byte unchanged.
fn walk_contents(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(root)
                .unwrap()
                .display()
                .to_string();
            out.push((rel, fs::read(entry.path()).unwrap()));
        }
    }
    out.sort();
    out
}

/// Write a skill with an optional `version:` frontmatter key and a body.
fn skill(root: &Path, name: &str, version: Option<&str>, body: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    let version_line = version
        .map(|v| format!("version: {v}\n"))
        .unwrap_or_default();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: The {name} skill.\n{version_line}---\n\n{body}\n"),
    )
    .unwrap();
}

fn roots(base: &Path, labels: &[&str]) -> Vec<ForeignRootRef> {
    labels
        .iter()
        .map(|l| ForeignRootRef {
            label: format!("@{l}"),
            path: base.join(l),
        })
        .collect()
}

#[test]
fn the_same_skill_in_two_roots_with_different_contents_is_a_conflict() {
    // The shape of the real finding: superpowers at two different versions in two
    // different trees, each live, with nothing on the machine able to say so.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    let codex = base.join("codex");
    let cache = base.join("cache");
    fs::create_dir_all(&codex).unwrap();
    fs::create_dir_all(&cache).unwrap();

    skill(
        &codex,
        "superpowers",
        Some("4.2.0"),
        "the older instructions",
    );
    skill(
        &cache,
        "superpowers",
        Some("6.1.1"),
        "the newer instructions",
    );
    // A skill present in only one root is not a conflict.
    skill(&codex, "solitary", None, "alone");

    let observations = collate::survey(&roots(base, &["codex", "cache"]));
    let clusters = collate::cluster(observations);

    let conflict = clusters
        .iter()
        .find(|c| c.name == "superpowers")
        .expect("superpowers is clustered");
    assert!(
        conflict.is_conflict(),
        "two distinct contents is a conflict"
    );
    assert_eq!(conflict.distinct_contents(), 2);
    assert_eq!(
        conflict.versions(),
        vec!["4.2.0".to_string(), "6.1.1".to_string()],
        "both live versions are named"
    );
    assert_eq!(conflict.observations.len(), 2);

    let solitary = clusters.iter().find(|c| c.name == "solitary").unwrap();
    assert!(!solitary.is_conflict(), "one copy is never a conflict");
}

#[test]
fn identical_copies_across_roots_are_a_duplicate_not_a_conflict() {
    // The nineteen byte-identical symlink aliases on the real machine are a dedup,
    // not a decision (Spec II §7 "identity"). Reporting them as conflicts would
    // bury the two that matter.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    for root in ["a", "b"] {
        fs::create_dir_all(base.join(root)).unwrap();
        skill(&base.join(root), "pdf", Some("1.0.0"), "identical bytes");
    }

    let clusters = collate::cluster(collate::survey(&roots(base, &["a", "b"])));
    let pdf = clusters.iter().find(|c| c.name == "pdf").unwrap();

    assert_eq!(pdf.observations.len(), 2, "both copies are observed");
    assert_eq!(pdf.distinct_contents(), 1);
    assert!(
        !pdf.is_conflict(),
        "byte-identical copies are a duplicate, not an ambiguity for a human"
    );
    assert!(pdf.is_duplicate());
}

#[test]
fn a_cluster_of_many_copies_with_fewer_distinct_contents_is_reported_precisely() {
    // The `test-driven-development` shape on the real machine: six copies, four
    // distinct contents. Both numbers matter — "six copies" alone overstates the
    // ambiguity, "four contents" alone understates the mess.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    let bodies = ["one", "one", "two", "three", "four", "four"];
    let labels: Vec<String> = (0..bodies.len()).map(|i| format!("r{i}")).collect();
    for (label, body) in labels.iter().zip(bodies) {
        fs::create_dir_all(base.join(label)).unwrap();
        skill(&base.join(label), "test-driven-development", None, body);
    }

    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let clusters = collate::cluster(collate::survey(&roots(base, &refs)));
    let tdd = clusters.first().expect("one cluster");

    assert_eq!(tdd.observations.len(), 6, "six copies");
    assert_eq!(tdd.distinct_contents(), 4, "four distinct contents");
    assert!(tdd.is_conflict());
}

#[test]
fn conflicts_are_filed_to_the_inbox_and_name_where_each_version_lives() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    for root in ["codex", "cache"] {
        fs::create_dir_all(base.join(root)).unwrap();
    }
    skill(&base.join("codex"), "superpowers", Some("4.2.0"), "older");
    skill(&base.join("cache"), "superpowers", Some("6.1.1"), "newer");

    let index = index(tmp.path());
    let clusters = collate::cluster(collate::survey(&roots(base, &["codex", "cache"])));
    let filed = collate::report_conflicts(&index, &clusters).unwrap();

    assert_eq!(filed.len(), 1, "one conflict, one item");
    let item = &filed[0];
    assert_eq!(item.kind, InboxKind::VersionConflict);
    assert!(item.title.contains("superpowers"));
    // The body answers "which version, where" — the question collate exists for.
    assert!(item.body.contains("4.2.0"), "body: {}", item.body);
    assert!(item.body.contains("6.1.1"), "body: {}", item.body);
    assert!(
        item.body.contains("@codex"),
        "body names the root: {}",
        item.body
    );
    assert!(item.body.contains("@cache"));

    // Re-collating an unchanged machine does not nag.
    let again = collate::report_conflicts(&index, &clusters).unwrap();
    assert_eq!(again[0].id, item.id);
    assert_eq!(InboxChannel::new(&index).items().unwrap().len(), 1);
}

#[test]
fn collating_never_writes_to_a_foreign_root() {
    // A foreign root is indexed, not owned. The survey must leave it byte-identical.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    let root = base.join("claude");
    fs::create_dir_all(&root).unwrap();
    skill(&root, "pdf", Some("1.0.0"), "contents");

    let before = walk_contents(&root);
    let index = index(tmp.path());
    let clusters = collate::cluster(collate::survey(&roots(base, &["claude"])));
    collate::report_conflicts(&index, &clusters).unwrap();

    assert_eq!(
        walk_contents(&root),
        before,
        "collate is read-only on foreign roots"
    );
}

#[test]
fn a_two_level_container_layout_is_surveyed_too() {
    // Hermes nests <category>/<skill>; an indexer that walked one level would miss
    // whole categories (PRIOR-ART-ACTIONS #29).
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    let hermes = base.join("hermes");
    fs::create_dir_all(hermes.join("nara")).unwrap();
    skill(&hermes.join("nara"), "para", None, "nested");

    let observed = collate::survey(&roots(base, &["hermes"]));
    assert!(
        observed.iter().any(|o| o.name == "para"),
        "a nested skill is surveyed: {observed:?}"
    );
}

#[test]
fn a_plugin_installed_at_two_versions_is_a_conflict_and_backups_are_not() {
    // The shape measured on the reference machine: one plugin live at several
    // versions in different caches — plus a pile of backup and clone directories
    // holding older copies. Counting the backups would report a hundred conflicts
    // that are nobody's problem and bury the handful that are.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();

    let plugin = |rel: &str, name: &str, version: &str, skills: &[&str]| {
        let dir = base.join(rel);
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        fs::write(
            dir.join(".claude-plugin/plugin.json"),
            format!("{{\"name\": \"{name}\", \"version\": \"{version}\"}}"),
        )
        .unwrap();
        for s in skills {
            let sd = dir.join("skills").join(s);
            fs::create_dir_all(&sd).unwrap();
            fs::write(
                sd.join("SKILL.md"),
                format!("---\nname: {s}\ndescription: d.\n---\n"),
            )
            .unwrap();
        }
    };

    // Two LIVE installations at different versions.
    plugin("codex/superpowers", "superpowers", "4.2.0", &["a", "b"]);
    plugin(
        "claude/plugins/cache/official/superpowers/6.1.1",
        "superpowers",
        "6.1.1",
        &["a"],
    );
    // Transient copies that must be ignored.
    plugin(
        "codex/.tmp/plugins-backup-XYZ/repo/plugins/superpowers",
        "superpowers",
        "5.1.0",
        &["a"],
    );
    plugin(
        "codex/.tmp/plugins-clone-ABC/plugins/superpowers",
        "superpowers",
        "5.1.3",
        &["a"],
    );
    // A plugin at a single version is not a conflict.
    plugin("codex/lonely", "lonely", "1.0.0", &[]);

    let observed = collate::survey_plugins(&[base.to_path_buf()], 8);
    assert!(
        observed.iter().all(|p| !collate::is_transient(&p.path)),
        "backup and clone directories must not be surveyed: {observed:?}"
    );

    let conflicts = collate::plugin_conflicts(observed);
    assert_eq!(
        conflicts.len(),
        1,
        "only superpowers conflicts: {conflicts:?}"
    );
    let sp = &conflicts[0];
    assert_eq!(sp.name, "superpowers");
    assert_eq!(
        sp.versions(),
        vec!["4.2.0".to_string(), "6.1.1".to_string()],
        "the live versions, and only those"
    );
    assert_eq!(
        sp.installations
            .iter()
            .find(|i| i.version.as_deref() == Some("4.2.0"))
            .unwrap()
            .skills,
        2
    );

    // And it reaches the user as one inbox item naming both.
    let index = index(tmp.path());
    let filed = collate::report_plugin_conflicts(&index, &conflicts).unwrap();
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].kind, InboxKind::VersionConflict);
    assert!(filed[0].body.contains("4.2.0") && filed[0].body.contains("6.1.1"));
}
