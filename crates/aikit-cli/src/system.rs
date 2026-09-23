//! Owner-side settings disclosure for the O:I System surface (Wave 5).
//!
//! This is the AIKit owner contribution required by Wave 5 System: one
//! read-only command (`aikit system --json`) that projects AIKit's *native*
//! composition truth into the frozen `oi.product-settings-disclosure/v2`
//! descriptor. It is deliberately a read model over already-resolved state —
//! the Project-world read model, the credential-world read model, the
//! SessionSpace authored states and the staging diff. Nothing here resolves,
//! mutates, spawns a child process or materialises a projection.
//!
//! The four contract distinctions are projected, never collapsed:
//!
//! * `declared`  — what the human authored (profile ids, scope origins, action
//!   refs). `null` where nobody authored anything — never a clone of the
//!   resolved `effective` value (§4.8).
//! * `effective` — what AIKit resolved (capability/actor/context horizons).
//! * `active`    — what is materialised now (generation + resolution hash).
//! * `staged`    — the `aikit diff` preview (AIKit's real stage semantics).
//!
//! Stage -> preview/explain -> apply/discard is AIKit's own pipeline
//! (`stage` -> `diff`/`explain` -> `apply` -> `rollback`); this module only
//! discloses it, it does not re-implement it. Credential material is
//! presence-only by construction (`CredentialBindingState` carries no secret
//! field; `CredentialWorldDisclosure` carries identity, provenance and status).

use serde::Serialize;
use serde_json::{json, Value};

use aikit_core::credential::SecretProvider;
use aikit_core::credential_world::ProviderRosterKnowledge;
use aikit_core::{disclose_credential_world, CredentialWorldDisclosure, Result};
use aikit_tui::PaletteBackend;

use crate::app::{AikitApplication, Service, StageRequest};
use crate::session_space_service::SessionSpaceServiceOps;

pub const DISCLOSURE_SCHEMA: &str = "oi.product-settings-disclosure/v2";
pub const CONTRACT_REVISION: &str = "wave-5/system.1";
pub const OWNER_ID: &str = "ai-kit";
pub const OWNER_REF: &str = "ai-kit/system-disclosure";
pub const READING_COMMAND: [&str; 3] = ["aikit", "system", "--json"];
/// The canonical-body convention `reading_digest` obeys (§4.5), named so the
/// composition kernel can verify AIKit's digest against a documented contract.
pub const DIGEST_COVERS: &str = "sha256 over the whole descriptor with every *_unix_ms field zeroed (disclosed_at_unix_ms, owner.observed_at_unix_ms, every axes.*.provenance.observed_at_unix_ms) and owner.reading_digest null";

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Serialize a borrowed value, never moving it. Every field of the read model
/// is borrowed through this so the disclosure composes one consistent reading.
fn tv<T: Serialize + ?Sized>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn provenance(path: &str, observed_at: i64) -> Value {
    json!({
        "owner_ref": OWNER_REF,
        "path": path,
        "observed_at_unix_ms": observed_at,
    })
}

/// A `declared`/`effective`/`active` axis: a value plus its provenance.
fn axis(value: Value, path: &str, observed_at: i64) -> Value {
    json!({ "value": value, "provenance": provenance(path, observed_at) })
}

/// The `staged` axis: a value plus provenance, a stage ref and a stage state.
fn staged_axis(
    value: Value,
    stage_ref: &str,
    stage_state: &str,
    path: &str,
    observed_at: i64,
) -> Value {
    json!({
        "value": value,
        "provenance": provenance(path, observed_at),
        "stage_ref": stage_ref,
        "stage_state": stage_state,
    })
}

/// A disclosure setting. `declared` carries the authored value (or `null` where
/// nothing was authored) with its own `declared_path` provenance; `effective`
/// and `active` carry the resolved and materialised readings with
/// `provenance_path`. The three axes are never three labels for one resolved
/// value (§4.8): a resolved binding is not `declared`, and a null `declared` is
/// the honest reading for a value nobody authored.
#[allow(clippy::too_many_arguments)]
fn setting(
    key: &str,
    title: &str,
    kind: &str,
    declared: Value,
    declared_path: &str,
    effective: Value,
    active: Value,
    staged: Value,
    stage_state: &str,
    expected_effect: &str,
    native_path: &str,
    provenance_path: &str,
    observed_at: i64,
    materialisation_ref: Value,
) -> Value {
    json!({
        "key": key,
        "title": title,
        "kind": kind,
        "axes": {
            "declared": axis(declared, declared_path, observed_at),
            "effective": axis(effective, provenance_path, observed_at),
            "active": {
                "value": active,
                "provenance": provenance(provenance_path, observed_at),
                "materialisation_ref": materialisation_ref,
            },
            "staged": staged_axis(staged, native_path, stage_state, provenance_path, observed_at),
            "expected_effect": { "summary": expected_effect, "ref": "aikit diff" },
        },
        "mutable": false,
        "native_path": native_path,
        "bootstrap": false,
        "drift": {
            "state": "none",
            "between": ["declared", "effective"],
            "remediation_action_ref": null,
        },
    })
}

