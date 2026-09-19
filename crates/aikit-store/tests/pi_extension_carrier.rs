//! The pi extension carrier rides the trust gate.
//!
//! The carrier is a `hook` capsule whose projected payload runs with full
//! session permissions inside every pi session, so the whole design stands
//! on one property: **pi never loads a revision the owner has not
//! reviewed.** These tests prove that property against the real first-party
//! registry — not a fixture — through the one path projection may take: what
//! the resolver yields as active feeds `plan_hooks_projection`, and an
//! inactive carrier sweeps.
//!
//! Unseen (a new revision nobody reviewed) → not active → swept.
//! Trusted (`aikit trust record`) → active → projected, content-addressed.
//! Blocked (standing refusal, identity-keyed) → not active → swept, even
//! though a trusted revision was projected moments before.

use std::path::{Path, PathBuf};

use aikit_adapters::profiles::for_slug;
use aikit_adapters::{
    plan_hooks_projection, HookCarrierSource, HooksProjectionOutcome, ProjectionDir,
    HOOKS_PROJECTION_OWNERSHIP,
};
use aikit_core::capsule::Kind;
use aikit_core::catalog::Catalog;
use aikit_core::resolve::ResolvedView;
use aikit_core::trust::MemoryTrust;
use aikit_core::{
    resolve, CapsuleId, ContextDescriptor, LayerOrigin, PoolPatch, RegistrySource, ResolveRequest,
    ScopeKind, ScopeLayer, TrustState,
};
use aikit_store::registry::{load_registry, RegistryLoad};

const CARRIER_ID: &str = "hook/aikit/pi-extension-carrier";

/// The shipped registry copied into a temp dir (capsule roots are relative
/// to the registry, and the test must stay hermetic), loaded clean.
fn real_registry() -> (tempfile::TempDir, RegistryLoad) {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&repo_root.join("registry"), &tmp.path().join("registry"));
    let load = load_registry(&tmp.path().join("registry"), RegistrySource::personal()).unwrap();
    assert!(
        load.problems.is_empty(),
        "the shipped registry must load clean: {:#?}",
        load.problems
    );
    (tmp, load)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Resolve the real registry with exactly one enabled capability — the
/// carrier — under the supplied trust oracle.
fn resolve_carrier(root: &Path, load: &RegistryLoad, trust: &MemoryTrust) -> ResolvedView {
    let request = ResolveRequest {
        context: ContextDescriptor::for_project(root),
        layers: vec![ScopeLayer::new(
            ScopeKind::Project,
            LayerOrigin::new("pi_extension_carrier::trust_gate"),
            PoolPatch {
                enable: [CapsuleId::parse(CARRIER_ID).unwrap()].into(),
                ..Default::default()
            },
        )],
        policy: Default::default(),
    };
    resolve(&load.catalog, trust, &request).unwrap()
}

/// The carrier payload exactly as the shipped capsule carries it.
fn carrier_payload(load: &RegistryLoad) -> String {
    let capsule = load
        .catalog
        .get(&CapsuleId::parse(CARRIER_ID).unwrap())
        .expect("the carrier ships in the first-party registry");
    let hook = capsule.hook().expect("a hook capsule");
    std::fs::read_to_string(capsule.root.as_ref().unwrap().join(&hook.entry))
        .expect("the payload ships beside the manifest")
}

/// Plan the pi hooks projection for whatever the resolver yielded under
/// `trust`, against a temp home the planner never escapes: `~` in the
/// profile's declared seam expands to this test's home, the way the
/// applying caller expands it.
fn plan(
    load: &RegistryLoad,
    root: &Path,
    trust: &MemoryTrust,
    home: &Path,
) -> HooksProjectionOutcome {
    let view = resolve_carrier(root, load, trust);
    let active = view
        .active_of_kind(Kind::Hook)
        .iter()
        .any(|active| active.id.to_string() == CARRIER_ID);
    let carrier = active.then(|| HookCarrierSource {
        payload: carrier_payload(load),
    });
    plan_hooks_projection(
        carrier,
        for_slug("pi").expect("pi carries an embedded profile"),
        &ProjectionDir::new(
            home.join(".aikit/ctx/projections/pi"),
            "~/.aikit/ctx/projections/pi",
        ),
        |asked| {
            assert_eq!(
                asked, "~/.pi/agent/settings.json",
                "the planner reads exactly the seam the profile declares"
            );
            let seeded = home.join(".pi/agent/settings.json");
            if seeded.is_file() {
                std::fs::read_to_string(&seeded).map(Some)
            } else {
                Ok(None)
            }
        },
        |dir| Ok(list_names(dir)),
    )
    .unwrap()
}

fn list_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Apply a plan's write items the way the applying caller would: `~` expands
/// against the caller's home (here, the test's temp home).
fn apply(outcome: &HooksProjectionOutcome, home: &Path) {
    let items: Vec<&aikit_core::projection::ProjectionItem> = match outcome {
        HooksProjectionOutcome::Projected(plan) => vec![&plan.settings_item, &plan.carrier_item],
        HooksProjectionOutcome::Swept(plan) => vec![&plan.settings_item],
        HooksProjectionOutcome::NotProjected { reason } => panic!("nothing to apply: {reason}"),
    };
    for item in items {
        let aikit_core::projection::ProjectionItem::Write { path, contents } = item else {
            panic!("every planned item is a write: {item:?}");
        };
        let target = expand_home(path, home);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, contents).unwrap();
    }
}

