//! Model selection for the existing resident owner. Catalogue identity, scoped
//! source policy, native Agency authority and credential delivery are separate
//! inputs. A configured body is not reported as an inference result.
//!
//! How a bound policy reaches the harness is declared, not assumed: the
//! harness profile joined by the launch program names the selector surface
//! ([`dispatch_for`]) — per-invocation argv flags where the census observed
//! them (pi), the native session's model configuration where the harness
//! selects through its `model` config key (claude-code, codex, and the
//! provider-plural ACP harnesses whose per-session selection the encounter
//! decides) — and a `none` dispatch refuses with its declared reason. A bound
//! policy is delivered or the open fails; it is never silently ignored. The
//! Pi RPC path keeps its existing surface unchanged.
//!
//! Credential delivery has two routes into the same scrubbed final-child
//! environment: the selected-model policy names its credential and target
//! variable explicitly (the pi dispatch path), and the harness profile
//! declares, per provider, the env var a harness's native launch reads
//! ([`profile_environment`]). Both materialise through the identical seam —
//! native store, explicit env import, or a declared ref through the resolver
//! suite — and neither ever passes an empty or ambient value. The seam itself
//! lives once, in [`crate::credential_delivery`].
use super::{error, native_admission, read_binding};
use crate::credential_delivery::{credential, ModelCredential};
use crate::encounter_service::{
    EncounterContextAdmission, EncounterProtocol, EncounterProvider, EncounterRequiredSource,
    EncounterService,
};
use aikit_core::credential::{CredentialRef, SecretRequirementRef};
use aikit_core::harness_profile::HarnessProfile;
use aikit_core::harness_profile::ModelDispatchPosture;
use aikit_core::resource::{canonical_model_ref, CredentialCondition, ProviderRef};
use aikit_core::{ResourceRef, Result};
use aikit_store::{AikitHome, CredentialBindingStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

/// An explicit AIKit-owned dispatch policy supplied through the existing native
/// provider configuration as a pinned SourceRef. It narrows an existing grant;
/// neither this source nor a profile creates the grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelPolicy {
    pub schema: String,
    pub agent_ref: ResourceRef,
    pub world_ref: ResourceRef,
    pub authority_ref: ResourceRef,
    pub bounds_refs: Vec<ResourceRef>,
    pub model_ref: ResourceRef,
    pub provider_ref: ProviderRef,
    pub native_provider: String,
    pub provider_native_id: String,
    pub expires_at_unix_ms: u64,
    pub credential: Option<ModelCredential>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PreparedModel {
    pub policy_source: EncounterRequiredSource,
    pub policy: ModelPolicy,
    pub catalogue_entry: Value,
    pub catalogue_digest: String,
    pub agency_source: Value,
    pub agency_ref: ResourceRef,
    pub world_binding_ref: ResourceRef,
    pub credential_reading: Option<Value>,
    /// Explicit API key, verified Codex own-login, or a route needing no key.
    /// This is a source/readiness fact, never a claim of USD cost or inference.
    #[serde(default)]
    pub credential_mode: String,
    /// The declared dispatch this policy is delivered through. Part of the
    /// serialized basis, so a dispatch change is a basis change.
    pub dispatch: ModelDispatchDelivery,
}

/// The ACP stable schema's session configuration option for the model: the
/// connection adapter observes exactly `id == "model"` with category `model`
/// and sets it with `configId: "model"` (see `agent_connection` and
/// `interactive_connection`). It is the one session-side surface a declared
/// config-key selector can be delivered through, and it exists only where the
/// harness advertises it.
const ACP_SESSION_MODEL_SELECTOR: &str = "model";

/// How a bound model policy is delivered to this harness. Serialized into the
/// resident basis; the variants name the delivery, never a harness guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", deny_unknown_fields)]
pub(crate) enum ModelDispatchDelivery {
    /// The profile declares the per-invocation argv flags the harness reads
    /// (pi: `--provider`/`--model`); the launch appends them and the
    /// harness's own state must still confirm the selection.
    Argv {
        provider_flag: String,
        model_flag: String,
    },
    /// The harness selects its model through its `model` config key —
    /// declared as `selector-kind = "config-key"` (claude-code, codex) or by
    /// a provider-plural posture that leaves per-session selection to the
    /// encounter. The delivery is the native session's own model
    /// configuration option of the declared name: admitted against the
    /// harness's advertised list and confirmed by readback. The harness's
    /// on-disk default config is never rewritten.
    ConfigKey { name: String },
}

/// The dispatch determination for one provider: how a bound policy is
/// delivered, and the one provider the harness natively binds when its
/// profile declares a native binding (the same limit the roster reads
/// through `model_harness_binding`, here applied on the dispatch side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelDispatch {
    pub native_provider_ref: Option<String>,
    pub delivery: ModelDispatchDelivery,
}

/// A profile-derived ACP provider is bound to its embedded connection argv,
/// not to the basename of its wrapper (`npx` for Codex ACP). An arbitrary
/// from_profile string alongside foreign argv cannot claim Codex's login or
/// model-selection contract.
fn declared_provider_profile(
    provider: &EncounterProvider,
) -> Result<Option<&'static HarnessProfile>> {
    if let Some(slug) = provider.from_profile.as_deref() {
        let profile = aikit_adapters::profiles::for_slug(slug)
            .ok_or_else(|| error(format!("unknown harness profile {slug}")))?;
        let derived = crate::encounter_profile_provider::derive_provider(
            profile,
            provider.id.clone(),
            provider.label.clone(),
        )?;
        if provider.argv != derived.argv
            || provider.argv_fallback != derived.argv_fallback
            || provider.protocol != derived.protocol
        {
            return Err(error(format!(
                "profile-derived provider {} differs from the embedded {} connection facts",
                provider.id, slug
            )));
        }
        return Ok(Some(profile));
    }
    Ok(provider
        .argv
        .first()
        .and_then(|program| aikit_adapters::profiles::for_argv_program(program)))
}

