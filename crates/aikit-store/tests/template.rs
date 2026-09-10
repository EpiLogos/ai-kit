//! Template instantiation — the composables spec's §9.4 gap.
//!
//! `Kind::Template` has always parsed and catalogued; nothing ever instantiated
//! one. The decision that shapes this: instantiating a template **writes into the
//! user's project**, which is outside `~/.aikit/state/`, so by STANDARDS §6 it is
//! a Procedure — planned, diffable, reversible — rather than a bespoke copy loop.
//! It therefore inherits the whole safety story for free, and these tests hold it
//! to exactly the guarantees the Procedure engine makes.
//!
//! Parameters reuse the profile parameter types (SPEC-II §5) rather than
//! inventing a second parameter system.

mod common;
use common::*;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use aikit_core::procedure::MutationIsolation;
use aikit_store::home::AikitHome;
use aikit_store::procedure::ProcedureRunner;
use aikit_store::template;

fn home(dir: &Path) -> AikitHome {
    let home = AikitHome::at(dir.join("aikit-home"));
    home.ensure_layout().unwrap();
    home
}

/// A template capsule whose payload is a small service scaffold with two
/// parameters used in both a path and a file body.
fn service_template(fixture: &RegistryFixture) {
    fixture.capsule(
        "template/service/neo4j-client",
        "template",
        r#"root = "payload"
destination = "services/{{service_name}}"

[[template.params]]
name = "service_name"
type = "string"
required = true

[[template.params]]
name = "port"
type = "integer"
default = 7687
"#,
        "",
        &[
            (
                "payload/{{service_name}}.toml",
                "name = \"{{service_name}}\"\nport = {{port}}\n",
            ),
            (
                "payload/README.md",
                "# {{service_name}}\n\nA neo4j client.\n",
            ),
        ],
    );
}

#[test]
fn instantiating_a_template_plans_a_procedure_rather_than_writing_directly() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    service_template(&fixture);
    let home = home(tmp.path());
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();

    let load = aikit_store::registry::load_registry(
        fixture.root(),
        aikit_core::RegistrySource::personal(),
    )
    .unwrap();
    let capsule =
        aikit_core::catalog::Catalog::get(&load.catalog, &cid("template/service/neo4j-client"))
            .unwrap();

    let mut values = BTreeMap::new();
    values.insert("service_name".to_string(), "graph".to_string());

    let procedure = template::plan_instantiation(&home, capsule, &project, &values).unwrap();

    // It is a Procedure, with an inverse per edit, and nothing has been written.
    assert_eq!(procedure.plan.edits.len(), 2, "one edit per payload file");
    assert!(
        !project.join("services/graph").exists(),
        "planning writes nothing"
    );

    // The diff is reviewable before anything happens.
    let diff = ProcedureRunner::new(&home).diff(&procedure).unwrap();
    assert!(diff.edits.iter().all(|e| e.creates), "both files are new");
    assert!(diff.render().contains("undo:"));
}

#[test]
fn instantiation_substitutes_parameters_in_both_paths_and_contents() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    service_template(&fixture);
    let home = home(tmp.path());
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();

    let load = aikit_store::registry::load_registry(
        fixture.root(),
        aikit_core::RegistrySource::personal(),
    )
    .unwrap();
    let capsule =
        aikit_core::catalog::Catalog::get(&load.catalog, &cid("template/service/neo4j-client"))
            .unwrap();

    let mut values = BTreeMap::new();
    values.insert("service_name".to_string(), "graph".to_string());

    let procedure = template::plan_instantiation(&home, capsule, &project, &values).unwrap();
    ProcedureRunner::new(&home).run(&procedure).unwrap();

    // The destination and the file name both carried a parameter.
    let root = project.join("services/graph");
    let config = root.join("graph.toml");
    assert!(config.is_file(), "the file name was substituted: {root:?}");

    let text = fs::read_to_string(&config).unwrap();
    assert!(
        text.contains("name = \"graph\""),
        "contents substituted: {text}"
    );
    assert!(
        text.contains("port = 7687"),
        "an omitted parameter takes its declared default: {text}"
    );
    assert!(fs::read_to_string(root.join("README.md"))
        .unwrap()
        .contains("# graph"));
}