/// Compose the provider roster and credential statuses from the persisted
/// binding records. Presence and refs only — never a secret value.
fn credential_world(service: &Service) -> Result<CredentialWorldDisclosure> {
    use aikit_adapters::NativeSecureStoreProvider;
    use aikit_core::credential::{
        SecretMaterialisationClass, SecretProviderDescriptor, SecretProviderTier,
        SecretRequirement, SecretRequirementRef,
    };

    let bindings = aikit_store::CredentialBindingStore::new(service.home()).list()?;
    let mut providers = Vec::new();
    let mut requirements = Vec::new();
    for binding in &bindings {
        let descriptor = if binding.provider_tier == SecretProviderTier::OsSecureStore {
            NativeSecureStoreProvider::with_binding(Some(binding))
                .descriptor(&binding.credential_ref)
        } else {
            // For every tier the native adapter does not own (declared secret
            // refs, explicit environment import, the Linux encrypted
            // fallback), the binding record is itself the provider fact.
            // Projecting it through the native adapter's unbound descriptor
            // would call a bound credential unbound.
            SecretProviderDescriptor {
                provider_ref: binding.provider_ref.clone(),
                provider_kind: binding
                    .metadata
                    .get("provider_kind")
                    .cloned()
                    .unwrap_or_else(|| {
                        binding
                            .provider_ref
                            .as_str()
                            .trim_start_matches("provider:")
                            .to_string()
                    }),
                tier: binding.provider_tier,
                available: !binding.revoked,
                headless_capable: true,
                assurance: "persisted binding record; the material is retained by the named provider"
                    .into(),
                degradation: (binding.provider_tier == SecretProviderTier::ExplicitEnvironmentImport)
                    .then(|| {
                        "environment import is the lowest-assurance credential tier and is never promoted"
                            .to_string()
                    }),
                supported_credentials: (!binding.revoked)
                    .then(|| binding.credential_ref.clone())
                    .into_iter()
                    .collect(),
                supported_materialisation: [binding.materialisation.clone()]
                    .into_iter()
                    .collect(),
                binding_provenance: binding.binding_provenance.clone(),
                revision_or_lease_class: binding.revision_or_lease_class.clone(),
            }
        };
        providers.push(descriptor);
        let requirement_ref = SecretRequirementRef::new(format!(
            "secret-requirement:{}",
            binding.credential_ref.as_str()
        ))?;
        requirements.push(SecretRequirement {
            requirement_ref,
            credential_ref: binding.credential_ref.clone(),
            consumer_ref: "operator:aikit-system".into(),
            purpose: "provider authentication".into(),
            permitted_materialisation: [
                SecretMaterialisationClass::ProviderNativeLease,
                SecretMaterialisationClass::ProcessEnv,
            ]
            .into_iter()
            .collect(),
        });
    }
    Ok(disclose_credential_world(
        ProviderRosterKnowledge::Observed { providers },
        &requirements,
        true,
        false,
    ))
}

/// Whether a store CLI is on PATH. A pure path probe — nothing is executed,
/// so `installed` means the boundary was found, never that a vault is
/// unlocked.
fn binary_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

/// Which secret stores could resolve a declared credential ref on this
/// machine right now. Presence only — there is nothing here a value could
/// occupy, and an unlocked vault is deliberately indistinguishable from a
/// locked one.
fn secret_stores() -> Value {
    use aikit_adapters::NativeSecureStoreProvider;
    use aikit_adapters::NativeSecureStoreStatus;
    use aikit_core::credential::CredentialRef;

    let keychain_available = CredentialRef::new("credential:aikit/doctor-probe")
        .ok()
        .map(|probe| {
            NativeSecureStoreProvider::new().status(&probe) != NativeSecureStoreStatus::Unavailable
        })
        .unwrap_or(false);
    let row = |store: &str, scheme: &str, installed: bool, route: &str| {
        json!({
            "store": store,
            "scheme": scheme,
            "availability": if installed { "available" } else { "not found" },
            "route": route,
        })
    };
    json!([
        row(
            "OS secure store",
            "keychain://",
            keychain_available,
            "the platform credential store AIKit binds material into",
        ),
        row(
            "1Password",
            "op://",
            binary_on_path("op"),
            "the op CLI resolves item fields at materialisation",
        ),
        row(
            "varlock",
            "varlock://",
            binary_on_path("varlock"),
            "the varlock CLI resolves sealed env entries (the declared native default)",
        ),
        row(
            "pass",
            "pass://",
            binary_on_path("pass"),
            "the pass(1) gpg-backed store resolves entries at materialisation",
        ),
    ])
}

/// The security posture AIKit actually keeps, as disclosure rows: the trust
/// ledger by state, capture-time secret scanning, and the environment-import
/// gate. Counts and named facts only.
fn security_posture(service: &Service) -> Result<Value> {
    use aikit_core::trust::TrustState;
    use aikit_store::trust::TrustStore;

    let snapshot = TrustStore::new(service.index()).snapshot()?;
    let mut states = std::collections::BTreeMap::new();
    for state in snapshot.entries().values() {
        let name = match state {
            TrustState::Unseen => "unseen",
            TrustState::Dismissed => "dismissed",
            TrustState::Quarantined => "quarantined",
            TrustState::Reviewed => "reviewed",
            TrustState::Trusted => "trusted",
            TrustState::Blocked => "blocked",
            TrustState::Superseded => "superseded",
        };
        *states.entry(name).or_insert(0u64) += 1;
    }
    Ok(json!({
        "trust": {
            "keys": snapshot.len(),
            "states": states,
        },
        "capture_scanning": {
            "enabled": true,
            "families": ["token-shape", "secret-name-context", "entropy"],
            "law": "a captured possible secret is quarantined and never enters the ordinary registry",
        },
        "environment_import_gate": {
            "state": "closed",
            "law": "environment import happens only under an explicit --from-env choice; a matching variable alone never makes it eligible",
        },
    }))
}