/// The declared dispatch for one provider, decided from the harness profile's
/// models layer joined by the launch program — never from the connection
/// protocol alone. The old protocol gate refused every ACP provider because
/// ACP selection semantics were unassumed; the profiles now declare the
/// surfaces, so each harness gets exactly its declared delivery and a
/// harness with no declared surface refuses with its declared reason. The
/// Pi RPC adapter carries its selection into the session open and keeps its
/// existing surface unchanged.
pub(crate) fn dispatch_for(provider: &EncounterProvider) -> Result<ModelDispatch> {
    match provider.protocol {
        EncounterProtocol::PiRpc | EncounterProtocol::PrimeRpc => Ok(ModelDispatch {
            native_provider_ref: None,
            delivery: ModelDispatchDelivery::Argv {
                provider_flag: "--provider".into(),
                model_flag: "--model".into(),
            },
        }),
        EncounterProtocol::Acp => {
            let Some(program) = provider.argv.first() else {
                return Err(error(
                    "A model-selected provider needs a launch program; no dispatch surface is declared for an empty command",
                ));
            };
            let Some(profile) = declared_provider_profile(provider)? else {
                return Err(error(format!(
                    "The launch program {program} joins no harness profile, so no model \
                     dispatch surface is declared for it; a bound policy is never delivered \
                     through an assumed surface"
                )));
            };
            let Some(models) = &profile.models else {
                return Err(error(format!(
                    "The {} profile declares no models layer, so no model dispatch is declared",
                    profile.slug
                )));
            };
            let native_provider_ref = match &models.dispatch {
                ModelDispatchPosture::NativeProviderBinding { provider_ref, .. } => {
                    Some(provider_ref.clone())
                }
                _ => None,
            };
            let delivery = match &models.dispatch {
                ModelDispatchPosture::ProviderPlural => match &models.argv_selectors {
                    Some(selectors) => ModelDispatchDelivery::Argv {
                        provider_flag: selectors.provider.clone(),
                        model_flag: selectors.model.clone(),
                    },
                    None => ModelDispatchDelivery::ConfigKey {
                        name: ACP_SESSION_MODEL_SELECTOR.into(),
                    },
                },
                ModelDispatchPosture::NativeProviderBinding {
                    selector_kind,
                    selector_name,
                    ..
                } => {
                    if selector_kind != "config-key" || selector_name != ACP_SESSION_MODEL_SELECTOR
                    {
                        return Err(error(format!(
                            "The {} profile declares a {selector_kind} model selector named \
                             {selector_name}; the encounter has no delivery for that surface \
                             and does not improvise one",
                            profile.slug
                        )));
                    }
                    ModelDispatchDelivery::ConfigKey {
                        name: selector_name.clone(),
                    }
                }
                ModelDispatchPosture::None { reason } => {
                    return Err(error(format!(
                        "The {} profile declares no model dispatch: {reason}",
                        profile.slug
                    )));
                }
            };
            Ok(ModelDispatch {
                native_provider_ref,
                delivery,
            })
        }
    }
}

/// A harness that binds one provider natively serves exactly that provider;
/// a provider-plural harness takes any provider's policy.
fn check_dispatch_provider(dispatch: &ModelDispatch, policy: &ModelPolicy) -> Result<()> {
    match &dispatch.native_provider_ref {
        Some(declared) if declared != policy.provider_ref.as_str() => Err(error(format!(
            "The harness profile binds {declared} natively; the policy names {}, \
             which this harness does not serve",
            policy.provider_ref
        ))),
        _ => Ok(()),
    }
}

fn read_policy(source: &EncounterRequiredSource) -> Result<ModelPolicy> {
    for path in [
        source.path.clone(),
        source.path.canonicalize().map_err(error)?,
    ] {
        if path
            .ancestors()
            .any(|p| p.join(".no-agent-retrieval").exists())
        {
            return Err(error(
                "Selected model policy is withheld from Agent retrieval",
            ));
        }
    }
    EncounterContextAdmission {
        sources: vec![source.clone()],
        source_activations: vec![],
        projection: None,
        activation: None,
    }
    .verify()?;
    let bytes = fs::read(&source.path).map_err(error)?;
    if format!("blake3:{}", blake3::hash(&bytes).to_hex()) != source.content_digest {
        return Err(error("Model policy changed while it was being read"));
    }
    let policy: ModelPolicy = serde_json::from_slice(&bytes).map_err(error)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_millis();
    if policy.schema != "aikit.model-dispatch-policy/v1"
        || u128::from(policy.expires_at_unix_ms) <= now
        || policy.bounds_refs.is_empty()
        || policy.bounds_refs.len() > 64
        || policy.native_provider.is_empty()
        || policy.native_provider.len() > 128
        || !policy
            .native_provider
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        || policy.provider_native_id.is_empty()
        || policy.provider_native_id.len() > 1024
        || policy.provider_native_id.starts_with('-')
        || policy.provider_native_id.chars().any(char::is_control)
    {
        return Err(error(
            "Model dispatch needs a current, bounded, explicit native policy",
        ));
    }
    canonical_model_ref(policy.model_ref.as_str())?;
    Ok(policy)
}

pub(crate) fn prepare(
    home: &AikitHome,
    session: &ResourceRef,
    provider: &EncounterProvider,
) -> Result<Option<PreparedModel>> {
    let Some(source) = &provider.model_policy else {
        return Ok(None);
    };
    let dispatch = dispatch_for(provider)?;
    let binding = read_binding(home, session)?
        .ok_or_else(|| error("Selected model needs a real Agency/WorldBinding, not a profile"))?;
    let admitted = native_admission(&binding)?;
    let policy = read_policy(source)?;
    let determination = &admitted.receipt["determination"];
    if policy.agent_ref != admitted.agent_ref
        || policy.world_ref != admitted.world_ref
        || !admitted.authorises(&ResourceRef::parse("action/aikit/model-realise")?)
        || !determination["authority_refs"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(policy.authority_ref)))
        || !policy.bounds_refs.iter().all(|r| {
            determination["bounds_refs"]
                .as_array()
                .is_some_and(|a| a.contains(&json!(r)))
        })
    {
        return Err(error("Current Agency does not authorise this selected model policy, World, authority or bounds"));
    }
    check_dispatch_provider(&dispatch, &policy)?;
    let (catalogue, _) = aikit_store::model_catalogue::resolved_catalogue(home);
    let entry = catalogue.get(&policy.model_ref).ok_or_else(|| {
        error("Selected Model is absent from the canonical catalogue; detection does not mint it")
    })?;
    let routes: Vec<_> = entry
        .routes
        .iter()
        .filter(|r| r.provider == policy.provider_ref && r.claims(&policy.provider_native_id))
        .collect();
    if routes.is_empty() {
        return Err(error(
            "Native model/provider does not name a declared route for the canonical Model",
        ));
    }
    let requires_credential = routes
        .iter()
        .any(|r| r.credential != CredentialCondition::NotRequired);
    let codex_own_login = if requires_credential && policy.credential.is_none() {
        let profile = declared_provider_profile(provider)?;
        let program = profile
            .filter(|profile| profile.slug == "codex")
            .and_then(|profile| {
                if provider.from_profile.is_some() {
                    // The ACP wrapper is npx; its validated embedded profile
                    // names the native Codex executable whose store it uses.
                    profile
                        .presence
                        .as_ref()
                        .and_then(|presence| presence.executables.first())
                        .map(String::as_str)
                } else {
                    // An explicitly configured Codex launch may name an absolute
                    // executable. Probe precisely the program that will launch.
                    provider.argv.first().map(String::as_str)
                }
            });
        match program {
            Some(program) => crate::harness_auth::codex_chatgpt_login_ready(
                &crate::probe::probe_runner(),
                home,
                program,
                policy.provider_ref.as_str(),
            )?,
            None => false,
        }
    } else {
        false
    };
    if requires_credential && policy.credential.is_none() && !codex_own_login {
        return Err(error(
            "The declared model route requires an explicitly resolved credential or a verified native Codex own-login for this exact provider",
        ));
    }
    let credential_reading = policy
        .credential
        .as_ref()
        .map(|c| credential(home, session, c, false).map(|r| r.0))
        .transpose()?;
    let catalogue_entry = serde_json::to_value(entry).map_err(error)?;
    let catalogue_digest = format!(
        "blake3:{}",
        blake3::hash(catalogue_entry.to_string().as_bytes()).to_hex()
    );
    let credential_mode = if codex_own_login {
        "codex-chatgpt-own-login"
    } else if policy.credential.is_some() {
        "explicit-api-binding"
    } else {
        "not-required"
    };
    Ok(Some(PreparedModel {
        policy_source: source.clone(),
        policy,
        catalogue_entry,
        catalogue_digest,
        agency_source: serde_json::to_value(&binding.agency_source).map_err(error)?,
        agency_ref: admitted.agency_ref,
        world_binding_ref: admitted.world_binding_ref,
        credential_reading,
        credential_mode: credential_mode.into(),
        dispatch: dispatch.delivery,
    }))
}

