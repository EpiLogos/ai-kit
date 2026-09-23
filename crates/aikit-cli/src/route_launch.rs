//! Route-aware launchers (ADR 0005 Stage 2): run any harness with any model
//! through a declared route, with credential presence confirmed at launch.
//!
//! The composition is deliberately a thin join over facts that already exist:
//! the harness profile's models layer decides *whether and how* a model
//! choice can reach the harness ([`aikit_core::harness_profile`]), the model
//! catalogue owns identity, Actuation's detection and capability intakes
//! supply route availability, and the credential store confirms presence.
//! Nothing here invents a selector, mints a Model, or materialises a key for
//! the presence check. A route that is catalogued but unobserved refuses
//! naming that; a route whose key is unbound refuses naming the bind
//! remediation; a harness whose profile declares no dispatch refuses with the
//! profile's own reason. Coverage honesty beats coverage theater.
//!
//! The stable AgentRef law is untouched: this is a launcher, not an identity
//! grant. Nothing here creates an Agent, an AgentSession or a WorldBinding.

use std::process::Command;

use aikit_adapters::actuation_harness_detection::{
    intake_actuation_capabilities, intake_actuation_detection,
};
use aikit_adapters::actuation_model_routes::{
    harness_provider_reachability, join_model_routes_with_reach, observed_provider_models,
    CredentialEvidence,
};
use aikit_adapters::connection_process::ModelEnvironment;
use aikit_adapters::profiles;
use aikit_adapters::runner::CommandRunner;
use aikit_core::harness_profile::{ModelDispatchPosture, ModelsLayer};
use aikit_core::model_harness_binding::HarnessProviderGate;
use aikit_core::resource::{
    canonical_model_ref, CredentialCondition, ModelRoute, ModelRouteSet, ProviderRef, ResourceRef,
    RouteAvailability, RouteUsability,
};
use aikit_core::{AikitError, Result};
use aikit_store::model_catalogue::{load_provider_catalogs, resolved_catalogue};
use aikit_store::{AikitHome, CredentialBindingStore};
use serde_json::json;

use crate::credential_delivery::ModelCredential;

fn error(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("route_launch", message.to_string())
}

/// The Actuation binary every intake asks, mirroring the client surface.
const ACTUATION_BIN: &str = "actuation";

/// The consumer identity stamped on route-launch credential resolutions. A
/// launcher is not an AgentSession, so it names itself instead of borrowing a
/// session ref.
pub(crate) fn launch_consumer(harness: &str) -> Result<ResourceRef> {
    ResourceRef::parse(format!("harness-run/aikit-route-launch/{harness}"))
}

/// A composed launch: everything decided, nothing spawned yet. The argv and
/// the (optional) scrubbed environment can be disclosed variable-names-only
/// before the child takes over the terminal. The derived `Debug` is safe:
/// [`ModelEnvironment`]'s own `Debug` names variables, never material.
#[derive(Debug)]
pub struct RouteLaunchPlan {
    /// The profile slug the launch was composed against.
    pub harness: String,
    pub program: String,
    pub model: ResourceRef,
    pub provider: ProviderRef,
    pub provider_native_id: String,
    pub route_kind: &'static str,
    /// Credential disclosure for humans: variable names and binding refs,
    /// never material.
    pub credential_disclosure: String,
    /// Delivered credential variable names (the values live only in the
    /// spawned `Command`).
    pub delivered_env_vars: Vec<String>,
    pub argv: Vec<String>,
    pub environment: Option<ModelEnvironment>,
    pub notes: Vec<String>,
}

/// Which route of a set the launcher would take, with honest explanations
/// when none can be taken. Selection reuses the route set's own
/// usable/viable helpers; ambiguity refuses rather than silently picking.
pub(crate) enum RouteSelection<'a> {
    Selected(&'a ModelRoute),
    Ambiguous(Vec<ProviderRef>),
    /// Viable routes exist but none is usable today; each reason is carried.
    NoneUsable(Vec<String>),
    /// No viable route at all (nothing observed).
    NoneViable(Vec<String>),
}

pub(crate) fn select_route<'a>(
    set: &'a ModelRouteSet,
    pin: Option<&ProviderRef>,
) -> RouteSelection<'a> {
    let narrowed = match pin {
        Some(provider) => set.viable_pinned(provider),
        None => set.viable(),
    };
    if narrowed.is_empty() {
        return RouteSelection::NoneViable(
            set.routes
                .iter()
                .filter(|route| pin.is_none_or(|p| route.provider == *p))
                .map(|route| match &route.availability {
                    RouteAvailability::Observed { .. } => {
                        format!("{}: observed but excluded from selection", route.provider)
                    }
                    RouteAvailability::Unobserved { reason }
                    | RouteAvailability::Unavailable { reason } => {
                        format!("{}: {reason}", route.provider)
                    }
                })
                .collect(),
        );
    }
    let usable: Vec<&ModelRoute> = narrowed
        .into_iter()
        .filter(|route| route.is_usable())
        .collect();
    match usable.len() {
        0 => RouteSelection::NoneUsable(
            set.routes
                .iter()
                .filter(|route| pin.is_none_or(|p| route.provider == *p))
                .filter_map(|route| match route.usability() {
                    RouteUsability::NeedsCredential { hint } => Some(format!(
                        "{} needs a credential and none is bound ({hint}); bind it with \
                         `aikit credential setup credential:{}`",
                        route.provider,
                        route
                            .provider
                            .as_str()
                            .strip_prefix("provider:")
                            .unwrap_or(route.provider.as_str())
                    )),
                    RouteUsability::NotObserved { reason } => {
                        Some(format!("{}: {reason}", route.provider))
                    }
                    RouteUsability::Usable => None,
                })
                .collect(),
        ),
        1 => RouteSelection::Selected(usable[0]),
        _ => RouteSelection::Ambiguous(usable.iter().map(|route| route.provider.clone()).collect()),
    }
}