/// The credential inventory the settings page renders as lifecycle rows: one
/// row per persisted binding with its declared location and lifecycle
/// timestamps. Presence, refs and timestamps only — there is no field a
/// secret value could occupy.
fn credential_inventory(service: &Service) -> Result<Value> {
    let bindings = aikit_store::CredentialBindingStore::new(service.home()).list()?;
    let rows: Vec<Value> = bindings
        .iter()
        .map(|binding| {
            json!({
                "credential": binding.credential_ref.as_str(),
                "provider": binding.provider_ref.as_str(),
                "tier": binding.provider_tier,
                "materialisation": binding.materialisation,
                "declared_secret_ref": binding
                    .declared_secret_ref
                    .as_ref()
                    .map(|secret_ref| secret_ref.to_string()),
                "bound_at_unix_seconds": binding.bound_at_unix_seconds,
                "last_rotated_at_unix_seconds": binding.last_rotated_at_unix_seconds,
                "revoked": binding.revoked,
                "provenance": binding.binding_provenance,
            })
        })
        .collect();
    Ok(json!(rows))
}

/// The owner's authored model book, at presence-and-refs strength: which
/// files the overlay loaded, each entry's ref and source, and which authored
/// facts it carries. Judgement content (notes, quirk text, reasons) lives in
/// `aikit model-catalogue show`; this row only says the book exists.
fn authored_models(service: &Service) -> Value {
    let load = aikit_store::model_catalogue::load_owner_catalogue(service.home());
    json!({
        "files": load
            .files
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        "problems": load.problems,
        "entries": load
            .catalogue
            .entries()
            .map(|entry| {
                let facts = entry.book.as_ref();
                json!({
                    "model": entry.model,
                    "name": entry.name,
                    "source": entry.source,
                    "authored": {
                        "class": facts.is_some_and(|book| book.class.is_some()),
                        "quirks": facts.map(|book| book.quirks.len()).unwrap_or(0),
                        "use_for": facts.map(|book| book.use_for.len()).unwrap_or(0),
                        "preference": facts.and_then(|book| book.preference.as_ref()).map(|p| p.rank),
                        "excluded": facts.is_some_and(|book| book.excluded()),
                    },
                })
            })
            .collect::<Vec<_>>(),
    })
}

/// The usage overlays the active composition carries, one entry per active
/// capability that has at least one overlay.
fn usage_overlays(service: &Service) -> Value {
    let view = service.resolved();
    let rows: Vec<Value> = view
        .active
        .keys()
        .filter_map(|id| {
            view.explain(id).map(|explanation| {
                json!({
                    "capability": id.to_string(),
                    "overlays": explanation.skill_usage_overlays,
                })
            })
        })
        .filter(|row| {
            !row["overlays"]
                .as_array()
                .map(|v| v.is_empty())
                .unwrap_or(true)
        })
        .collect();
    json!(rows)
}

/// The canonical action disclosure: read-only reads are `disclosed`; native
/// mutations that exist but are not exposed through this seam in Wave 5 are
/// named as `missing_native_obligation` (rendered as obligations, never as
/// disabled controls).
fn actions() -> Vec<Value> {
    let read_only = [
        (
            "aikit.status",
            "Read the effective capability view",
            "aikit status --json",
            &["capability", "project"][..],
        ),
        (
            "aikit.explain",
            "Explain why a capability or Resource has its current evidence",
            "aikit explain --json <resource>",
            &["capability", "resource", "credential"][..],
        ),
        (
            "aikit.history",
            "Read evidence-bearing recent/familiar/changed/recoverable history",
            "aikit history --json [resource]",
            &["capability", "resource"][..],
        ),
        (
            "aikit.diff",
            "Preview what applying the current declarations would change",
            "aikit diff --json",
            &["capability"][..],
        ),
        (
            "aikit.compose",
            "Compose the launch plan (Central profile + Actuation receipt)",
            "aikit compose --json",
            &["project", "harness", "model"][..],
        ),
        (
            "aikit.doctor",
            "Run the health checks",
            "aikit doctor --json",
            &["project"][..],
        ),
        (
            "aikit.model-catalogue",
            "Read the model catalogue and provider roster",
            "aikit model-catalogue --json",
            &["model", "provider"][..],
        ),
        (
            "aikit.method",
            "Discover Methods (skills carrying the METHOD: prefix)",
            "aikit method --json",
            &["method", "skill"][..],
        ),
        (
            "aikit.credential.list",
            "List safe persisted credential binding metadata (presence only)",
            "aikit credential list --json",
            &["credential"][..],
        ),
    ];

    let mut out = Vec::new();
    for (action_ref, title, native_path, subject_kinds) in read_only {
        out.push(json!({
            "action_ref": action_ref,
            "title": title,
            "args": [],
            "availability": "disclosed",
            "unavailable_reason": null,
            "subject_kinds": subject_kinds,
            "authority": { "requires": [], "granted_by": "operator:aikit", "evidence_ref": null },
            "exposure": { "ui": true, "agent": true, "headless": true },
            "explain": { "ref": "aikit explain", "command": ["aikit", "explain", "--json"] },
            "history": { "ref": "aikit history", "command": ["aikit", "history", "--json"] },
            "native_path": native_path,
        }));
    }

    // Mutating ops that exist natively but are not yet disclosed through the
    // System seam in this wave. Named as obligations, exactly as the Agency
    // Gateway section names them today.
    let obligations = [
        (
            "aikit.session.up",
            "Bring up a session topology",
            "aikit session up",
        ),
        (
            "aikit.session.attach",
            "Attach to a session",
            "aikit session attach",
        ),
        (
            "aikit.session.down",
            "Tear down a session",
            "aikit session down",
        ),
        (
            "aikit.apply",
            "Materialise the staged declarations into a generation",
            "aikit apply",
        ),
        (
            "aikit.rollback",
            "Discard the current generation (return the previous)",
            "aikit rollback",
        ),
    ];
    for (action_ref, title, native_path) in obligations {
        out.push(json!({
            "action_ref": action_ref,
            "title": title,
            "args": [],
            "availability": "missing_native_obligation",
            "unavailable_reason": "native operation exists but is not disclosed through the System seam in Wave 5",
            "subject_kinds": ["session", "capability"],
            "authority": { "requires": [], "granted_by": "operator:aikit", "evidence_ref": null },
            "exposure": { "ui": false, "agent": false, "headless": false },
            "explain": { "ref": "aikit explain", "command": ["aikit", "explain", "--json"] },
            "history": { "ref": "aikit history", "command": ["aikit", "history", "--json"] },
            "native_path": native_path,
        }));
    }
    out
}

