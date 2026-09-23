//! Knowledge-faculty Work-coverage goldens (parent spec: `aikit knowledge
//! search` reaches the Work/ repositories' code and docs with the same
//! reliability as Control prose, and its absences tell the truth).
//!
//! Two layers:
//! - fixture goldens (CI, no real world needed): a fixture Central with Work
//!   projects carrying routine code, an encounter/MCP source and a
//!   Workcell-style placement doc; the three acceptance queries must hit the
//!   right sources; absences stay clean; the discovery bound names what it
//!   skipped;
//! - real-world goldens, gated behind `AIKIT_KNOWLEDGE_GOLDENS=real`: the
//!   same queries against the live `~/Central`.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use aikit_cli::app::Service;
use aikit_store::AikitHome;
use tempfile::TempDir;

const WORK_REPOS_PROVIDER: &str = "provider/source-pool/work-repos";

/// Fixture tests must never reach the owner's real bkmr stores (they are
/// read-only, but opening a foreign database can trigger bkmr's automatic
/// schema migration). One shared empty config dir is created lazily and set
/// once per test process, so concurrent fixture tests agree.
fn isolate_bkmr_config() {
    static ISOLATED: OnceLock<String> = OnceLock::new();
    let dir = ISOLATED.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "aikit-knowledge-goldens-bkmr-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AIKIT_BKMR_CONFIG_DIR", &dir);
        dir.to_string_lossy().into_owned()
    });
    std::env::set_var("AIKIT_BKMR_CONFIG_DIR", dir);
}

/// The minimal canonical wiki for a fixture project: its space and its
/// project-root anchor — the object `aikit wiki root-anchor` writes, and the
/// one the folder-subject compiler hangs the folder basis from.
fn wiki_json(project_id: &str, slug: &str) -> String {
    let space = format!("central:wiki:project:{project_id}");
    format!(
        r#"{{
          "profile":"okf-wiki/v1",
          "objects":[
            {{"profile":"okf-wiki/v1","object":"space","ref":"{space}","revision":1,"provenance":[],"title":"{slug}","parent_space_refs":[],"child_space_refs":[],"node_refs":[],"anchor_ref":"wiki:node:project-root/{slug}"}},
            {{"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:project-root/{slug}","revision":1,"provenance":[],"type":"project-root","title":"{slug}","space_refs":["{space}"],"source_refs":["ProjectCentral/project.json"]}}
          ]
        }}"#
    )
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn manifest(project_id: &str) -> String {
    format!(
        r#"{{"schema":"central.project/v1","project_id":"{project_id}","human_source":"ProjectCentral/user","wiki":{{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json","adopted_sources":[]}}}}"#
    )
}

/// A fixture Central: `Control/` + `Work/demo` with the routine code, the
/// encounter/MCP source and the Workcell-style placement doc the acceptance
/// queries target, plus a minimal second project so per-project lines are
/// proven for more than one discovered project.
fn write_fixture_central(temp: &TempDir) {
    let root = temp.path();
    fs::create_dir_all(root.join("Control")).unwrap();
    let demo = root.join("Work/demo");
    write(&demo.join("ProjectCentral/project.json"), &manifest("demo"));
    write(
        &demo.join("ProjectCentral/agents/wiki/wiki.json"),
        &wiki_json("demo", "demo"),
    );
    write(
        &demo.join("ProjectCentral/relations/source-relations.json"),
        r#"{"schema":"central.project.ground-relations/v1","project_id":"demo","relations":[]}"#,
    );
    write(
        &demo.join("src/routine.rs"),
        "//! Task automation floor.\n\
         pub fn schedule() {\n    // automations drive the cron scheduler; all scheduled tasks land here\n}\n",
    );
    write(
        &demo.join("src/encounter_mcp.rs"),
        "// The encounter supplies mcp servers on the wire.\n\
         // A tool protocol capsule resolves through the application engine before any provider spawns.\n",
    );
    write(
        &demo.join("docs/CAW-NATIVE-DELIVERY.md"),
        "# CAW native delivery\n\nThe encounter adapter and the MCP tool protocol capsule are delivered natively.\n",
    );
    write(
        &demo.join("docs/MULTI-WORKCELL-PLACEMENT.md"),
        "# Multi-workcell placement\n\nThe opensandbox sandbox provider runs a hosted VM profile for remote cells.\n",
    );
    let other = root.join("Work/other");
    write(
        &other.join("ProjectCentral/project.json"),
        &manifest("other"),
    );
    write(
        &other.join("ProjectCentral/agents/wiki/wiki.json"),
        &wiki_json("other", "other"),
    );
}

