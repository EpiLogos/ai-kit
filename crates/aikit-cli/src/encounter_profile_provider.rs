//! The connection builder: a harness profile's `sessions.connect` facts become
//! a working, validated encounter provider — profile-derived, never
//! hand-copied.
//!
//! Three operations live here, and every provider whose connection facts
//! exist passes through exactly one of them:
//!
//! * [`derive_provider`] maps one embedded [`HarnessProfile`]'s session
//!   connection facts onto an [`EncounterProvider`]. The profile is the only
//!   source of the argv, the fallback argv variants and the protocol; the
//!   caller supplies only the identity (`id`, `label`).
//! * [`resolve_provider`] is the load-time half: a configured provider that
//!   carries `from_profile` and an empty argv is resolved through
//!   `derive_provider` at provider-load time, so a profile update flows into
//!   every open without the owner re-copying JSON. A `from_profile` provider
//!   that also carries an explicit argv is a contradiction and is refused
//!   naming both. `required_context` and `model_policy` are owner additions
//!   that survive resolution untouched — they pin material and policy, they
//!   do not describe the connection.
//! * [`ensure_connection_facts_reachable`] is the honesty gate both paths
//!   share: a declared launch environment or working directory that cannot
//!   actually reach the provider child is refused, never accepted and
//!   dropped (see its doc for why the launch contract cannot carry them
//!   today).

use std::collections::BTreeMap;

use aikit_adapters::profiles;
use aikit_core::harness_profile::{HarnessProfile, SessionProtocol};
use aikit_core::{AikitError, Result};

use crate::encounter_service::{EncounterProtocol, EncounterProvider};

/// Derive the encounter provider a harness profile's connection facts
/// produce.
///
/// The sessions layer decides the protocol face: `acp` maps to the ACP
/// provider protocol, `rpc` to the pi-rpc provider protocol. A `process`
/// protocol has no protocol provider — its face is the harness's own command
/// surface — and is refused. A profile without connection facts is refused
/// (profile validation already refuses acp/rpc without `[sessions.connect]`;
/// this guard keeps the derivation honest independently of that law).
pub fn derive_provider(
    profile: &HarnessProfile,
    id: impl Into<String>,
    label: impl Into<String>,
) -> Result<EncounterProvider> {
    let sessions = profile.sessions.as_ref().ok_or_else(|| {
        AikitError::new(
            "encounter.profile_sessions_missing",
            format!(
                "the {} harness profile declares no sessions layer, so there are no \
                 connection facts to derive a provider from",
                profile.slug
            ),
        )
    })?;
    let protocol = match sessions.protocol {
        SessionProtocol::Acp => EncounterProtocol::Acp,
        SessionProtocol::Rpc => EncounterProtocol::PiRpc,
        SessionProtocol::Process => {
            return Err(AikitError::new(
                "encounter.profile_protocol_unsupported",
                format!(
                    "the {} harness profile declares the process protocol: process protocol \
                     has no protocol provider; its face is the harness's own command surface",
                    profile.slug
                ),
            ));
        }
    };
    let connect = sessions.connect.as_ref().ok_or_else(|| {
        AikitError::new(
            "encounter.profile_connect_missing",
            format!(
                "the {} harness profile declares the {:?} protocol but carries no \
                     [sessions.connect] connection facts; derive needs the exact argv that \
                     puts the harness into its protocol mode",
                profile.slug, sessions.protocol
            ),
        )
    })?;
    // The profile's env/cwd cannot reach the child today (scrubbed
    // credential-only launch environment; the encounter open supplies the
    // project-bound working directory). Refuse rather than derive a provider
    // that would silently drop them.
    ensure_connect_facts_reachable(&connect.env, &connect.cwd)?;
    Ok(EncounterProvider {
        protocol,
        id: id.into(),
        label: label.into(),
        argv: connect.argv.clone(),
        argv_fallback: connect.argv_fallback.clone(),
        // Non-empty env / declared cwd are refused above; the derived
        // provider carries no connection environment of its own.
        env: BTreeMap::new(),
        cwd: None,
        from_profile: Some(profile.slug.clone()),
        required_context: None,
        model_policy: None,
        body_ref: None,
        body_revision: None,
        body_faculties: None,
    })
}

