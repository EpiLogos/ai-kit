//! Real application operations over filesystem-backed registries, scoped
//! declarations, Procedures, generations and the existing package exporter.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use aikit_cli::app::repertoire::{RepertoirePreview, RepertoireRequest};
use aikit_cli::app::{AikitApplication, ApplyRequest, Service};
use aikit_cli::skillset_package_cli::{self, SetPackageArgs, SetPackageCmd, SetPackageSub};
use aikit_core::scope::ScopeKind;
use aikit_core::{CapsuleId, ProfileId};
use aikit_store::generation;
use aikit_store::home::AikitHome;
use aikit_store::procedure::ProcedureRunner;
use tempfile::TempDir;

const CONTEXT: &str = "ctx_01HZYREPERTOIRE0000000000";
const SESSION: &str = "ses_01HZYREPERTOIRE0000000000";

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

struct World {
    owned: TempDir,
    home: AikitHome,
    project: PathBuf,
}
impl World {
    fn new() -> Self {
        let owned = tempfile::tempdir().unwrap();
        let home = AikitHome::at(owned.path().join("home"));
        let project = owned.path().join("project");
        for (name, description) in [
            ("plain", "Read the actual project."),
            ("method", "METHOD: Carry the change to Return."),
            ("field", "METHODOLOGY: Orient the development field."),
            ("withheld", "An intentionally unselected member."),
        ] {
            let id = format!("skill/development/{name}");
            let root = home.root().join("registries/personal/capsules").join(&id);
            write(&root.join("manifest.toml"), &format!("schema = 1\nid = \"{id}\"\nkind = \"skill\"\nname = \"{name}\"\ndescription = \"{description}\"\n[skill]\nroot = \"payload\"\n"));
            write(&root.join("payload/SKILL.md"), &format!("---\nname: {name}\ndescription: \"{description}\"\n---\n\n# {name}\n\nUse the project's own source and report real evidence.\n"));
            write(
                &root.join("payload/references/checks.md"),
                "Run the owner's focused checks before publication.\n",
            );
        }
        write(&home.root().join("registries/personal/profiles/development.toml"), "schema = 1\nid = \"profile/development\"\nenable = [\"skill/development/plain\", \"skill/development/method\", \"skill/development/field\"]\n");
        write(
            &project.join(".aikit/profile.toml"),
            "# authored project contribution\nschema = 1\n",
        );
        write(
            &project.join("CLAUDE.md"),
            "Human-owned project instructions remain intact.\n",
        );
        write(
            &project.join("AGENTS.md"),
            "Foreign harness instructions remain intact.\n",
        );
        aikit_store::skillsets::create(&home, "implementer", &[id("plain"), id("method")], &[])
            .unwrap();
        aikit_store::skillsets::create(&home, "orientation", &[id("field")], &[]).unwrap();
        write(&home.root().join("skillsets/implementer/set.toml"), "[package.author]\nname = \"Development repertoire source\"\n\n[[package.environment]]\nname = \"PROJECT_ROOT\"\npurpose = \"the project against which the practice acts\"\n");
        home.ensure_layout().unwrap();
        let load = aikit_cli::app::load_catalog(&home, Some(&project)).unwrap();
        let index = aikit_store::index::Index::open(&home.database()).unwrap();
        use aikit_core::catalog::Catalog;
        for capsule in load.catalog.capsules() {
            let key = aikit_core::TrustKey::new(
                capsule.source.clone().unwrap(),
                capsule.id.clone(),
                capsule.revision.clone().unwrap(),
            );
            aikit_store::trust::TrustStore::new(&index)
                .record(
                    &key,
                    aikit_core::TrustState::Reviewed,
                    Some("test's exact authored source review"),
                )
                .unwrap();
        }
        Self {
            owned,
            home,
            project,
        }
    }
    fn service(&self) -> Service {
        let env: BTreeMap<String, String> =
            [("AIKIT_CONTEXT_ID", CONTEXT), ("AIKIT_SESSION_ID", SESSION)]
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect();
        Service::open(self.home.clone(), &self.project, |key| {
            env.get(key).cloned()
        })
        .unwrap()
    }
    fn request(&self, scope: ScopeKind, sets: &[&str], profile: bool) -> RepertoireRequest {
        RepertoireRequest {
            scope,
            profile: profile.then(|| ProfileId::parse("profile/development").unwrap()),
            skill_sets: sets.iter().map(|reference| (*reference).into()).collect(),
        }
    }
    fn compose(&self) -> (Service, aikit_cli::app::repertoire::RepertoireApplication) {
        let mut service = self.service();
        let preview = service
            .preview_repertoire(self.request(ScopeKind::Project, &["implementer"], true))
            .unwrap();
        let result = service.apply_repertoire(preview).unwrap();
        (service, result)
    }
}
fn id(name: &str) -> CapsuleId {
    CapsuleId::parse(&format!("skill/development/{name}")).unwrap()
}
fn empty_request() -> RepertoireRequest {
    RepertoireRequest {
        scope: ScopeKind::Project,
        profile: None,
        skill_sets: vec![],
    }
}

#[test]
fn profile_precedence_and_project_session_set_union_share_one_inspectable_reading() {
    let world = World::new();
    let (mut service, first) = world.compose();
    assert_eq!(first.reading.skill_sets, ["implementer"]);
    assert_eq!(first.reading.members.len(), 2);
    assert!(first
        .reading
        .members
        .iter()
        .all(|member| member.projected && member.revision.is_some()));
    let preview = service
        .preview_repertoire(world.request(ScopeKind::Session, &["orientation"], false))
        .unwrap();
    assert_eq!(preview.reading.skill_sets, ["implementer", "orientation"]);
    assert!(
        !world
            .home
            .session_overlay(&aikit_core::SessionId::parse(SESSION).unwrap())
            .exists(),
        "preview is write-free"
    );
    let applied = service.apply_repertoire(preview).unwrap();
    let forms: BTreeMap<_, _> = applied
        .reading
        .members
        .iter()
        .map(|member| (member.id.clone(), member.practice.as_str()))
        .collect();
    assert_eq!(forms[&id("plain")], "Skill");
    assert_eq!(forms[&id("method")], "METHOD");
    assert_eq!(forms[&id("field")], "METHODOLOGY");
    // Sets cannot override a higher-scope Profile exclusion or grant eligibility.
    write(&world.home.session_overlay(&aikit_core::SessionId::parse(SESSION).unwrap()), &format!("schema = 1\nsession_id = \"{SESSION}\"\nskill_sets = [\"orientation\"]\ndisable = [\"skill/development/plain\"]\n"));
    service.refresh().unwrap();
    let reading = service.resolved_repertoire().unwrap();
    let plain = reading
        .members
        .iter()
        .find(|member| member.id == id("plain"))
        .unwrap();
    assert!(!plain.projected);
    assert!(reading
        .render()
        .contains("withheld skill/development/plain"));
    assert!(reading
        .package_commands
        .iter()
        .all(|command| command.contains("set package") && command.contains(CONTEXT)));
    let structured = serde_json::to_value(&reading).unwrap();
    assert_eq!(structured["schema"], "aikit.resolved-repertoire/v1");
    assert_eq!(
        structured["members"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|member| member["projected"] == true)
            .count(),
        2
    );
    assert!(
        fs::read_to_string(world.project.join(".aikit/profile.toml"))
            .unwrap()
            .starts_with("# authored project contribution")
    );
}

