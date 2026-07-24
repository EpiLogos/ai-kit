//! The native Agent Skills form, validated and preserved.
//!
//! A skill is a directory with a required `SKILL.md` carrying `name` and
//! `description` frontmatter, plus optional `scripts/`, `references/` and
//! `assets/`. That structure is *progressive disclosure*: the model reads
//! `SKILL.md` and reaches for the rest only when it needs to. A projection that
//! flattened it, or dropped the subdirectories, would quietly change the skill's
//! behaviour while appearing to work.
//!
//! So the two properties tested here are: an invalid skill is refused by name,
//! and a valid one comes out the other side with its shape intact.

mod common;

use common::*;

use std::path::Path;

use aikit_adapters::clients::agent_skills::{self, AgentSkill};
use aikit_core::projection::MaterializationMode;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[test]
fn a_well_formed_skill_directory_validates_and_reports_its_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let root = write_agent_skill(dir.path(), "code-review", "Reviews Rust for correctness.");

    let skill = agent_skills::validate(&root).unwrap();
    assert_eq!(skill.name, "code-review");
    assert_eq!(skill.description, "Reviews Rust for correctness.");
    assert_eq!(skill.root, root);
}

#[test]
fn the_progressive_disclosure_subdirectories_are_recorded_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let root = write_agent_skill(dir.path(), "code-review", "Reviews Rust.");
    std::fs::create_dir_all(root.join("assets")).unwrap();
    write(&root.join("assets/logo.svg"), "<svg/>");

    let skill = agent_skills::validate(&root).unwrap();
    assert_eq!(skill.disclosure, vec!["assets", "references", "scripts"]);
    assert_eq!(
        skill.files,
        vec![
            "SKILL.md".to_string(),
            "assets/logo.svg".to_string(),
            "references/deep.md".to_string(),
            "scripts/check.sh".to_string(),
        ]
    );
}

#[test]
fn a_minimal_skill_with_only_a_skill_md_is_perfectly_valid() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tiny");
    write(
        &root.join("SKILL.md"),
        "---\nname: tiny\ndescription: Does one thing.\n---\n\nBody.\n",
    );

    let skill = agent_skills::validate(&root).unwrap();
    assert_eq!(skill.files, vec!["SKILL.md".to_string()]);
    assert!(skill.disclosure.is_empty());
}

#[test]
fn a_directory_without_a_skill_md_is_refused_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("empty");
    std::fs::create_dir_all(&root).unwrap();

    let error = agent_skills::validate(&root).unwrap_err();
    assert_eq!(error.code(), "skill.invalid");
    assert!(
        error.message().contains("SKILL.md"),
        "the message has to name the missing file: {}",
        error.message()
    );
}

#[test]
fn a_skill_md_with_no_frontmatter_at_all_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("bare");
    write(&root.join("SKILL.md"), "# Just a heading\n\nNo frontmatter.\n");

    let error = agent_skills::validate(&root).unwrap_err();
    assert_eq!(error.code(), "skill.invalid");
    assert!(error.message().contains("frontmatter"));
}

#[test]
fn an_unterminated_frontmatter_block_is_refused_rather_than_read_as_a_body() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("unterminated");
    write(
        &root.join("SKILL.md"),
        "---\nname: x\ndescription: y\n\n# Where did the closing marker go\n",
    );

    let error = agent_skills::validate(&root).unwrap_err();
    assert_eq!(error.code(), "skill.invalid");
    assert!(error.message().contains("closed"), "got: {}", error.message());
}

#[test]
fn a_missing_name_and_a_missing_description_are_each_named_specifically() {
    let dir = tempfile::tempdir().unwrap();

    let no_name = dir.path().join("no-name");
    write(&no_name.join("SKILL.md"), "---\ndescription: Does a thing.\n---\n");
    let error = agent_skills::validate(&no_name).unwrap_err();
    assert_eq!(error.code(), "skill.invalid");
    assert!(error.message().contains("name"), "got: {}", error.message());

    let no_description = dir.path().join("no-description");
    write(&no_description.join("SKILL.md"), "---\nname: thing\n---\n");
    let error = agent_skills::validate(&no_description).unwrap_err();
    assert_eq!(error.code(), "skill.invalid");
    assert!(
        error.message().contains("description"),
        "a skill with no description cannot be chosen by a model: {}",
        error.message()
    );
}

#[test]
fn an_empty_description_is_as_bad_as_a_missing_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("blank");
    write(&root.join("SKILL.md"), "---\nname: blank\ndescription: \"  \"\n---\n");

    assert_eq!(
        agent_skills::validate(&root).unwrap_err().code(),
        "skill.invalid"
    );
}