/// Resolve a configured provider's connection facts at provider-load time.
///
/// A provider naming `from_profile` takes its whole connection face
/// (protocol, argv, fallback variants) from the embedded profile — the
/// stored `protocol` field is superseded, never merged. A `from_profile`
/// provider that also carries an explicit argv is a contradiction and is
/// refused naming both. Any other provider passes through unchanged.
pub fn resolve_provider(provider: EncounterProvider) -> Result<EncounterProvider> {
    let Some(slug) = provider.from_profile.clone() else {
        return Ok(provider);
    };
    if !provider.argv.is_empty() {
        return Err(AikitError::new(
            "encounter.from_profile_argv_conflict",
            format!(
                "provider {} declares both from_profile {slug} and an explicit argv; a \
                 profile-derived provider takes its connection facts from the profile — \
                 remove the argv or the from_profile slug",
                provider.id
            ),
        )
        .with("from_profile", slug)
        .with("provider", provider.id.clone()));
    }
    let profile = profiles::for_slug(&slug).ok_or_else(|| {
        AikitError::new(
            "encounter.from_profile_unknown",
            format!(
                "provider {} names from_profile {slug}, which is no embedded harness \
                 profile; the provider cannot resolve its connection facts",
                provider.id
            ),
        )
        .with("from_profile", slug.clone())
    })?;
    let mut derived = derive_provider(profile, provider.id.clone(), provider.label.clone())?;
    // Owner additions pin material and policy; they are not connection facts
    // and survive the resolution untouched.
    derived.required_context = provider.required_context;
    derived.model_policy = provider.model_policy;
    Ok(derived)
}

/// Refuse declared connection environment or working directory facts that
/// cannot reach the provider child.
///
/// Today the launch contract cannot carry either:
///
/// * the final provider child runs under the scrubbed final-child
///   environment, whose non-allowlisted entries are credential deliveries
///   bound through the models layer's declared key delivery
///   (`aikit-core::credential` shape law); a profile-declared literal env
///   var has no route into that contract, and the selected-model and
///   task-bound launchers re-materialise their own environment at their
///   final exec, where a provider-carried env would be silently dropped;
/// * the encounter open supplies the working directory — the open request's
///   `cwd` is validated against the space's project binding, recorded in the
///   binding and compared at reconnect. A provider-declared cwd would either
///   break that law or be ignored.
///
/// Either fact is therefore refused where it is declared, naming the fields,
/// instead of being accepted and dropped.
pub fn ensure_connection_facts_reachable(provider: &EncounterProvider) -> Result<()> {
    ensure_connect_facts_reachable(&provider.env, &provider.cwd)
}

