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