/// One usable route, or the reason the launch refuses. This is the honest
/// three-tier reading: model supported, route observed, route usable.
fn route_for_launch<'a>(
    set: &'a ModelRouteSet,
    pin: Option<&ProviderRef>,
) -> Result<&'a ModelRoute> {
    match select_route(set, pin) {
        RouteSelection::Selected(route) => Ok(route),
        RouteSelection::Ambiguous(providers) => Err(error(format!(
            "several usable routes reach {} ({}); pin one with --provider — selection is \
             explicit, never a guess",
            set.model,
            providers
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
        RouteSelection::NoneUsable(reasons) => {
            // When the blocker is an unbound credential, the refusal carries
            // the shared vocabulary outcome: `credential-gated`, naming the
            // missing credential and the bind remediation — never an opaque
            // failure and never a live call that hangs on authentication.
            let gated = set.routes.iter().any(|route| {
                pin.is_none_or(|p| route.provider == *p)
                    && matches!(route.usability(), RouteUsability::NeedsCredential { .. })
            });
            if gated {
                return Err(AikitError::new(
                    "route_launch.credential_gated",
                    format!(
                        "credential-gated: no observed+usable route reaches {} today:\n  - {}",
                        set.model,
                        reasons.join("\n  - ")
                    ),
                )
                .with("outcome", "credential-gated"));
            }
            Err(error(format!(
                "no observed+usable route reaches {} today:\n  - {}",
                set.model,
                reasons.join("\n  - ")
            )))
        }
        // No viable route at all (nothing observed).
        RouteSelection::NoneViable(reasons) => Err(error(format!(
            "no observed+usable route reaches {} today:\n  - {}",
            set.model,
            reasons.join("\n  - ")
        ))),
    }
}

/// The profile's provider gate as a launch-time verdict, carrying the
/// profile's own reason on refusal.
fn launch_gate<'a>(harness: &str, models: &'a ModelsLayer) -> Result<LaunchDispatch<'a>> {
    match &models.dispatch {
        ModelDispatchPosture::None { reason } => Err(error(format!(
            "the {harness} profile records no model dispatch, so no foreign model can be \
             route-launched into it; the profile's declared reason: {reason}"
        ))),
        ModelDispatchPosture::ProviderPlural => {
            let Some(selectors) = &models.argv_selectors else {
                return Err(error(format!(
                    "the {harness} profile declares provider-plural dispatch but records no \
                     observed per-invocation argv selectors; no selector was invented — launch \
                     {harness} through its own surface, or record its flags in the profile's \
                     `argv-selectors` once observed on its command surface"
                )));
            };
            Ok(LaunchDispatch::ProviderPlural { selectors })
        }
        ModelDispatchPosture::NativeProviderBinding {
            provider_ref,
            selector_kind,
            selector_name,
        } => Ok(LaunchDispatch::NativeBinding {
            provider_ref,
            selector_kind,
            selector_name,
        }),
    }
}

enum LaunchDispatch<'a> {
    ProviderPlural {
        selectors: &'a aikit_core::harness_profile::ModelArgvSelectors,
    },
    NativeBinding {
        provider_ref: &'a str,
        selector_kind: &'a str,
        selector_name: &'a str,
    },
}

impl LaunchDispatch<'_> {
    fn gate(&self) -> HarnessProviderGate {
        match self {
            LaunchDispatch::ProviderPlural { .. } => HarnessProviderGate::Open,
            LaunchDispatch::NativeBinding { provider_ref, .. } => HarnessProviderGate::LimitedTo {
                provider_ref: (*provider_ref).to_string(),
            },
        }
    }

    /// The argv fragment that carries the model choice into the harness, per
    /// its declared selector surface. A selector the launch path cannot honour
    /// (a config key: there is no one-shot spawn-time config surface) refuses
    /// rather than delivering a key and silently ignoring the model choice.
    fn model_args(&self, native_provider: &str, native_id: &str) -> Result<Vec<String>> {
        match self {
            LaunchDispatch::ProviderPlural { selectors } => Ok(vec![
                selectors.provider.clone(),
                native_provider.to_string(),
                selectors.model.clone(),
                native_id.to_string(),
            ]),
            LaunchDispatch::NativeBinding {
                selector_kind,
                selector_name,
                ..
            } => match *selector_kind {
                "argv-flag" => Ok(vec![(*selector_name).to_string(), native_id.to_string()]),
                other => Err(error(format!(
                    "the harness profile binds its provider through a {other:?} selector \
                     ({selector_name:?}), and AIKit's launch path has no one-shot {other:?} \
                     surface for it; the model choice cannot be delivered at spawn — record an \
                     argv selector in the profile once one is observed on the harness's command \
                     surface, or launch the harness directly and let its own config decide the \
                     model"
                ))),
            },
        }
    }
}