#[test]
fn unchanged_reapply_writes_nothing_and_repairs_a_missing_second_target_at_the_same_revision() {
    let world = World::new();
    let (mut service, first) = world.compose();
    let current = world
        .home
        .context_dir(&service.descriptor().context_id)
        .join("current");
    let claude = current.join("projections/claude/.claude/skills/plain");
    let pi = current.join("projections/pi/.pi/skills/plain");
    let before = fs::symlink_metadata(&claude).unwrap().modified().unwrap();
    let pi_before = fs::symlink_metadata(&pi).unwrap().modified().unwrap();
    let declarations = fs::metadata(world.project.join(".aikit/profile.toml"))
        .unwrap()
        .modified()
        .unwrap();
    let preview = service.preview_repertoire(empty_request()).unwrap();
    assert!(preview.diff.edits.is_empty());
    let reapplied = service.apply_repertoire(preview).unwrap();
    assert_eq!(first.reading.generation, reapplied.reading.generation);
    assert_eq!(reapplied.applied_edits, 0);
    assert_eq!(
        before,
        fs::symlink_metadata(&claude).unwrap().modified().unwrap()
    );
    assert_eq!(
        pi_before,
        fs::symlink_metadata(&pi).unwrap().modified().unwrap()
    );
    assert_eq!(
        declarations,
        fs::metadata(world.project.join(".aikit/profile.toml"))
            .unwrap()
            .modified()
            .unwrap()
    );
    fs::remove_file(&pi).unwrap();
    let accepted = first.reading.members.clone();
    let preview = service.preview_repertoire(empty_request()).unwrap();
    let repaired = service.apply_repertoire(preview).unwrap();
    assert!(pi.join("SKILL.md").is_file());
    assert_eq!(accepted, repaired.reading.members);
    assert_eq!(
        before,
        fs::symlink_metadata(&claude).unwrap().modified().unwrap(),
        "good destination stays untouched"
    );
    assert_eq!(first.reading.generation, repaired.reading.generation);
    assert!(world.project.join(".pi/skills/plain/SKILL.md").is_file());
}

#[test]
fn failed_source_and_stale_preview_leave_accepted_projection_and_authored_bytes_intact() {
    let world = World::new();
    let (mut service, first) = world.compose();
    let declaration = fs::read(world.project.join(".aikit/profile.toml")).unwrap();
    let preview = service
        .preview_repertoire(world.request(ScopeKind::Session, &["orientation"], false))
        .unwrap();
    let payload = world
        .home
        .root()
        .join("registries/personal/capsules/skill/development/field/payload/SKILL.md");
    fs::remove_file(&payload).unwrap();
    let error = service.apply_repertoire(preview).unwrap_err();
    assert_eq!(error.code(), "composition.preview_stale", "{error:?}");
    let source_changes: serde_json::Value =
        serde_json::from_str(&error.details()["source_changes"]).unwrap();
    let changed = source_changes
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["id"] == id("field").to_string())
        .unwrap();
    assert!(changed["reviewed_revision"].is_string());
    assert_ne!(changed["reviewed_revision"], changed["observed_revision"]);
    assert_eq!(
        generation::current(&world.home.context_dir(&service.descriptor().context_id)).unwrap(),
        first.reading.generation
    );
    assert_eq!(
        fs::read(world.project.join(".aikit/profile.toml")).unwrap(),
        declaration
    );
    assert!(!world
        .home
        .session_overlay(&aikit_core::SessionId::parse(SESSION).unwrap())
        .exists());
    assert_eq!(
        fs::read_to_string(world.project.join("CLAUDE.md")).unwrap(),
        "Human-owned project instructions remain intact.\n"
    );
}

#[test]
fn interrupted_after_declaration_recovers_through_the_same_procedure_without_duplicate_edits() {
    let world = World::new();
    let service = world.service();
    let preview = service
        .preview_repertoire(world.request(ScopeKind::Project, &["implementer"], true))
        .unwrap();
    let reviewed_id = preview.procedure.id.clone();
    // Execute the actual durable Procedure, then stop before generation commit.
    ProcedureRunner::new(&world.home)
        .run(&preview.procedure)
        .unwrap();
    let retained = serde_json::to_vec(&preview).unwrap();
    drop(service);
    let mut restarted = world.service();
    let preview: RepertoirePreview = serde_json::from_slice(&retained).unwrap();
    let recovered = restarted.apply_repertoire(preview.clone()).unwrap();
    assert_eq!(recovered.procedure, reviewed_id);
    assert_eq!(recovered.applied_edits, 0);
    assert!(recovered.recovered);
    assert!(world.project.join(".pi/skills/plain/SKILL.md").is_file());
    // Uncertain delivery after the effect is read back, not repeated.
    let again = restarted.apply_repertoire(preview).unwrap();
    assert!(again.recovered);
    assert_eq!(again.reading.generation, recovered.reading.generation);
    assert_eq!(again.applied_edits, 0);
}

#[test]
fn procedure_undo_and_generation_rollback_restore_managed_state_and_preserve_foreign_instructions()
{
    let world = World::new();
    let (mut service, first) = world.compose();
    let before = fs::read(world.project.join(".aikit/profile.toml")).unwrap();
    let preview = service
        .preview_repertoire(world.request(ScopeKind::Session, &["orientation"], false))
        .unwrap();
    let second = service.apply_repertoire(preview).unwrap();
    assert_ne!(first.reading.generation, second.reading.generation);
    ProcedureRunner::new(&world.home)
        .undo(&second.procedure)
        .unwrap();
    let restored = service.rollback().unwrap();
    service.refresh().unwrap();
    assert_eq!(Some(restored.now_current), first.reading.generation);
    assert_eq!(
        service.resolved_repertoire().unwrap().skill_sets,
        ["implementer"]
    );
    assert_eq!(
        fs::read(world.project.join(".aikit/profile.toml")).unwrap(),
        before
    );
    assert_eq!(
        fs::read_to_string(world.project.join("AGENTS.md")).unwrap(),
        "Foreign harness instructions remain intact.\n"
    );
    assert!(world.project.join(".pi/skills/plain/SKILL.md").is_file());
}

