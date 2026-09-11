//! The wiring proof: the CLI backend observes Workcell and projects it as an
//! owned disclosure, driving the `intake_workcell_instances` observer that
//! existed in `aikit-adapters` but had no production caller. Run against the
//! real machine — whether or not `workcell` is installed, the reading is a
//! real observation (`Observed` or `Unavailable`), never the `not_attempted`
//! default that means "nobody looked".

use aikit_cli::app::Service;
use aikit_core::workcell_world::WorkcellKnowledge;
use aikit_store::AikitHome;
use aikit_tui::backend::PaletteBackend;
use std::collections::BTreeMap;

fn service(home: &std::path::Path, root: &std::path::Path) -> Service {
    let mut env = BTreeMap::new();
    env.insert(
        "AIKIT_CONTEXT_ID".to_owned(),
        aikit_core::ContextId::generate().to_string(),
    );
    Service::open(AikitHome::at(home), root, |key| env.get(key).cloned()).unwrap()
}

#[test]
fn the_cli_backend_observes_workcell_and_projects_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    std::fs::create_dir_all(&root).unwrap();

    let service = service(tmp.path(), &root);
    let disclosure = service
        .workcell_world()
        .expect("observing Workcell does not fail")
        .expect("a producer is attached — the observer had no caller before");

    // A real observation: either the registry was read, or the binary could not
    // be read. Both are "we looked"; neither is the `not_attempted` default.
    match &disclosure.knowledge {
        WorkcellKnowledge::Observed { .. } | WorkcellKnowledge::Unavailable { .. } => {}
        WorkcellKnowledge::NotAttempted { reason } => {
            panic!("the producer must actually look, not answer not-attempted: {reason}")
        }
    }
}

#[test]
fn the_workcell_reading_is_cached_across_calls() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    std::fs::create_dir_all(&root).unwrap();

    let service = service(tmp.path(), &root);
    let first = service.workcell_world().unwrap().unwrap();
    let second = service.workcell_world().unwrap().unwrap();
    assert_eq!(first, second);
}