#[test]
fn a_name_that_would_not_survive_becoming_a_directory_is_refused() {
    // The name becomes a directory under `.claude/skills/`; a name with a slash
    // in it would put the projection somewhere nobody asked for.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("sneaky");
    write(
        &root.join("SKILL.md"),
        "---\nname: ../../etc/passwd\ndescription: Nope.\n---\n",
    );

    let error = agent_skills::validate(&root).unwrap_err();
    assert_eq!(error.code(), "skill.invalid");
    assert!(error.message().contains("name"));
}

#[test]
fn quoted_values_and_extra_frontmatter_keys_are_handled_without_complaint() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("quoted");
    write(
        &root.join("SKILL.md"),
        "---\nname: \"code-review\"\ndescription: 'Reviews: carefully, and well.'\n\
         license: MIT\nallowed-tools: [Read, Grep]\n---\n\nBody\n",
    );

    let skill = agent_skills::validate(&root).unwrap();
    assert_eq!(skill.name, "code-review");
    assert_eq!(
        skill.description, "Reviews: carefully, and well.",
        "a colon inside a value must not truncate it"
    );
}

// ---------------------------------------------------------------------------
// Projection preserves the shape
// ---------------------------------------------------------------------------

#[test]
fn a_linked_projection_is_one_item_because_the_directory_goes_across_whole() {
    let dir = tempfile::tempdir().unwrap();
    let root = write_agent_skill(dir.path(), "code-review", "Reviews Rust.");
    let skill = agent_skills::validate(&root).unwrap();

    let items = skill
        .project(Path::new(".claude/skills"), MaterializationMode::Link)
        .unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].destination().unwrap(),
        Path::new(".claude/skills/code-review")
    );
}

#[test]
fn a_copied_projection_reproduces_every_file_in_its_own_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    let root = write_agent_skill(dir.path(), "code-review", "Reviews Rust.");
    let skill = agent_skills::validate(&root).unwrap();

    let items = skill
        .project(Path::new(".claude/skills"), MaterializationMode::Copy)
        .unwrap();

    let destinations: Vec<String> = items
        .iter()
        .filter_map(|i| i.destination())
        .map(|p| p.display().to_string())
        .collect();
    assert_eq!(
        destinations,
        vec![
            ".claude/skills/code-review/SKILL.md",
            ".claude/skills/code-review/references/deep.md",
            ".claude/skills/code-review/scripts/check.sh",
        ],
        "flattening the tree would break progressive disclosure"
    );
}

#[test]
fn a_projected_copy_really_reproduces_the_tree_on_disk() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = write_agent_skill(source.path(), "code-review", "Reviews Rust.");
    let skill = agent_skills::validate(&root).unwrap();

    let items = skill
        .project(Path::new(".claude/skills"), MaterializationMode::Copy)
        .unwrap();
    materialize(&items, target.path());

    assert_eq!(
        tree_of(&target.path().join(".claude/skills/code-review")),
        vec![
            "SKILL.md".to_string(),
            "references/".to_string(),
            "references/deep.md".to_string(),
            "scripts/".to_string(),
            "scripts/check.sh".to_string(),
        ]
    );
    let projected = std::fs::read_to_string(
        target.path().join(".claude/skills/code-review/SKILL.md"),
    )
    .unwrap();
    assert!(
        projected.contains("name: code-review"),
        "the frontmatter has to survive the trip"
    );
}

#[test]
fn a_projection_that_would_escape_its_root_is_impossible_to_construct() {
    // Not a runtime check in the writer: `AgentSkill::project` builds items
    // through the validating constructors, so the escape is refused at plan time.
    let dir = tempfile::tempdir().unwrap();
    let root = write_agent_skill(dir.path(), "code-review", "Reviews Rust.");
    let skill = agent_skills::validate(&root).unwrap();

    let error = skill
        .project(Path::new("../../elsewhere"), MaterializationMode::Link)
        .unwrap_err();
    assert_eq!(error.code(), "projection.destination_escapes_root");
}

#[test]
fn the_export_name_can_be_overridden_without_touching_the_skill_on_disk() {
    // Two registries can both ship a `code-review`; the export name is how a
    // collision is resolved, and it must not require editing the payload.
    let dir = tempfile::tempdir().unwrap();
    let root = write_agent_skill(dir.path(), "code-review", "Reviews Rust.");
    let skill = AgentSkill {
        name: "rust-code-review".to_string(),
        ..agent_skills::validate(&root).unwrap()
    };

    let items = skill
        .project(Path::new(".claude/skills"), MaterializationMode::Link)
        .unwrap();
    assert_eq!(
        items[0].destination().unwrap(),
        Path::new(".claude/skills/rust-code-review")
    );
    assert!(
        std::fs::read_to_string(root.join("SKILL.md"))
            .unwrap()
            .contains("name: code-review"),
        "renaming an export must not rewrite the capsule payload"
    );
}