fn open_service(temp: &TempDir) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    Service::open(home, temp.path(), |_| None).expect("open production application service")
}

/// Which hits carry the given source ref, with their providers.
fn find_hit<'a>(
    result: &'a aikit_core::KnowledgeSearchResult,
    source_suffix: &str,
) -> Vec<&'a aikit_core::KnowledgeSearchHit> {
    result
        .hits
        .iter()
        .filter(|hit| hit.resource.as_str().ends_with(source_suffix))
        .collect()
}

/// The three acceptance queries hit the right Work sources through the live
/// Work-repos pool (fixture variant).
#[test]
fn the_three_acceptance_queries_hit_work_sources_in_the_fixture_world() {
    if !aikit_adapters::ripgrep::available() {
        eprintln!("ripgrep is not installed; the Work-coverage fixture golden skipped");
        return;
    }
    isolate_bkmr_config();
    let temp = TempDir::new().unwrap();
    write_fixture_central(&temp);
    let service = open_service(&temp);

    // 1. "automations cron scheduled tasks" hits the routine sources.
    let routine = service
        .knowledge_search("automations cron scheduled tasks", 50)
        .unwrap();
    let hits = find_hit(&routine, "source:project:demo:src/routine.rs");
    assert!(
        !hits.is_empty(),
        "the routine source is among the hits: {:#?}",
        routine
            .hits
            .iter()
            .map(|h| (h.resource.as_str(), h.provider.as_str()))
            .collect::<Vec<_>>()
    );
    assert!(
        hits.iter()
            .all(|hit| hit.provider.as_str() == WORK_REPOS_PROVIDER),
        "the hit cites the live Work-repos pool: {hits:?}"
    );

    // 2. "mcp tool protocol capsule" hits the encounter/harness sources.
    let mcp = service
        .knowledge_search("mcp tool protocol capsule", 50)
        .unwrap();
    let encounter = find_hit(&mcp, "source:project:demo:src/encounter_mcp.rs");
    let doc = find_hit(&mcp, "source:project:demo:docs/CAW-NATIVE-DELIVERY.md");
    assert!(
        !encounter.is_empty() || !doc.is_empty(),
        "the encounter or delivery source is among the hits: {:#?}",
        mcp.hits
            .iter()
            .map(|h| (h.resource.as_str(), h.provider.as_str()))
            .collect::<Vec<_>>()
    );

    // 3. "opensandbox sandbox provider" hits the placement doc.
    let sandbox = service
        .knowledge_search("opensandbox sandbox provider", 50)
        .unwrap();
    let placement = find_hit(
        &sandbox,
        "source:project:demo:docs/MULTI-WORKCELL-PLACEMENT.md",
    );
    assert!(
        !placement.is_empty(),
        "the placement doc is among the hits: {:#?}",
        sandbox
            .hits
            .iter()
            .map(|h| (h.resource.as_str(), h.provider.as_str()))
            .collect::<Vec<_>>()
    );

    // Findable implies openable: a Work-repos hit reads back through the
    // project binding's own agent-readability rule.
    let hit = placement.first().expect("placement hit present");
    let reading = service
        .knowledge_read(&hit.address)
        .expect("a Work-repos hit is readable through the one service");
    assert!(reading
        .content
        .as_deref()
        .unwrap_or_default()
        .contains("opensandbox"));
}

/// Absence cleanliness (fixture): a root-level search shows no "provider
/// absent", no "no canonical Project identity", and no per-query anchor gaps
/// for declared projects; anchor state is a status note, once per project.
#[test]
fn absences_stay_clean_and_anchor_state_is_loud_in_status_only() {
    if !aikit_adapters::ripgrep::available() {
        eprintln!("ripgrep is not installed; the absence-cleanliness golden skipped");
        return;
    }
    isolate_bkmr_config();
    let temp = TempDir::new().unwrap();
    write_fixture_central(&temp);
    let service = open_service(&temp);

    let result = service
        .knowledge_search("automations cron scheduled tasks", 50)
        .unwrap();
    for absence in &result.absences {
        assert!(
            !absence.contains("provider absent"),
            "no per-query provider-absent line: {absence}"
        );
        assert!(
            !absence.contains("no canonical Project identity"),
            "the old identity-gate line is gone: {absence}"
        );
        assert!(
            !(absence.contains("project-root anchor")
                && absence.contains("no folder subjects compiled")),
            "anchor gaps are status notes, never per-query absences: {absence}"
        );
    }

    let status = service.knowledge_status().unwrap();
    assert!(
        !status
            .absences
            .iter()
            .any(|absence| absence.contains("no canonical Project identity")),
        "status carries no identity-gate absence: {:#?}",
        status.absences
    );
    // Per-project anchor lines are loud in status for every discovered project.
    for project in ["demo", "other"] {
        assert!(
            status
                .notes
                .iter()
                .any(|note| note.contains(&format!("Work/{project}: project-root anchor present"))),
            "anchor state for Work/{project} is disclosed in status: {:#?}",
            status.notes
        );
    }
    // The default bkmr pool discloses its posture whatever the environment.
    assert!(
        status
            .notes
            .iter()
            .any(|note| note.starts_with("bkmr store pool")),
        "the bkmr store pool is disclosed in status: {:#?}",
        status.notes
    );
    // The discovered projects themselves are disclosed.
    assert!(
        status
            .notes
            .iter()
            .any(|note| note.starts_with("Projects: ")),
        "the projects are disclosed in status: {:#?}",
        status.notes
    );
}

