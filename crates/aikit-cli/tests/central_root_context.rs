//! Location/identity regression: no Profile, payload read or child manufacture.
use aikit_cli::app::Service;
use aikit_core::project::ProjectBindingLocator;
use aikit_store::AikitHome;
use aikit_tui::backend::PaletteBackend;
use std::{fs, path::Path};

fn central(path: &Path) {
    for directory in [
        "Control/user/private",
        "Control/agents",
        "Control/machines",
        "Work",
        ".central",
    ] {
        fs::create_dir_all(path.join(directory)).unwrap();
    }
    fs::write(path.join("Control/user/private/.no-agent-retrieval"), "").unwrap();
    fs::write(
        path.join("Control/user/private/secret.md"),
        "PRIVATE_NOT_CONTEXT",
    )
    .unwrap();
}
fn open(home: &Path, root: &Path, cwd: &Path) -> Service {
    Service::open(AikitHome::at(home), cwd, |key| match key {
        "CENTRAL_ROOT" => Some(root.display().to_string()),
        _ => None,
    })
    .unwrap()
}
#[test]
fn central_root_control_and_work_container_share_the_same_root_binding() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Central");
    central(&root);
    for cwd in [&root, &root.join("Control/user"), &root.join("Work")] {
        let service = open(&temp.path().join("aikit-home"), &root, cwd);
        let binding = service.project_binding().unwrap().unwrap();
        assert_eq!(binding.project.as_str(), "control:root");
        assert_eq!(
            binding.source.unwrap().as_str(),
            "central:source:control:root:Control"
        );
        assert!(
            matches!(binding.locator, ProjectBindingLocator::LocalDirectory { path } if path == root.canonicalize().unwrap())
        );
        assert_eq!(
            service.context().project_root.as_ref().unwrap(),
            &root.canonicalize().unwrap()
        );
        assert!(!serde_json::to_string(service.view())
            .unwrap()
            .contains("PRIVATE_NOT_CONTEXT"));
    }
    assert!(!root.join(".aikit").exists());
    assert!(!root.join("ProjectCentral").exists());
    assert_eq!(
        fs::read_to_string(root.join("Control/user/private/secret.md")).unwrap(),
        "PRIVATE_NOT_CONTEXT"
    );
}
#[test]
fn an_existing_child_identity_does_not_require_an_aikit_profile() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Central");
    central(&root);
    let child = root.join("Work/child");
    fs::create_dir_all(child.join("ProjectCentral")).unwrap();
    fs::write(child.join("ProjectCentral/project.json"), r#"{"schema":"central.project/v1","project_id":"project:child","human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}"#).unwrap();
    let service = open(&temp.path().join("home"), &root, &child);
    assert_eq!(
        service.project_binding().unwrap().unwrap().project.as_str(),
        "project:child"
    );
    assert!(!child.join(".aikit").exists());
    assert_eq!(
        open(&temp.path().join("home"), &root, &root)
            .project_binding()
            .unwrap()
            .unwrap()
            .project
            .as_str(),
        "control:root"
    );
}
#[test]
fn configured_root_does_not_claim_an_unrelated_directory() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Central");
    central(&root);
    let unrelated = temp.path().join("unrelated");
    fs::create_dir(&unrelated).unwrap();
    let service = open(&temp.path().join("home"), &root, &unrelated);
    assert!(service.project_binding().unwrap().is_none());
    assert!(service.context().project_root.is_none());
    assert!(service.model_roster().unwrap().is_none());
}
#[test]
fn a_changed_root_is_an_explicit_failure_not_a_binding_free_success() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Central");
    central(&root);
    let service = open(&temp.path().join("home"), &root, &root);
    fs::remove_dir(root.join("Work")).unwrap();
    assert_eq!(
        service.project_binding().unwrap_err().code(),
        "central.root_context_unavailable"
    );
    assert_eq!(
        service.model_roster().unwrap_err().code(),
        "central.root_context_unavailable"
    );
}
#[cfg(unix)]
#[test]
fn redirected_control_is_not_accepted_as_a_root_binding() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Central");
    central(&root);
    fs::rename(root.join("Control"), root.join("elsewhere")).unwrap();
    std::os::unix::fs::symlink(root.join("elsewhere"), root.join("Control")).unwrap();
    let result = Service::open(AikitHome::at(temp.path().join("home")), &root, |key| {
        (key == "CENTRAL_ROOT").then(|| root.display().to_string())
    });
    assert_eq!(
        result.err().unwrap().code(),
        "central.root_context_unavailable"
    );
}
