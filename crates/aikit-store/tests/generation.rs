//! Generations: the immutable, content-addressed materialization of a view.
//!
//! `ARCHITECTURE.md` §6 makes one promise above all others — **a failed build
//! never replaces the existing view** — and release-blocking case 6 restates it:
//! "a failed projection leaves the previous generation active". Every test here
//! runs against a real directory tree with real symlinks, because the property is
//! a property of `rename(2)` and of the order in which pointers are written, and
//! neither of those can be exercised in the abstract.
//!
//! The four that carry the most weight are named so they are easy to find:
//!
//! * `a_failure_during_materialization_leaves_the_previous_generation_current`
//! * `a_failure_after_materialization_leaves_the_previous_generation_current`
//! * `two_concurrent_commits_from_one_base_produce_exactly_one_winner`
//! * `rollback_restores_the_previous_tree_exactly`

mod common;

use common::*;

use std::fs;
use std::path::{Path, PathBuf};

use aikit_core::projection::{
    ActivationEffect, MaterializationMode, ProjectionItem, ProjectionPlan, ResolvedContext,
};
use aikit_core::{GenerationId, ResolvedView, TargetId};
use aikit_store::generation::{self, GenerationBuilder};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A registry with one script, one skill, one hook and one guidance capsule.
fn registry(dir: &Path) -> RegistryFixture {
    let fixture = RegistryFixture::at(dir.join("registry"));
    fixture.script("script/test/nt");
    fixture.skill("skill/rust/review");
    fixture.hook("hook/gate/secrets");
    fixture.guidance("guidance/mode/research");
    fixture
}

fn context_dir(dir: &Path) -> PathBuf {
    let path = dir.join("state/contexts/ctx_test");
    fs::create_dir_all(path.join("generations")).unwrap();
    path
}

/// Plans covering every projection item kind, against real payload paths.
fn plans(context: &ResolvedContext, marker: &str) -> Vec<ProjectionPlan> {
    let skill_root = context.root_of(&cid("skill/rust/review")).unwrap();
    let hook_root = context.root_of(&cid("hook/gate/secrets")).unwrap();
    let guidance_root = context.root_of(&cid("guidance/mode/research")).unwrap();

    vec![
        ProjectionPlan::new(TargetId::claude_code(), ActivationEffect::live())
            .with_item(
                ProjectionItem::link(skill_root.join("payload"), ".claude/skills/review").unwrap(),
            )
            .with_item(
                ProjectionItem::write(
                    ".claude/settings.json",
                    format!("{{\"marker\":\"{marker}\"}}"),
                )
                .unwrap(),
            ),
        ProjectionPlan::new(TargetId::new(TargetId::HOOKS), ActivationEffect::live()).with_item(
            ProjectionItem::copy(hook_root.join("payload/check"), "10-gate-secrets").unwrap(),
        ),
        ProjectionPlan::new(TargetId::new(TargetId::GUIDANCE), ActivationEffect::live()).with_item(
            ProjectionItem::link(
                guidance_root.join("payload/guidance.md"),
                "00-mode-research.md",
            )
            .unwrap(),
        ),
        ProjectionPlan::new(TargetId::shell(), ActivationEffect::live())
            .with_item(ProjectionItem::shim("nt", cid("script/test/nt"), "nt").unwrap()),
    ]
}

fn build_and_commit(
    context_dir: &Path,
    resolved: &ResolvedContext,
    marker: &str,
    base: Option<&GenerationId>,
) -> GenerationId {
    let staged = GenerationBuilder::new()
        .build(context_dir, &resolved.view, &plans(resolved, marker))
        .unwrap();
    staged.commit(base).unwrap().id
}

// ---------------------------------------------------------------------------
// The built tree
// ---------------------------------------------------------------------------

#[test]
fn a_built_generation_has_the_layout_the_specification_documents() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(
        &fixture,
        &["script/test/nt", "skill/rust/review", "hook/gate/secrets"],
    );
    let ctx = context_dir(tmp.path());

    let staged = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &plans(&resolved, "one"))
        .unwrap();

    for expected in [
        "resolution.lock.toml",
        "metadata.json",
        "bin",
        "hooks",
        "guidance",
        "projections",
    ] {
        assert!(
            staged.path().join(expected).exists(),
            "{expected} is missing from the staged generation"
        );
    }
}

