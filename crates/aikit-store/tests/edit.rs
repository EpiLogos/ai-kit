//! Format-preserving TOML editing.
//!
//! `<repo>/.aikit/profile.toml` is a file a person wrote, committed and will read
//! again in a code review. If AIKit reformats it — reorders keys, drops the
//! comment explaining why the regression hook is off, collapses the hand-wrapped
//! array — then toggling one capability produces a twenty-line diff and the team
//! stops letting AIKit near the file. That is a product failure, so the
//! preservation is tested rather than hoped for.

mod common;

use common::*;

use aikit_store::edit::{OverlayDocument, ProfileDocument};

const HAND_WRITTEN: &str = r#"# The payments repository's shared AIKit profile.
# Reviewed in #eng-payments before any change.
schema = 1

profiles = [
  "profile/code/rust",
  # worktree-safe is required by the security team
  "profile/agents/worktree-safe",
]

enable = [
  "skill/project/payments-domain",
]

# Disabled because it takes eleven minutes on this repo.
disable = [
  "hook/verify/full-regression",
]

[config."script/test/cargo-nextest"]
profile = "ci"    # matches .github/workflows/test.yml
"#;

// ---------------------------------------------------------------------------
// Preservation
// ---------------------------------------------------------------------------

#[test]
fn a_toggle_preserves_comments_key_order_and_hand_formatting() {
    let mut doc = ProfileDocument::parse(HAND_WRITTEN).unwrap();
    doc.enable(&cid("script/test/cargo-nextest"));
    let after = doc.to_string();

    for kept in [
        "# The payments repository's shared AIKit profile.",
        "# Reviewed in #eng-payments before any change.",
        "# worktree-safe is required by the security team",
        "# Disabled because it takes eleven minutes on this repo.",
        "# matches .github/workflows/test.yml",
    ] {
        assert!(after.contains(kept), "lost the comment `{kept}`:\n{after}");
    }

    assert!(
        after.find("schema").unwrap() < after.find("profiles").unwrap(),
        "key order must be the author's, not serde's"
    );
    assert!(after.find("enable").unwrap() < after.find("disable").unwrap());
    assert!(
        after.contains("script/test/cargo-nextest"),
        "the toggle should actually have happened"
    );
}

#[test]
fn a_no_op_edit_leaves_the_file_byte_identical() {
    let mut doc = ProfileDocument::parse(HAND_WRITTEN).unwrap();
    doc.enable(&cid("skill/project/payments-domain")); // already enabled
    assert_eq!(
        doc.to_string(),
        HAND_WRITTEN,
        "re-enabling something already enabled must not rewrite the file"
    );
}

#[test]
fn parsing_and_rendering_without_an_edit_is_the_identity() {
    let doc = ProfileDocument::parse(HAND_WRITTEN).unwrap();
    assert_eq!(doc.to_string(), HAND_WRITTEN);
}

#[test]
fn disabling_something_that_was_enabled_moves_it_rather_than_listing_it_twice() {
    let mut doc = ProfileDocument::parse(HAND_WRITTEN).unwrap();
    doc.disable(&cid("skill/project/payments-domain"));
    let after = doc.to_string();

    let patch = ProfileDocument::parse(&after).unwrap().patch().unwrap();
    assert!(!patch.enable.contains(&cid("skill/project/payments-domain")));
    assert!(patch.disable.contains(&cid("skill/project/payments-domain")));
    assert!(
        patch.validate().is_ok(),
        "a capsule may never end up in both lists"
    );
    assert!(after.contains("# Disabled because it takes eleven minutes on this repo."));
}

#[test]
fn clearing_removes_a_declaration_entirely() {
    let mut doc = ProfileDocument::parse(HAND_WRITTEN).unwrap();
    doc.clear(&cid("hook/verify/full-regression"));
    let patch = doc.patch().unwrap();

    assert!(patch.disable.is_empty());
    assert!(
        doc.to_string().contains("# Disabled because it takes"),
        "the surrounding prose is the author's, not ours to delete"
    );
}