/// Resolve the model's routes from the catalogue join, exactly as the compose
/// surface does: identity from the catalogue, availability from Actuation's
/// detection plus reachable harness dispatch, credential state from the
/// binding store (presence only, never materialised).
pub(crate) fn joined_routes(
    runner: &dyn CommandRunner,
    home: &AikitHome,
    model: &ResourceRef,
) -> Result<(ModelRouteSet, Vec<String>)> {
    let (catalogue, mut notes) = resolved_catalogue(home);
    catalogue.get(model).ok_or_else(|| {
        error(format!(
            "{model} is absent from the canonical Model catalogue; route launching addresses \
             catalogued identity only — author an owner entry under \
             <AIKIT home>/model-catalogue/ or refresh a Provider Source"
        ))
    })?;

    let detection = intake_actuation_detection(runner, ACTUATION_BIN);
    let capabilities = intake_actuation_capabilities(runner, ACTUATION_BIN);
    let (mut observed, detection_notes) = observed_provider_models(&detection);
    notes.extend(detection_notes);
    let (documents, problems) = load_provider_catalogs(home);
    notes.extend(problems);
    for document in documents {
        let outcome = aikit_adapters::provider_catalog_source::ProviderCatalogOutcome::Observed {
            observations: document.observations,
            source: document.source,
            observed_at: document.observed_at,
        };
        observed.extend(aikit_adapters::provider_catalog_source::observed_router_routes(&outcome));
    }
    let (reachable, reach_notes) = harness_provider_reachability(&detection, &capabilities);
    notes.extend(reach_notes);

    let bindings = match CredentialBindingStore::new(home).list() {
        Ok(bindings) => bindings,
        Err(bind_error) => {
            notes.push(format!(
                "credential bindings unreadable ({bind_error}) — routes are reported without \
                 their credential state, never as usable"
            ));
            Vec::new()
        }
    };
    let credentials = CredentialEvidence::from_binding_refs(
        bindings
            .iter()
            .filter(|binding| !binding.revoked)
            .map(|binding| binding.credential_ref.as_str().to_string()),
    );

    let join = join_model_routes_with_reach(&catalogue, &observed, &reachable, &credentials);
    notes.extend(join.notes.clone());
    let set = join
        .route_sets
        .into_iter()
        .find(|set| set.model == *model)
        .ok_or_else(|| error(format!("{model} resolved with no route set")))?;
    Ok((set, notes))
}

/// Compose the launch plan for `harness` running `model` (optionally pinned to
/// `provider`). No process is spawned and no key is materialised by planning;
/// the caller chooses [`run_plan`] or a disclosure-only rendering.
pub fn plan_route_launch(
    home: &AikitHome,
    harness: &str,
    model_ref: &str,
    provider_pin: Option<&str>,
    passthrough: &[String],
) -> Result<RouteLaunchPlan> {
    plan_route_launch_with_runner(
        // The route join is a probe-shaped spawn of `actuation`: bounded, so a
        // hanging or missing binary refuses inside the shared budget instead
        // of silently stalling the launch.
        &crate::probe::probe_runner(),
        home,
        harness,
        model_ref,
        provider_pin,
        passthrough,
    )
}

pub(crate) fn plan_route_launch_with_runner(
    runner: &dyn CommandRunner,
    home: &AikitHome,
    harness: &str,
    model_ref: &str,
    provider_pin: Option<&str>,
    passthrough: &[String],
) -> Result<RouteLaunchPlan> {
    let registry_slug = super::client::catalog_slug_for(harness).ok_or_else(|| {
        error(format!(
            "`{harness}` is not a harness the client registry knows; see `aikit client status` \
             for the registered surface"
        ))
    })?;
    // The registry's catalog slug and the profile registry's join key agree
    // through the same mapping the client surface uses (gemini-cli -> gemini).
    let slug = profiles::slug_for_target(&aikit_core::TargetId::new(registry_slug))
        .unwrap_or(registry_slug);
    let profile = profiles::for_slug(slug).ok_or_else(|| {
        error(format!(
            "no harness profile is carried for {slug}; the route portal composes launches from \
             profile facts only and invents none"
        ))
    })?;
    let models = profile.models.as_ref().ok_or_else(|| {
        error(format!(
            "the {slug} profile declares no models layer; the route portal cannot say how a \
             model choice would reach it"
        ))
    })?;
    let dispatch = launch_gate(slug, models)?;
    let pin = provider_pin
        .map(ProviderRef::parse)
        .transpose()
        .map_err(|e| {
            error(format!(
                "--provider must be a `provider:<vendor>` ref: {}",
                e.message()
            ))
        })?;

    // The profile's provider gate narrows before the join, so a foreign
    // provider is refused in the profile's own words rather than surfacing as
    // an unobserved route.
    if let LaunchDispatch::NativeBinding { provider_ref, .. } = &dispatch {
        if let Some(pin) = &pin {
            let (compatible, why) = aikit_core::model_harness_binding::gate_candidate(
                &dispatch.gate(),
                Some(pin.as_str()),
            );
            if !compatible {
                return Err(error(format!(
                    "the {slug} profile natively binds {}; the pinned {pin} cannot serve it — \
                     {why}",
                    provider_ref
                )));
            }
        }
    }

    let model = canonical_model_ref(model_ref)?;
    let (set, mut notes) = joined_routes(runner, home, &model)?;
    let route = route_for_launch(&set, pin.as_ref())?;

    // A natively bound harness can only serve its own provider's models; the
    // selected route must sit inside that binding.
    if let LaunchDispatch::NativeBinding { provider_ref, .. } = &dispatch {
        if route.provider.as_str() != *provider_ref {
            return Err(error(format!(
                "the {slug} profile natively binds {provider_ref}; the selected route runs via \
                 {} — the binding cannot serve that provider's models",
                route.provider
            )));
        }
    }

    let native_provider = route
        .provider
        .as_str()
        .strip_prefix("provider:")
        .unwrap_or(route.provider.as_str())
        .to_string();
    let mut model_args = dispatch.model_args(&native_provider, &route.provider_native_id)?;

    let program = profile
        .presence
        .as_ref()
        .and_then(|presence| presence.executables.first())
        .cloned()
        .ok_or_else(|| {
            error(format!(
                "the {slug} profile declares no presence executable; there is nothing to launch"
            ))
        })?;

    // Credential presence was already confirmed through the join (binding
    // store, never materialised): the selected route is usable, which means
    // its condition is Satisfied or NotRequired. Name it for the disclosure.
    let credential_disclosure = match &route.credential {
        CredentialCondition::NotRequired => "not required by this route".to_string(),
        CredentialCondition::Satisfied { binding_ref, .. } => {
            format!("bound ({binding_ref})")
        }
        CredentialCondition::Required { .. } => {
            return Err(error(
                "credential-gated: the selected route reports an unbound credential; \
                 refusing to launch a body that cannot authenticate",
            )
            .with("outcome", "credential-gated"))
        }
    };

    // Key delivery materialises only where the profile declares an env-var
    // path for this provider, through the same seam every other launch uses.
    // pi (and kin) declare no env-var path: the harness's own store stands,
    // and the child inherits the caller's environment unchanged.
    let (environment, delivered_env_vars, delivery_notes) =
        delivery_environment(home, slug, models, &route.provider, &native_provider)?;
    let mut disclosure_notes = Vec::new();
    disclosure_notes.extend(delivery_notes);

    notes.extend(disclosure_notes);
    model_args.extend(passthrough.iter().cloned());
    let mut argv = vec![program.clone()];
    argv.extend(model_args);

    Ok(RouteLaunchPlan {
        harness: slug.to_string(),
        program,
        model: model.clone(),
        provider: route.provider.clone(),
        provider_native_id: route.provider_native_id.clone(),
        route_kind: route.kind.as_str(),
        credential_disclosure,
        delivered_env_vars,
        argv,
        environment,
        notes,
    })
}