#[test]
fn the_lock_file_is_a_full_re_readable_serialization_of_the_resolved_view() {
    // The lock is what `aikit explain` reads after the fact and what a rollback
    // has to be able to describe. A lossy summary would make both impossible.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt", "skill/rust/review"]);
    let ctx = context_dir(tmp.path());

    let staged = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &plans(&resolved, "one"))
        .unwrap();

    let text = fs::read_to_string(staged.path().join("resolution.lock.toml")).unwrap();
    let reread: ResolvedView = toml::from_str(&text).unwrap();

    assert_eq!(reread.hash, resolved.view.hash);
    assert_eq!(reread.active.len(), resolved.view.active.len());
    assert_eq!(reread, resolved.view);
}

#[test]
fn each_projection_item_kind_lands_as_the_thing_it_says_it_is() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt", "skill/rust/review"]);
    let ctx = context_dir(tmp.path());

    let staged = GenerationBuilder::new()
        .with_mode(MaterializationMode::Link)
        .build(&ctx, &resolved.view, &plans(&resolved, "one"))
        .unwrap();
    let root = staged.path();

    let linked = root.join("projections/claude/.claude/skills/review");
    assert!(
        fs::symlink_metadata(&linked)
            .unwrap()
            .file_type()
            .is_symlink(),
        "a Link item should be a symlink in link mode"
    );
    assert!(linked.join("SKILL.md").is_file(), "the link must resolve");

    let written = root.join("projections/claude/.claude/settings.json");
    assert_eq!(
        fs::read_to_string(&written).unwrap(),
        "{\"marker\":\"one\"}"
    );

    let copied = root.join("hooks/10-gate-secrets");
    assert!(
        !fs::symlink_metadata(&copied)
            .unwrap()
            .file_type()
            .is_symlink(),
        "a Copy item is a copy even in link mode"
    );
    assert!(fs::read_to_string(&copied).unwrap().contains("exit 0"));

    let shim = root.join("bin/nt");
    let body = fs::read_to_string(&shim).unwrap();
    assert!(
        body.contains("script/test/nt"),
        "the shim names its capsule"
    );
    assert!(body.starts_with("#!"), "a shim has to be executable text");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(&shim).unwrap().permissions().mode() & 0o111,
            0,
            "a shim on the contextual PATH must be executable"
        );
    }
}

#[test]
fn metadata_records_the_base_the_hash_and_the_isolation_it_was_built_under() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());

    let first = build_and_commit(&ctx, &resolved, "one", None);
    let second = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &plans(&resolved, "two"))
        .unwrap()
        .commit(Some(&first))
        .unwrap();

    let metadata = generation::read_metadata(&second.path).unwrap();
    assert_eq!(metadata.generation_id, second.id);
    assert_eq!(metadata.base_generation.as_ref(), Some(&first));
    assert_eq!(metadata.resolution_hash, resolved.view.hash.to_string());
    assert_eq!(
        metadata.isolation,
        aikit_core::Isolation::Shared,
        "a generation must record the isolation it was actually built under"
    );
    assert!(!metadata.targets.is_empty());
}

#[test]
fn a_generation_stamps_the_generation_format_and_is_recognised_by_it() {
    // PRIOR-ART-ACTIONS #8: `generation_format: 1` is present from the first
    // commit, and its presence is the test for "this directory is a generation".
    // Impossible to retrofit, so it is asserted from the ground up.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());

    let committed = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &plans(&resolved, "one"))
        .unwrap()
        .commit(None)
        .unwrap();

    let metadata = generation::read_metadata(&committed.path).unwrap();
    assert_eq!(metadata.generation_format, generation::GENERATION_FORMAT);
    assert!(
        generation::is_generation(&committed.path),
        "a stamped directory is recognised as a generation"
    );

    let not_a_generation = tmp.path().join("random-dir");
    fs::create_dir_all(&not_a_generation).unwrap();
    assert!(
        !generation::is_generation(&not_a_generation),
        "a directory with no generation_format stamp is not a generation"
    );
}