fn package_args(out: &Path) -> SetPackageArgs {
    SetPackageArgs {
        set: ".".into(),
        target: Some("pi".into()),
        out: Some(out.to_path_buf()),
        native: false,
        with_codex_overlay: false,
        allow_partial: false,
        receipt: None,
    }
}
fn source_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            (
                entry.path().strip_prefix(root).unwrap().to_path_buf(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn effective_repertoire_uses_the_existing_package_export_verify_and_diff_without_source_writes() {
    let world = World::new();
    let (mut service, _) = world.compose();
    let preview = service
        .preview_repertoire(world.request(ScopeKind::Session, &["orientation"], false))
        .unwrap();
    let applied = service.apply_repertoire(preview).unwrap();
    let before_registry = source_snapshot(&world.home.registries());
    let before_sets = source_snapshot(&world.home.root().join("skillsets"));
    let out = world.owned.path().join("portable-pi");
    let receipt = skillset_package_cli::run(
        &service,
        SetPackageCmd {
            command: SetPackageSub::Export(package_args(&out)),
        },
    )
    .unwrap();
    assert_eq!(receipt["source_unchanged"], true);
    assert!(out.join("package.json").is_file());
    let inspected = skillset_package_cli::load_package(&service, ".").unwrap();
    let revisions: BTreeMap<_, _> = inspected
        .package
        .members
        .iter()
        .map(|member| (member.id.clone(), member.revision.clone()))
        .collect();
    for member in &applied.reading.members {
        assert_eq!(
            revisions[&member.id.to_string()],
            member.revision.clone().unwrap()
        );
    }
    skillset_package_cli::run(
        &service,
        SetPackageCmd {
            command: SetPackageSub::Verify(package_args(&out)),
        },
    )
    .unwrap();
    let diff = skillset_package_cli::run(
        &service,
        SetPackageCmd {
            command: SetPackageSub::Diff(package_args(&out)),
        },
    )
    .unwrap();
    assert_eq!(diff["current"], true, "{diff}");
    for key in ["changed", "missing", "extra"] {
        assert_eq!(diff["files"][key].as_array().unwrap().len(), 0, "{diff}");
    }
    assert_eq!(before_registry, source_snapshot(&world.home.registries()));
    assert_eq!(
        before_sets,
        source_snapshot(&world.home.root().join("skillsets"))
    );
}

#[test]
fn a_foreign_pi_tree_is_refused_before_scope_or_generation_changes() {
    let world = World::new();
    write(
        &world.project.join(".pi/skills/foreign/SKILL.md"),
        "foreign bytes\n",
    );
    let service = world.service();
    let before = fs::read(world.project.join(".aikit/profile.toml")).unwrap();
    let error = service
        .preview_repertoire(world.request(ScopeKind::Project, &["implementer"], true))
        .unwrap_err();
    assert_eq!(error.code(), "projection.pi_tree_owned");
    assert_eq!(
        before,
        fs::read(world.project.join(".aikit/profile.toml")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(world.project.join(".pi/skills/foreign/SKILL.md")).unwrap(),
        "foreign bytes\n"
    );
    assert_eq!(
        generation::current(&world.home.context_dir(&service.descriptor().context_id)).unwrap(),
        None
    );
}

#[test]
fn existing_apply_also_projects_pi_and_reuses_unchanged_material() {
    let world = World::new();
    let (mut service, first) = world.compose();
    let applied = service
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .unwrap();
    assert_eq!(Some(applied.id), first.reading.generation);
    assert!(world.project.join(".pi/skills/plain/SKILL.md").is_file());
}

#[test]
fn effective_orientation_changes_export_provenance_without_changing_accepted_sources() {
    let world = World::new();
    let (mut service, _) = world.compose();
    let before = skillset_package_cli::load_package(&service, ".")
        .unwrap()
        .package;
    let sources = source_snapshot(&world.home.registries());
    let revision = before
        .members
        .iter()
        .find(|member| member.id == id("plain").to_string())
        .unwrap()
        .revision
        .clone();
    service
        .set_skill_usage_overlay(
            &id("plain"),
            ScopeKind::Session,
            &aikit_core::profile::SkillUsageOverlayPatch {
                inherit: true,
                description: Some("Inspect the exact candidate before a Return.".into()),
                guidance: Some(
                    "Retain the selected verification source and report its exact revision.".into(),
                ),
                reviewed_against: None,
            },
        )
        .unwrap();
    let after = skillset_package_cli::load_package(&service, ".")
        .unwrap()
        .package;
    assert_ne!(before.source_revision, after.source_revision);
    let member = after
        .members
        .iter()
        .find(|member| member.id == id("plain").to_string())
        .unwrap();
    assert_eq!(revision, member.revision);
    assert!(member
        .description
        .contains("Inspect the exact candidate before a Return."));
    let markdown = member
        .files
        .iter()
        .find(|file| file.path == "SKILL.md")
        .unwrap()
        .inline
        .as_ref()
        .unwrap();
    assert!(String::from_utf8_lossy(markdown).contains("Retain the selected verification source"));
    let context_dir = world.home.context_dir(&service.descriptor().context_id);
    for target in ["claude/.claude", "pi/.pi"] {
        let effective = fs::read(context_dir.join(format!(
            "current/projections/{target}/skills/plain/SKILL.md"
        )))
        .unwrap();
        assert_eq!(&effective, markdown);
    }
    assert_eq!(sources, source_snapshot(&world.home.registries()));
}

#[test]
fn effective_package_keeps_additive_metadata_and_reports_conflicting_owner_claims() {
    let world = World::new();
    let (mut service, _) = world.compose();
    write(&world.home.root().join("skillsets/orientation/set.toml"), "[[package.environment]]\nname = \"VERIFY_ROOT\"\npurpose = \"the selected verification ground\"\n");
    let preview = service
        .preview_repertoire(world.request(ScopeKind::Session, &["orientation"], false))
        .unwrap();
    service.apply_repertoire(preview).unwrap();
    let package = skillset_package_cli::load_package(&service, ".")
        .unwrap()
        .package;
    assert_eq!(
        package
            .environment
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["PROJECT_ROOT", "VERIFY_ROOT"]
    );
    write(
        &world.home.root().join("skillsets/orientation/set.toml"),
        "[package.author]\nname = \"A conflicting author claim\"\n",
    );
    let error = skillset_package_cli::load_package(&service, ".")
        .err()
        .expect("conflicting metadata must remain explicit");
    assert_eq!(error.code(), "skillset.package.metadata_conflict");
}

#[test]
fn retained_generation_repairs_missing_provenance_and_avoids_full_rebuild_material() {
    use aikit_adapters::clients::{
        claude::ClaudeAdapter, codex::CodexAdapter, dsh::DshAdapter, pi::PiAdapter,
    };
    use aikit_core::platform::TargetId;
    use aikit_core::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};
    let world = World::new();
    let (mut service, first) = world.compose();
    let context_dir = world.home.context_dir(&service.descriptor().context_id);
    let context = service.projection_context().unwrap();
    let plans = [
        ProjectionPlan::new(TargetId::shell(), ActivationEffect::immediate("shell bin/")),
        ClaudeAdapter::new(context_dir.join("projections/claude"))
            .plan(&context)
            .unwrap(),
        PiAdapter::new(context_dir.join("projections/pi"))
            .plan(&context)
            .unwrap(),
        CodexAdapter::new(world.project.clone())
            .plan(&context)
            .unwrap(),
        DshAdapter::new(context_dir.join("projections/dsh"))
            .plan(&context)
            .unwrap(),
    ];
    let before_clock = std::time::Instant::now();
    let full_observer = generation::GenerationObserver::default();
    let full = generation::GenerationBuilder::new()
        .with_observer(full_observer.clone())
        .build(&context_dir, service.resolved(), &plans)
        .unwrap();
    let before_entries = walkdir::WalkDir::new(full.path())
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.depth() > 0)
        .count();
    assert!(before_entries > 0);
    full.commit(first.reading.generation.as_ref()).unwrap();
    let before_ms = before_clock.elapsed().as_millis();
    let full_work = full_observer.snapshot();
    assert!(full_work.target_content_writes > 0);
    assert!(full_work.destination_entries_inspected > 0);
    assert!(full_work.tree_hash_operations > 1);
    let retained_observer = generation::GenerationObserver::default();
    let retained_clock = std::time::Instant::now();
    generation::GenerationBuilder::new()
        .with_observer(retained_observer.clone())
        .reuse_current(
            &context_dir,
            service.resolved(),
            &plans,
            first.reading.generation.as_ref(),
        )
        .unwrap()
        .unwrap();
    let retained_ms = retained_clock.elapsed().as_millis();
    let retained_work = retained_observer.snapshot();
    assert_eq!(retained_work.target_content_writes, 0);
    assert_eq!(retained_work.target_link_writes, 0);
    assert_eq!(retained_work.tree_hash_operations, 1);
    assert!(retained_work.destination_checks > 0);
    assert!(retained_work.destination_entries_inspected > 0);
    assert!(retained_work.canonical_file_reads < retained_work.payload_hash_operations,
        "shared accepted files are actually read once per scan, while every destination contributes to identity");
    assert!(retained_work.destination_entries_inspected < full_work.destination_entries_inspected);
    assert!(retained_work.canonical_file_reads < full_work.canonical_file_reads);
    let generations = context_dir.join("generations");
    let directory_mtime = fs::metadata(&generations).unwrap().modified().unwrap();
    let clock = std::time::Instant::now();
    let reapplied = service
        .apply_repertoire(service.preview_repertoire(empty_request()).unwrap())
        .unwrap();
    let after_ms = clock.elapsed().as_millis();
    assert!(reapplied.reused_generation);
    let application_work = reapplied.observation.as_ref().unwrap();
    assert_eq!(application_work.generation, retained_work);
    assert_eq!(
        directory_mtime,
        fs::metadata(&generations).unwrap().modified().unwrap(),
        "unchanged reapply creates no staging material"
    );
    eprintln!("repertoire material measurement: full_build_entries={before_entries} retained_new_staging_entries=0 full_build_commit_ms={before_ms} retained_generation_ms={retained_ms} retained_preview_apply_ms={after_ms} managed_edits={} full_work={} retained_work={} application_observation={}", reapplied.applied_edits, serde_json::to_string(&full_work).unwrap(), serde_json::to_string(&retained_work).unwrap(), serde_json::to_string(application_work).unwrap());
    fs::remove_file(context_dir.join("current/metadata.json")).unwrap();
    fs::remove_file(context_dir.join("current/resolution.lock.toml")).unwrap();
    let repaired = service
        .apply_repertoire(service.preview_repertoire(empty_request()).unwrap())
        .unwrap();
    assert_eq!(first.reading.generation, repaired.reading.generation);
    assert!(!repaired.reused_generation);
    generation::read_metadata(&context_dir.join("current")).unwrap();
    generation::read_lock(&context_dir.join("current")).unwrap();
    assert_eq!(repaired.applied_edits, 0);
}

#[test]
fn human_json_and_native_tui_backend_receive_the_identical_application_reading() {
    use aikit_tui::backend::PaletteBackend;
    let world = World::new();
    let (mut service, applied) = world.compose();
    let native_tui = PaletteBackend::repertoire_reading(&service)
        .unwrap()
        .unwrap();
    let result = aikit_cli::app::repertoire::run(
        &mut service,
        aikit_cli::app::repertoire::RepertoireArgs {
            profile: None,
            skill_sets: vec![],
            scope: "project".into(),
            apply: true,
            from_preview: None,
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&native_tui).unwrap(),
        result["reading"]
    );
    assert_eq!(native_tui, applied.reading);
    assert!(result["human"]
        .as_str()
        .unwrap()
        .starts_with(&native_tui.render()));
    let observation: aikit_cli::app::repertoire::RepertoireApplicationObservation =
        serde_json::from_value(result["application"]["observation"].clone()).unwrap();
    assert_eq!(observation.generation.target_content_writes, 0);
    assert!(result["human"]
        .as_str()
        .unwrap()
        .contains(&observation.render()));
}

#[test]
fn actual_failed_material_and_application_keep_truthful_operation_observations() {
    use aikit_core::platform::TargetId;
    use aikit_core::projection::{ActivationEffect, ProjectionItem, ProjectionPlan};
    let world = World::new();
    let mut service = world.service();
    let context_dir = world.home.context_dir(&service.descriptor().context_id);
    let source_before = source_snapshot(&world.home.root().join("registries"));
    let observer = generation::GenerationObserver::default();
    let mut plan = ProjectionPlan::new(
        TargetId::shell(),
        ActivationEffect::immediate("actual material"),
    );
    plan.items.push(ProjectionItem::Write {
        path: PathBuf::from("completed-before-refusal.txt"),
        contents: "The actual first write completed before the missing source.\n".into(),
    });
    plan.items.push(ProjectionItem::Copy {
        from: world.project.join("genuinely-absent-source"),
        to: PathBuf::from("never-copied.txt"),
    });
    let error = generation::GenerationBuilder::new()
        .with_observer(observer.clone())
        .build(&context_dir, service.resolved(), &[plan])
        .err()
        .unwrap();
    assert_eq!(error.code(), "generation.source_missing");
    let observed = observer.snapshot();
    let retained_failure: generation::GenerationObservation =
        serde_json::from_str(&error.details()["generation.observation"]).unwrap();
    assert_eq!(retained_failure, observed);
    assert_eq!(observed.target_content_writes, 1);
    assert_eq!(observed.target_link_writes, 0);
    assert_eq!(observed.tree_hash_operations, 0);
    assert_eq!(generation::current(&context_dir).unwrap(), None);
    assert!(fs::read_dir(context_dir.join("generations"))
        .unwrap()
        .next()
        .is_none());
    assert_eq!(
        source_before,
        source_snapshot(&world.home.root().join("registries"))
    );

    let mut preview = service
        .preview_repertoire(world.request(ScopeKind::Project, &["implementer"], true))
        .unwrap();
    preview.reading.targets.clear();
    let error = service.apply_repertoire(preview).unwrap_err();
    assert_eq!(error.code(), "composition.preview_stale");
    let failed: aikit_cli::app::repertoire::RepertoireApplicationObservation =
        serde_json::from_str(&error.details()["composition.application_observation"]).unwrap();
    assert_eq!(
        failed.generation,
        generation::GenerationObservation::default()
    );
    assert_eq!(generation::current(&context_dir).unwrap(), None);
}

#[test]
fn native_tui_profile_and_skillset_selection_retains_and_applies_the_actual_procedure() {
    use aikit_tui::application::{Overlay, TuiRuntime, TuiState, UiAction};
    use aikit_tui::application_service::ApplicationService;
    use aikit_tui::backend::PaletteBackend;
    let world = World::new();
    let mut owner = world.service();
    let profile = ProfileId::parse("profile/development").unwrap();
    assert!(PaletteBackend::repertoire_profiles(&owner).contains(&profile));
    let context_dir = world.home.context_dir(&owner.descriptor().context_id);
    let project_before = fs::read(world.project.join(".aikit/profile.toml")).unwrap();
    let claude_before = fs::read(world.project.join("CLAUDE.md")).unwrap();
    let agents_before = fs::read(world.project.join("AGENTS.md")).unwrap();
    let sources_before = source_snapshot(&world.home.root().join("registries"));
    let application = {
        let mut service = ApplicationService::new(&mut owner);
        let mut runtime = TuiRuntime::new();
        let mut state = TuiState::default();
        for action in [
            UiAction::SelectComposeProfile(Some(profile.clone())),
            UiAction::ToggleComposeSkillSet {
                name: "implementer".into(),
            },
            UiAction::ToggleComposeSkillSet {
                name: "orientation".into(),
            },
            UiAction::SetMutationScope(ScopeKind::Project),
            UiAction::RequestRepertoirePreview,
        ] {
            state = runtime.step(&mut service, state, action).unwrap();
        }
        let preview = state.repertoire_preview.clone().unwrap();
        assert_eq!(preview.request.profile, Some(profile));
        assert_eq!(
            preview.request.skill_sets,
            vec!["implementer", "orientation"]
        );
        assert_eq!(preview.request.scope, ScopeKind::Project);
        assert_eq!(
            preview
                .reading
                .members
                .iter()
                .filter(|member| member.projected)
                .count(),
            3
        );
        assert_eq!(generation::current(&context_dir).unwrap(), None);
        assert_eq!(
            project_before,
            fs::read(world.project.join(".aikit/profile.toml")).unwrap()
        );
        state = runtime
            .step(&mut service, state, UiAction::RequestApply)
            .unwrap();
        assert_eq!(state.overlay, Some(Overlay::ConfirmApply));
        assert_eq!(
            state.repertoire_preview.as_ref().unwrap().procedure.id,
            preview.procedure.id
        );
        assert_eq!(generation::current(&context_dir).unwrap(), None);
        state = runtime
            .step(&mut service, state, UiAction::ConfirmApply)
            .unwrap();
        let applied = state.repertoire_application.unwrap();
        assert_eq!(applied.procedure, preview.procedure.id);
        assert_eq!(state.status.unwrap().message, applied.render());
        assert!(state.repertoire_preview.is_none());
        assert!(state.overlay.is_none());
        assert!(
            applied
                .observation
                .as_ref()
                .unwrap()
                .generation
                .target_content_writes
                > 0
        );
        let json = serde_json::to_value(&applied).unwrap();
        let structured: aikit_core::repertoire::RepertoireApplication =
            serde_json::from_value(json.clone()).unwrap();
        assert_eq!(structured, applied);
        assert_eq!(structured.render(), applied.render());
        assert_eq!(
            json["procedure"],
            serde_json::to_value(&preview.procedure.id).unwrap()
        );
        applied
    };
    assert_eq!(
        application.reading.generation,
        generation::current(&context_dir).unwrap()
    );
    assert_eq!(application.reading, owner.resolved_repertoire().unwrap());
    owner
        .verify_repertoire_procedure(&application.reading, &application.procedure)
        .unwrap();
    assert_eq!(
        claude_before,
        fs::read(world.project.join("CLAUDE.md")).unwrap()
    );
    assert_eq!(
        agents_before,
        fs::read(world.project.join("AGENTS.md")).unwrap()
    );
    assert_eq!(
        sources_before,
        source_snapshot(&world.home.root().join("registries"))
    );
}

#[test]
fn native_tui_selection_or_scope_changes_invalidate_retained_preview_without_effects() {
    use aikit_tui::application::{Overlay, TuiRuntime, TuiState, UiAction};
    use aikit_tui::application_service::ApplicationService;
    let world = World::new();
    let mut owner = world.service();
    let context_dir = world.home.context_dir(&owner.descriptor().context_id);
    let project_before = fs::read(world.project.join(".aikit/profile.toml")).unwrap();
    let sources_before = source_snapshot(&world.home.root().join("registries"));
    {
        let mut service = ApplicationService::new(&mut owner);
        let mut runtime = TuiRuntime::new();
        let mut state = TuiState::default();
        for action in [
            UiAction::SelectComposeProfile(Some(ProfileId::parse("profile/development").unwrap())),
            UiAction::ToggleComposeSkillSet {
                name: "implementer".into(),
            },
            UiAction::SetMutationScope(ScopeKind::Project),
        ] {
            state = runtime.step(&mut service, state, action).unwrap();
        }
        for mutation in [
            UiAction::ToggleComposeSkillSet {
                name: "orientation".into(),
            },
            UiAction::SetMutationScope(ScopeKind::Session),
            UiAction::SelectComposeProfile(None),
        ] {
            state = runtime
                .step(&mut service, state, UiAction::RequestRepertoirePreview)
                .unwrap();
            assert!(state.repertoire_preview.is_some());
            state = runtime
                .step(&mut service, state, UiAction::RequestApply)
                .unwrap();
            assert_eq!(state.overlay, Some(Overlay::ConfirmApply));
            state = runtime.step(&mut service, state, mutation).unwrap();
            assert!(state.repertoire_preview.is_none());
            state = runtime
                .step(&mut service, state, UiAction::RequestApply)
                .unwrap();
            state = runtime
                .step(&mut service, state, UiAction::ConfirmApply)
                .unwrap();
            assert!(state.repertoire_application.is_none());
            assert_eq!(generation::current(&context_dir).unwrap(), None);
            assert_eq!(
                project_before,
                fs::read(world.project.join(".aikit/profile.toml")).unwrap()
            );
            assert!(!world
                .home
                .session_overlay(&aikit_core::SessionId::parse(SESSION).unwrap())
                .exists());
        }
    }
    assert_eq!(
        sources_before,
        source_snapshot(&world.home.root().join("registries"))
    );
}

#[test]
fn satisfied_repertoire_replay_rechecks_mutated_accepted_source_before_new_effects() {
    let world = World::new();
    let (service, application) = world.compose();
    let context_dir = world.home.context_dir(&service.descriptor().context_id);
    let runner = ProcedureRunner::new(&world.home);
    let procedure = runner.load(&application.procedure).unwrap();
    assert!(runner.run(&procedure).unwrap().already_satisfied);
    let project_before = fs::read(world.project.join(".aikit/profile.toml")).unwrap();
    let material_before = source_snapshot(&context_dir);
    let record = runner.procedure_dir(&application.procedure);
    let record_before = source_snapshot(&record);
    assert!(
        !record_before.is_empty(),
        "the actual retained Procedure exists"
    );
    let source = world
        .home
        .root()
        .join("registries/personal/capsules/skill/development/plain/payload/SKILL.md");
    let mut changed = fs::read(&source).unwrap();
    changed.extend_from_slice(b"\nThis actual accepted source changed after application.\n");
    fs::write(&source, &changed).unwrap();
    let refusal = runner.run(&procedure).unwrap_err();
    assert!(
        refusal.code().starts_with("procedure.precondition"),
        "{refusal:?}"
    );
    assert_eq!(
        application.reading.generation,
        generation::current(&context_dir).unwrap()
    );
    assert_eq!(
        project_before,
        fs::read(world.project.join(".aikit/profile.toml")).unwrap()
    );
    assert_eq!(material_before, source_snapshot(&context_dir));
    assert_eq!(record_before, source_snapshot(&record));
    assert_eq!(changed, fs::read(source).unwrap());
    assert!(runner.verify_applied(&application.procedure).is_err());
}

#[test]
fn saved_preview_cannot_change_native_isolation_or_owner_attribution() {
    let world = World::new();
    let mut service = world.service();
    let preview = service
        .preview_repertoire(world.request(ScopeKind::Project, &["implementer"], true))
        .unwrap();
    let original = fs::read(world.project.join(".aikit/profile.toml")).unwrap();
    let foreign_shadow = world.owned.path().join("unreviewed-shadow");
    let mut shadow_change = preview.clone();
    shadow_change.procedure.isolation = aikit_core::procedure::MutationIsolation::Staged {
        shadow: foreign_shadow.clone(),
    };
    let mut owner_change = preview;
    owner_change.procedure.kind = aikit_core::procedure::ProcedureKind::SkillSet {
        operation: "unreviewed-owner-operation".into(),
        set: "unreviewed-set".into(),
    };
    for changed in [shadow_change, owner_change] {
        let error = service.apply_repertoire(changed).unwrap_err();
        assert_eq!(error.code(), "composition.preview_invalid");
        assert_eq!(
            original,
            fs::read(world.project.join(".aikit/profile.toml")).unwrap()
        );
        assert_eq!(
            generation::current(&world.home.context_dir(&service.descriptor().context_id)).unwrap(),
            None
        );
        assert!(!foreign_shadow.exists());
    }
}

#[test]
fn uncertain_saved_preview_replay_repairs_actual_retained_targets_before_success() {
    let world = World::new();
    let mut service = world.service();
    let preview = service
        .preview_repertoire(world.request(ScopeKind::Project, &["implementer"], true))
        .unwrap();
    assert!(preview.reading.generation.is_none());
    let first = service.apply_repertoire(preview.clone()).unwrap();
    let context = world.home.context_dir(&service.descriptor().context_id);
    let pi = context.join("current/projections/pi/.pi/skills/plain");
    fs::remove_file(&pi).unwrap();
    let declarations = fs::read(world.project.join(".aikit/profile.toml")).unwrap();
    let recovered = service.apply_repertoire(preview.clone()).unwrap();
    assert!(recovered.recovered);
    assert!(!recovered.reused_generation);
    assert_eq!(recovered.applied_edits, 0);
    assert_eq!(first.reading.generation, recovered.reading.generation);
    assert!(pi.join("SKILL.md").is_file());
    assert_eq!(
        declarations,
        fs::read(world.project.join(".aikit/profile.toml")).unwrap()
    );
    let settled = service.apply_repertoire(preview).unwrap();
    assert!(settled.reused_generation);
    assert!(settled.recovered);
}

#[test]
fn retained_repair_reconciles_valid_wrong_lock_metadata_and_extra_managed_entries() {
    let world = World::new();
    let (mut service, first) = world.compose();
    let current = world
        .home
        .context_dir(&service.descriptor().context_id)
        .join("current");
    let original_lock = generation::read_lock(&current).unwrap();
    let original_metadata = generation::read_metadata(&current).unwrap();
    let mut wrong_lock = original_lock.clone();
    wrong_lock.catalog_revision = "valid-but-unreviewed-catalogue".into();
    write(
        &current.join("resolution.lock.toml"),
        &toml::to_string_pretty(&wrong_lock).unwrap(),
    );
    let mut wrong_metadata = original_metadata.clone();
    wrong_metadata.targets[0].digest = "valid-but-unreviewed-target".into();
    write(
        &current.join("metadata.json"),
        &serde_json::to_string_pretty(&wrong_metadata).unwrap(),
    );
    write(
        &current.join("projections/pi/unreviewed/extra.md"),
        "extra managed material",
    );
    let foreign = world.owned.path().join("foreign-instruction-target");
    write(
        &foreign.join("AGENTS.md"),
        "Foreign source must survive a managed extra-link removal.",
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink(&foreign, current.join("foreign-extra")).unwrap();
    let repaired = service
        .apply_repertoire(service.preview_repertoire(empty_request()).unwrap())
        .unwrap();
    assert_eq!(first.reading.generation, repaired.reading.generation);
    assert!(!repaired.reused_generation);
    assert_eq!(original_lock, generation::read_lock(&current).unwrap());
    let metadata = generation::read_metadata(&current).unwrap();
    assert_eq!(
        metadata
            .targets
            .iter()
            .map(|record| (&record.target, &record.digest, record.items, &record.effect))
            .collect::<Vec<_>>(),
        original_metadata
            .targets
            .iter()
            .map(|record| (&record.target, &record.digest, record.items, &record.effect))
            .collect::<Vec<_>>()
    );
    assert!(!current.join("projections/pi/unreviewed").exists());
    assert!(!current.join("foreign-extra").exists());
    assert_eq!(
        fs::read_to_string(foreign.join("AGENTS.md")).unwrap(),
        "Foreign source must survive a managed extra-link removal."
    );
    let settled = service
        .apply_repertoire(service.preview_repertoire(empty_request()).unwrap())
        .unwrap();
    assert!(
        settled.reused_generation,
        "final lock, metadata, plans and material all validate"
    );
    // A target-only repair preserves matching publication provenance.
    fs::remove_file(current.join("projections/pi/.pi/skills/plain")).unwrap();
    service
        .apply_repertoire(service.preview_repertoire(empty_request()).unwrap())
        .unwrap();
    assert_eq!(metadata, generation::read_metadata(&current).unwrap());
}

#[cfg(unix)]
#[test]
fn retained_generation_root_link_is_refused_without_writes_into_foreign_material() {
    let world = World::new();
    let (mut service, first) = world.compose();
    let context = world.home.context_dir(&service.descriptor().context_id);
    let generation_root = context
        .join("generations")
        .join(first.reading.generation.as_ref().unwrap().as_str());
    let foreign = world.owned.path().join("foreign-retained-copy");
    fs::rename(&generation_root, &foreign).unwrap();
    fs::remove_file(foreign.join("projections/pi/.pi/skills/plain")).unwrap();
    std::os::unix::fs::symlink(&foreign, &generation_root).unwrap();
    let before = source_snapshot(&foreign);
    let declarations = fs::read(world.project.join(".aikit/profile.toml")).unwrap();
    let error = service
        .apply_repertoire(service.preview_repertoire(empty_request()).unwrap())
        .unwrap_err();
    assert_eq!(error.code(), "generation.repair_foreign_link");
    assert_eq!(before, source_snapshot(&foreign));
    assert!(!foreign.join("projections/pi/.pi/skills/plain").exists());
    assert_eq!(
        declarations,
        fs::read(world.project.join(".aikit/profile.toml")).unwrap()
    );
    assert_eq!(
        first.reading.generation,
        generation::current(&context).unwrap()
    );
}

#[test]
fn emitted_package_command_reconstructs_selected_session_from_another_directory() {
    let world = World::new();
    let (mut service, _) = world.compose();
    let preview = service
        .preview_repertoire(world.request(ScopeKind::Session, &["orientation"], false))
        .unwrap();
    let applied = service.apply_repertoire(preview).unwrap();
    let expected = serde_json::to_value(
        skillset_package_cli::load_package(&service, ".")
            .unwrap()
            .package,
    )
    .unwrap();
    let command = applied
        .reading
        .package_commands
        .iter()
        .find(|command| command.contains("package inspect "))
        .unwrap();
    let command = command.replacen(
        " aikit -C ",
        &format!(" '{}' -C ", env!("CARGO_BIN_EXE_aikit")),
        1,
    );
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &format!("{command} --json")])
        .current_dir(world.owned.path())
        .env("AIKIT_HOME", world.owned.path().join("unrelated-home"))
        .env("AIKIT_CONTEXT_ID", "ctx_wrong-parent")
        .env("AIKIT_SESSION_ID", "ses_wrong-parent")
        .env("AIKIT_TASK", "unrelated-parent-task")
        .env("AIKIT_ISOLATION", "worktree")
        .env("AIKIT_HOST", "unrelated-host")
        .env("AIKIT_MUX", "tmux")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["data"]["members"], expected["members"]);
    assert_eq!(result["data"]["identity"], expected["identity"]);
    assert_eq!(
        result["data"]["source_revision"],
        expected["source_revision"]
    );
}