/// Named native obligations the owner discloses so the surface renders an
/// obligation instead of a disabled button.
fn obligations() -> Vec<&'static str> {
    vec![
        "aikit.session.up / attach / down — native session lifecycle ops not disclosed through the System seam in Wave 5",
        "aikit.apply / aikit.rollback — the stage -> apply/discard verbs exist natively but are not callable through this seam in Wave 5",
        "HarnessComposition Component/Contract/Contribution/Surface body — available only through `aikit compose --json`, not projected inline",
        "Procedure listing — `aikit procedure` is a native read not projected inline in this wave",
        "AgentSession ecology (SessionEcologyReadModel) — not wired into the System reading in this wave",
    ]
}

/// The canonical digest body (§4.5): the whole descriptor with every `*_unix_ms`
/// field zeroed (`disclosed_at_unix_ms`, `owner.observed_at_unix_ms`, and every
/// `axes.*.provenance.observed_at_unix_ms`) and `owner.reading_digest` null.
fn canonical_digest_body(body: &Value) -> Value {
    let mut canonical = body.clone();
    canonical["disclosed_at_unix_ms"] = json!(0);
    canonical["owner"]["observed_at_unix_ms"] = json!(0);
    if let Some(sections) = canonical.get_mut("sections").and_then(Value::as_array_mut) {
        for section in sections.iter_mut() {
            if let Some(settings) = section.get_mut("settings").and_then(Value::as_array_mut) {
                for setting in settings.iter_mut() {
                    if let Some(axes) = setting.get_mut("axes").and_then(Value::as_object_mut) {
                        for axis in ["declared", "effective", "active", "staged"] {
                            if let Some(prov) = axes
                                .get_mut(axis)
                                .and_then(|a| a.get_mut("provenance"))
                                .and_then(Value::as_object_mut)
                            {
                                prov.insert("observed_at_unix_ms".into(), json!(0));
                            }
                        }
                    }
                }
            }
        }
    }
    canonical
}

/// Compute the canonical reading digest over the timestamp-zeroed body and
/// write it into `body["owner"]["reading_digest"]`. Two readings of an
/// unchanged world therefore produce the same digest — the digest changes only
/// when the reading changes, never when the clock changes.
fn finalize_digest(body: &mut Value) -> Result<()> {
    use sha2::{Digest, Sha256};
    let canonical = canonical_digest_body(body);
    let bytes = serde_json::to_vec(&canonical).map_err(|error| {
        aikit_core::AikitError::new(
            "system.digest_encode_failed",
            format!("could not encode descriptor for digest: {error}"),
        )
    })?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    body["owner"]["reading_digest"] = json!(digest);
    Ok(())
}

/// The reason a project-less reading is degraded. Fixed text: it must not vary
/// with the invocation cwd, so two mounts of the same degraded world agree.
const NO_PROJECT_REASON: &str = "no project context is bound: AIKit's settings disclosure needs a project root (run from within a project, or pass --cwd)";

/// A degraded-but-honest reading for a context whose project world cannot be
/// resolved. The descriptor is still a valid `oi.product-settings-disclosure/v2`
/// document with a stable reason — it never exits with an error just because
/// the invocation cwd is not a project root.
fn degraded_disclosure(
    service: &Service,
    observed_at: i64,
    error: &aikit_core::AikitError,
) -> Value {
    let _ = service; // availability/degradations carry the whole story here
    let mut body = json!({
        "schema": DISCLOSURE_SCHEMA,
        "product_id": OWNER_ID,
        "contract_revision": CONTRACT_REVISION,
        "disclosed_at_unix_ms": observed_at,
        "owner": {
            "owner_id": OWNER_ID,
            "owner_ref": OWNER_REF,
            "owner_version": env!("CARGO_PKG_VERSION"),
            "reading_command": READING_COMMAND,
            "reading_digest": null,
            "reading_digest_covers": DIGEST_COVERS,
            "observed_at_unix_ms": observed_at,
        },
        "about": "Resolution and composition layer: sources -> skills -> sets -> profiles -> sessions. Owns none of models/harnesses/session tools — resolves and composes them for a context.",
        "sections": [],
        "actions": actions(),
        "availability": { "state": "degraded", "reason": NO_PROJECT_REASON },
        "degradations": [{
            "subject_ref": null,
            "state": "unavailable",
            "reason": NO_PROJECT_REASON,
            "native_error": error.message(),
        }],
        "obligations": obligations(),
    });
    finalize_digest(&mut body).unwrap_or(());
    body
}

