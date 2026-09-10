//! Real standalone executable, canonical resolver and on-disk SessionSpace CAS.
use std::{path::Path, process::Command};
use aikit_core::session_space_application::SessionSpaceProjectContextBinding;
use serde_json::{json, Value};

fn call(home: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
        .env("AIKIT_HOME", home)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .arg("-C").arg(cwd).args(args).output().unwrap()
}
fn read(home: &Path, cwd: &Path, args: &[&str]) -> Value {
    let output=call(home,cwd,args);
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn native_context_read_binds_exact_project_through_preview_apply_and_survives_restart() {
    let temp=tempfile::tempdir().unwrap();
    let home=temp.path().join("home");
    let project=temp.path().join("project");
    let other=temp.path().join("other");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&other).unwrap();
    assert!(!call(&home,&project,&["project-context"]).status.success(),"ordinary unbound directory cannot fabricate a context binding");
    for (id,directory) in [("native-project",&project),("native-other",&other)] {
        let result=Command::new(env!("CARGO_BIN_EXE_aikit")).env("AIKIT_HOME",&home)
            .env_remove("AIKIT_CONTEXT_ID").env_remove("AIKIT_ISOLATION")
            .current_dir(directory).args(["--json","project","bind",id,"--directory"])
            .arg(directory).arg("--no-default-skill-sets").output().unwrap();
        assert!(result.status.success(),"{} {}",String::from_utf8_lossy(&result.stderr),String::from_utf8_lossy(&result.stdout));
    }
    // Exercise the public ProjectCentral filesystem contract; the consuming
    // Cradle test separately provisions this ground through the real ctrl CLI.
    let manifest=project.join("ProjectCentral/project.json");
    std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    std::fs::write(&manifest,json!({"schema":"central.project/v1","project_id":"central/native-context",
        "human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}).to_string()).unwrap();
    let binding=read(&home,&project,&["project-context"]);
    let typed:SessionSpaceProjectContextBinding=serde_json::from_value(binding.clone()).unwrap();
    assert_eq!(typed.project, *typed.context.project());
    assert_eq!(typed.project.as_str(),"central/native-context","native manifest identity survives directory and specification naming");
    assert_eq!(typed.context.basis.project_binding.source.as_ref().unwrap().as_str(),"source:central:central/native-context:manifest");
    assert!(!typed.context.basis.resolver_hash.is_empty());
    assert!(!typed.context.provenance.is_empty());
    let other_binding=read(&home,&other,&["project-context"]);
    assert_ne!(binding["project"],other_binding["project"],"native resolver keeps distinct project identities");
    assert!(read(&home,&project,&["list"]).as_array().unwrap().is_empty(),"context reading creates no SessionSpace");
    let space="session-space/native-context";
    let preview=read(&home,&project,&["create",space,"--label","Native context"]);
    assert!(read(&home,&project,&["list"]).as_array().unwrap().is_empty(),"create only stages intent");
    read(&home,&project,&["apply","--preview-json",&preview.to_string()]);
    let intent=json!({"operation":"bind-project-context","binding":binding});
    let preview=read(&home,&project,&["stage","--space",space,"--intent-json",&intent.to_string()]);
    let project_ref=typed.project.to_string();
    assert!(read(&home,&project,&["discover","--project",&project_ref]).as_array().unwrap().is_empty(),"staging does not bind membership");
    let applied=read(&home,&project,&["apply","--preview-json",&preview.to_string()]);
    let state=read(&home,&project,&["open",space]);
    assert_eq!(state,applied["after"],"fresh process reopens exact applied canonical state");
    assert_eq!(read(&home,&project,&["discover","--project",&project_ref]),json!([state]));
    let other_ref=other_binding["project"].as_str().unwrap();
    assert!(read(&home,&other,&["discover","--project",other_ref]).as_array().unwrap().is_empty());
    let stale=call(&home,&project,&["apply","--preview-json",&preview.to_string()]);
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("preview_stale"));
    assert_eq!(read(&home,&project,&["history",space]).as_array().unwrap().len(),2,"stale apply writes no receipt");
    std::fs::write(&manifest,"{").unwrap();
    let invalid=call(&home,&project,&["project-context"]);
    assert!(!invalid.status.success(),"invalid native identity must not fall back to a directory-derived ProjectRef");
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("projectcentral.manifest_invalid"));
}