#[test]
fn a_cosmetic_property_is_excluded_from_the_generation_identity() {
    // PRIOR-ART-ACTIONS #9: a `[properties]` table in the lock is cosmetic and
    // excluded from equality — otherwise every label edit would invalidate every
    // generation. Two views identical but for a property build to the SAME id, and
    // the property still round-trips through the lock so `explain` can read it.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt", "skill/rust/review"]);
    let ctx = context_dir(tmp.path());
    let plan_set = plans(&resolved, "same");

    let plain = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &plan_set)
        .unwrap();

    let mut labelled_view = resolved.view.clone();
    labelled_view
        .properties
        .insert("label".to_string(), "known-good".to_string());
    let labelled = GenerationBuilder::new()
        .build(&ctx, &labelled_view, &plan_set)
        .unwrap();

    assert_eq!(
        plain.id(),
        labelled.id(),
        "a cosmetic property must not change the content-addressed generation id"
    );

    let lock = fs::read_to_string(labelled.path().join("resolution.lock.toml")).unwrap();
    assert!(
        lock.contains("known-good"),
        "the [properties] table lives in the lock, so explain can read it back: {lock}"
    );

    let plain_lock = fs::read_to_string(plain.path().join("resolution.lock.toml")).unwrap();
    assert!(
        !plain_lock.contains("[properties]"),
        "an empty properties table is not written, so existing locks are unchanged"
    );
}

#[test]
fn relabelling_an_identical_generation_updates_the_label_without_minting_a_new_one() {
    // The end-to-end proof of PRIOR-ART-ACTIONS #9: re-applying an identical
    // resolution but with a cosmetic label attaches the label to the existing
    // generation in place — it does not create a second, near-duplicate one, and
    // `current` does not move.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());
    let plan_set = plans(&resolved, "one");

    let first = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &plan_set)
        .unwrap()
        .commit(None)
        .unwrap();

    let mut labelled = resolved.view.clone();
    labelled
        .properties
        .insert("label".to_string(), "known-good".to_string());
    let again = GenerationBuilder::new()
        .build(&ctx, &labelled, &plan_set)
        .unwrap()
        .commit(Some(&first.id))
        .unwrap();

    assert_eq!(
        first.id, again.id,
        "identical content is the same generation"
    );
    assert_eq!(
        generation::list(&ctx).unwrap().len(),
        1,
        "no duplicate generation was minted for a label change"
    );
    assert_eq!(generation::current(&ctx).unwrap(), Some(first.id.clone()));

    let view = generation::read_lock(&ctx.join("generations").join(again.id.as_str())).unwrap();
    assert_eq!(
        view.properties.get("label").map(String::as_str),
        Some("known-good"),
        "the label was written onto the existing generation's lock"
    );
}

#[test]
fn relabelling_replaces_the_lock_atomically_and_leaves_no_temp_file() {
    // `relabel` is the only write that lands inside an already-committed
    // generation. A truncate-then-write would leave a window in which the lock of
    // an immutable generation is empty — unreadable forever, and undetectable
    // because the lock is excluded from the content hash. It must therefore write
    // beside the lock and rename over it, leaving nothing behind.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());

    let committed = build_and_commit(&ctx, &resolved, "one", None);
    let dir = ctx.join("generations").join(committed.as_str());

    let mut properties = std::collections::BTreeMap::new();
    properties.insert("label".to_string(), "known-good".to_string());
    generation::relabel(&dir, &properties).unwrap();

    // The lock is complete and re-readable, and the label is there.
    let view = generation::read_lock(&dir).unwrap();
    assert_eq!(
        view.properties.get("label").map(String::as_str),
        Some("known-good")
    );
    assert_eq!(
        view.active.len(),
        resolved.view.active.len(),
        "the rest of the lock survived the rewrite"
    );

    // No `.tmp` sibling is left in the generation directory.
    let strays: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(strays.is_empty(), "a temp file was left behind: {strays:?}");

    // Re-labelling with the same properties is a no-op that still leaves it valid.
    generation::relabel(&dir, &properties).unwrap();
    assert!(generation::read_lock(&dir).is_ok());
}

#[test]
fn a_brokered_fallback_is_written_into_the_generation_rather_than_lost() {
    // §3: a shared working tree cannot give two sibling tasks different native
    // skill surfaces, and the adapter has to say so instead of pretending. The
    // generation is where that sentence has to survive, because it is what
    // `aikit explain` reads back weeks later.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["skill/rust/review"]);
    let ctx = context_dir(tmp.path());
    assert_eq!(
        resolved.view.context.isolation,
        aikit_core::Isolation::Shared,
        "the default really is shared; a worktree is opt-in"
    );

    let skill_root = resolved.root_of(&cid("skill/rust/review")).unwrap();
    let honest = vec![ProjectionPlan::new(
        TargetId::codex(),
        ActivationEffect::brokered(
            "this task uses the session's shared working tree (shared), and this client's skill \
             directory lives in the tree",
        ),
    )
    .with_item(ProjectionItem::link(skill_root.join("payload"), "skills/review").unwrap())];

    let committed = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &honest)
        .unwrap()
        .commit(None)
        .unwrap();

    let metadata = generation::read_metadata(&committed.path).unwrap();
    assert_eq!(metadata.isolation, aikit_core::Isolation::Shared);
    assert!(
        metadata
            .notes
            .iter()
            .any(|n| n.contains("shared working tree")),
        "the stated reason must be recorded, not dropped: {:?}",
        metadata.notes
    );
    assert!(metadata
        .targets
        .iter()
        .any(|t| t.effect.contains("brokered")));
}