impl PreparedModel {
    pub fn fingerprint(&self) -> Result<String> {
        Ok(format!(
            "blake3:{}",
            blake3::hash(&serde_json::to_vec(self).map_err(error)?).to_hex()
        ))
    }
    pub fn require_same(&self, current: &Option<Self>) -> Result<()> {
        if current.as_ref() != Some(self) {
            return Err(error("Selected model source/catalogue/Agency/credential basis changed; explicitly re-resolve the resident"));
        }
        Ok(())
    }
}

/// The exec resolution of one declared launch program: `Some(reason)` names
/// why the program cannot be resolved to an executable; `None` means it
/// resolves. An explicit path (absolute, or containing a separator) resolves
/// when it exists; a bare name resolves when an executable file of that name
/// sits on the given `PATH`. This is a resolution check only, made once
/// before any exec — a program that resolves but fails later is the protocol
/// or the model speaking, never a wrong argv.
fn unresolvable_program_reason_in(
    program: &str,
    search: Option<&std::ffi::OsStr>,
) -> Option<String> {
    if program.is_empty() {
        return Some("the program is empty".to_string());
    }
    let path = std::path::Path::new(program);
    if path.is_absolute() || program.contains('/') {
        return (!path.exists()).then(|| "the path does not exist".to_string());
    }
    let Some(search) = search else {
        return Some("PATH is not set".to_string());
    };
    let hit = std::env::split_paths(search).any(|dir| {
        let candidate = dir.join(path);
        candidate.is_file() && is_executable_file(&candidate)
    });
    (!hit).then(|| "no executable file with this name is on PATH".to_string())
}

fn unresolvable_program_reason(program: &str) -> Option<String> {
    unresolvable_program_reason_in(program, std::env::var_os("PATH").as_deref())
}

#[cfg(unix)]
fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &std::path::Path) -> bool {
    path.is_file()
}

/// The declared launch variant the final exec rides: the primary argv when
/// its program resolves, otherwise the first declared `argv_fallback`
/// variant whose program resolves, in declared order. Declared variants are
/// resolved exactly once, before any exec — never retried, never discovered.
/// When no declared variant resolves, the launch refuses naming every
/// declared variant and what was checked.
fn resolved_launch_variant(provider: &EncounterProvider) -> Result<(Vec<String>, usize)> {
    let mut checked: Vec<String> = Vec::new();
    for (index, variant) in std::iter::once(&provider.argv)
        .chain(provider.argv_fallback.iter())
        .enumerate()
    {
        let declared = if index == 0 {
            "primary".to_string()
        } else {
            format!("fallback {index}")
        };
        let Some(program) = variant.first() else {
            checked.push(format!("{declared}: empty launch argv"));
            continue;
        };
        match unresolvable_program_reason(program) {
            Some(reason) => checked.push(format!("{declared} `{program}` ({reason})")),
            None => return Ok((variant.clone(), index)),
        }
    }
    Err(error(format!(
        "No declared launch variant of provider {} resolves for exec; every declared variant \
         was checked and unresolvable: {}",
        provider.id,
        checked.join("; ")
    )))
}

/// The launch a bound policy resolves to: the declared provider variant
/// selected by exec resolution, carrying the declared dispatch flags where
/// the dispatch rides argv (pi's native flags select its model, and the
/// harness's real get_state and assistant result must also confirm the same
/// provider/id; these arguments alone are not proof). Where the selection
/// rides the native session's model configuration instead, the selected
/// variant starts unchanged and the resident delivers the selection after
/// the session exists.
fn selected_launch(
    provider: &EncounterProvider,
    model: &PreparedModel,
) -> Result<(Vec<String>, usize)> {
    let (base, launch_variant) = resolved_launch_variant(provider)?;
    let ModelDispatchDelivery::Argv {
        provider_flag,
        model_flag,
    } = &model.dispatch
    else {
        return Ok((base, launch_variant));
    };
    if base.is_empty()
        || base.iter().any(|a| {
            a == "--"
                || a == provider_flag
                || a == model_flag
                || a.starts_with(&format!("{provider_flag}="))
                || a.starts_with(&format!("{model_flag}="))
        })
    {
        return Err(error("Model-selected provider needs one unambiguous native provider/model binding; conflicting flags are not rewritten"));
    }
    let mut argv = base;
    argv.extend([
        provider_flag.clone(),
        model.policy.native_provider.clone(),
        model_flag.clone(),
        model.policy.provider_native_id.clone(),
    ]);
    Ok((argv, launch_variant))
}

/// Scoped final-child environment. Owned by the adapters' spawn seam (where
/// every provider child is created); raw material is not serializable and is
/// never stored in model, provider, task, delivery or material receipts.
use aikit_adapters::connection_process::ModelEnvironment;