#[test]
fn an_enable_list_that_does_not_exist_yet_is_created() {
    let mut doc = ProfileDocument::parse("schema = 1\n# nothing here yet\n").unwrap();
    doc.enable(&cid("script/test/nt"));
    let text = doc.to_string();

    assert!(text.contains("# nothing here yet"));
    assert!(text.contains("script/test/nt"));
    assert_eq!(
        ProfileDocument::parse(&text).unwrap().patch().unwrap().enable,
        vec![cid("script/test/nt")]
    );
}

#[test]
fn per_capsule_configuration_can_be_set_without_disturbing_its_neighbours() {
    let mut doc = ProfileDocument::parse(HAND_WRITTEN).unwrap();
    doc.set_config(
        &cid("script/test/cargo-nextest"),
        "retries",
        toml_edit::value(2),
    );
    let text = doc.to_string();

    assert!(text.contains("# matches .github/workflows/test.yml"));
    assert!(text.contains("retries = 2"));

    let patch = ProfileDocument::parse(&text).unwrap().patch().unwrap();
    let table = &patch.config[&cid("script/test/cargo-nextest")];
    assert_eq!(table["profile"].as_str(), Some("ci"));
    assert_eq!(table["retries"].as_integer(), Some(2));
}

#[test]
fn adding_a_profile_reference_appends_rather_than_replacing_the_list() {
    let mut doc = ProfileDocument::parse(HAND_WRITTEN).unwrap();
    doc.use_profile(&pid("profile/code/general"));
    let text = doc.to_string();

    assert!(text.contains("# worktree-safe is required by the security team"));
    let patch = ProfileDocument::parse(&text).unwrap().patch().unwrap();
    assert_eq!(patch.profiles.len(), 3);
    assert!(patch.profiles.contains(&pid("profile/code/general")));
}

// ---------------------------------------------------------------------------
// Files on disk
// ---------------------------------------------------------------------------

#[test]
fn opening_a_missing_project_profile_creates_a_minimal_valid_one() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".aikit/profile.toml");

    let mut doc = ProfileDocument::open(&path).unwrap();
    doc.enable(&cid("script/test/nt"));
    doc.save().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("schema = 1"));
    assert_eq!(
        ProfileDocument::parse(&text).unwrap().patch().unwrap().enable,
        vec![cid("script/test/nt")]
    );
}

#[test]
fn saving_replaces_the_file_atomically_and_leaves_no_debris() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("profile.toml");
    std::fs::write(&path, HAND_WRITTEN).unwrap();

    let mut doc = ProfileDocument::open(&path).unwrap();
    doc.enable(&cid("script/test/nt"));
    doc.save().unwrap();

    assert!(std::fs::read_to_string(&path).unwrap().contains("script/test/nt"));
    let leftovers: Vec<String> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n != "profile.toml")
        .collect();
    assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
}

#[test]
fn a_local_profile_is_edited_the_same_way_as_a_shared_one() {
    // `profile.local.toml` is git-ignored, but it is still a file a person reads.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".aikit/profile.local.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "# my machine only\nschema = 1\nenable = [\"script/local/tail\"]\n")
        .unwrap();

    let mut doc = ProfileDocument::open(&path).unwrap();
    doc.disable(&cid("hook/gate/secrets"));
    doc.save().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# my machine only"));
    assert!(text.contains("script/local/tail"));
    assert!(text.contains("hook/gate/secrets"));
}

#[test]
fn a_malformed_file_is_refused_with_a_stable_code_rather_than_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("profile.toml");
    std::fs::write(&path, "enable = [ this is not toml").unwrap();

    let error = ProfileDocument::open(&path).unwrap_err();
    assert_eq!(error.code(), "edit.parse_error");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "enable = [ this is not toml",
        "a file we could not understand must be left exactly as it was"
    );
}