// ---------------------------------------------------------------------------
// Committing and the pointers
// ---------------------------------------------------------------------------

#[test]
fn committing_names_the_generation_by_content_and_points_current_at_it() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());

    let id = build_and_commit(&ctx, &resolved, "one", None);

    assert!(ctx.join("generations").join(id.as_str()).is_dir());
    assert_eq!(generation::current(&ctx).unwrap(), Some(id.clone()));
    assert_eq!(generation::previous(&ctx).unwrap(), None);
    assert!(
        ctx.join("current").join("resolution.lock.toml").is_file(),
        "AIKIT_VIEW points at `current`, so the lock must be reachable through it"
    );
}

#[test]
fn the_current_path_is_stable_across_generation_swaps() {
    // `AIKIT_VIEW=.../current` is exported into shells and clients. If the path
    // changed per generation, every already-running shell would be stale.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());

    let first = build_and_commit(&ctx, &resolved, "one", None);
    let view_path = generation::current_path(&ctx);
    let first_target = fs::canonicalize(&view_path).unwrap();

    let second = build_and_commit(&ctx, &resolved, "two", Some(&first));

    assert_eq!(view_path, generation::current_path(&ctx));
    assert_ne!(first_target, fs::canonicalize(&view_path).unwrap());
    assert_eq!(generation::current(&ctx).unwrap(), Some(second));
    assert_eq!(generation::previous(&ctx).unwrap(), Some(first));
}

#[test]
fn an_identical_rebuild_is_the_same_generation_rather_than_a_duplicate() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());

    let first = build_and_commit(&ctx, &resolved, "one", None);
    let again = build_and_commit(&ctx, &resolved, "one", Some(&first));

    assert_eq!(first, again, "the generation id is a content hash");
    assert_eq!(generation::list(&ctx).unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// A failed build never replaces the existing view
// ---------------------------------------------------------------------------

#[test]
fn a_failure_during_materialization_leaves_the_previous_generation_current() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt", "skill/rust/review"]);
    let ctx = context_dir(tmp.path());

    let good = build_and_commit(&ctx, &resolved, "one", None);
    let before = walk_contents(&ctx.join("current"));

    // A plan naming a payload that is not there: exactly what a half-synced
    // registry or a capsule deleted underneath you produces.
    let broken = vec![
        ProjectionPlan::new(TargetId::claude_code(), ActivationEffect::live()).with_item(
            ProjectionItem::link(
                fixture.root().join("does/not/exist"),
                ".claude/skills/ghost",
            )
            .unwrap(),
        ),
    ];

    let error = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &broken)
        .unwrap_err();
    assert_eq!(error.code(), "generation.source_missing");

    assert_eq!(generation::current(&ctx).unwrap(), Some(good));
    assert_eq!(walk_contents(&ctx.join("current")), before);
    assert!(
        staging_dirs(&ctx).is_empty(),
        "a failed build must not leave a staging directory behind"
    );
}

#[test]
fn a_failure_after_materialization_leaves_the_previous_generation_current() {
    // The tree is built, validated and sitting in its temp directory; the commit
    // is then refused because someone else moved `current` in between. Nothing
    // about the live view may have changed.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt", "skill/rust/review"]);
    let ctx = context_dir(tmp.path());

    let first = build_and_commit(&ctx, &resolved, "one", None);
    let staged = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &plans(&resolved, "three"))
        .unwrap();
    assert!(staged.path().is_dir(), "it really did materialize");

    // Someone else applies in the meantime.
    let second = build_and_commit(&ctx, &resolved, "two", Some(&first));
    let live = walk_contents(&ctx.join("current"));

    let error = staged.commit(Some(&first)).unwrap_err();
    assert_eq!(error.code(), "generation.stale_base");
    assert_eq!(
        error.details().get("expected").map(String::as_str),
        Some(first.as_str())
    );
    assert_eq!(
        error.details().get("actual").map(String::as_str),
        Some(second.as_str())
    );

    assert_eq!(generation::current(&ctx).unwrap(), Some(second));
    assert_eq!(walk_contents(&ctx.join("current")), live);
    assert!(staging_dirs(&ctx).is_empty());
}