/// Whether the profile declares no env-var delivery for this provider at all
/// (as opposed to declaring one that is presently unbound — that case refuses
/// inside the delivery seam).
fn delivered_nothing_declared(models: &ModelsLayer, provider_ref: &str) -> bool {
    models.key_delivery.as_ref().is_none_or(|delivery| {
        !delivery
            .env_var
            .iter()
            .any(|entry| entry.provider_ref == provider_ref)
    })
}

/// Run the composed launch in the foreground: the harness owns the terminal
/// exactly as if the operator had typed it, with the delivered keys present
/// only in its scrubbed environment. Returns the child's exit code.
///
/// This is the one spawn this surface performs, and it is the model path: the
/// pre-checks refuse before it. A program that is not on PATH is
/// `unreachable`; the plan stage has already refused `credential-gated` when
/// the route's credential was unbound, so an unauthenticated launch never
/// starts and cannot hang on a login it cannot complete.
pub fn run_plan(plan: &RouteLaunchPlan) -> Result<i32> {
    let (program, args) = plan
        .argv
        .split_first()
        .ok_or_else(|| error("empty launch argv"))?;
    if crate::probe::which(program).is_none() {
        return Err(AikitError::new(
            "route_launch.harness_unreachable",
            format!(
                "`{program}` is not on PATH (unreachable); install the harness or adjust PATH \
                 and retry the launch"
            ),
        )
        .with("outcome", "unreachable")
        .with("program", program.clone()));
    }
    let mut command = Command::new(program);
    command.args(args);
    if let Some(environment) = plan.environment.as_ref() {
        environment.apply(&mut command);
    }
    let status = command.status().map_err(|e| {
        error(format!(
            "could not launch `{program}`: {e} — is the harness installed and on PATH?"
        ))
    })?;
    Ok(status.code().unwrap_or(1))
}

/// Materialise the profile-declared key delivery for one route's provider
/// through the shared credential seam. Returns the (possibly empty) scrubbed
/// environment, the delivered variable names, and disclosure notes. Nothing
/// is written to disk and nothing appears in receipts; the material exists
/// only inside the returned [`ModelEnvironment`].
pub(crate) fn delivery_environment(
    home: &AikitHome,
    slug: &str,
    models: &ModelsLayer,
    provider: &ProviderRef,
    native_provider: &str,
) -> Result<(Option<ModelEnvironment>, Vec<String>, Vec<String>)> {
    delivery_environment_with(
        home,
        slug,
        models,
        provider,
        native_provider,
        &crate::credential_delivery::default_declared_resolver,
    )
}

/// [`delivery_environment`] with the declared-ref store boundary injected, so
/// tests script the store instead of touching a live vault.
pub(crate) fn delivery_environment_with(
    home: &AikitHome,
    slug: &str,
    models: &ModelsLayer,
    provider: &ProviderRef,
    native_provider: &str,
    resolve_declared: &dyn Fn(
        &aikit_core::SecretRef,
    ) -> Result<aikit_core::credential::SecretValue>,
) -> Result<(Option<ModelEnvironment>, Vec<String>, Vec<String>)> {
    let mut environment = ModelEnvironment::new();
    let mut delivered = Vec::new();
    let mut notes = Vec::new();
    let Some(declared) = models
        .key_delivery
        .as_ref()
        .and_then(|delivery| {
            delivery
                .env_var
                .iter()
                .find(|entry| entry.provider_ref == provider.as_str())
        })
        .cloned()
    else {
        if delivered_nothing_declared(models, provider.as_str()) {
            notes.push(format!(
                "the {slug} profile declares no env-var key path for {provider}; the \
                 harness's own credential store stands"
            ));
        }
        return Ok((None, delivered, notes));
    };
    let consumer = launch_consumer(slug)?;
    let use_ = ModelCredential {
        requirement_ref: aikit_core::credential::SecretRequirementRef::new(format!(
            "secret-requirement:{native_provider}-route-launch"
        ))?,
        credential_ref: aikit_core::credential::CredentialRef::new(format!(
            "credential:{native_provider}"
        ))?,
        target_env: declared.env_var.clone(),
        from_env: None,
    };
    let (_, secret) = crate::credential_delivery::credential_resolved(
        home,
        &consumer,
        &use_,
        true,
        resolve_declared,
    )?;
    let secret = secret.ok_or_else(|| error("Missing delivered key material"))?;
    environment.push_credential(declared.env_var.clone(), secret)?;
    delivered.push(declared.env_var.clone());
    Ok((
        (!environment.is_empty()).then_some(environment),
        delivered,
        notes,
    ))
}