#[test]
fn task_consumer_verifies_actual_retained_material_in_addition_to_planned_reading() {
    let world = World::new();
    let (mut service, applied) = world.compose();
    service
        .verify_repertoire_application(&applied.reading)
        .unwrap();
    let current = world
        .home
        .context_dir(&service.descriptor().context_id)
        .join("current");
    let pi = current.join("projections/pi/.pi/skills/plain");
    fs::remove_file(&pi).unwrap();
    assert_eq!(
        service.resolved_repertoire().unwrap(),
        applied.reading,
        "a planned reading cannot detect lost target material"
    );
    let error = service
        .verify_repertoire_application(&applied.reading)
        .unwrap_err();
    assert_eq!(error.code(), "composition.application_drift");
    let repaired = service
        .apply_repertoire(service.preview_repertoire(empty_request()).unwrap())
        .unwrap();
    service
        .verify_repertoire_application(&repaired.reading)
        .unwrap();
}

#[cfg(unix)]
#[test]
fn retained_generation_repairs_owned_entry_kind_drift_without_following_foreign_links() {
    let world = World::new();
    let (mut service, first) = world.compose();
    let current = world
        .home
        .context_dir(&service.descriptor().context_id)
        .join("current");
    let pi = current.join("projections/pi/.pi/skills/plain");
    fs::remove_file(&pi).unwrap();
    write(&pi.join("unexpected/copied.md"), "owned directory drift");
    let repaired = service
        .apply_repertoire(service.preview_repertoire(empty_request()).unwrap())
        .unwrap();
    assert_eq!(first.reading.generation, repaired.reading.generation);
    assert!(pi.is_symlink());
    assert!(pi.join("SKILL.md").is_file());
    let root = current.join("projections/pi/.pi/skills");
    fs::remove_dir_all(&root).unwrap();
    write(&root, "owned file drift at an expected directory");
    service
        .apply_repertoire(service.preview_repertoire(empty_request()).unwrap())
        .unwrap();
    assert!(root.is_dir());
    assert!(root.join("plain/SKILL.md").is_file());
    let foreign = world.owned.path().join("foreign-kind-drift");
    write(&foreign.join("AGENTS.md"), "foreign source remains intact");
    fs::remove_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&foreign, &root).unwrap();
    service
        .apply_repertoire(service.preview_repertoire(empty_request()).unwrap())
        .unwrap();
    assert!(!root.is_symlink());
    assert!(root.join("plain/SKILL.md").is_file());
    assert_eq!(
        fs::read_to_string(foreign.join("AGENTS.md")).unwrap(),
        "foreign source remains intact"
    );
}