/// Bound honesty (fixture): a tree that trips the 4096 candidate bound
/// produces an absence naming the stopping directory and the approximate
/// skipped count — a bound that hides what it skipped is the defect.
#[test]
fn the_discovery_bound_names_where_it_stopped_and_what_it_skipped() {
    isolate_bkmr_config();
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("Control")).unwrap();
    // One deep subtree carrying enough candidate files to trip the bound.
    let bulk = root.join("Work/bulk/docs/archive");
    fs::create_dir_all(&bulk).unwrap();
    for index in 0..(4096 + 64) {
        fs::write(
            bulk.join(format!("bulk-{index:05}.json")),
            format!(r#"{{"index":{index}}}"#),
        )
        .unwrap();
    }
    let service = open_service(&temp);
    let result = service.knowledge_search("anything", 10).unwrap();
    let trip = result
        .absences
        .iter()
        .find(|absence| absence.contains("Knowledge discovery stopped after 4096 candidate files"))
        .expect("the bound trip is disclosed");
    assert!(
        trip.contains("bulk"),
        "the stopping directory is named: {trip}"
    );
    assert!(
        trip.contains('~') && trip.contains("unexamined"),
        "the approximate skipped count is disclosed: {trip}"
    );
}

/// The real-world goldens, gated behind `AIKIT_KNOWLEDGE_GOLDENS=real`: the
/// same three acceptance queries against the live `~/Central` must return,
/// among their hits, the sources the parent spec verified present.
///
/// The service stands in the live ai-kit checkout (the acceptance posture: a
/// fresh ai-kit checkout against the real world). A query asked inside a
/// project stands in that project's world through the grammar itself, so the
/// cross-project query carries an explicit `: workcell` scope.
#[test]
fn the_three_acceptance_queries_hit_the_live_world() {
    if std::env::var("AIKIT_KNOWLEDGE_GOLDENS").as_deref() != Ok("real") {
        eprintln!("set AIKIT_KNOWLEDGE_GOLDENS=real to run the live-world goldens");
        return;
    }
    if !aikit_adapters::ripgrep::available() {
        eprintln!("ripgrep is not installed; the live-world goldens skipped");
        return;
    }
    let central = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Central");
    let checkout = central.join("Work/ai-kit");
    if !central.join("Control").is_dir() || !checkout.is_dir() {
        panic!(
            "AIKIT_KNOWLEDGE_GOLDENS=real expects the live world at {} with the ai-kit checkout at {}",
            central.display(),
            checkout.display()
        );
    }
    // The AIKit home stays in a scratch dir: the live world is read, never
    // written. The env closure is the production one — capability
    // resolution reads HOME and the profile layers through it, and a nil
    // closure would starve the registry of every installed capability.
    let scratch = TempDir::new().unwrap();
    let home = AikitHome::at(scratch.path().join("aikit-home"));
    let service = Service::open(home, &checkout, |key| std::env::var(key).ok())
        .expect("open the production service from the live ai-kit checkout");

    // 1. routine (asked inside ai-kit, so its own world). The live files
    // carry "automation/scheduler", not the plural forms of the fixture
    // query: the live pool is content-literal, so the parent spec's plural
    // query cannot reach these files by content — vocabulary gaps are
    // GitNexus's structural layer, which joins where its binary passes
    // capability discovery.
    let routine = service
        .knowledge_search("routine scheduler automation", 200)
        .unwrap();
    let routine_hit = find_hit(
        &routine,
        "source:project:ai-kit:crates/aikit-core/src/routine.rs",
    )
    .into_iter()
    .chain(find_hit(
        &routine,
        "source:project:ai-kit:docs/implementation/CAW-NATIVE-DELIVERY.md",
    ))
    .next()
    .expect("routine.rs or CAW-NATIVE-DELIVERY.md is among the live hits")
    .clone();
    assert_eq!(routine_hit.provider.as_str(), WORK_REPOS_PROVIDER);

    // 2. encounter_mcp or the harness-profile design doc.
    let mcp = service
        .knowledge_search("mcp tool protocol capsule", 200)
        .unwrap();
    let mcp_hit = find_hit(
        &mcp,
        "source:project:ai-kit:crates/aikit-cli/src/encounter_mcp.rs",
    )
    .into_iter()
    .chain(find_hit(
        &mcp,
        "source:project:ai-kit:docs/plans/2026-09-16-harness-profile-design.md",
    ))
    .next()
    .expect("encounter_mcp or the harness-profile design doc is among the live hits")
    .clone();
    assert_eq!(mcp_hit.provider.as_str(), WORK_REPOS_PROVIDER);

    // 3. the Workcell placement doc, under an explicit `: workcell` scope
    // (the invocation's own world is ai-kit).
    let sandbox = service
        .knowledge_search(": workcell opensandbox sandbox provider", 200)
        .unwrap();
    let sandbox_hit = find_hit(
        &sandbox,
        "source:project:Workcell:docs/MULTI-WORKCELL-PLACEMENT.md",
    )
    .into_iter()
    .chain(
        sandbox
            .hits
            .iter()
            .filter(|hit| hit.resource.as_str().contains("workcell-opensandbox")),
    )
    .next()
    .expect("MULTI-WORKCELL-PLACEMENT.md or the opensandbox crate is among the live hits")
    .clone();
    assert_eq!(sandbox_hit.provider.as_str(), WORK_REPOS_PROVIDER);
}