#[test]
fn an_instantiation_is_undoable_like_any_other_procedure() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    service_template(&fixture);
    let home = home(tmp.path());
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();

    let load = aikit_store::registry::load_registry(
        fixture.root(),
        aikit_core::RegistrySource::personal(),
    )
    .unwrap();
    let capsule =
        aikit_core::catalog::Catalog::get(&load.catalog, &cid("template/service/neo4j-client"))
            .unwrap();
    let mut values = BTreeMap::new();
    values.insert("service_name".to_string(), "graph".to_string());

    let procedure = template::plan_instantiation(&home, capsule, &project, &values).unwrap();
    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure).unwrap();
    assert!(project.join("services/graph/graph.toml").exists());

    runner.undo(&procedure.id).unwrap();
    assert!(
        !project.join("services/graph/graph.toml").exists(),
        "instantiation is reversible because it is a Procedure"
    );
}

#[test]
fn a_missing_required_parameter_is_refused_before_anything_is_planned() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    service_template(&fixture);
    let home = home(tmp.path());
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();

    let load = aikit_store::registry::load_registry(
        fixture.root(),
        aikit_core::RegistrySource::personal(),
    )
    .unwrap();
    let capsule =
        aikit_core::catalog::Catalog::get(&load.catalog, &cid("template/service/neo4j-client"))
            .unwrap();

    let error =
        template::plan_instantiation(&home, capsule, &project, &BTreeMap::new()).unwrap_err();
    assert_eq!(error.code(), "template.missing_parameter");
    assert!(
        error.message().contains("service_name"),
        "the error names the parameter: {}",
        error.message()
    );
}

#[test]
fn instantiating_over_an_existing_file_is_refused_rather_than_clobbering() {
    // A template drop-in must never silently overwrite the user's work. The
    // Procedure would technically be reversible, but "reversible" is not consent.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    service_template(&fixture);
    let home = home(tmp.path());
    let project = tmp.path().join("project");
    let existing = project.join("services/graph/graph.toml");
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, "mine, hand written\n").unwrap();

    let load = aikit_store::registry::load_registry(
        fixture.root(),
        aikit_core::RegistrySource::personal(),
    )
    .unwrap();
    let capsule =
        aikit_core::catalog::Catalog::get(&load.catalog, &cid("template/service/neo4j-client"))
            .unwrap();
    let mut values = BTreeMap::new();
    values.insert("service_name".to_string(), "graph".to_string());

    let error = template::plan_instantiation(&home, capsule, &project, &values).unwrap_err();
    assert_eq!(error.code(), "template.destination_occupied");
    assert_eq!(
        fs::read_to_string(&existing).unwrap(),
        "mine, hand written\n",
        "the user's file is untouched"
    );
}

#[test]
fn instantiation_into_a_repository_stages_on_a_branch() {
    // The isolation rule (Spec II §1.2) applies unchanged: a project under version
    // control gets a branch, not a shadow tree.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    service_template(&fixture);
    let home = home(tmp.path());
    let project = tmp.path().join("project");
    fs::create_dir_all(project.join(".git")).unwrap();

    let load = aikit_store::registry::load_registry(
        fixture.root(),
        aikit_core::RegistrySource::personal(),
    )
    .unwrap();
    let capsule =
        aikit_core::catalog::Catalog::get(&load.catalog, &cid("template/service/neo4j-client"))
            .unwrap();
    let mut values = BTreeMap::new();
    values.insert("service_name".to_string(), "graph".to_string());

    let procedure = template::plan_instantiation(&home, capsule, &project, &values).unwrap();
    assert!(
        matches!(procedure.isolation, MutationIsolation::GitBranch { .. }),
        "got {:?}",
        procedure.isolation
    );
}