fn expand_home(path: &Path, home: &Path) -> PathBuf {
    match path.to_str() {
        Some(text) if text.starts_with("~/") => home.join(text.trim_start_matches("~/")),
        _ => path.to_path_buf(),
    }
}

fn trusted_carrier(load: &RegistryLoad) -> MemoryTrust {
    let capsule = load
        .catalog
        .get(&CapsuleId::parse(CARRIER_ID).unwrap())
        .unwrap();
    let mut trust = MemoryTrust::default();
    trust.set(
        capsule.source.clone().unwrap(),
        capsule.id.clone(),
        capsule.revision.clone().unwrap(),
        TrustState::Trusted,
    );
    trust
}

fn settings_of(home: &Path) -> serde_json::Value {
    let raw = std::fs::read_to_string(home.join(".pi/agent/settings.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn an_unreviewed_revision_is_never_projected() {
    let (tmp, load) = real_registry();
    let home = tmp.path().join("home");
    let trust = MemoryTrust::default();

    let outcome = plan(&load, tmp.path(), &trust, &home);
    let HooksProjectionOutcome::Swept(plan) = &outcome else {
        panic!("an Unseen revision must not project: {outcome:?}");
    };
    assert!(
        plan.report.added.is_empty() && plan.report.replaced.is_empty(),
        "a sweep adds nothing: {:?}",
        plan.report
    );
    assert!(
        !home.join(".pi/agent/settings.json").exists(),
        "no registration was written for an unreviewed revision"
    );
}

#[test]
fn a_trust_recorded_revision_is_projected_content_addressed() {
    let (tmp, load) = real_registry();
    let home = tmp.path().join("home");
    let trust = trusted_carrier(&load);

    let outcome = plan(&load, tmp.path(), &trust, &home);
    let HooksProjectionOutcome::Projected(plan) = &outcome else {
        panic!("a Trusted revision projects: {outcome:?}");
    };
    let name = plan
        .carrier_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with(HOOKS_PROJECTION_OWNERSHIP) && name.ends_with(".ts"),
        "the projected file carries the ownership marker: {name}"
    );
    assert_eq!(
        plan.report.added,
        vec![name.clone()],
        "the carrier registers exactly once, under its content-addressed name"
    );

    apply(&outcome, &home);
    let projected = std::fs::read_to_string(&plan.carrier_path).unwrap();
    assert_eq!(
        projected,
        carrier_payload(&load),
        "the projected bytes are exactly the reviewed payload"
    );
    let settings = settings_of(&home);
    assert_eq!(
        settings["extensions"],
        serde_json::json!([plan.carrier_path.to_string_lossy().into_owned()]),
        "the settings array carries exactly the current carrier path"
    );
}

#[test]
fn a_new_unreviewed_revision_sweeps_the_previously_trusted_one() {
    let (tmp, load) = real_registry();
    let home = tmp.path().join("home");

    // A trusted revision projects.
    let trust = trusted_carrier(&load);
    apply(&plan(&load, tmp.path(), &trust, &home), &home);
    assert_eq!(
        settings_of(&home)["extensions"].as_array().unwrap().len(),
        1,
        "precondition: the trusted revision is registered"
    );

    // The registry ships a new revision nobody has reviewed: in the resolver's
    // eyes the carrier is no longer active, because the revision on offer is
    // Unseen. The projection must sweep, not keep the stale registration.
    let unseen = MemoryTrust::default();
    let outcome = plan(&load, tmp.path(), &unseen, &home);
    let HooksProjectionOutcome::Swept(plan) = &outcome else {
        panic!("an Unseen successor must sweep, not linger: {outcome:?}");
    };
    assert_eq!(
        plan.report.removed.len(),
        1,
        "the trusted predecessor's registration is swept: {:?}",
        plan.report
    );
    apply(&outcome, &home);
    let settings = settings_of(&home);
    let entries = settings["extensions"].as_array().unwrap();
    assert!(
        entries.is_empty(),
        "nothing AIKit owns stays registered: {entries:?}"
    );
}

#[test]
fn a_blocked_carrier_sweeps_even_after_a_trusted_revision_was_projected() {
    let (tmp, load) = real_registry();
    let home = tmp.path().join("home");

    apply(
        &plan(&load, tmp.path(), &trusted_carrier(&load), &home),
        &home,
    );
    assert!(
        settings_of(&home)["extensions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry
                .as_str()
                .is_some_and(|v| v.contains(HOOKS_PROJECTION_OWNERSHIP))),
        "precondition: the trusted revision was projected"
    );

    // The owner refuses the carrier — a standing verdict, identity-keyed, so
    // no future revision can sneak past it either.
    let mut blocked = MemoryTrust::default();
    blocked.block(
        RegistrySource::personal(),
        CapsuleId::parse(CARRIER_ID).unwrap(),
    );

    let outcome = plan(&load, tmp.path(), &blocked, &home);
    let HooksProjectionOutcome::Swept(_) = &outcome else {
        panic!("a blocked carrier must not project: {outcome:?}");
    };
    apply(&outcome, &home);
    let settings = settings_of(&home);
    let entries = settings["extensions"].as_array().unwrap();
    assert!(
        entries.iter().all(|entry| {
            !entry
                .as_str()
                .is_some_and(|value| value.contains(HOOKS_PROJECTION_OWNERSHIP))
        }),
        "no owned entry survives the block: {entries:?}"
    );
}