/// The harness-profile key delivery for one configured provider: the profile
/// joined by the launch program, its declared env-var deliveries materialised
/// through the same credential seam the selected-model path uses, each under
/// its declared variable in the scrubbed final-child environment.
///
/// Per declared provider:
///
/// * a current binding is materialised and delivered;
/// * no binding plus an own-login fact is an honest absence — the harness's
///   native login stands and availability disclosure already reports the
///   unbound credential;
/// * no binding without an own-login fact refuses the launch with the bind
///   remediation, instead of silently starting a body that cannot
///   authenticate;
/// * a revoked or expired binding refuses either way: a withdrawn key is
///   never bypassed through the harness's own login.
///
/// `None` means nothing was declared or bound: the child inherits the
/// caller's environment. Unbound Codex own-login returns `Some(empty)` so the
/// child retains its native login store path but cannot inherit an API key.
pub(crate) fn profile_environment(
    home: &AikitHome,
    session: &ResourceRef,
    provider: &EncounterProvider,
) -> Result<Option<ModelEnvironment>> {
    if provider.argv.is_empty() {
        return Ok(None);
    }
    let Some(profile) = declared_provider_profile(provider)? else {
        return Ok(None);
    };
    let Some(declared) = profile
        .models
        .as_ref()
        .and_then(|models| models.key_delivery.as_ref())
    else {
        return Ok(None);
    };
    if declared.env_var.is_empty() {
        return Ok(None);
    }
    let own_login: BTreeSet<&str> = declared
        .own_login
        .iter()
        .map(|fact| fact.provider_ref.as_str())
        .collect();
    let store = CredentialBindingStore::new(home);
    let mut environment = ModelEnvironment::new();
    let mut codex_own_login_unbound = false;
    for entry in &declared.env_var {
        let vendor = entry
            .provider_ref
            .strip_prefix("provider:")
            .unwrap_or(&entry.provider_ref);
        let credential_ref = CredentialRef::new(format!("credential:{vendor}"))?;
        let binding = store.load(&credential_ref)?;
        let Some(binding) = binding else {
            if own_login.contains(entry.provider_ref.as_str()) {
                if profile.slug == "codex" && entry.provider_ref == "provider:openai" {
                    codex_own_login_unbound = true;
                }
                continue;
            }
            return Err(error(format!(
                "The {} profile declares its native launch reads {} for {} and records no \
                 own-login fallback, but credential:{vendor} is not bound; bind it with \
                 `aikit credential setup credential:{vendor}` (or declare its store \
                 location with --ref) before launching this body",
                profile.slug, entry.env_var, entry.provider_ref,
            )));
        };
        if binding.revoked
            || binding
                .expires_at
                .as_deref()
                .map(|string| string.parse::<jiff::Timestamp>().map_err(error))
                .transpose()?
                .is_some_and(|deadline| deadline <= jiff::Timestamp::now())
        {
            return Err(error(
                "Declared key credential binding is revoked or expired; no environment bypass",
            ));
        }
        let use_ = ModelCredential {
            requirement_ref: SecretRequirementRef::new(format!(
                "secret-requirement:{vendor}-harness-delivery"
            ))?,
            credential_ref,
            target_env: entry.env_var.clone(),
            from_env: None,
        };
        let (_, secret) = credential(home, session, &use_, true)?;
        let secret = secret.ok_or_else(|| error("Missing delivered key material"))?;
        environment.push_credential(entry.env_var.clone(), secret)?;
    }
    // Codex's ChatGPT login uses its native store, not an API key. Keep the
    // final child scrubbed even with no delivered key so an ambient API key
    // cannot silently switch the invocation to a paid credential path.
    Ok((codex_own_login_unbound || !environment.is_empty()).then_some(environment))
}

/// The final-exec resolution of a bound-policy launch: the scoped argv and
/// environment, and which declared provider variant the argv rides. A
/// non-primary selection happens only because the primary launch program
/// could not be resolved for exec; the caller records it on its receipt
/// surface.
struct ResolvedExecution {
    argv: Vec<String>,
    environment: Option<ModelEnvironment>,
    /// 0 is the provider's primary argv; n > 0 is the nth declared
    /// `argv_fallback` variant.
    launch_variant: usize,
}

fn resolved_execution(
    home: &AikitHome,
    session: &ResourceRef,
    provider: &EncounterProvider,
) -> Result<ResolvedExecution> {
    let Some(model) = prepare(home, session, provider)? else {
        // No selected-model policy: the profile-declared key delivery is the
        // whole launch environment (None when nothing is declared and bound),
        // which is the route that carries keys to non-pi harnesses.
        let environment = profile_environment(home, session, provider)?;
        return Ok(ResolvedExecution {
            argv: provider.argv.clone(),
            environment,
            launch_variant: 0,
        });
    };
    let delivery = model
        .policy
        .credential
        .as_ref()
        .map(|c| {
            let (reading, secret) = credential(home, session, c, true)?;
            if model.credential_reading.as_ref() != Some(&reading) {
                return Err(error(
                    "Credential provider changed between resolution and launch",
                ));
            }
            Ok((
                c.target_env.clone(),
                secret.ok_or_else(|| error("Missing model secret material"))?,
            ))
        })
        .transpose()?;
    model.require_same(&prepare(home, session, provider)?)?;
    let mut environment = ModelEnvironment::new();
    if let Some((name, secret)) = delivery {
        environment.push_credential(name, secret)?;
    }
    // A profile-declared delivery rides the same scrubbed environment. The
    // pi profile declares no env-var deliveries, so the pi selected-model
    // path is unchanged by this join.
    if model.credential_mode == "codex-chatgpt-own-login" {
        // A key bound after prepare must not silently switch this selected
        // subscription-mode body onto paid API delivery. The final child is
        // scrubbed and receives no provider key in this mode.
        ensure_codex_api_key_still_unbound(home)?;
    } else if let Some(profile) = profile_environment(home, session, provider)? {
        environment.extend(profile)?;
    }
    let (argv, launch_variant) = selected_launch(provider, &model)?;
    Ok(ResolvedExecution {
        argv,
        environment: Some(environment),
        launch_variant,
    })
}

fn ensure_codex_api_key_still_unbound(home: &AikitHome) -> Result<()> {
    if CredentialBindingStore::new(home)
        .load(&CredentialRef::new("credential:openai")?)?
        .is_some()
    {
        return Err(error(
            "credential:openai changed after Codex own-login preparation; explicitly re-resolve the selected model",
        ));
    }
    Ok(())
}

pub(crate) fn execution(
    home: &AikitHome,
    session: &ResourceRef,
    provider: &EncounterProvider,
) -> Result<(Vec<String>, Option<ModelEnvironment>)> {
    resolved_execution(home, session, provider)
        .map(|resolved| (resolved.argv, resolved.environment))
}

pub(crate) fn direct_launcher(
    session: &ResourceRef,
    provider: &EncounterProvider,
    model: &PreparedModel,
) -> Result<Vec<String>> {
    let mut argv = vec![std::env::current_exe()
        .map_err(error)?
        .display()
        .to_string()];
    if let Some(prefix) = crate::session_space_verb_prefix() {
        argv.push(prefix.to_owned());
    }
    argv.push("encounter-model-exec".into());
    argv.extend([
        "--agent-session".into(),
        session.to_string(),
        "--provider".into(),
        provider.id.clone(),
        "--expected-model-basis".into(),
        model.fingerprint()?,
    ]);
    Ok(argv)
}