#[test]
fn a_first_commit_against_an_unexpected_base_is_also_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());

    let phantom = GenerationId::parse("gen_0000000000000000").unwrap();
    let staged = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &plans(&resolved, "one"))
        .unwrap();

    let error = staged.commit(Some(&phantom)).unwrap_err();
    assert_eq!(error.code(), "generation.stale_base");
    assert_eq!(generation::current(&ctx).unwrap(), None);
}

#[test]
fn two_concurrent_commits_from_one_base_produce_exactly_one_winner() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt", "skill/rust/review"]);
    let ctx = context_dir(tmp.path());

    let base = build_and_commit(&ctx, &resolved, "base", None);

    // Both panes build from the same base, with different content, and race.
    let left = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &plans(&resolved, "left"))
        .unwrap();
    let right = GenerationBuilder::new()
        .build(&ctx, &resolved.view, &plans(&resolved, "right"))
        .unwrap();
    assert_ne!(left.id(), right.id());

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let base_left = base.clone();
    let base_right = base.clone();
    let b1 = barrier.clone();
    let b2 = barrier.clone();

    let one = std::thread::spawn(move || {
        b1.wait();
        left.commit(Some(&base_left))
    });
    let two = std::thread::spawn(move || {
        b2.wait();
        right.commit(Some(&base_right))
    });

    let results = [one.join().unwrap(), two.join().unwrap()];
    let winners: Vec<_> = results.iter().filter(|r| r.is_ok()).collect();
    let losers: Vec<_> = results.iter().filter_map(|r| r.as_ref().err()).collect();

    assert_eq!(winners.len(), 1, "exactly one commit may win");
    assert_eq!(losers.len(), 1);
    assert_eq!(losers[0].code(), "generation.stale_base");

    let winner_id = winners[0].as_ref().unwrap().id.clone();
    assert_eq!(generation::current(&ctx).unwrap(), Some(winner_id));
    assert_eq!(generation::previous(&ctx).unwrap(), Some(base));
    assert!(staging_dirs(&ctx).is_empty());
}

// ---------------------------------------------------------------------------
// Rollback
// ---------------------------------------------------------------------------

#[test]
fn rollback_restores_the_previous_tree_exactly() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt", "skill/rust/review"]);
    let ctx = context_dir(tmp.path());

    let first = build_and_commit(&ctx, &resolved, "one", None);
    let first_tree = walk_contents(&ctx.join("current"));
    let second = build_and_commit(&ctx, &resolved, "two", Some(&first));
    let second_tree = walk_contents(&ctx.join("current"));
    assert_ne!(first_tree, second_tree);

    let outcome = generation::rollback(&ctx).unwrap();

    assert_eq!(outcome.now_current, first);
    assert_eq!(outcome.was_current, second);
    assert_eq!(generation::current(&ctx).unwrap(), Some(first));
    assert_eq!(
        generation::previous(&ctx).unwrap(),
        Some(second),
        "rollback swaps the two pointers, so it can be undone"
    );
    assert_eq!(
        walk_contents(&ctx.join("current")),
        first_tree,
        "the restored tree must be byte-for-byte what was there before"
    );
}

#[test]
fn rolling_back_twice_returns_to_where_it_started() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());

    let first = build_and_commit(&ctx, &resolved, "one", None);
    let second = build_and_commit(&ctx, &resolved, "two", Some(&first));

    generation::rollback(&ctx).unwrap();
    generation::rollback(&ctx).unwrap();
    assert_eq!(generation::current(&ctx).unwrap(), Some(second));
}

#[test]
fn rolling_back_with_nothing_to_roll_back_to_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());
    build_and_commit(&ctx, &resolved, "one", None);

    let error = generation::rollback(&ctx).unwrap_err();
    assert_eq!(error.code(), "generation.no_previous");
}

// ---------------------------------------------------------------------------
// Link mode and copy mode are logically the same projection
// ---------------------------------------------------------------------------