/// Emit the full `oi.product-settings-disclosure/v2` descriptor for the
/// current context. This never returns `Err` merely because the project world
/// could not be resolved: it degrades to an honest reading with a stable reason
/// instead of exiting with a cwd-dependent error.
pub fn disclose(service: &Service) -> Result<Value> {
    let observed_at = now_ms();
    let world = match aikit_tui::project_world_service::project_world(service) {
        Ok(world) => world,
        Err(error) => return Ok(degraded_disclosure(service, observed_at, &error)),
    };
    let world = world.with_credential_world(credential_world(service)?);
    // The per-harness default permission mode is authored in AIKit's own
    // state; its effect lands at each new session open, so there is no
    // materialised (active) reading to report here.
    let model_defaults_declared = crate::model_defaults::declared(service.home())?
        .map(|models| json!(models))
        .unwrap_or(Value::Null);
    let model_defaults_effective = if model_defaults_declared.is_null() {
        json!({})
    } else {
        model_defaults_declared.clone()
    };
    let permission_modes_declared = crate::permission_defaults::declared(service.home())?
        .map(|modes| json!(modes))
        .unwrap_or(Value::Null);
    let permission_modes_effective = match &permission_modes_declared {
        Value::Null => json!({}),
        declared => declared.clone(),
    };

    // SessionSpaces authored for this project (read-only; no spawn).
    let session_spaces =
        SessionSpaceServiceOps::session_space_discover(service, Some(&world.project.project))
            .map(|states| json!(states))
            .unwrap_or_else(|error| json!({ "error": error.message() }));

    // AIKit's real stage: a read-only diff preview. Clean = "none"; any
    // consequential change = "previewed".
    let scope = service.descriptor().default_mutation_scope();
    let staged = service.stage(StageRequest {
        scope,
        toggles: vec![],
    })?;
    let staged_clean = staged.added_dependencies.is_empty()
        && staged.dropped_dependencies.is_empty()
        && staged.still_unavailable.is_empty();
    let staged_value = json!({
        "scope": scope.as_str(),
        "would_add": staged.added_dependencies.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
        "would_drop": staged.dropped_dependencies.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
        "still_unavailable": staged.still_unavailable.iter().map(|(c, why)| json!({
            "capability": c.to_string(),
            "reason": why,
        })).collect::<Vec<_>>(),
        "active_after": staged.projected.active.len(),
    });
    let stage_state = if staged_clean { "none" } else { "previewed" };

    let effective_capabilities = world
        .capability_horizon
        .capabilities
        .iter()
        .map(|c| json!({ "id": c.resource.as_str(), "kind": c.kind.as_str(), "name": c.name }))
        .collect::<Vec<_>>();
    let effective_actions = world
        .capability_horizon
        .actions
        .iter()
        .map(|a| json!({ "id": a.resource.as_str(), "kind": a.kind.as_str(), "name": a.name }))
        .collect::<Vec<_>>();

    let generation = world
        .effective_revision
        .generation
        .as_ref()
        .map(|g| g.to_string());
    // The materialisation ref is the apply receipt / generation id. It is
    // `null` when no generation has been materialised in this context (the
    // resolution hash still names the effective state).
    let materialisation_ref = json!(generation.clone());
    let active_revision = json!({
        "generation": generation,
        "catalog_revision": world.effective_revision.catalog_revision,
        "resolution_hash": world.effective_revision.resolution_hash,
    });

    let project_json = tv(&world.project);
    let profiles_json = tv(&world.resolution_basis.profiles);
    let scopes_json = tv(&world.resolution_basis.scopes);
    let revision_json = tv(&world.effective_revision);
    let capability_horizon_json = tv(&world.capability_horizon.capabilities);
    let action_horizon_json = tv(&world.capability_horizon.actions);
    let resolved_sources_json = tv(&world.information_horizon.resolved_sources);
    let sources_json = tv(&world.information_horizon.sources);
    let retrieval_json = tv(&world.information_horizon.planned_retrieval);
    let models_json = tv(&world.actor_runtime.models);
    let harnesses_json = tv(&world.actor_runtime.harnesses);
    let actors_json = tv(&world.actor_runtime);
    let providers_json = tv(&world.credential_world.providers);
    let credentials_json = tv(&world.credential_world.credentials);
    let inventory_json = credential_inventory(service)?;
    let security_posture_json = security_posture(service)?;
    let secret_stores_json = secret_stores();
    let authored_models_json = authored_models(service);
    let overlays_json = usage_overlays(service);

    // Authored (declared) half of the resolution chain. These come from the
    // authored scope layers (the config files the human wrote), NOT from the
    // resolved read model — that is the whole point of the declared axis.
    let authored_scopes_json = match service.scope_layers() {
        Some(layers) => tv(layers),
        None => json!([]),
    };
    let authored_profiles: Vec<Value> = match service.scope_layers() {
        Some(layers) => {
            let mut set = std::collections::BTreeSet::new();
            for layer in layers {
                for profile in &layer.patch.profiles {
                    set.insert(profile.to_string());
                }
                for use_ in &layer.patch.uses {
                    set.insert(use_.profile.to_string());
                }
            }
            set.into_iter().map(|s| json!(s)).collect()
        }
        None => Vec::new(),
    };
    let authored_profiles_json = json!(authored_profiles);
    // The canonical Explain/History action refs are authored by AIKit's
    // contract; the effective and active refs are the same two that resolved.
    let declared_explain_history = json!(["action/aikit/explain", "action/aikit/history"]);
    let effective_explain_history = json!(["action/aikit/explain", "action/aikit/history"]);
    let active_explain_history = json!(["action/aikit/explain", "action/aikit/history"]);

    let sections = vec![
        json!({
            "id": "resolution",
            "title": "Project / Profile / scope",
            "settings": [
                setting(
                    "project.binding", "Project binding", "reference",
                    Value::Null, "ai-kit:project:binding:authored",
                    project_json.clone(), project_json,
                    Value::Null, "none",
                    "the staged axis is empty: project binding changes through the profile/project seam",
                    "aikit project show", "ai-kit:project:binding", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "resolution.profiles", "Profiles in the resolution basis", "table",
                    authored_profiles_json, "ai-kit:profile:registry",
                    profiles_json.clone(), profiles_json,
                    Value::Null, "none",
                    "applying a profile change re-resolves the basis",
                    "aikit profile", "ai-kit:resolution:profiles", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "resolution.scopes", "Ordered scope layers", "table",
                    authored_scopes_json, "ai-kit:scope:layers",
                    scopes_json.clone(), scopes_json,
                    Value::Null, "none",
                    "scope layer origins are the authored config files",
                    "aikit context", "ai-kit:resolution:scopes", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "resolution.generation", "Effective revision", "scalar",
                    Value::Null, "ai-kit:generation:authored",
                    revision_json.clone(), active_revision.clone(),
                    Value::Null, "none",
                    "applying the current stage mints a new generation",
                    "aikit status --all", "ai-kit:resolution:revision", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
        json!({
            "id": "skills",
            "title": "Skills / SkillSets / Methods / UsageOverlays",
            "settings": [
                setting(
                    "skills.capabilities", "Resolved capability horizon", "table",
                    Value::Null, "ai-kit:capability:authored",
                    json!(effective_capabilities),
                    tv(&world.projection.active_capabilities),
                    Value::Null, "none",
                    "the capability horizon is a resolution, not an authored list",
                    "aikit status --all", "ai-kit:resolution:capability-horizon", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "skills.actions", "Resolved action horizon", "table",
                    Value::Null, "ai-kit:action:authored",
                    json!(effective_actions),
                    tv(&world.projection.active_capabilities),
                    Value::Null, "none",
                    "actions are disclosed with explain/history refs",
                    "aikit status --all", "ai-kit:resolution:action-horizon", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "skills.usage_overlays", "Skill usage overlays in the active composition", "table",
                    Value::Null, "ai-kit:overlay:authored",
                    overlays_json.clone(),
                    overlays_json,
                    Value::Null, "none",
                    "overlays are additive user-authoritative guidance",
                    "aikit skill overlay show", "ai-kit:resolution:usage-overlays", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "skills.methods", "Discovered Methods", "reference",
                    Value::Null, "ai-kit:method:authored", Value::Null, Value::Null,
                    Value::Null, "none",
                    "Methods (skills carrying the METHOD: prefix) are a native read not projected inline in this wave",
                    "aikit method", "ai-kit:method:registry", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
        json!({
            "id": "context-sources",
            "title": "ContextSources / ContextResolution",
            "settings": [
                setting(
                    "context.resolved_sources", "Resolved context sources", "table",
                    Value::Null, "ai-kit:context-source:authored",
                    resolved_sources_json.clone(),
                    resolved_sources_json,
                    Value::Null, "none",
                    "resolved sources carry availability and provider bindings",
                    "aikit context", "ai-kit:resolution:context-sources", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "context.sources", "Context source horizon", "table",
                    Value::Null, "ai-kit:context-source:authored",
                    sources_json.clone(),
                    sources_json,
                    Value::Null, "none",
                    "horizon is addressability, not prompt inclusion",
                    "aikit context", "ai-kit:resolution:context-source-horizon", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "context.planned_retrieval", "Planned retrieval", "table",
                    Value::Null, "ai-kit:context-source:authored",
                    retrieval_json.clone(),
                    retrieval_json,
                    Value::Null, "none",
                    "named what may be retrieved later; never retrieved here",
                    "aikit context", "ai-kit:resolution:retrieval", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
        json!({
            "id": "models",
            "title": "Models / providers / credential refs",
            "settings": [
                setting(
                    "models.candidates", "Resolved model candidates", "table",
                    Value::Null, "ai-kit:model:authored",
                    models_json.clone(),
                    models_json,
                    Value::Null, "none",
                    "model candidates are resolved/observed, never authored in AIKit config",
                    "aikit model-catalogue", "ai-kit:resolution:model-candidates", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "models.providers", "Secret provider roster", "presence",
                    Value::Null, "ai-kit:credential:authored",
                    providers_json.clone(),
                    providers_json,
                    Value::Null, "none",
                    "presence only; no secret material crosses this boundary",
                    "aikit credential list", "ai-kit:credential:providers", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "models.credentials", "Credential statuses by requirement", "presence",
                    Value::Null, "ai-kit:credential:authored",
                    credentials_json.clone(),
                    credentials_json,
                    Value::Null, "none",
                    "presence and ref only; selected provider and tier, never a value",
                    "aikit credential list", "ai-kit:credential:requirements", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "models.inventory", "Credential inventory and lifecycle", "table",
                    Value::Null, "ai-kit:credential:authored",
                    inventory_json.clone(),
                    inventory_json,
                    Value::Null, "none",
                    "one row per binding: provider, declared location, added and last-rotated timestamps; never a value",
                    "aikit credential list", "ai-kit:credential:inventory", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "models.default", "Default model per harness", "table",
                    model_defaults_declared, "ai-kit:models:default:authored",
                    model_defaults_effective, Value::Null, Value::Null, "none",
                    "applies to new chats with native confirmation; explicit model policies and existing chats keep their selection",
                    "aikit config plan --setting ai-kit:models:models.default",
                    "ai-kit:models:default", observed_at, materialisation_ref.clone(),
                ),
                setting(
                    "models.authored", "Owner model book entries", "presence",
                    Value::Null, "ai-kit:model:authored",
                    authored_models_json.clone(),
                    authored_models_json,
                    Value::Null, "none",
                    "presence and refs only: the owner's authored model-catalogue entries and which authored facts each carries; never a secret and never the observed half",
                    "aikit model-catalogue show", "ai-kit:models:authored", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
        json!({
            "id": "permissions",
            "title": "Permissions / session permission modes",
            "settings": [
                setting(
                    "permissions.default-mode", "Default permission mode per harness", "table",
                    permission_modes_declared.clone(), "ai-kit:permissions:default-mode:authored",
                    permission_modes_effective,
                    Value::Null,
                    Value::Null, "none",
                    "applies when a new encounter session opens, and only to a mode the harness advertises; open sessions keep their mode",
                    "aikit config plan --setting ai-kit:permissions:permissions.default-mode",
                    "ai-kit:permissions:default-mode", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
        json!({
            "id": "harnesses",
            "title": "Harnesses / HarnessComposition",
            "settings": [
                setting(
                    "harnesses.candidates", "Resolved harness candidates", "table",
                    Value::Null, "ai-kit:harness:authored",
                    harnesses_json.clone(),
                    harnesses_json,
                    Value::Null, "none",
                    "harness candidates resolve from harnesses + Actuation detection",
                    "aikit compose --json", "ai-kit:resolution:harness-candidates", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "harnesses.actors", "Agent / Agency / Host disclosure", "table",
                    Value::Null, "ai-kit:actor:authored",
                    actors_json.clone(),
                    actors_json,
                    Value::Null, "none",
                    "requested actor identity is preserved even when unresolved",
                    "aikit compose --json", "ai-kit:resolution:actors", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "harnesses.composition", "Harness composition body", "reference",
                    Value::Null, "ai-kit:compose:authored", Value::Null, Value::Null,
                    Value::Null, "none",
                    "the full Component/Contract/Contribution/Surface body is available through the native compose seam",
                    "aikit compose --json", "ai-kit:compose", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
        json!({
            "id": "components",
            "title": "Components / providers / Surfaces",
            "settings": [
                setting(
                    "components.catalogue", "Component / Surface catalogue", "table",
                    Value::Null, "ai-kit:component:authored",
                    json!([]),
                    json!([]),
                    Value::Null, "none",
                    "the cross-system composition catalogue is empty for the desktop: harness-composition components are disclosed through `aikit compose --json`",
                    "aikit compose --json", "ai-kit:resolution:components", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
        json!({
            "id": "session-spaces",
            "title": "SessionSpaces / AgentSessions",
            "settings": [
                setting(
                    "sessions.spaces", "Authored SessionSpaces for this project", "table",
                    Value::Null, "ai-kit:session-space:authored",
                    session_spaces.clone(),
                    session_spaces,
                    Value::Null, "none",
                    "exactly one authored SessionSpace is canonical per project; ambiguity is never silently resolved",
                    "aikit session space list", "ai-kit:session-space:registry", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
        json!({
            "id": "security",
            "title": "Security / trust / secret stores",
            "settings": [
                setting(
                    "security.posture", "Security posture", "table",
                    Value::Null, "ai-kit:security:authored",
                    security_posture_json.clone(),
                    security_posture_json,
                    Value::Null, "none",
                    "the trust ledger, capture-time secret scanning and the environment-import gate, as AIKit actually keeps them",
                    "aikit doctor --json", "ai-kit:security:posture", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "security.secret_stores", "Usable secret stores", "table",
                    Value::Null, "ai-kit:security:authored",
                    secret_stores_json.clone(),
                    secret_stores_json,
                    Value::Null, "none",
                    "which stores could resolve a declared credential ref on this machine; installed means found, never unlocked",
                    "aikit credential explain --json", "ai-kit:security:secret-stores", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
        json!({
            "id": "resource-actions",
            "title": "Resource + Action horizon",
            "settings": [
                setting(
                    "horizon.capabilities", "Resource horizon (capabilities)", "table",
                    Value::Null, "ai-kit:capability:authored",
                    capability_horizon_json.clone(),
                    capability_horizon_json,
                    Value::Null, "none",
                    "every resource carries intent (eligibility/preference) and effective (availability/providers)",
                    "aikit status --all", "ai-kit:resolution:resource-horizon", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "horizon.actions", "Action horizon", "table",
                    Value::Null, "ai-kit:action:authored",
                    action_horizon_json.clone(),
                    action_horizon_json,
                    Value::Null, "none",
                    "canonical Explain/History actions attach to every subject",
                    "aikit status --all", "ai-kit:resolution:action-horizon", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "horizon.explain-history", "Canonical Explain/History actions", "reference",
                    declared_explain_history, "ai-kit:action:catalogue",
                    effective_explain_history,
                    active_explain_history,
                    Value::Null, "none",
                    "one Explain and one History action, contextualised to many subjects",
                    "aikit explain / aikit history", "ai-kit:resolution:actions", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
        json!({
            "id": "generation",
            "title": "Generation / Procedure",
            "settings": [
                setting(
                    "generation.current", "Current generation", "scalar",
                    Value::Null, "ai-kit:generation:authored",
                    tv(&world.effective_revision),
                    active_revision,
                    staged_value, stage_state,
                    "apply commits the staged declarations into a new generation; rollback returns the previous",
                    "aikit apply / aikit rollback", "ai-kit:resolution:revision", observed_at,
                    materialisation_ref.clone(),
                ),
                setting(
                    "generation.procedures", "Recorded Procedures", "table",
                    Value::Null, "ai-kit:procedure:authored", Value::Null, Value::Null,
                    Value::Null, "none",
                    "adopt/inspect/undo Procedures are native CLI reads not projected into this seam in Wave 5",
                    "aikit procedure", "ai-kit:procedure:registry", observed_at,
                    materialisation_ref.clone(),
                ),
            ],
        }),
    ];

    // Owner-level availability is derived from the actual degradation
    // observations (§4.7), not a literal: the reading is `available` only when
    // no subject reported degraded/unavailable.
    let mut degradations = Vec::new();
    for warning in &world.warnings {
        degradations.push(json!({
            "subject_ref": null,
            "state": "degraded",
            "reason": warning,
            "native_error": null,
        }));
    }
    let availability = if degradations.is_empty() {
        json!({ "state": "available", "reason": null })
    } else {
        json!({ "state": "degraded", "reason": "one or more disclosed subjects are degraded; see `degradations`" })
    };

    let mut body = json!({
        "schema": DISCLOSURE_SCHEMA,
        "product_id": OWNER_ID,
        "contract_revision": CONTRACT_REVISION,
        "disclosed_at_unix_ms": observed_at,
        "owner": {
            "owner_id": OWNER_ID,
            "owner_ref": OWNER_REF,
            "owner_version": env!("CARGO_PKG_VERSION"),
            "reading_command": READING_COMMAND,
            "reading_digest": null,
            "reading_digest_covers": DIGEST_COVERS,
            "observed_at_unix_ms": observed_at,
        },
        "about": "Resolution and composition layer: sources -> skills -> sets -> profiles -> sessions. Owns none of models/harnesses/session tools — resolves and composes them for a context.",
        "sections": sections,
        "actions": actions(),
        "availability": availability,
        "degradations": degradations,
        "obligations": obligations(),
    });

    finalize_digest(&mut body)?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_axis_carries_stage_state_and_ref() {
        let axis = staged_axis(json!([]), "aikit diff", "none", "aikit diff", 1);
        assert_eq!(axis["stage_state"], "none");
        assert_eq!(axis["stage_ref"], "aikit diff");
        assert_eq!(axis["provenance"]["owner_ref"], OWNER_REF);
    }

    #[test]
    fn setting_has_all_five_axes_and_no_mutable_control() {
        let s = setting(
            "k",
            "t",
            "scalar",
            Value::Null,
            "ai-kit:authored",
            json!(1),
            json!(1),
            Value::Null,
            "none",
            "no effect",
            "aikit status",
            "ai-kit:resolution",
            1,
            json!(null),
        );
        assert!(s["axes"]["declared"]["value"].is_null());
        assert!(s["axes"]["effective"]["provenance"]["observed_at_unix_ms"].is_number());
        assert!(s["axes"]["active"]["value"].is_number());
        assert!(s["axes"]["active"]["materialisation_ref"].is_null());
        assert_eq!(s["axes"]["staged"]["stage_state"], "none");
        assert_eq!(s["axes"]["expected_effect"]["ref"], "aikit diff");
        assert_eq!(s["mutable"], false);
        assert_eq!(s["drift"]["state"], "none");
    }

    #[test]
    fn declared_axis_carries_its_own_provenance_not_effective() {
        let s = setting(
            "k",
            "t",
            "scalar",
            json!(["authored"]),
            "ai-kit:authored",
            json!(["resolved"]),
            json!(["resolved"]),
            Value::Null,
            "none",
            "no effect",
            "aikit status",
            "ai-kit:resolution",
            1,
            json!(null),
        );
        assert_ne!(
            s["axes"]["declared"]["provenance"]["path"],
            s["axes"]["effective"]["provenance"]["path"]
        );
    }

    #[test]
    fn actions_name_session_ops_as_obligations_not_disabled_controls() {
        let actions = actions();
        let session_attach = actions
            .iter()
            .find(|a| a["action_ref"] == "aikit.session.attach")
            .expect("session attach is disclosed");
        assert_eq!(session_attach["availability"], "missing_native_obligation");
        assert_eq!(session_attach["exposure"]["headless"], false);

        let explain = actions
            .iter()
            .find(|a| a["action_ref"] == "aikit.explain")
            .expect("explain is disclosed");
        assert_eq!(explain["availability"], "disclosed");
        assert_eq!(explain["exposure"]["headless"], true);
    }
}