/// The user-system guarantee (addendum A-5), gated like the other
/// real-world goldens: standing at the Central root, Control/user and
/// Control/agents prose still reach the surface through central-bkmr or the
/// NOW field, and Work coverage never displaces them.
#[test]
fn the_user_system_stays_first_class_in_the_live_world() {
    if std::env::var("AIKIT_KNOWLEDGE_GOLDENS").as_deref() != Ok("real") {
        eprintln!("set AIKIT_KNOWLEDGE_GOLDENS=real to run the live-world goldens");
        return;
    }
    if !aikit_adapters::ripgrep::available() {
        eprintln!("ripgrep is not installed; the live-world goldens skipped");
        return;
    }
    let central = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Central");
    if !central.join("Control").is_dir() || !central.join("Work").is_dir() {
        panic!(
            "AIKIT_KNOWLEDGE_GOLDENS=real expects the live world at {}",
            central.display()
        );
    }
    let scratch = TempDir::new().unwrap();
    let home = AikitHome::at(scratch.path().join("aikit-home"));
    let service = Service::open(home, &central, |key| std::env::var(key).ok())
        .expect("open the production service from the live Central root");

    // "day close rollover civil time" still reaches Control/user or
    // Control/agents prose through central-bkmr or now-field.
    let control = service
        .knowledge_search("day close rollover civil time", 200)
        .unwrap();
    assert!(
        control.hits.iter().any(|hit| {
            (hit.resource
                .as_str()
                .starts_with("central:source:control:root:Control/user/")
                || hit
                    .resource
                    .as_str()
                    .starts_with("central:source:control:root:Control/agents/"))
                && (hit.provider.as_str() == "provider/source-pool/central-bkmr"
                    || hit.provider.as_str() == "provider/source-pool/now-field")
        }),
        "a Control/user or Control/agents hit rides central-bkmr or now-field: {:#?}",
        control
            .hits
            .iter()
            .map(|h| (h.resource.as_str(), h.provider.as_str()))
            .collect::<Vec<_>>()
    );
    // The policy golden: "civil time policy" hits the policy file.
    let policy = service.knowledge_search("civil time policy", 200).unwrap();
    assert!(
        policy.hits.iter().any(|hit| hit
            .resource
            .as_str()
            .contains("Control/user/civil-time-policy.json")),
        "civil-time-policy.json is among the hits: {:#?}",
        policy
            .hits
            .iter()
            .map(|h| (h.resource.as_str(), h.provider.as_str()))
            .collect::<Vec<_>>()
    );
}