impl EncounterService {
    /// Internal native exec, not an IPC operation and not a second runtime.
    pub fn exec_model(
        home: &AikitHome,
        session: &ResourceRef,
        provider_id: &str,
        expected: &str,
    ) -> Result<()> {
        let service = Self::new(home.clone())?;
        service.require_attached(session)?;
        let provider = service
            .providers()?
            .into_iter()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| error("Selected native model provider configuration was removed"))?;
        let model = prepare(home, session, &provider)?
            .ok_or_else(|| error("Model binding was removed; no unselected fallback"))?;
        if model.fingerprint()? != expected {
            return Err(error("Model launch basis changed since native admission"));
        }
        let resolved = resolved_execution(home, session, &provider)?;
        if resolved.launch_variant != 0 {
            // The open path's declared-variant loop cannot reach this re-exec
            // child, so the selection is journaled here — the same receipt
            // surface the open path records its launch attempts on. Receipt
            // trouble refuses the launch rather than execing unrecorded.
            service.store.append(
                session,
                &json!({
                    "kind":"native-model-launch-variant-selected",
                    "provider":provider_id,
                    "attempt":resolved.launch_variant,
                    "argv":resolved.argv,
                    "reason":"primary-launch-program-unresolvable"
                }),
            )?;
        }
        let (argv, environment) = (resolved.argv, resolved.environment);
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| error("Missing native model executable"))?;
        let mut command = Command::new(program);
        command.args(args);
        environment
            .ok_or_else(|| error("Missing scoped model environment"))?
            .apply(&mut command);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            Err(error(command.exec()))
        }
        #[cfg(not(unix))]
        {
            Err(error(
                "Scoped native model exec is unsupported on this platform",
            ))
        }
    }
}

/// Exact target of the existing native open-model operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterModelOpen {
    pub space: aikit_core::session_space::SessionSpaceRef,
    pub agent_session: ResourceRef,
    pub cwd: std::path::PathBuf,
    pub model_ref: ResourceRef,
    pub provider_ref: Option<ProviderRef>,
    pub body: Option<String>,
    pub expected_agency: aikit_adapters::agency_admission::AdmittedAgency,
}
pub(crate) fn validate_target(
    home: &AikitHome,
    session: &ResourceRef,
    configured: &EncounterProvider,
    request: &EncounterModelOpen,
) -> Result<()> {
    let binding = read_binding(home, session)?
        .ok_or_else(|| error("Selected model target lacks native Agency"))?;
    let admitted = native_admission(&binding)?;
    if admitted != request.expected_agency {
        return Err(error("Selected Agency/source/WorldBinding changed between composition and resident admission"));
    }
    let model = prepare(home, session, configured)?
        .ok_or_else(|| error("The configured body has no explicit model policy"))?;
    if model.policy.model_ref != request.model_ref
        || request
            .provider_ref
            .as_ref()
            .is_some_and(|p| p != &model.policy.provider_ref)
        || request.body.as_ref().is_some_and(|b| b != &configured.id)
    {
        return Err(error(
            "Resolved body/model does not match the explicit catalogue target",
        ));
    }
    Ok(())
}
impl EncounterService {
    pub(crate) fn open_model(&self, request: EncounterModelOpen) -> Result<Value> {
        self.require_attached(&request.agent_session)?;
        let mut candidates = Vec::new();
        for configured in self.providers()? {
            if configured.model_policy.is_none()
                || request.body.as_ref().is_some_and(|b| b != &configured.id)
            {
                continue;
            }
            if validate_target(&self.home, &request.agent_session, &configured, &request).is_ok()
                && self
                    .check_task_launch(&request.agent_session, &configured, &request.cwd)
                    .is_ok()
            {
                candidates.push(configured);
            }
        }
        if candidates.len() != 1 {
            return Err(error(if candidates.is_empty() {
                "No configured body has current model/source/credential/authority/protocol eligibility; no fallback selected"
            } else {
                "Several native bodies are eligible; explicitly select a body"
            }));
        }
        let provider = candidates.remove(0);
        let mut result = self.open_native(
            request.space.clone(),
            request.agent_session.clone(),
            provider.id,
            request.cwd.clone(),
            false,
            Some(&request),
        )?;
        result["selected"] = json!(true);
        result["executed"] = json!(false);
        result["standing"] = json!("native selected-model resident, not an inference result");
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::credential_provider::EnvironmentImportProvider;
    // The test seeds binding state through the provider trait.
    use aikit_core::credential::SecretProvider as _;

    fn provider_with_program(program: &str) -> EncounterProvider {
        EncounterProvider {
            protocol: EncounterProtocol::Acp,
            id: "probe".into(),
            label: "probe".into(),
            argv: vec![program.to_string()],
            body_ref: None,
            body_revision: None,
            from_profile: None,
            argv_fallback: Vec::new(),
            env: Default::default(),
            cwd: None,
            required_context: None,
            model_policy: None,
            now_context: None,
        }
    }

    fn session() -> ResourceRef {
        ResourceRef::parse("agent-session/key-delivery-probe").unwrap()
    }

    fn seeded_revoked_binding(home: &AikitHome, credential_ref: &CredentialRef) {
        let provider = EnvironmentImportProvider::from_value(
            credential_ref.clone(),
            "AIKIT_DELIVERY_PROBE_SOURCE",
            Some("fixture-material-not-a-real-key".into()),
        )
        .unwrap();
        let mut state = provider.binding_state(credential_ref).unwrap().unwrap();
        state.revoked = true;
        CredentialBindingStore::new(home).save(&state).unwrap();
    }

    #[test]
    fn a_launch_program_that_joins_no_profile_gets_no_delivery() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        // A bridge or wrapper program joins nothing: no declarations, no
        // environment, no scrub.
        let environment = profile_environment(
            &home,
            &session(),
            &provider_with_program("/opt/homebrew/bin/node"),
        )
        .unwrap();
        assert!(environment.is_none());
    }