#[test]
fn link_mode_and_copy_mode_produce_the_same_logical_projection() {
    // The user may be on a filesystem without symlinks, or may have asked for
    // copies. What lands has to be the same set of relative paths with the same
    // bytes — otherwise "copy mode" is a different product.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt", "skill/rust/review"]);

    let linked_ctx = context_dir(&tmp.path().join("linked"));
    let copied_ctx = context_dir(&tmp.path().join("copied"));

    let linked = GenerationBuilder::new()
        .with_mode(MaterializationMode::Link)
        .build(&linked_ctx, &resolved.view, &plans(&resolved, "same"))
        .unwrap()
        .commit(None)
        .unwrap();
    let copied = GenerationBuilder::new()
        .with_mode(MaterializationMode::Copy)
        .build(&copied_ctx, &resolved.view, &plans(&resolved, "same"))
        .unwrap()
        .commit(None)
        .unwrap();

    assert_eq!(
        logical_tree(&linked.path),
        logical_tree(&copied.path),
        "the two modes must present the same files with the same contents"
    );

    // …and they really were materialized differently.
    let link_target = linked.path.join("projections/claude/.claude/skills/review");
    let copy_target = copied.path.join("projections/claude/.claude/skills/review");
    assert!(fs::symlink_metadata(&link_target)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!fs::symlink_metadata(&copy_target)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn a_platform_without_symlinks_degrades_to_copies_and_says_so_in_the_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt", "skill/rust/review"]);
    let ctx = context_dir(tmp.path());

    let committed = GenerationBuilder::new()
        .with_mode(MaterializationMode::Link)
        .without_symlinks()
        .build(&ctx, &resolved.view, &plans(&resolved, "one"))
        .unwrap()
        .commit(None)
        .unwrap();

    let target = committed
        .path
        .join("projections/claude/.claude/skills/review");
    assert!(!fs::symlink_metadata(&target)
        .unwrap()
        .file_type()
        .is_symlink());

    let metadata = generation::read_metadata(&committed.path).unwrap();
    assert_eq!(metadata.materialization, MaterializationMode::Copy);
    assert!(
        metadata.notes.iter().any(|n| n.contains("symlink")),
        "the degradation has to be stated, not silent: {:?}",
        metadata.notes
    );
}

// ---------------------------------------------------------------------------
// Garbage collection
// ---------------------------------------------------------------------------

#[test]
fn gc_never_deletes_what_current_or_previous_point_at() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());

    let one = build_and_commit(&ctx, &resolved, "one", None);
    let two = build_and_commit(&ctx, &resolved, "two", Some(&one));
    let three = build_and_commit(&ctx, &resolved, "three", Some(&two));
    assert_eq!(generation::list(&ctx).unwrap().len(), 3);

    // Ask for nothing to be kept; the referenced pair still survives.
    let deleted = generation::gc(&ctx, 0).unwrap();

    assert_eq!(deleted, vec![one]);
    let remaining: Vec<GenerationId> = generation::list(&ctx).unwrap();
    assert!(remaining.contains(&two), "previous must survive gc");
    assert!(remaining.contains(&three), "current must survive gc");
    assert_eq!(remaining.len(), 2);

    assert!(ctx.join("current").join("metadata.json").is_file());
    assert!(ctx.join("previous").join("metadata.json").is_file());
}

#[test]
fn gc_keeps_the_requested_number_of_recent_generations() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());

    let mut last = build_and_commit(&ctx, &resolved, "g0", None);
    for i in 1..5 {
        last = build_and_commit(&ctx, &resolved, &format!("g{i}"), Some(&last));
    }
    assert_eq!(generation::list(&ctx).unwrap().len(), 5);

    let deleted = generation::gc(&ctx, 4).unwrap();
    assert_eq!(deleted.len(), 1);
    assert_eq!(generation::list(&ctx).unwrap().len(), 4);
}

#[test]
fn gc_on_a_context_with_nothing_to_collect_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = registry(tmp.path());
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let ctx = context_dir(tmp.path());
    build_and_commit(&ctx, &resolved, "one", None);

    assert!(generation::gc(&ctx, 10).unwrap().is_empty());
    assert_eq!(generation::list(&ctx).unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Staging directories left behind. Should always be empty between operations.
fn staging_dirs(context_dir: &Path) -> Vec<PathBuf> {
    let generations = context_dir.join("generations");
    let Ok(entries) = fs::read_dir(&generations) else {
        return vec![];
    };
    entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
        })
        .collect()
}

/// The logical content of a generation: relative paths and bytes, symlinks
/// followed, ignoring the metadata that records *how* it was built.
fn logical_tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    walk_contents(root)
        .into_iter()
        .filter(|(path, _)| path != "metadata.json")
        .collect()
}
