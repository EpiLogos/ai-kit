mod common;

use common::*;

use std::fs;

use aikit_core::projection::{ActivationEffect, ProjectionItem, ProjectionPlan, ResolvedContext};
use aikit_core::resource::ResourceRef;
use aikit_core::{ContextId, TargetId};
use aikit_store::generation::GenerationBuilder;
use aikit_store::history_evidence::{
    all_contexts_generation_history_evidence, generation_history_evidence, source_history_evidence,
};
use aikit_store::AikitHome;

/// One skill, one script — enough to mint generations with different content.
fn registry(dir: &std::path::Path) -> RegistryFixture {
    let fixture = RegistryFixture::at(dir.join("registry"));
    fixture.skill("skill/rust/review");
    fixture.script("script/test/nt");
    fixture
}

fn context(home: &AikitHome, name: &str) -> std::path::PathBuf {
    let dir = home
        .root()
        .join("state/contexts")
        .join(name)
        .join("generations");
    fs::create_dir_all(&dir).unwrap();
    dir.parent().unwrap().to_path_buf()
}

fn skill_plan(resolved: &ResolvedContext, marker: &str) -> Vec<ProjectionPlan> {
    let root = resolved.root_of(&cid("skill/rust/review")).unwrap();
    vec![
        ProjectionPlan::new(TargetId::claude_code(), ActivationEffect::live())
            .with_item(ProjectionItem::write(format!(".claude/{marker}.json"), "{}").unwrap())
            .with_item(
                ProjectionItem::link(root.join("payload"), ".claude/skills/review").unwrap(),
            ),
    ]
}

fn write_source_layout(home: &AikitHome, id: &str, digest: &str, skill_id: &str) {
    let root = home.root().join("sources").join(id);
    fs::create_dir_all(root.join("snapshots").join(digest)).unwrap();
    fs::write(
        root.join("source.toml"),
        format!(
            "schema = 1\nid = \"{id}\"\n[kind.directory]\npath = \"/tmp/pack-{id}\"\ncontrol_ground = false\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("state.toml"),
        format!(
            "candidate_snapshot = \"{digest}\"\nactive_snapshot = \"{digest}\"\nhistory = []\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("snapshots").join(digest).join("snapshot.toml"),
        format!(
            "schema = 1\nsource = \"{id}\"\ndigest = \"{digest}\"\nskills = [
  {{ id = \"{skill_id}\", name = \"review\", source_path = \"review\" }},
]\n"
        ),
    )
    .unwrap();
}

#[test]
fn generation_history_names_the_capsules_it_carried() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["skill/rust/review"]);
    let home = AikitHome::at(tmp.path().join("home"));
    home.ensure_layout().unwrap();
    let ctx = context(&home, "ctx_a");

    let staged = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &skill_plan(&resolved, "one"))
        .unwrap();
    staged.commit(None).unwrap();

    let entries = generation_history_evidence(&home, &ContextId::parse("ctx_a").unwrap()).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0]
        .canonical_refs
        .contains(&ResourceRef::parse("skill/rust/review").unwrap()));

    // The whole point of the join: history filtered by a skill resource now
    // reaches the generation that carried it.
    assert!(entries[0].matches(&ResourceRef::parse("skill/rust/review").unwrap()));
}

#[test]
fn the_generation_lifecycle_is_readable_across_contexts() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let with_skill = resolve_fixture(&fixture, &["skill/rust/review"]);
    let without = resolve_fixture(&fixture, &["script/test/nt"]);
    let home = AikitHome::at(tmp.path().join("home"));
    home.ensure_layout().unwrap();

    // Two contexts, minted by two different resolution identities — the shape
    // an unpinned apply flow produces. The caller's current context is the
    // empty one.
    for (name, resolved, marker) in [("ctx_a", &with_skill, "one"), ("ctx_b", &without, "two")] {
        let ctx = context(&home, name);
        let staged = GenerationBuilder::new()
            .build(&ctx, &resolved.view, &skill_plan(resolved, marker))
            .unwrap();
        staged.commit(None).unwrap();
    }

    let current =
        generation_history_evidence(&home, &ContextId::parse("ctx_current").unwrap()).unwrap();
    assert!(
        current.is_empty(),
        "the caller's context has no generations"
    );

    let swept = all_contexts_generation_history_evidence(&home).unwrap();
    assert_eq!(swept.len(), 2, "both contexts' generations are readable");
    assert!(swept
        .iter()
        .any(|entry| entry.details.get("context").map(String::as_str) == Some("ctx_a")));
}

/// Two contexts, one generation each, and the lock file of the first.
fn two_generations(tmp: &std::path::Path) -> (AikitHome, std::path::PathBuf) {
    let fixture = registry(tmp);
    let with_skill = resolve_fixture(&fixture, &["skill/rust/review"]);
    let without = resolve_fixture(&fixture, &["script/test/nt"]);
    let home = AikitHome::at(tmp.join("home"));
    home.ensure_layout().unwrap();
    for (name, resolved, marker) in [("ctx_a", &with_skill, "one"), ("ctx_b", &without, "two")] {
        let ctx = context(&home, name);
        let staged = GenerationBuilder::new()
            .build(&ctx, &resolved.view, &skill_plan(resolved, marker))
            .unwrap();
        staged.commit(None).unwrap();
    }
    let generations = home.root().join("state/contexts/ctx_a/generations");
    let generation = fs::read_dir(generations)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.join("resolution.lock.toml").is_file())
        .unwrap();
    (home.clone(), generation.join("resolution.lock.toml"))
}