fn ensure_connect_facts_reachable(
    env: &BTreeMap<String, String>,
    cwd: &Option<String>,
) -> Result<()> {
    match (env.is_empty(), cwd.is_none()) {
        (true, true) => Ok(()),
        (false, true) => Err(AikitError::new(
            "encounter.connect_facts_unreachable",
            format!(
                "declared connect.env cannot reach the child yet: the provider child runs \
                 under the scrubbed credential-only launch environment (models-layer key \
                 delivery), so the declared variables {} would be silently dropped — \
                 bind the credential through the models layer instead",
                env.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
        )),
        (true, false) => Err(AikitError::new(
            "encounter.connect_facts_unreachable",
            format!(
                "declared connect.cwd ({}) cannot reach the child yet: the encounter open \
                 supplies the project-bound working directory, recorded in the binding and \
                 compared at reconnect — a provider-declared cwd would contradict that law",
                cwd.as_deref().unwrap_or_default()
            ),
        )),
        (false, false) => Err(AikitError::new(
            "encounter.connect_facts_unreachable",
            format!(
                "declared connect.env ({}) and connect.cwd ({}) cannot reach the child yet: \
                 the provider child runs under the scrubbed credential-only launch \
                 environment, and the encounter open supplies the project-bound working \
                 directory — neither declared fact would be honored",
                env.keys().cloned().collect::<Vec<_>>().join(", "),
                cwd.as_deref().unwrap_or_default()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedded(slug: &str) -> &'static HarnessProfile {
        profiles::for_slug(slug).unwrap_or_else(|| panic!("{slug} must resolve"))
    }

    // -- parity with the working hand-written providers ---------------------
    //
    // The parity references are the two hand-written providers that work
    // today (`~/.aikit/state/encounter-providers/gemini-acp.json` and
    // `pi.json`). These tests never read machine files: the expected argv is
    // stated inline from the embedded profiles and cross-checked by hand
    // against those files.

    #[test]
    fn gemini_derives_the_acp_face_with_the_declared_fallback_variant() {
        let provider = derive_provider(embedded("gemini"), "gemini-acp", "Gemini ACP").unwrap();

        assert_eq!(provider.protocol, EncounterProtocol::Acp);
        assert_eq!(provider.argv, ["gemini", "--acp"]);
        assert_eq!(
            provider.argv_fallback,
            [["gemini", "--experimental-acp"]],
            "the hand-written working provider at \
             ~/.aikit/state/encounter-providers/gemini-acp.json carries the fallback \
             variant as its primary because the installed gemini is the 0.29.x line; the \
             profile declares the 0.30+ `--acp` primary with the renamed flag as the \
             declared fallback the open path tries when the primary fails pre-initialize"
        );
        assert_eq!(provider.from_profile.as_deref(), Some("gemini"));
        assert!(provider.env.is_empty() && provider.cwd.is_none());
        assert!(provider.required_context.is_none() && provider.model_policy.is_none());
    }

    #[test]
    fn pi_derives_the_rpc_face_matching_the_hand_written_argv_prefix() {
        let provider = derive_provider(embedded("pi"), "pi", "Pi").unwrap();

        assert_eq!(provider.protocol, EncounterProtocol::PiRpc);
        assert_eq!(
            provider.argv,
            ["pi", "--mode", "rpc"],
            "a prefix of the hand-written pi.json argv: the owner overlays the \
             `-e <mcp-bridge>` extension on top of exactly these facts; the test states \
             the prefix inline and never reads the machine file"
        );
        assert!(provider.argv_fallback.is_empty());
    }

    #[test]
    fn hermes_and_kimi_derive_their_first_party_acp_faces() {
        // The hermes CLI profile is process-protocol; its first-party ACP
        // bridge is the hermes-acp profile, whose connect names `hermes acp`.
        let hermes = derive_provider(embedded("hermes-acp"), "hermes", "Hermes").unwrap();
        assert_eq!(hermes.protocol, EncounterProtocol::Acp);
        assert_eq!(hermes.argv, ["hermes", "acp"]);

        let kimi = derive_provider(embedded("kimi"), "kimi", "Kimi").unwrap();
        assert_eq!(kimi.protocol, EncounterProtocol::Acp);
        assert_eq!(kimi.argv, ["kimi", "acp"]);
    }

    // -- refusals -----------------------------------------------------------

    #[test]
    fn a_process_protocol_profile_has_no_protocol_provider_and_is_refused() {
        let profile: HarnessProfile = toml::from_str(
            r#"
schema = "aikit.harness-profile/v1"
slug = "process-faced"
edition = "cli"

[sessions]
posture = "observed"
protocol = "process"
"#,
        )
        .unwrap();
        let error = derive_provider(&profile, "process-faced", "Process").unwrap_err();

        assert_eq!(error.code(), "encounter.profile_protocol_unsupported");
        let message = error.to_string();
        assert!(
            message.contains("process protocol has no protocol provider"),
            "the refusal states the standing: {message}"
        );
        assert!(
            message.contains("harness's own command surface"),
            "the refusal names the honest face: {message}"
        );
    }

    #[test]
    fn a_profile_without_sessions_layer_is_refused() {
        let profile: HarnessProfile = toml::from_str(
            "schema = \"aikit.harness-profile/v1\"\nslug = \"bare\"\nedition = \"cli\"\n",
        )
        .unwrap();
        let error = derive_provider(&profile, "bare", "Bare").unwrap_err();

        assert_eq!(error.code(), "encounter.profile_sessions_missing");
    }

    #[test]
    fn declared_connect_env_refuses_because_it_cannot_reach_the_child() {
        let profile: HarnessProfile = toml::from_str(
            r#"
schema = "aikit.harness-profile/v1"
slug = "envy"
edition = "cli"

[sessions]
posture = "observed"
protocol = "acp"

[sessions.connect]
argv = ["envy", "--acp"]

[sessions.connect.env]
ENVY_BASE_URL = "https://api.example"
"#,
        )
        .unwrap();
        let error = derive_provider(&profile, "envy", "Envy").unwrap_err();

        assert_eq!(error.code(), "encounter.connect_facts_unreachable");
        let message = error.to_string();
        assert!(
            message.contains("cannot reach the child yet") && message.contains("ENVY_BASE_URL"),
            "the refusal names the unreachable fact instead of accepting and dropping it: \
             {message}"
        );
    }

    #[test]
    fn declared_connect_cwd_refuses_because_the_open_supplies_the_working_directory() {
        let profile: HarnessProfile = toml::from_str(
            r#"
schema = "aikit.harness-profile/v1"
slug = "rooted"
edition = "cli"

[sessions]
posture = "observed"
protocol = "rpc"

[sessions.connect]
argv = ["rooted", "--mode", "rpc"]
cwd = "/opt/rooted"
"#,
        )
        .unwrap();
        let error = derive_provider(&profile, "rooted", "Rooted").unwrap_err();

        assert_eq!(error.code(), "encounter.connect_facts_unreachable");
        assert!(
            error.to_string().contains("/opt/rooted"),
            "the refusal names the declared directory: {error}"
        );
    }

    // -- load-time resolution ----------------------------------------------

    #[test]
    fn a_from_profile_provider_resolves_through_the_profile_at_load_time() {
        let configured = EncounterProvider {
            id: "gemini-acp".into(),
            label: "Gemini via profile".into(),
            from_profile: Some("gemini".into()),
            protocol: EncounterProtocol::Acp,
            argv: Vec::new(),
            argv_fallback: Vec::new(),
            env: Default::default(),
            cwd: None,
            required_context: None,
            model_policy: None,
            body_ref: None,
            body_revision: None,
            body_faculties: None,
        };

        let resolved = resolve_provider(configured).unwrap();

        assert_eq!(resolved.argv, ["gemini", "--acp"]);
        assert_eq!(resolved.argv_fallback, [["gemini", "--experimental-acp"]]);
        assert_eq!(resolved.from_profile.as_deref(), Some("gemini"));
    }

    #[test]
    fn a_from_profile_provider_with_an_explicit_argv_is_a_contradiction() {
        let configured = EncounterProvider {
            id: "gemini-acp".into(),
            label: "Gemini".into(),
            from_profile: Some("gemini".into()),
            protocol: EncounterProtocol::Acp,
            argv: vec![
                "/opt/homebrew/bin/gemini".into(),
                "--experimental-acp".into(),
            ],
            argv_fallback: Vec::new(),
            env: Default::default(),
            cwd: None,
            required_context: None,
            model_policy: None,
            body_ref: None,
            body_revision: None,
            body_faculties: None,
        };

        let error = resolve_provider(configured).unwrap_err();

        assert_eq!(error.code(), "encounter.from_profile_argv_conflict");
        let message = error.to_string();
        assert!(
            message.contains("from_profile gemini") && message.contains("explicit argv"),
            "the refusal names both sides of the contradiction: {message}"
        );
    }

    #[test]
    fn an_unknown_from_profile_slug_is_refused_at_resolution() {
        let configured = EncounterProvider {
            id: "ghost".into(),
            label: "Ghost".into(),
            from_profile: Some("ghost".into()),
            protocol: EncounterProtocol::Acp,
            argv: Vec::new(),
            argv_fallback: Vec::new(),
            env: Default::default(),
            cwd: None,
            required_context: None,
            model_policy: None,
            body_ref: None,
            body_revision: None,
            body_faculties: None,
        };

        let error = resolve_provider(configured).unwrap_err();

        assert_eq!(error.code(), "encounter.from_profile_unknown");
    }

    #[test]
    fn a_provider_without_from_profile_passes_through_resolution_unchanged() {
        let configured = EncounterProvider {
            id: "pi".into(),
            label: "Pi".into(),
            from_profile: None,
            protocol: EncounterProtocol::PiRpc,
            argv: vec!["pi".into(), "--mode".into(), "rpc".into()],
            argv_fallback: Vec::new(),
            env: Default::default(),
            cwd: None,
            required_context: None,
            model_policy: None,
            body_ref: None,
            body_revision: None,
            body_faculties: None,
        };
        let snapshot = configured.clone();

        let resolved = resolve_provider(configured).unwrap();
        assert_eq!(resolved.id, snapshot.id);
        assert_eq!(resolved.protocol, snapshot.protocol);
        assert_eq!(resolved.argv, snapshot.argv);
        assert_eq!(resolved.argv_fallback, snapshot.argv_fallback);
        assert_eq!(resolved.from_profile, snapshot.from_profile);
        assert_eq!(resolved.required_context, snapshot.required_context);
        assert_eq!(resolved.model_policy, snapshot.model_policy);
    }
}
