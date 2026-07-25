//! Writable set operations use real directories and remain confined to AIKit.

use std::fs;

use aikit_store::home::AikitHome;
use aikit_store::procedure::ProcedureRunner;
use aikit_store::skillsets::{self, SetFile};

#[test]
fn rename_and_delete_are_real_and_delete_is_recoverable() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("home"));
    home.ensure_layout().unwrap();
    skillsets::create(&home, "old", &[], &[]).unwrap();
    fs::write(skillsets::dir(&home, "old").join("human-note"), "keep me").unwrap();

    let renamed = skillsets::rename(&home, "old", "new").unwrap();
    assert_eq!(renamed.name, "new");
    assert!(!skillsets::dir(&home, "old").exists());
    assert_eq!(
        fs::read_to_string(skillsets::dir(&home, "new").join("human-note")).unwrap(),
        "keep me"
    );

    let recovery = skillsets::delete_to_trash(&home, "new").unwrap();
    assert!(!skillsets::dir(&home, "new").exists());
    assert_eq!(
        fs::read_to_string(recovery.join("human-note")).unwrap(),
        "keep me"
    );
    assert!(recovery.starts_with(home.state().join("trash/skillsets")));
}

#[test]
fn set_names_cannot_escape_the_skillset_root() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("home"));
    home.ensure_layout().unwrap();

    for name in ["../escape", "/absolute", ".", "nested/../../escape", ""] {
        let error = skillsets::create(&home, name, &[], &[]).unwrap_err();
        assert_eq!(error.code(), "skillset.invalid_name", "{name}");
    }
    assert!(!temp.path().join("escape").exists());
}

#[test]
fn every_writable_set_mutation_is_a_real_undoable_procedure() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("home"));
    home.ensure_layout().unwrap();
    let first: aikit_core::CapsuleId = "skill/demo/one".parse().unwrap();
    let second: aikit_core::CapsuleId = "skill/demo/two".parse().unwrap();
    let runner = ProcedureRunner::new(&home);

    let create =
        skillsets::plan_create(&home, "review", std::slice::from_ref(&first), &[]).unwrap();
    runner.run(&create).unwrap();
    assert_eq!(skillsets::load(&home, "review").unwrap().len(), 1);

    let add = skillsets::plan_add(&home, "review", std::slice::from_ref(&second)).unwrap();
    runner.run(&add).unwrap();
    assert_eq!(skillsets::load(&home, "review").unwrap().len(), 2);
    runner.undo(&add.id).unwrap();
    assert_eq!(
        skillsets::load(&home, "review")
            .unwrap()
            .members
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec![first.clone()]
    );

    let remove = skillsets::plan_remove(&home, "review", std::slice::from_ref(&first)).unwrap();
    runner.run(&remove).unwrap();
    assert!(skillsets::load(&home, "review").unwrap().is_empty());
    runner.undo(&remove.id).unwrap();
    assert_eq!(skillsets::load(&home, "review").unwrap().len(), 1);

    let rename = skillsets::plan_rename(&home, "review", "renamed").unwrap();
    runner.run(&rename).unwrap();
    assert!(skillsets::dir(&home, "renamed").is_dir());
    runner.undo(&rename.id).unwrap();
    assert!(skillsets::dir(&home, "review").is_dir());

    let (delete, recovery) = skillsets::plan_delete(&home, "review").unwrap();
    runner.run(&delete).unwrap();
    assert!(!skillsets::dir(&home, "review").exists());
    assert!(recovery.is_dir());
    runner.undo(&delete.id).unwrap();
    assert!(skillsets::dir(&home, "review").is_dir());
    assert!(!recovery.exists());

    runner.undo(&create.id).unwrap();
    assert!(!skillsets::dir(&home, "review").exists());
}

#[test]
fn procedure_mutations_preserve_include_provenance_and_remove_it_at_its_source() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("home"));
    home.ensure_layout().unwrap();
    let set_path = skillsets::dir(&home, "review");
    fs::create_dir_all(&set_path).unwrap();
    let local: aikit_core::CapsuleId = "skill/demo/local".parse().unwrap();
    let included: aikit_core::CapsuleId = "skill/shared/included".parse().unwrap();
    let added: aikit_core::CapsuleId = "skill/demo/added".parse().unwrap();
    fs::write(set_path.join("members"), format!("{local}\n")).unwrap();
    let original_note = SetFile {
        description: "Review across registries".to_string(),
        include: vec![included.clone()],
        order: vec!["included-first".to_string()],
        patterns: vec!["skill/demo/*".to_string()],
    };
    fs::write(
        set_path.join("set.toml"),
        toml::to_string_pretty(&original_note).unwrap(),
    )
    .unwrap();
    let runner = ProcedureRunner::new(&home);

    let add = skillsets::plan_add(&home, "review", &[included.clone(), added.clone()]).unwrap();
    runner.run(&add).unwrap();
    let members = fs::read_to_string(set_path.join("members")).unwrap();
    assert!(members.contains(&added.to_string()));
    assert!(
        !members.contains(&included.to_string()),
        "an include-backed member must not be materialized into members"
    );
    let note_after_add: SetFile =
        toml::from_str(&fs::read_to_string(set_path.join("set.toml")).unwrap()).unwrap();
    assert_eq!(note_after_add, original_note);

    let remove = skillsets::plan_remove(&home, "review", std::slice::from_ref(&included)).unwrap();
    runner.run(&remove).unwrap();
    let note_after_remove: SetFile =
        toml::from_str(&fs::read_to_string(set_path.join("set.toml")).unwrap()).unwrap();
    assert!(note_after_remove.include.is_empty());
    assert_eq!(note_after_remove.description, original_note.description);
    assert_eq!(note_after_remove.order, original_note.order);
    assert_eq!(note_after_remove.patterns, original_note.patterns);
    assert!(!skillsets::load(&home, "review")
        .unwrap()
        .members
        .contains_key(&included));

    runner.undo(&remove.id).unwrap();
    let restored: SetFile =
        toml::from_str(&fs::read_to_string(set_path.join("set.toml")).unwrap()).unwrap();
    assert_eq!(restored, original_note);
    assert!(skillsets::load(&home, "review")
        .unwrap()
        .members
        .contains_key(&included));
}