fn lock_summaries(home: &AikitHome) -> std::path::PathBuf {
    home.cache().join("history/generation-locks.v1.json")
}

#[test]
fn a_generation_lock_is_parsed_once_and_the_reading_is_the_same_without_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, lock) = two_generations(tmp.path());

    let first = all_contexts_generation_history_evidence(&home).unwrap();
    assert_eq!(first.len(), 2);
    assert!(lock_summaries(&home).is_file(), "what a lock said is kept");

    // The same reading from the kept summaries; and the same again with none kept.
    let second = all_contexts_generation_history_evidence(&home).unwrap();
    assert_eq!(second, first);
    fs::remove_file(lock_summaries(&home)).unwrap();
    assert_eq!(
        all_contexts_generation_history_evidence(&home).unwrap(),
        first
    );
    assert!(lock_summaries(&home).is_file());

    // A hit does not read the lock: a same-length, same-time stand-in that is not TOML at all
    // is not noticed, because the reading came from what the real lock said.
    let original = fs::read_to_string(&lock).unwrap();
    let modified = fs::metadata(&lock).unwrap().modified().unwrap();
    fs::write(&lock, "x".repeat(original.len())).unwrap();
    fs::File::options()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(
        all_contexts_generation_history_evidence(&home).unwrap(),
        first
    );

    // A lock that is no longer the file the summary came from is read again - here, refused.
    fs::write(&lock, "x".repeat(original.len() + 1)).unwrap();
    let refused = all_contexts_generation_history_evidence(&home).unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("not a readable resolution lock"),
        "{refused}"
    );
}

#[test]
fn a_rewritten_lock_is_derived_again_and_says_what_it_now_says() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, lock) = two_generations(tmp.path());
    let before = all_contexts_generation_history_evidence(&home).unwrap();
    assert!(before
        .iter()
        .all(|entry| !entry.details.contains_key("project")));

    let text = fs::read_to_string(&lock).unwrap();
    assert!(text.contains("[context]\n"), "{text}");
    fs::write(
        &lock,
        text.replacen("[context]\n", "[context]\nproject_id = \"demo\"\n", 1),
    )
    .unwrap();
    let after = all_contexts_generation_history_evidence(&home).unwrap();
    let changed: Vec<_> = after
        .iter()
        .filter(|entry| entry.details.get("project").map(String::as_str) == Some("demo"))
        .collect();
    assert_eq!(changed.len(), 1, "{after:?}");
    assert!(changed[0]
        .canonical_refs
        .contains(&ResourceRef::parse("project/demo").unwrap()));
}

#[test]
fn an_unreadable_foreign_or_unwritable_summary_cache_costs_the_reading_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _lock) = two_generations(tmp.path());
    let expected = all_contexts_generation_history_evidence(&home).unwrap();
    let cache = lock_summaries(&home);

    fs::write(&cache, "{ not json").unwrap();
    assert_eq!(
        all_contexts_generation_history_evidence(&home).unwrap(),
        expected
    );
    fs::write(&cache, r#"{"version":99,"capsules":[],"generations":{}}"#).unwrap();
    assert_eq!(
        all_contexts_generation_history_evidence(&home).unwrap(),
        expected
    );
    // A summary that points past its capsule table is not believed.
    fs::write(
        &cache,
        r#"{"version":1,"capsules":[],"generations":{"x":{"lock":{"len":1,"mtime_ns":1},"resolution_hash":"h","project":null,"active":[7]}}}"#,
    )
    .unwrap();
    assert_eq!(
        all_contexts_generation_history_evidence(&home).unwrap(),
        expected
    );

    // Nowhere to keep it: a file where its directory should be.
    fs::remove_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(cache.parent().unwrap(), "in the way").unwrap();
    assert_eq!(
        all_contexts_generation_history_evidence(&home).unwrap(),
        expected
    );
}

#[test]
fn a_sweep_lets_go_of_the_summaries_of_generations_that_are_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _lock) = two_generations(tmp.path());
    all_contexts_generation_history_evidence(&home).unwrap();
    let kept = |home: &AikitHome| -> usize {
        let text = fs::read_to_string(lock_summaries(home)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["generations"].as_object().unwrap().len()
    };
    assert_eq!(kept(&home), 2);
    fs::remove_dir_all(home.root().join("state/contexts/ctx_b")).unwrap();
    assert_eq!(
        all_contexts_generation_history_evidence(&home)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(kept(&home), 1);
}

#[test]
fn source_history_projects_registrations_and_snapshots_with_skill_refs() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path().join("home"));
    home.ensure_layout().unwrap();
    write_source_layout(&home, "testsrc", "digest0001", "skill/testsrc/review");

    let entries = source_history_evidence(&home).unwrap();
    assert_eq!(entries.len(), 2, "one registration, one snapshot");

    let registration = &entries[0];
    assert_eq!(
        registration.subject,
        ResourceRef::parse("source/testsrc").unwrap()
    );
    assert!(
        registration.summary.contains("directory"),
        "the summary names the registration kind: {}",
        registration.summary
    );

    let snapshot = &entries[1];
    assert!(snapshot.summary.contains("active"));
    assert!(snapshot.summary.contains("1 skill"));
    assert!(snapshot
        .canonical_refs
        .contains(&ResourceRef::parse("skill/testsrc/review").unwrap()));
    // A promotion is history for its skills too.
    assert!(snapshot.matches(&ResourceRef::parse("skill/testsrc/review").unwrap()));

    let empty = AikitHome::at(tmp.path().join("nowhere"));
    assert!(
        source_history_evidence(&empty).unwrap().is_empty(),
        "a home with no sources projects nothing"
    );
}
