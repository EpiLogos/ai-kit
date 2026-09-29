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
        ..SetFile::default()
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

#[test]
fn referenced_children_are_shared_not_copied_and_cycles_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("home"));
    home.ensure_layout().unwrap();
    let vision: aikit_core::CapsuleId = "skill/demo/vision".parse().unwrap();
    let operate: aikit_core::CapsuleId = "skill/demo/operate".parse().unwrap();
    let runner = ProcedureRunner::new(&home);
    skillsets::create(&home, "documentation", std::slice::from_ref(&vision), &[]).unwrap();
    skillsets::create(&home, "author", std::slice::from_ref(&operate), &[]).unwrap();
    skillsets::create(&home, "guardian", &[], &[]).unwrap();

    // Two parents carry the same child by reference.
    for parent in ["author", "guardian"] {
        let plan =
            skillsets::plan_add_children(&home, parent, &["documentation".to_string()]).unwrap();
        runner.run(&plan).unwrap();
    }
    let author = skillsets::load(&home, "author").unwrap();
    assert!(author.all_members().contains(&vision));
    assert_eq!(author.child_refs, vec!["documentation".to_string()]);
    let child = &author.children[0];
    assert_eq!(child.attached_by.as_deref(), Some("documentation"));
    // Never copied into the parent's own membership file.
    let own = fs::read_to_string(skillsets::dir(&home, "author").join("members")).unwrap();
    assert!(!own.contains("skill/demo/vision"));

    // A revision to the child reaches every parent.
    let extra: aikit_core::CapsuleId = "skill/demo/design".parse().unwrap();
    skillsets::add(&home, "documentation", std::slice::from_ref(&extra)).unwrap();
    assert!(skillsets::load(&home, "guardian")
        .unwrap()
        .all_members()
        .contains(&extra));

    // A cycle and a dangling reference are refused before any write.
    let before = fs::read(skillsets::dir(&home, "documentation").join("members")).unwrap();
    let cycle =
        skillsets::plan_add_children(&home, "documentation", &["author".to_string()]).unwrap_err();
    assert_eq!(cycle.code(), "skillset.reference_cycle");
    let dangling =
        skillsets::plan_add_children(&home, "author", &["nowhere".to_string()]).unwrap_err();
    assert_eq!(dangling.code(), "skillset.reference_unresolved");
    assert!(!skillsets::dir(&home, "documentation")
        .join("set.toml")
        .exists());
    assert_eq!(
        fs::read(skillsets::dir(&home, "documentation").join("members")).unwrap(),
        before
    );
}

#[test]
fn registry_sets_resolve_by_semantic_ref_through_a_directory_source() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("home"));
    home.ensure_layout().unwrap();
    // A directory skill source whose owner keeps portable sets beside it.
    let owner = temp.path().join("Central");
    fs::create_dir_all(owner.join("skills")).unwrap();
    fs::create_dir_all(owner.join("skillsets/documentation")).unwrap();
    fs::create_dir_all(owner.join("skillsets/accounts")).unwrap();
    fs::write(
        owner.join("skillsets/index.toml"),
        "schema = 1\n\n[[skillset]]\nsemantic_ref = \"central:documentation\"\ndirectory = \"documentation\"\nchild_refs = [\"central:accounts\"]\n\n[[skillset]]\nsemantic_ref = \"central:accounts\"\ndirectory = \"accounts\"\n",
    )
    .unwrap();
    fs::write(
        owner.join("skillsets/documentation/members"),
        "skill/central/vision\n",
    )
    .unwrap();
    fs::write(
        owner.join("skillsets/accounts/members"),
        "skill/aikit/html-account\n",
    )
    .unwrap();
    fs::create_dir_all(home.root().join("sources/central")).unwrap();
    fs::write(
        home.root().join("sources/central/source.toml"),
        format!(
            "schema = 1\nid = \"central\"\nkind = \"directory\"\npath = \"{}\"\n",
            owner.join("skills").display()
        ),
    )
    .unwrap();

    let set = skillsets::load(&home, "central:documentation").unwrap();
    assert_eq!(set.semantic_ref.as_deref(), Some("central:documentation"));
    assert_eq!(set.all_members().len(), 2);
    assert_eq!(
        set.children[0].attached_by.as_deref(),
        Some("central:accounts")
    );
    assert!(set.revision.is_some());

    // A home set may carry a registry set by semantic ref.
    skillsets::create(&home, "factory-agent", &[], &[]).unwrap();
    let plan = skillsets::plan_add_children(
        &home,
        "factory-agent",
        &["central:documentation".to_string()],
    )
    .unwrap();
    ProcedureRunner::new(&home).run(&plan).unwrap();
    assert_eq!(
        skillsets::load(&home, "factory-agent")
            .unwrap()
            .all_members()
            .len(),
        2
    );
}