    #[test]
    fn an_unbound_declared_key_with_an_own_login_fact_is_an_honest_absence() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        // claude-code declares ANTHROPIC_API_KEY and an own-login fallback,
        // so an unbound binding does not refuse the launch.
        let environment =
            profile_environment(&home, &session(), &provider_with_program("claude")).unwrap();
        assert!(environment.is_none());
    }

    #[test]
    fn an_unbound_required_key_refuses_the_launch_with_the_bind_remediation() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        // kimi declares MOONSHOT_API_KEY with no evidenced own-login store:
        // launching without a binding would start a body that cannot
        // authenticate, so it refuses instead.
        let error =
            profile_environment(&home, &session(), &provider_with_program("kimi")).unwrap_err();
        let message = error.message();
        assert!(message.contains("MOONSHOT_API_KEY"), "{message}");
        assert!(message.contains("credential:moonshot"), "{message}");
        assert!(message.contains("aikit credential setup"), "{message}");
    }

    #[test]
    fn a_revoked_binding_refuses_the_launch_instead_of_being_bypassed() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        seeded_revoked_binding(&home, &CredentialRef::new("credential:moonshot").unwrap());
        let error =
            profile_environment(&home, &session(), &provider_with_program("kimi")).unwrap_err();
        assert!(error.message().contains("revoked or expired"), "{error}");
    }

    #[test]
    fn pi_declares_no_env_delivery_so_its_launch_is_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        let environment =
            profile_environment(&home, &session(), &provider_with_program("pi")).unwrap();
        assert!(environment.is_none(), "pi keeps its policy-delivery path");
    }

    #[test]
    fn codex_own_login_refuses_a_key_bound_after_preparation() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        ensure_codex_api_key_still_unbound(&home).unwrap();
        let credential_ref = CredentialRef::new("credential:openai").unwrap();
        let provider = EnvironmentImportProvider::from_value(
            credential_ref.clone(),
            "AIKIT_CODEX_KEY_SWITCH_PROBE",
            Some("diagnostic-material".into()),
        )
        .unwrap();
        let state = provider.binding_state(&credential_ref).unwrap().unwrap();
        CredentialBindingStore::new(&home).save(&state).unwrap();
        let error = ensure_codex_api_key_still_unbound(&home).unwrap_err();
        assert!(error.message().contains("changed after Codex own-login"));
        assert!(error.message().contains("explicitly re-resolve"));
    }

    // --- dispatch determination ---

    fn acp_provider(argv: &[&str]) -> EncounterProvider {
        EncounterProvider {
            protocol: EncounterProtocol::Acp,
            id: "probe".into(),
            label: "probe".into(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
            body_ref: None,
            body_revision: None,
            from_profile: None,
            argv_fallback: Vec::new(),
            env: Default::default(),
            cwd: None,
            required_context: None,
            model_policy: None,
            now_context: None,
        }
    }

    fn pi_rpc_provider(argv: &[&str]) -> EncounterProvider {
        let mut provider = acp_provider(argv);
        provider.protocol = EncounterProtocol::PiRpc;
        provider
    }

    fn policy_naming(provider_ref: &str) -> ModelPolicy {
        ModelPolicy {
            schema: "aikit.model-dispatch-policy/v1".into(),
            agent_ref: ResourceRef::parse("agent/probe").unwrap(),
            world_ref: ResourceRef::parse("world/probe").unwrap(),
            authority_ref: ResourceRef::parse("authority/probe").unwrap(),
            bounds_refs: vec![ResourceRef::parse("bound/probe").unwrap()],
            model_ref: ResourceRef::parse("model/probe").unwrap(),
            provider_ref: ProviderRef::parse(provider_ref).unwrap(),
            native_provider: "probe-native".into(),
            provider_native_id: "probe-model-1".into(),
            expires_at_unix_ms: u64::MAX,
            credential: None,
        }
    }

    /// The expected determination for one declared executable: the declared
    /// argv dispatch, a config-key session selection (with the natively bound
    /// provider when the profile declares one), or a refusal.
    enum ExpectedDispatch {
        Argv,
        ConfigKey(&'static str),
        Refusal,
    }

    #[test]
    fn pi_rpc_keeps_its_unchanged_argv_dispatch_even_for_a_bridge_program() {
        // The Pi RPC adapter carries the selection into its session open and
        // pi's native flags select its model; a bridge program (which joins
        // no profile) must keep working exactly as before.
        let dispatch = dispatch_for(&pi_rpc_provider(&[
            "/opt/homebrew/bin/node",
            "/bridges/codex-resident.mjs",
        ]))
        .unwrap();
        assert_eq!(dispatch.native_provider_ref, None);
        assert_eq!(
            dispatch.delivery,
            ModelDispatchDelivery::Argv {
                provider_flag: "--provider".into(),
                model_flag: "--model".into(),
            }
        );
    }

    #[test]
    fn a_config_key_profile_declares_the_session_selection_and_binds_its_provider() {
        let claude = dispatch_for(&acp_provider(&["claude"])).unwrap();
        assert_eq!(
            claude.delivery,
            ModelDispatchDelivery::ConfigKey {
                name: "model".into()
            }
        );
        assert_eq!(
            claude.native_provider_ref.as_deref(),
            Some("provider:anthropic"),
            "claude-code binds provider:anthropic natively"
        );
        let codex = dispatch_for(&acp_provider(&["codex"])).unwrap();
        assert_eq!(
            codex.delivery,
            ModelDispatchDelivery::ConfigKey {
                name: "model".into()
            }
        );
        assert_eq!(
            codex.native_provider_ref.as_deref(),
            Some("provider:openai")
        );
    }

    #[test]
    fn a_provider_plural_acp_harness_rides_the_session_model_selector() {
        // gemini and kimi declare provider-plural dispatch with no observed
        // argv flags: per-session selection is the encounter's to decide, and
        // the ACP session's own model configuration is the surface it rides.
        for program in ["gemini", "kimi"] {
            let dispatch = dispatch_for(&acp_provider(&[program])).unwrap();
            assert_eq!(dispatch.native_provider_ref, None);
            assert_eq!(
                dispatch.delivery,
                ModelDispatchDelivery::ConfigKey {
                    name: "model".into()
                },
                "{program}: provider-plural selection rides the session selector"
            );
        }
    }

    #[test]
    fn a_declared_none_harness_refuses_with_its_declared_reason() {
        for (program, fragment) in [
            (
                "openclaw",
                "The catalog capability descriptor is undeclared for openclaw",
            ),
            (
                "cursor-agent",
                "Cursor's model surface is subscription-mediated",
            ),
            (
                "opencode",
                "The catalog declares no capability document for opencode",
            ),
        ] {
            let error = dispatch_for(&acp_provider(&[program])).unwrap_err();
            let message = error.message();
            assert!(
                message.contains("declares no model dispatch"),
                "{program}: {message}"
            );
            assert!(
                message.contains(fragment),
                "{program}: the declared reason must travel: {message}"
            );
        }
    }

    #[test]
    fn a_harness_without_a_models_layer_and_a_bridge_program_refuse() {
        let error = dispatch_for(&acp_provider(&["hermes-acp"])).unwrap_err();
        assert!(
            error.message().contains("declares no models layer"),
            "{}",
            error.message()
        );
        let error = dispatch_for(&acp_provider(&[
            "/opt/homebrew/bin/node",
            "/bridges/pi-acp/index.js",
        ]))
        .unwrap_err();
        assert!(
            error.message().contains("joins no harness profile"),
            "{}",
            error.message()
        );
    }

    #[test]
    fn a_policy_outside_the_declared_native_binding_refuses() {
        let dispatch = dispatch_for(&acp_provider(&["claude"])).unwrap();
        let error =
            check_dispatch_provider(&dispatch, &policy_naming("provider:openai")).unwrap_err();
        let message = error.message();
        assert!(message.contains("provider:anthropic"), "{message}");
        assert!(message.contains("provider:openai"), "{message}");
        check_dispatch_provider(&dispatch, &policy_naming("provider:anthropic")).unwrap();
    }

    #[test]
    fn a_provider_plural_dispatch_takes_any_provider_policy() {
        let dispatch = dispatch_for(&acp_provider(&["gemini"])).unwrap();
        check_dispatch_provider(&dispatch, &policy_naming("provider:gemini")).unwrap();
        check_dispatch_provider(&dispatch, &policy_naming("provider:openrouter")).unwrap();
    }

    #[test]
    fn profile_derived_codex_acp_uses_its_exact_connection_facts() {
        let profile = aikit_adapters::profiles::for_slug("codex").unwrap();
        let provider = crate::encounter_profile_provider::derive_provider(
            profile,
            "codex-profile".to_string(),
            "Codex profile".to_string(),
        )
        .unwrap();
        assert_eq!(provider.argv.first().map(String::as_str), Some("npx"));
        let dispatch = dispatch_for(&provider).unwrap();
        assert_eq!(
            dispatch.native_provider_ref.as_deref(),
            Some("provider:openai")
        );
        assert!(
            matches!(dispatch.delivery, ModelDispatchDelivery::ConfigKey { ref name } if name == "model")
        );
        let mut forged = provider;
        forged.argv = vec!["npx".into(), "-y".into(), "foreign-acp".into()];
        let error = dispatch_for(&forged).unwrap_err();
        assert!(error.message().contains("differs from the embedded codex"));
    }

    #[test]
    fn every_declared_executable_resolves_to_its_expected_dispatch() {
        // The whole declared table in one place: the dispatch determination
        // is decided by the embedded profiles, so the expectation is legible
        // and drifts loudly.
        let expected: &[(&str, ExpectedDispatch)] = &[
            ("claude", ExpectedDispatch::ConfigKey("provider:anthropic")),
            ("codex", ExpectedDispatch::ConfigKey("provider:openai")),
            ("gemini", ExpectedDispatch::ConfigKey("")),
            ("kimi", ExpectedDispatch::ConfigKey("")),
            ("qwen", ExpectedDispatch::ConfigKey("")),
            ("ollama", ExpectedDispatch::ConfigKey("")),
            ("hermes", ExpectedDispatch::ConfigKey("")),
            ("goose", ExpectedDispatch::ConfigKey("")),
            ("pi", ExpectedDispatch::Argv),
            ("openclaw", ExpectedDispatch::Refusal),
            ("cursor-agent", ExpectedDispatch::Refusal),
            ("opencode", ExpectedDispatch::Refusal),
            ("grok-bot", ExpectedDispatch::Refusal),
            ("grok", ExpectedDispatch::Refusal),
            ("aider", ExpectedDispatch::Refusal),
        ];
        for (program, expected) in expected {
            let determined = dispatch_for(&acp_provider(&[program]));
            match expected {
                ExpectedDispatch::Argv => assert!(matches!(
                    determined.unwrap().delivery,
                    ModelDispatchDelivery::Argv { .. }
                )),
                ExpectedDispatch::ConfigKey(provider) => {
                    let dispatch = determined.unwrap();
                    assert!(matches!(
                        dispatch.delivery,
                        ModelDispatchDelivery::ConfigKey { .. }
                    ));
                    assert_eq!(
                        dispatch.native_provider_ref.as_deref(),
                        (!provider.is_empty()).then_some(*provider)
                    );
                }
                ExpectedDispatch::Refusal => assert!(
                    determined.is_err(),
                    "{program}: a declared-none dispatch must refuse"
                ),
            }
        }
    }

    #[test]
    fn profiles_without_a_declared_executable_are_unreachable_by_program_join() {
        // zcode, gemini-antigravity and deepseek-harness declare `none`
        // dispatches but no executable (the census recorded no binary name),
        // so no launch program joins them: an ACP provider naming such a
        // binary refuses as profile-less rather than with the declared
        // reason. The refusal stands either way — a bound policy is never
        // silently ignored.
        for program in ["zcode", "gemini-antigravity", "deepseek-harness"] {
            let error = dispatch_for(&acp_provider(&[program])).unwrap_err();
            assert!(
                error.message().contains("joins no harness profile"),
                "{program}: {}",
                error.message()
            );
        }
    }

    // --- dispatch translation ---

    /// A fake executable at a controlled path: the file exists and carries
    /// the executable bit on unix, so launch-program resolution resolves it.
    fn fake_executable(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    fn prepared_with_dispatch(dispatch: ModelDispatchDelivery) -> PreparedModel {
        PreparedModel {
            policy_source: EncounterRequiredSource {
                source: ResourceRef::parse("source/probe-policy").unwrap(),
                revision: aikit_core::SourceRevision::parse("policy/1").unwrap(),
                path: std::path::PathBuf::from("/tmp/probe-policy.json"),
                content_digest: format!("blake3:{}", blake3::hash(b"probe-policy").to_hex()),
            },
            policy: policy_naming("provider:gemini"),
            catalogue_entry: serde_json::json!({}),
            catalogue_digest: "blake3:probe".into(),
            agency_source: serde_json::json!({}),
            agency_ref: ResourceRef::parse("agency/probe").unwrap(),
            world_binding_ref: ResourceRef::parse("world-binding/probe").unwrap(),
            credential_reading: None,
            credential_mode: "not-required".into(),
            dispatch,
        }
    }

    #[test]
    fn an_argv_dispatch_appends_exactly_the_declared_flags() {
        let temp = tempfile::tempdir().unwrap();
        let program = fake_executable(temp.path(), "probe-pi");
        let provider = pi_rpc_provider(&[program.to_str().unwrap(), "--mode", "rpc"]);
        let model = prepared_with_dispatch(ModelDispatchDelivery::Argv {
            provider_flag: "--provider".into(),
            model_flag: "--model".into(),
        });
        let (argv, variant) = selected_launch(&provider, &model).unwrap();
        assert_eq!(variant, 0);
        assert_eq!(
            argv,
            vec![
                program.display().to_string(),
                "--mode".to_string(),
                "rpc".to_string(),
                "--provider".to_string(),
                "probe-native".to_string(),
                "--model".to_string(),
                "probe-model-1".to_string(),
            ]
        );
    }

    #[test]
    fn a_conflicting_native_flag_refuses_without_rewriting() {
        let temp = tempfile::tempdir().unwrap();
        let program = fake_executable(temp.path(), "probe-pi-conflict");
        let model = prepared_with_dispatch(ModelDispatchDelivery::Argv {
            provider_flag: "--provider".into(),
            model_flag: "--model".into(),
        });
        for existing in [vec!["--model", "other"], vec!["--provider=zai"], vec!["--"]] {
            let mut argv = vec![program.to_str().unwrap().to_string()];
            argv.extend(existing.iter().map(|s| s.to_string()));
            let provider = pi_rpc_provider(&argv.iter().map(|s| s.as_str()).collect::<Vec<_>>());
            let error = selected_launch(&provider, &model).unwrap_err();
            assert!(
                error
                    .message()
                    .contains("conflicting flags are not rewritten"),
                "{existing:?}: {}",
                error.message()
            );
        }
    }

    #[test]
    fn a_config_key_dispatch_launches_the_provider_unchanged() {
        // The selection rides the session's model configuration after the
        // session exists, so the launch argv is the provider's own, never
        // rewritten with pi's flags.
        let temp = tempfile::tempdir().unwrap();
        let program = fake_executable(temp.path(), "probe-gemini");
        let provider = acp_provider(&[program.to_str().unwrap(), "--experimental-acp"]);
        let model = prepared_with_dispatch(ModelDispatchDelivery::ConfigKey {
            name: "model".into(),
        });
        let argv = selected_launch(&provider, &model).unwrap().0;
        assert_eq!(argv, provider.argv);
    }

    // --- declared launch-variant resolution at the final exec ---

    #[test]
    fn an_unresolvable_primary_rides_the_first_resolvable_declared_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let second = fake_executable(temp.path(), "probe-resolved-fallback");
        let mut provider = acp_provider(&["/nonexistent/aikit-probe/primary-program"]);
        provider.argv_fallback = vec![
            vec!["/nonexistent/aikit-probe/first-fallback".to_string()],
            vec![second.display().to_string()],
        ];
        // Declared order decides: the first variant whose program resolves
        // is selected, skipping only the unresolvable ones before it.
        let (argv, variant) = resolved_launch_variant(&provider).unwrap();
        assert_eq!(variant, 2);
        assert_eq!(argv, vec![second.display().to_string()]);
    }

    #[test]
    fn a_resolvable_primary_is_never_swapped_for_a_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let primary = fake_executable(temp.path(), "probe-resolved-primary");
        let mut provider = acp_provider(&[primary.to_str().unwrap()]);
        provider.argv_fallback = vec![vec!["/nonexistent/aikit-probe/fallback".to_string()]];
        let (argv, variant) = resolved_launch_variant(&provider).unwrap();
        assert_eq!(variant, 0);
        assert_eq!(argv, provider.argv, "a resolving primary is never swapped");
    }

    #[test]
    fn a_selected_fallback_carries_the_declared_dispatch_flags() {
        // The dispatch delivery joins whichever declared variant the exec
        // resolution selected — a fallback is a full argv for the same
        // harness in the same protocol mode, so the flags ride it too.
        let temp = tempfile::tempdir().unwrap();
        let fallback = fake_executable(temp.path(), "probe-fallback-flags");
        let mut provider = pi_rpc_provider(&["/nonexistent/aikit-probe/primary"]);
        provider.argv_fallback = vec![vec![fallback.to_str().unwrap().to_string()]];
        let model = prepared_with_dispatch(ModelDispatchDelivery::Argv {
            provider_flag: "--provider".into(),
            model_flag: "--model".into(),
        });
        let (argv, variant) = selected_launch(&provider, &model).unwrap();
        assert_eq!(variant, 1);
        assert_eq!(
            argv,
            vec![
                fallback.display().to_string(),
                "--provider".to_string(),
                "probe-native".to_string(),
                "--model".to_string(),
                "probe-model-1".to_string(),
            ]
        );
    }

    #[test]
    fn no_resolvable_declared_variant_refuses_naming_every_checked_variant() {
        let mut provider = acp_provider(&["/nonexistent/aikit-probe/primary"]);
        provider.argv_fallback = vec![vec!["aikit-probe-no-such-executable".to_string()]];
        let error = resolved_launch_variant(&provider).unwrap_err();
        let message = error.message();
        assert!(message.contains("every declared variant"), "{message}");
        assert!(message.contains(provider.id.as_str()), "{message}");
        assert!(
            message
                .contains("primary `/nonexistent/aikit-probe/primary` (the path does not exist)"),
            "{message}"
        );
        assert!(
            message.contains(
                "fallback 1 `aikit-probe-no-such-executable` (no executable file with this \
                 name is on PATH)"
            ),
            "{message}"
        );
    }

    #[test]
    fn a_bare_name_resolves_only_through_an_executable_hit_on_the_search_path() {
        let temp = tempfile::tempdir().unwrap();
        let executable = fake_executable(temp.path(), "probe-on-path");
        let search = Some(temp.path().as_os_str());
        assert_eq!(
            unresolvable_program_reason_in("probe-on-path", search),
            None,
            "an executable hit on PATH resolves"
        );
        let plain = temp.path().join("probe-not-executable");
        fs::write(&plain, b"not executable").unwrap();
        assert_eq!(
            unresolvable_program_reason_in("probe-not-executable", search),
            Some("no executable file with this name is on PATH".to_string()),
            "a PATH hit without the executable bit is not a resolution"
        );
        assert_eq!(
            unresolvable_program_reason_in("probe-absent", search),
            Some("no executable file with this name is on PATH".to_string())
        );
        assert_eq!(
            unresolvable_program_reason_in(
                "probe-on-path",
                Some(std::path::Path::new("/nonexistent-aikit-probe-path").as_os_str())
            ),
            Some("no executable file with this name is on PATH".to_string())
        );
        assert_eq!(
            unresolvable_program_reason_in(executable.to_str().unwrap(), None),
            None,
            "an explicit path resolves without any PATH"
        );
        assert_eq!(
            unresolvable_program_reason_in("/nonexistent/aikit-probe/primary", None),
            Some("the path does not exist".to_string())
        );
    }

    #[test]
    fn no_policy_resolves_to_no_dispatch_and_no_effect() {
        // The no-policy path returns before any profile join: a provider
        // without a bound policy is untouched, whatever its protocol or argv.
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        let mut provider = acp_provider(&["/opt/homebrew/bin/gemini", "--experimental-acp"]);
        provider.model_policy = None;
        assert!(prepare(&home, &session(), &provider).unwrap().is_none());
        let mut bridge = acp_provider(&["/opt/homebrew/bin/node", "/bridges/x.mjs"]);
        bridge.protocol = EncounterProtocol::PiRpc;
        assert!(prepare(&home, &session(), &bridge).unwrap().is_none());
    }
}