/// The JSON disclosure for a plan (or a dry run): variable names and binding
/// refs only, never material.
pub fn plan_disclosure(plan: &RouteLaunchPlan) -> serde_json::Value {
    json!({
        "harness": plan.harness,
        "program": plan.program,
        "model": plan.model.as_str(),
        "provider": plan.provider.as_str(),
        "provider_native_id": plan.provider_native_id,
        "route_kind": plan.route_kind,
        "credential": plan.credential_disclosure,
        "delivered_env_vars": plan.delivered_env_vars,
        "environment_scrubbed": plan.environment.is_some(),
        "argv": plan.argv,
        "notes": plan.notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::runner::Output;
    use aikit_core::credential::{
        CredentialBindingState, SecretMaterialisationClass, SecretProviderRef, SecretProviderTier,
    };
    use aikit_core::resource::{ModelRouteKind, RouteAvailability};
    use aikit_core::secret_ref::SecretRef;
    use std::collections::BTreeMap;

    // -- fixtures ------------------------------------------------------------

    /// A stub Actuation: answers the two intakes from canned records so the
    /// join runs without the real binary.
    struct StubRunner {
        detection: String,
        capability: String,
    }

    impl CommandRunner for StubRunner {
        fn run(&self, argv: &[String]) -> Result<Output> {
            if argv.contains(&"detect".to_string()) {
                Ok(Output::success(self.detection.clone()))
            } else if argv.contains(&"capability".to_string()) {
                Ok(Output::success(self.capability.clone()))
            } else {
                Ok(Output::failure(2, "unexpected argv"))
            }
        }
    }

    fn detection_record(slugs: &[&str]) -> String {
        let harnesses: Vec<String> = slugs
            .iter()
            .map(|slug| {
                format!(
                    r#"{{"slug":"{slug}","harness_ref":"harness/{slug}","state":"detected",
                        "receipts":{{"executable":"/usr/local/bin/{slug}"}}}}"#
                )
            })
            .collect();
        let harnesses = harnesses.join(",");
        format!(
            r#"{{"schema":"actuation.harness-detection/v1","document":"detection",
                "detection_ref":"detection:fixture","observed_at":"2026-09-19T00:00:00Z",
                "catalog_revision":9,"detector":{{"implementation":"fixture","version":"0"}},
                "harnesses":[{harnesses}],"absent":[],"availability":"complete"}}"#
        )
    }

    fn capability_entry(slug: &str, providers: &[&str]) -> String {
        capability_entry_with_credential(slug, providers, true)
    }

    fn capability_entry_with_credential(slug: &str, providers: &[&str], required: bool) -> String {
        let provider_entries: Vec<String> = providers
            .iter()
            .map(|provider| {
                format!(
                    r#"{{"provider_ref":"{provider}",
                        "selector":{{"kind":"argv-flag","name":"--model"}},
                        "credential":{{"required":{required},"hint":"{provider} inference credential"}}}}"#
                )
            })
            .collect();
        format!(
            r#"{{"harness_slug":"{slug}",
                "model_dispatch":{{"kind":"native-provider-binding","providers":[{}]}}}}"#,
            provider_entries.join(",")
        )
    }

    fn capability_record(entries: &[String]) -> String {
        format!(
            r#"{{"schema":"actuation.harness-capability/v1","document":"capability",
                "catalog_revision":9,"capabilities":[{}]}}"#,
            entries.join(",")
        )
    }

    fn home() -> (tempfile::TempDir, AikitHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().join("aikit"));
        (dir, home)
    }

    fn seed_owner_route(home: &AikitHome, model: &str, providers: &[&str]) {
        let dir = home.root().join("model-catalogue");
        std::fs::create_dir_all(&dir).unwrap();
        let routes: Vec<String> = providers
            .iter()
            .map(|provider| {
                format!(
                    r#"{{"provider":"{provider}","kind":"provider-native",
                        "provider_native_ids":["fixture-native-id"],
                        "credential":{{"condition":"required","hint":"{provider} inference credential"}}}}"#
                )
            })
            .collect();
        let entry = format!(
            r#"[{{"model":"{model}","name":"Fixture","description":"owner route fixture",
                "routes":[{}],"source":"source/owner"}}]"#,
            routes.join(",")
        );
        std::fs::write(dir.join("fixture.json"), entry).unwrap();
    }

    fn seed_binding(home: &AikitHome, vendor: &str) {
        let state = binding_fixture(
            &aikit_core::credential::CredentialRef::new(format!("credential:{vendor}")).unwrap(),
            &format!("provider:test/{vendor}"),
        );
        aikit_store::credentials::CredentialBindingStore::new(home)
            .save(&state)
            .unwrap();
    }

    /// A current, bound, non-revoked binding record with no declared
    /// location. Field-complete by construction; tests adjust the one fact
    /// they exercise.
    fn binding_fixture(
        credential_ref: &aikit_core::credential::CredentialRef,
        provider_ref: &str,
    ) -> CredentialBindingState {
        CredentialBindingState {
            credential_ref: credential_ref.clone(),
            provider_ref: SecretProviderRef::new(provider_ref).unwrap(),
            provider_tier: SecretProviderTier::OsSecureStore,
            materialisation: SecretMaterialisationClass::ProcessEnv,
            binding_provenance: "fixture".into(),
            revision_or_lease_class: None,
            expires_at: None,
            revoked: false,
            metadata: BTreeMap::new(),
            declared_secret_ref: None,
            bound_at_unix_seconds: Some(1_700_000_000),
            last_rotated_at_unix_seconds: None,
            last_verified_at_unix_seconds: None,
        }
    }

    /// An owner route whose provider needs no credential at all.
    fn seed_owner_route_not_required(home: &AikitHome, model: &str, provider: &str) {
        let dir = home.root().join("model-catalogue");
        std::fs::create_dir_all(&dir).unwrap();
        let entry = format!(
            r#"[{{"model":"{model}","name":"Fixture local","description":"owner local route",
                "routes":[{{"provider":"{provider}","kind":"local-serving",
                    "provider_native_ids":["fixture-native-local"],
                    "credential":{{"condition":"not-required"}}}}],"source":"source/owner"}}]"#
        );
        std::fs::write(dir.join("fixture-local.json"), entry).unwrap();
    }

    fn runner_for_capability(detected: &[&str], entries: &[String]) -> StubRunner {
        StubRunner {
            detection: detection_record(detected),
            capability: capability_record(entries),
        }
    }

    fn runner_for(detected: &[&str], dispatches: &[(&str, &[&str])]) -> StubRunner {
        let entries: Vec<String> = dispatches
            .iter()
            .map(|(slug, providers)| capability_entry(slug, providers))
            .collect();
        StubRunner {
            detection: detection_record(detected),
            capability: capability_record(&entries),
        }
    }

    // -- refusals ------------------------------------------------------------

    #[test]
    fn an_unknown_harness_is_refused_naming_the_registry() {
        let (_dir, home) = home();
        let runner = runner_for(&[], &[]);
        let error = plan_route_launch_with_runner(
            &runner,
            &home,
            "no-such-harness",
            "model:llama3.2",
            None,
            &[],
        )
        .unwrap_err();
        assert!(error.message().contains("client registry"), "{error}");
    }

    #[test]
    fn a_none_dispatch_harness_refuses_with_the_profile_declared_reason() {
        let (_dir, home) = home();
        let runner = runner_for(&["zcode"], &[]);
        let error = plan_route_launch_with_runner(
            &runner,
            &home,
            "zcode",
            "model:claude-opus-5",
            None,
            &[],
        )
        .unwrap_err();
        let message = error.message();
        assert!(
            message.contains("declares no native provider binding"),
            "the refusal carries the profile's own reason: {message}"
        );
    }

    #[test]
    fn a_provider_plural_harness_without_observed_selectors_refuses_without_inventing() {
        let (_dir, home) = home();
        let runner = runner_for(&["gemini"], &[("gemini", &["provider:gemini"])]);
        let error = plan_route_launch_with_runner(
            &runner,
            &home,
            "gemini",
            "model:claude-opus-5",
            None,
            &[],
        )
        .unwrap_err();
        let message = error.message();
        assert!(
            message.contains("no observed per-invocation argv selectors"),
            "{message}"
        );
        assert!(message.contains("no selector was invented"), "{message}");
    }

    #[test]
    fn a_foreign_provider_pin_is_refused_in_the_binding_words() {
        let (_dir, home) = home();
        let runner = runner_for(
            &["claude-code", "codex"],
            &[("claude-code", &["provider:anthropic"])],
        );
        let error = plan_route_launch_with_runner(
            &runner,
            &home,
            "claude",
            "model:gpt-5.4",
            Some("provider:openai"),
            &[],
        )
        .unwrap_err();
        let message = error.message();
        assert!(
            message.contains("natively binds provider:anthropic"),
            "{message}"
        );
        assert!(message.contains("provider:openai"), "{message}");
    }

    #[test]
    fn a_config_key_selector_refuses_instead_of_ignoring_the_model_choice() {
        let (_dir, home) = home();
        // claude-code binds provider:anthropic through a config-key selector;
        // the route exists and the key is bound, but the model choice has no
        // one-shot spawn surface, so the launch refuses rather than delivering
        // a key and silently running whatever model the harness config has.
        let runner = runner_for(
            &["claude-code"],
            &[("claude-code", &["provider:anthropic"])],
        );
        seed_binding(&home, "anthropic");
        let error = plan_route_launch_with_runner(
            &runner,
            &home,
            "claude",
            "model:claude-opus-5",
            None,
            &[],
        )
        .unwrap_err();
        let message = error.message();
        assert!(message.contains("config-key"), "{message}");
        assert!(message.contains("argv selector"), "{message}");
    }

    #[test]
    fn an_unobserved_route_refuses_naming_what_is_missing() {
        let (_dir, home) = home();
        // model:deepseek-chat is catalogued (first-party seed), but no
        // detected harness dispatches to provider:deepseek in this fixture:
        // the route is declared, not observed, and the refusal says which.
        let runner = runner_for(&["pi"], &[("pi", &["provider:openai"])]);
        let error =
            plan_route_launch_with_runner(&runner, &home, "pi", "model:deepseek-chat", None, &[])
                .unwrap_err();
        let message = error.message();
        assert!(message.contains("no observed+usable route"), "{message}");
        assert!(
            message.contains("provider:deepseek"),
            "the refusal names the provider that was not observed: {message}"
        );
    }

    #[test]
    fn an_unbound_required_key_refuses_with_the_bind_remediation() {
        let (_dir, home) = home();
        // pi is detected and dispatches provider:deepseek, so the route is
        // observed; nothing is bound for it, so the launch refuses with the
        // exact bind remediation instead of starting a body that cannot
        // authenticate.
        let runner = runner_for(
            &["pi"],
            &[("pi", &["provider:deepseek", "provider:openai"])],
        );
        let error =
            plan_route_launch_with_runner(&runner, &home, "pi", "model:deepseek-chat", None, &[])
                .unwrap_err();
        let message = error.message();
        assert!(message.contains("needs a credential"), "{message}");
        assert!(
            message.contains("aikit credential setup credential:deepseek"),
            "the refusal names the bind remediation: {message}"
        );
        // The shared vocabulary: the refusal is the credential-gated outcome,
        // named on the error, and it fires before any model-path work — no
        // key delivery, no launch, nothing that could hang on a login.
        assert_eq!(
            error.details().get("outcome").map(String::as_str),
            Some("credential-gated"),
            "the refusal carries the vocabulary outcome: {message}"
        );
    }

    #[test]
    fn a_launch_program_off_path_is_unreachable_before_any_spawn() {
        // Probe discipline at the spawn gate: a plan can compose (planning
        // spawns nothing), but `run_plan` refuses with the named outcome when
        // the launch program cannot be found — never an opaque exec error
        // after the fact.
        let plan = RouteLaunchPlan {
            harness: "fixture".to_string(),
            program: "aikit-run-plan-missing-binary-xyz".to_string(),
            model: aikit_core::resource::canonical_model_ref("model:fixture").unwrap(),
            provider: ProviderRef::parse("provider:fixture").unwrap(),
            provider_native_id: "fixture-native".to_string(),
            route_kind: "provider-native",
            credential_disclosure: "not required by this route".to_string(),
            delivered_env_vars: vec![],
            argv: vec!["aikit-run-plan-missing-binary-xyz".to_string()],
            environment: None,
            notes: vec![],
        };
        let error = run_plan(&plan).unwrap_err();
        assert_eq!(error.code(), "route_launch.harness_unreachable");
        assert_eq!(
            error.details().get("outcome").map(String::as_str),
            Some("unreachable"),
            "the refusal carries the vocabulary outcome"
        );
    }

    #[test]
    fn a_revoked_binding_is_no_evidence_of_presence() {
        let (_dir, home) = home();
        let runner = runner_for(&["pi"], &[("pi", &["provider:deepseek"])]);
        let mut state = binding_fixture(
            &aikit_core::credential::CredentialRef::new("credential:deepseek").unwrap(),
            "provider:test/deepseek",
        );
        state.revoked = true;
        aikit_store::credentials::CredentialBindingStore::new(&home)
            .save(&state)
            .unwrap();
        let error =
            plan_route_launch_with_runner(&runner, &home, "pi", "model:deepseek-chat", None, &[])
                .unwrap_err();
        assert!(
            error.message().contains("needs a credential"),
            "a revoked binding must not satisfy the route: {error}"
        );
    }

    #[test]
    fn several_usable_routes_refuse_asking_for_an_explicit_pin() {
        let (_dir, home) = home();
        // Two providers can both serve the owner-catalogued model, and both
        // keys are bound: selection is explicit, never a guess.
        seed_owner_route(
            &home,
            "model:fixture-multi",
            &["provider:probea", "provider:probeb"],
        );
        let runner = runner_for(&["pi"], &[("pi", &["provider:probea", "provider:probeb"])]);
        seed_binding(&home, "probea");
        seed_binding(&home, "probeb");
        let error =
            plan_route_launch_with_runner(&runner, &home, "pi", "model:fixture-multi", None, &[])
                .unwrap_err();
        let message = error.message();
        assert!(message.contains("several usable routes"), "{message}");
        assert!(message.contains("--provider"), "{message}");
        // And the pin resolves the ambiguity to one route.
        let plan = plan_route_launch_with_runner(
            &runner,
            &home,
            "pi",
            "model:fixture-multi",
            Some("provider:probeb"),
            &[],
        )
        .unwrap();
        assert_eq!(plan.provider.as_str(), "provider:probeb");
    }

    #[test]
    fn a_model_outside_the_catalogue_is_refused_without_minting() {
        let (_dir, home) = home();
        let runner = runner_for(&["pi"], &[]);
        let error = plan_route_launch_with_runner(
            &runner,
            &home,
            "pi",
            "model:nobody-catalogued",
            None,
            &[],
        )
        .unwrap_err();
        assert!(
            error.message().contains("canonical Model catalogue"),
            "{error}"
        );
    }

    // -- the happy path ------------------------------------------------------

    #[test]
    fn a_provider_plural_launch_composes_the_observed_flags_and_no_env() {
        let (_dir, home) = home();
        // pi: detected, dispatching provider:deepseek, key bound. The profile
        // declares --provider/--model argv selectors and no env-var key path,
        // so the launch is the harness with the route's flags and an
        // unchanged environment (its own credential store stands).
        let runner = runner_for(&["pi"], &[("pi", &["provider:deepseek"])]);
        seed_binding(&home, "deepseek");
        let plan = plan_route_launch_with_runner(
            &runner,
            &home,
            "pi",
            "model:deepseek-chat",
            None,
            &["--no-session-persistence".to_string()],
        )
        .unwrap();
        assert_eq!(plan.harness, "pi");
        assert_eq!(plan.program, "pi");
        assert_eq!(
            plan.argv,
            vec![
                "pi".to_string(),
                "--provider".to_string(),
                "deepseek".to_string(),
                "--model".to_string(),
                "deepseek-chat".to_string(),
                "--no-session-persistence".to_string(),
            ]
        );
        assert!(
            plan.environment.is_none(),
            "pi declares no env-var key path"
        );
        assert!(plan.delivered_env_vars.is_empty());
        assert!(
            plan.credential_disclosure
                .contains("bound (credential:deepseek)"),
            "presence is disclosed by binding ref, never material: {}",
            plan.credential_disclosure
        );
        // The disclosure names variables and refs, never material (there is
        // none here, and the render is pinned anyway).
        let disclosure = plan_disclosure(&plan);
        assert_eq!(disclosure["route_kind"], "harness-native");
    }

    #[test]
    fn a_local_route_needs_no_credential_and_still_selects() {
        let (_dir, home) = home();
        // An owner-catalogued model whose declared provider needs no
        // credential (local serving behind a detected harness dispatch) is
        // usable with nothing bound at all.
        seed_owner_route_not_required(&home, "model:fixture-local", "provider:probelocal");
        let runner = runner_for_capability(
            &["pi"],
            &[capability_entry_with_credential(
                "pi",
                &["provider:probelocal"],
                false,
            )],
        );
        let plan =
            plan_route_launch_with_runner(&runner, &home, "pi", "model:fixture-local", None, &[])
                .unwrap();
        assert_eq!(plan.provider.as_str(), "provider:probelocal");
        assert!(plan.credential_disclosure.contains("not required"));
    }

    // -- the scripted argv/env proof -----------------------------------------

    #[test]
    fn a_provider_plural_launch_delivers_the_key_under_its_declared_var_scrubbed() {
        // The one composition the embedded profiles cannot produce today (pi
        // has selectors but no env path; gemini/kimi have env paths but no
        // observed selectors), composed from a synthetic profile carrying
        // exactly the facts the schema allows: provider-plural dispatch,
        // observed argv selectors, one declared key delivery. Only the
        // declared-ref *store boundary* is scripted (the resolver suite's own
        // test convention); argv composition, variable law, the scrubbed
        // environment and the child process are the real paths.
        let (_dir, home) = home();
        let models = aikit_core::harness_profile::ModelsLayer {
            posture: aikit_core::harness_profile::LayerPosture::Observed,
            dispatch: ModelDispatchPosture::ProviderPlural,
            roster_note: None,
            compatibility_note: None,
            argv_selectors: Some(aikit_core::harness_profile::ModelArgvSelectors {
                provider: "--provider".to_string(),
                model: "--model".to_string(),
            }),
            key_delivery: Some(aikit_core::harness_profile::ModelKeyDeliveryLayer {
                env_var: vec![aikit_core::harness_profile::ModelKeyDelivery {
                    provider_ref: "provider:probevendor".to_string(),
                    env_var: "PROBE_VENDOR_API_KEY".to_string(),
                }],
                own_login: vec![],
                note: None,
            }),
        };

        // Route and binding: the join normally produces the first and the
        // store the second; both are stated as facts here.
        let route = aikit_core::resource::ModelRoute {
            model: canonical_model_ref("model:fixture").unwrap(),
            provider: ProviderRef::parse("provider:probevendor").unwrap(),
            kind: ModelRouteKind::ProviderNative,
            provider_native_id: "probe-native-x".to_string(),
            endpoint: None,
            availability: RouteAvailability::Observed {
                detection_ref: "detection:fixture".into(),
            },
            credential: CredentialCondition::Satisfied {
                hint: "probevendor inference credential".into(),
                binding_ref: "credential:probevendor".into(),
            },
            provenance: vec![],
        };

        // The argv half: program + provider/model flags + passthrough.
        let dispatch = launch_gate("probe-harness", &models).unwrap();
        let mut argv = vec!["probe-harness".to_string()];
        argv.extend(
            dispatch
                .model_args("probevendor", &route.provider_native_id)
                .unwrap(),
        );
        argv.push("--flag-from-caller".to_string());
        assert_eq!(
            argv,
            vec![
                "probe-harness",
                "--provider",
                "probevendor",
                "--model",
                "probe-native-x",
                "--flag-from-caller",
            ]
        );

        // The env half: a binding whose declared ref names an env:// source,
        // resolved through the injected store boundary, delivered under the
        // declared variable into a scrubbed child.
        std::env::set_var(
            "ROUTE_LAUNCH_PROBE_MATERIAL",
            "probe-material-not-a-real-key",
        );
        let mut state = binding_fixture(
            &aikit_core::credential::CredentialRef::new("credential:probevendor").unwrap(),
            "provider:test/probevendor",
        );
        state.declared_secret_ref =
            Some(SecretRef::parse("env://ROUTE_LAUNCH_PROBE_MATERIAL").unwrap());
        aikit_store::credentials::CredentialBindingStore::new(&home)
            .save(&state)
            .unwrap();

        let scripted = |secret_ref: &SecretRef| {
            let SecretRef::Env { name } = secret_ref else {
                panic!("the fixture binds env:// only");
            };
            aikit_core::credential::SecretValue::new(
                std::env::var(name).expect("the probe material is set"),
            )
        };
        let (environment, delivered, _notes) = delivery_environment_with(
            &home,
            "probe-harness",
            &models,
            &route.provider,
            "probevendor",
            &scripted,
        )
        .unwrap();
        assert_eq!(delivered, vec!["PROBE_VENDOR_API_KEY".to_string()]);
        let environment = environment.expect("a declared delivery builds an environment");

        // The scripted child proves both halves of the security contract:
        // the key is present under its declared variable, and the ambient
        // environment (including an unrelated key) is withheld.
        std::env::set_var("UNRELATED_ROUTE_PROBE_API_KEY", "ambient-must-not-leak");
        let probe_argv = [
            "/bin/sh".to_string(),
            "-c".to_string(),
            "if [ -n \"$PROBE_VENDOR_API_KEY\" ]; then echo delivered; else echo missing; fi; \
             if [ -n \"$UNRELATED_ROUTE_PROBE_API_KEY\" ]; then echo leaked; else echo withheld; fi; \
             if [ -n \"$HOME\" ]; then echo home-kept; else echo home-scrubbed; fi"
                .to_string(),
        ];
        let mut command = std::process::Command::new(&probe_argv[0]);
        command.args(&probe_argv[1..]);
        environment.apply(&mut command);
        let output = command.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines[0], "delivered", "the key reaches the child: {stdout}");
        assert_eq!(
            lines[1], "withheld",
            "the ambient environment is scrubbed: {stdout}"
        );
        assert_eq!(
            lines[2], "home-kept",
            "the allowlist keeps the harness working: {stdout}"
        );
    }
}