#[test]
fn task_repertoire_requires_observed_applied_procedure_bound_to_actual_child_context() {
    let world = World::new();
    let (mut service, first) = world.compose();
    service
        .verify_repertoire_procedure(&first.reading, &first.procedure)
        .unwrap();
    let unobserved = aikit_core::ProcedureId::generate();
    assert!(service
        .verify_repertoire_procedure(&first.reading, &unobserved)
        .is_err());
    let planned = service
        .preview_repertoire(world.request(ScopeKind::Session, &["orientation"], false))
        .unwrap();
    ProcedureRunner::new(&world.home)
        .save(&planned.procedure)
        .unwrap();
    let error = service
        .verify_repertoire_procedure(&first.reading, &planned.procedure.id)
        .unwrap_err();
    assert_eq!(error.code(), "procedure.not_applied");
    let child = service.apply_repertoire(planned).unwrap();
    service
        .verify_repertoire_procedure(&child.reading, &child.procedure)
        .unwrap();
    let other_context = "ctx_unrelated-child-context";
    let other_session = "ses_unrelated-child-session";
    let other = Service::open(world.home.clone(), &world.project, |key| match key {
        "AIKIT_CONTEXT_ID" => Some(other_context.into()),
        "AIKIT_SESSION_ID" => Some(other_session.into()),
        _ => None,
    })
    .unwrap();
    assert!(other
        .verify_repertoire_procedure(&child.reading, &child.procedure)
        .is_err());
    // The parent application's old selection also cannot label the successor.
    assert!(service
        .verify_repertoire_procedure(&child.reading, &first.procedure)
        .is_err());
}