// ---------------------------------------------------------------------------
// Session overlays
// ---------------------------------------------------------------------------

#[test]
fn a_session_overlay_is_created_with_its_session_id_and_parses_as_one() {
    let tmp = tempfile::tempdir().unwrap();
    let session = aikit_core::SessionId::generate();
    let path = tmp.path().join("overlay.toml");

    let mut overlay = OverlayDocument::open(&path, &session).unwrap();
    overlay.enable(&cid("guidance/mode/research"));
    overlay.set_base_generation(Some(
        &aikit_core::GenerationId::parse("gen_b71f2fdeadbeef01").unwrap(),
    ));
    overlay.save().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    let parsed: aikit_core::profile::SessionOverlayFile = toml::from_str(&text).unwrap();
    assert_eq!(parsed.session_id, session);
    assert_eq!(
        parsed.base_generation.unwrap().as_str(),
        "gen_b71f2fdeadbeef01"
    );
    assert_eq!(parsed.patch.enable, vec![cid("guidance/mode/research")]);
}

#[test]
fn an_overlays_patch_reads_back_through_the_overlay_parser_not_the_project_one() {
    // Regression: an overlay carries `session_id` and `base_generation`, which the
    // strict project-profile parser rejects. `OverlayDocument::patch()` must read
    // the file back as a session overlay, or `aikit enable --scope session` breaks
    // the moment the overlay is re-resolved.
    let tmp = tempfile::tempdir().unwrap();
    let session = aikit_core::SessionId::generate();
    let path = tmp.path().join("overlay.toml");

    let mut overlay = OverlayDocument::open(&path, &session).unwrap();
    overlay.enable(&cid("skill/rust/rust-review"));
    overlay.disable(&cid("hook/verify/full-regression"));
    overlay.set_base_generation(Some(
        &aikit_core::GenerationId::parse("gen_b71f2fdeadbeef01").unwrap(),
    ));
    overlay.save().unwrap();

    // Reopen from disk (session_id and base_generation now on disk) and read the
    // patch: this is exactly what resolution does.
    let reopened = OverlayDocument::open(&path, &session).unwrap();
    let patch = reopened.patch().expect("an overlay with session_id must still parse");
    assert_eq!(patch.enable, vec![cid("skill/rust/rust-review")]);
    assert_eq!(patch.disable, vec![cid("hook/verify/full-regression")]);
}

#[test]
fn updating_an_overlays_base_generation_preserves_the_rest_of_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let session = aikit_core::SessionId::generate();
    let path = tmp.path().join("overlay.toml");
    std::fs::write(
        &path,
        format!(
            "schema = 1\nsession_id = \"{session}\"\nbase_generation = \"gen_0000000000000001\"\n\
             # switched research mode on while reading the migration\n\
             enable = [\"guidance/mode/research\"]\n"
        ),
    )
    .unwrap();

    let mut overlay = OverlayDocument::open(&path, &session).unwrap();
    overlay.set_base_generation(Some(
        &aikit_core::GenerationId::parse("gen_b71f2fdeadbeef01").unwrap(),
    ));
    overlay.save().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# switched research mode on while reading the migration"));
    assert!(text.contains("gen_b71f2fdeadbeef01"));
    assert!(!text.contains("gen_0000000000000001"));
}

#[test]
fn an_overlay_for_a_different_session_is_refused_rather_than_adopted() {
    // Two sessions sharing an overlay file would be exactly the global mutable
    // active set the architecture refuses to have.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("overlay.toml");
    let mine = aikit_core::SessionId::generate();
    let theirs = aikit_core::SessionId::generate();
    std::fs::write(
        &path,
        format!("schema = 1\nsession_id = \"{theirs}\"\nenable = []\n"),
    )
    .unwrap();

    let error = OverlayDocument::open(&path, &mine).unwrap_err();
    assert_eq!(error.code(), "edit.session_mismatch");
}