#[test]
fn hypothetical_native_target_metadata_is_inspected_and_bound_before_apply() {
    use aikit_core::repertoire::RepertoireItemKind;
    let world = World::new();
    write(
        &world
            .home
            .root()
            .join("registries/personal/profiles/orientation.toml"),
        "schema = 1\nid = \"profile/orientation\"\nenable = [\"skill/development/field\"]\n",
    );
    let (mut service, first) = world.compose();
    let context_dir = world.home.context_dir(&service.descriptor().context_id);
    let current = context_dir.join("current");
    let declarations = fs::read(world.project.join(".aikit/profile.toml")).unwrap();
    let declaration_mtime = fs::metadata(world.project.join(".aikit/profile.toml"))
        .unwrap()
        .modified()
        .unwrap();
    let field_source = world
        .home
        .root()
        .join("registries/personal/capsules/skill/development/field/payload");
    let source_body = fs::read(field_source.join("SKILL.md")).unwrap();
    let claude_foreign = fs::read(world.project.join("CLAUDE.md")).unwrap();
    let agents_foreign = fs::read(world.project.join("AGENTS.md")).unwrap();
    let request = RepertoireRequest {
        scope: ScopeKind::Session,
        profile: Some(ProfileId::parse("profile/orientation").unwrap()),
        skill_sets: vec!["orientation".into()],
    };
    let preview = service.preview_repertoire(request).unwrap();
    assert_eq!(preview.package_command_basis, "current-scope-declarations");
    assert_eq!(preview.reading.members.len(), 3);
    assert!(!world
        .home
        .session_overlay(&aikit_core::SessionId::parse(SESSION).unwrap())
        .exists());
    assert_eq!(
        generation::current(&context_dir).unwrap(),
        first.reading.generation
    );
    assert_eq!(
        fs::read(world.project.join(".aikit/profile.toml")).unwrap(),
        declarations
    );
    assert_eq!(
        fs::metadata(world.project.join(".aikit/profile.toml"))
            .unwrap()
            .modified()
            .unwrap(),
        declaration_mtime
    );
    assert_eq!(
        fs::read(field_source.join("SKILL.md")).unwrap(),
        source_body
    );
    for (target, destination) in [
        ("claude-code", ".claude/skills/field"),
        ("pi", ".pi/skills/field"),
    ] {
        let plan = preview
            .target_plans
            .iter()
            .find(|plan| plan.target == target)
            .unwrap();
        assert!(plan.differs_from_current_selection);
        let item = plan
            .items
            .iter()
            .find(|item| item.destination.as_deref() == Some(Path::new(destination)))
            .unwrap();
        assert_eq!(item.kind, RepertoireItemKind::Link);
        assert_eq!(item.source.as_deref(), Some(field_source.to_str().unwrap()));
        assert!(item.content_hash.is_none());
        assert!(!current
            .join(format!("projections/{target}/{destination}"))
            .exists());
        assert!(preview.render().contains(destination));
    }
    let json = serde_json::to_string(&preview).unwrap();
    assert!(json.contains(".claude/skills/field") && json.contains(".pi/skills/field"));
    assert!(!json.contains("Human-owned project instructions remain intact."));
    assert!(!json.contains("Foreign harness instructions remain intact."));
    assert!(!json.contains("Use the project's own source and report real evidence."));
    let reading_hash = preview.reading.resolution_hash.clone();
    let mut tampered = preview.clone();
    let item = tampered
        .target_plans
        .iter_mut()
        .find(|plan| plan.target == "pi")
        .unwrap()
        .items
        .iter_mut()
        .find(|item| item.destination.as_deref() == Some(Path::new(".pi/skills/field")))
        .unwrap();
    item.source = Some(
        world
            .owned
            .path()
            .join("unreviewed-source")
            .display()
            .to_string(),
    );
    assert_eq!(
        service.apply_repertoire(tampered).unwrap_err().code(),
        "composition.preview_invalid"
    );
    assert!(!world
        .home
        .session_overlay(&aikit_core::SessionId::parse(SESSION).unwrap())
        .exists());
    assert_eq!(
        generation::current(&context_dir).unwrap(),
        first.reading.generation
    );
    let result = service.apply_repertoire(preview.clone()).unwrap();
    assert_eq!(result.reading.resolution_hash, reading_hash);
    assert_eq!(result.procedure, preview.procedure.id);
    for (target, destination) in [
        ("claude", ".claude/skills/field"),
        ("pi", ".pi/skills/field"),
    ] {
        let actual = current.join(format!("projections/{target}/{destination}"));
        assert_eq!(fs::read_link(&actual).unwrap(), field_source);
        assert_eq!(fs::read(actual.join("SKILL.md")).unwrap(), source_body);
    }
    assert_eq!(
        fs::read(world.project.join("CLAUDE.md")).unwrap(),
        claude_foreign
    );
    assert_eq!(
        fs::read(world.project.join("AGENTS.md")).unwrap(),
        agents_foreign
    );
    let repeat = service.preview_repertoire(empty_request()).unwrap();
    assert!(repeat
        .target_plans
        .iter()
        .all(|plan| !plan.differs_from_current_selection));
    let replay = service.apply_repertoire(preview).unwrap();
    assert!(replay.recovered && replay.reused_generation);
    assert_eq!(replay.reading.generation, result.reading.generation);
}
