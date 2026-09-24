//! `aikit harness auth` — the login face over the models layer's key-delivery
//! declarations.
//!
//! Every harness profile declares, per provider it can serve, how a key
//! reaches it: the env var its native launch reads, and/or the own-login fact
//! that the harness authenticates through a store of its own. The 2026-09-23
//! login-commands pass made the own-login fact runnable where the harness's
//! own command surface verifies a one-shot login command (schema:
//! `ModelLoginCommand`). This module is the face over those declarations:
//!
//! * describe (`--json`): the harness's auth options verbatim — env-var
//!   names, own-login entries with their declared login argv (or its absence
//!   plus the note) — without executing anything. This is what a settings
//!   face renders next to the API-key input.
//! * default: run the declared login argv in this terminal. The child
//!   inherits stdio and the caller's environment unchanged: this is the
//!   user's own terminal login, not a connection launch, so nothing is
//!   scrubbed and no [`aikit_adapters::connection_process::ModelEnvironment`]
//!   is built. Where the harness declares no one-shot login, the refusal
//!   prints the own-login note as the instruction instead of inventing a
//!   command.
//!
//! Terminal ownership follows the one foreground contract this crate already
//! has for interactive harness faces ([`crate::route_launch::run_plan`]): the
//! child is spawned in this process group with inherited stdio, so the
//! terminal's Ctrl-C reaches the login process exactly as it reaches any
//! foreground command. AIKit deliberately does not hand the child its own
//! process group: a detached group that is not made the terminal's foreground
//! group would stop the child on its first terminal read (SIGTTIN), and
//! making it foreground safely requires the SIGTTOU-blocked `tcsetpgrp`
//! discipline this `#![forbid(unsafe_code)]` crate cannot express.

use std::process::Command;

use aikit_adapters::profiles;
use aikit_adapters::runner::CommandRunner;
use aikit_core::credential::CredentialRef;
use aikit_core::harness_profile::HarnessProfile;
use aikit_core::{AikitError, Result};
use aikit_store::{AikitHome, CredentialBindingStore};
use serde::{Deserialize, Serialize};

/// One declared env-var delivery, by name: the provider whose key the
/// variable delivers. A name, never material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVarOption {
    pub provider_ref: String,
    pub env_var: String,
}

/// One own-login entry: whether the profile declares a runnable one-shot
/// login (with the declared argv verbatim — a public command, not a secret)
/// or the entry is note-only. The census note rides either way; it is the
/// instruction to render beside the option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnLoginOption {
    pub provider_ref: String,
    pub runnable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argv: Option<Vec<String>>,
    pub note: String,
}

/// The harness's auth options as a settings face renders them: env-var names
/// and own-login entries beside the API-key input, plus the layer's overall
/// key-posture note where one is declared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthFace {
    pub slug: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_var: Vec<EnvVarOption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub own_login: Vec<OwnLoginOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A composed login: everything decided, nothing spawned yet. Mirrors the
/// plan/run split of the route launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginPlan {
    pub slug: String,
    pub argv: Vec<String>,
}

/// Resolve the harness — a registry name, registered alias, catalog slug or
/// embedded profile slug — to its embedded profile.
fn profile_for(harness: &str) -> Result<&'static HarnessProfile> {
    let slug = crate::client::catalog_slug_for(harness)
        .or_else(|| profiles::slug_for_target(&aikit_core::TargetId::new(harness)))
        .unwrap_or(harness);
    profiles::for_slug(slug).ok_or_else(|| {
        AikitError::new(
            "harness_auth.unknown_harness",
            format!(
                "`{harness}` is not a harness AIKit carries a profile for; `aikit client \
                 status` shows the registered surface, and this verb accepts any embedded \
                 profile slug (codex, claude-code, opencode, ...)"
            ),
        )
        .with("harness", harness.to_string())
    })
}

/// The harness's declared auth options, read from its profile. Nothing is
/// executed and no credential is read: presence stays with the credential
/// store, exactly as in the disclosure read model.
pub fn auth_face(harness: &str) -> Result<AuthFace> {
    let profile = profile_for(harness)?;
    let Some(delivery) = profile
        .models
        .as_ref()
        .and_then(|models| models.key_delivery.as_ref())
    else {
        return Ok(AuthFace {
            slug: profile.slug.clone(),
            env_var: Vec::new(),
            own_login: Vec::new(),
            note: None,
        });
    };
    Ok(AuthFace {
        slug: profile.slug.clone(),
        env_var: delivery
            .env_var
            .iter()
            .map(|entry| EnvVarOption {
                provider_ref: entry.provider_ref.clone(),
                env_var: entry.env_var.clone(),
            })
            .collect(),
        own_login: delivery
            .own_login
            .iter()
            .map(|fact| OwnLoginOption {
                provider_ref: fact.provider_ref.clone(),
                runnable: fact.login.is_some(),
                argv: fact.login.as_ref().map(|login| login.argv.clone()),
                note: fact.note.clone(),
            })
            .collect(),
        note: delivery.note.clone(),
    })
}

/// A narrowly evidenced alternative to an API binding for Codex's own
/// OpenAI route. This does not turn a ChatGPT login into an API credential or
/// change the catalogue's global route availability. Only a declared Codex
/// login, an absent API binding, and a successful native login-status probe
/// permit the caller to use the harness's own store. Revocation and expiry
/// always refuse, even if Codex remains logged in separately.
pub(crate) fn codex_chatgpt_login_ready(
    runner: &dyn CommandRunner,
    home: &AikitHome,
    harness_program: &str,
    provider_ref: &str,
) -> Result<bool> {
    if provider_ref != "provider:openai" {
        return Ok(false);
    }
    let Some(profile) = profiles::for_argv_program(harness_program) else {
        return Ok(false);
    };
    if profile.slug != "codex" {
        return Ok(false);
    }
    let declared = profile
        .models
        .as_ref()
        .and_then(|models| models.key_delivery.as_ref())
        .and_then(|delivery| {
            delivery.own_login.iter().find(|fact| {
                fact.provider_ref == provider_ref
                    && fact.login.as_ref().is_some_and(|login| {
                        login.argv.len() == 2
                            && login.argv[0] == "codex"
                            && login.argv[1] == "login"
                    })
            })
        });
    if declared.is_none() {
        return Ok(false);
    }
    let credential_ref = CredentialRef::new("credential:openai")?;
    if let Some(binding) = CredentialBindingStore::new(home).load(&credential_ref)? {
        if binding.revoked
            || binding
                .expires_at
                .as_deref()
                .map(|value| {
                    value.parse::<jiff::Timestamp>().map_err(|error| {
                        AikitError::new("harness_auth.binding_expiry_invalid", error.to_string())
                    })
                })
                .transpose()?
                .is_some_and(|deadline| deadline <= jiff::Timestamp::now())
        {
            return Err(AikitError::new(
                "harness_auth.binding_revoked_or_expired",
                "credential:openai is revoked or expired; Codex own-login does not bypass that refusal",
            ));
        }
        return Ok(false);
    }
    let argv = [
        harness_program.to_owned(),
        "login".to_owned(),
        "status".to_owned(),
    ];
    let output = runner.run(&argv)?;
    let stdout = output.stdout.trim();
    let stderr = output.stderr.trim();
    // Codex 0.155.1 reports this status on stderr. Accept the exact native
    // answer from one stream only; warnings or an unknown login mode are not
    // affirmative subscription evidence.
    Ok(output.status == 0
        && ((stdout == "Logged in using ChatGPT" && stderr.is_empty())
            || (stderr == "Logged in using ChatGPT" && stdout.is_empty())))
}

/// Compose the login plan for one harness: the one runnable own-login entry
/// its profile declares. A note-only harness refuses here, with its own note
/// as the instruction; several runnable entries refuse rather than let the
/// launcher guess.
pub fn plan_login(harness: &str) -> Result<LoginPlan> {
    let profile = profile_for(harness)?;
    let slug = profile.slug.as_str();
    let runnable: Vec<_> = profile
        .models
        .as_ref()
        .and_then(|models| models.key_delivery.as_ref())
        .map(|delivery| {
            delivery
                .own_login
                .iter()
                .filter(|fact| fact.login.is_some())
                .collect()
        })
        .unwrap_or_default();
    match runnable.as_slice() {
        [fact] => Ok(LoginPlan {
            slug: slug.to_string(),
            argv: fact.login.as_ref().expect("filtered runnable").argv.clone(),
        }),
        [] => {
            let note = profile
                .models
                .as_ref()
                .and_then(|models| models.key_delivery.as_ref())
                .and_then(|delivery| {
                    delivery
                        .own_login
                        .first()
                        .map(|fact| fact.note.as_str())
                        .or(delivery.note.as_deref())
                })
                .unwrap_or("the profile declares no key-delivery facts at all");
            Err(AikitError::new(
                "harness_auth.no_login_declared",
                format!("no declared one-shot login for {slug}; {note}"),
            )
            .with("harness", slug.to_string()))
        }
        _ => {
            let binaries: Vec<&str> = runnable
                .iter()
                .filter_map(|fact| fact.login.as_ref())
                .filter_map(|login| login.argv.first().map(String::as_str))
                .collect();
            Err(AikitError::new(
                "harness_auth.ambiguous_login",
                format!(
                    "several one-shot logins are declared for {slug} ({}); AIKit does not \
                     guess — run one directly",
                    binaries.join(", ")
                ),
            )
            .with("harness", slug.to_string()))
        }
    }
}

/// Run the declared login argv in this terminal: stdio inherited, the
/// caller's environment unchanged. Returns the child's exit code.
pub fn run_login_argv(argv: &[String]) -> Result<i32> {
    let (program, args) = argv.split_first().ok_or_else(|| {
        AikitError::new(
            "harness_auth.empty_login_argv",
            "the declared login argv is empty",
        )
    })?;
    let mut command = Command::new(program);
    command.args(args);
    let status = command.status().map_err(|error| {
        AikitError::new(
            "harness_auth.spawn_failed",
            format!(
                "could not launch `{program}`: {error} — is the harness installed and on PATH?"
            ),
        )
        .with("program", program.to_string())
    })?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    use aikit_adapters::credential_provider::EnvironmentImportProvider;
    use aikit_adapters::runner::SystemRunner;
    use aikit_core::credential::SecretProvider as _;

    #[test]
    fn describe_reports_codex_runnable_login_and_env_var_names_without_executing() {
        let face = auth_face("codex").unwrap();
        assert_eq!(face.slug, "codex");
        assert_eq!(
            face.env_var,
            vec![EnvVarOption {
                provider_ref: "provider:openai".to_string(),
                env_var: "OPENAI_API_KEY".to_string(),
            }]
        );
        assert_eq!(face.own_login.len(), 1);
        let own_login = &face.own_login[0];
        assert!(own_login.runnable, "codex verifies a one-shot login");
        assert_eq!(
            own_login.argv.as_deref(),
            Some(["codex".to_string(), "login".to_string()].as_slice())
        );
        assert!(own_login.note.contains("codex login"));

        // The describe document serializes for the settings face, argv included.
        let json = serde_json::to_value(&face).unwrap();
        assert_eq!(json["own_login"][0]["argv"][0], "codex");
        assert_eq!(json["env_var"][0]["env_var"], "OPENAI_API_KEY");
    }

    #[test]
    fn describe_reports_claude_note_only_with_its_note_as_the_instruction() {
        let face = auth_face("claude").unwrap();
        assert_eq!(face.slug, "claude-code");
        assert_eq!(face.env_var.len(), 1, "ANTHROPIC_API_KEY is declared");
        assert_eq!(face.own_login.len(), 1);
        let own_login = &face.own_login[0];
        assert!(
            !own_login.runnable,
            "claude's login is the in-TUI flow; no one-shot command is declared"
        );
        assert!(own_login.argv.is_none());
        assert!(
            own_login.note.contains("claude login"),
            "the note is the instruction: {}",
            own_login.note
        );
    }

    #[test]
    fn run_mode_refuses_a_note_only_harness_printing_the_note_as_the_instruction() {
        let error = plan_login("claude").unwrap_err();
        assert_eq!(error.code(), "harness_auth.no_login_declared");
        let message = error.message();
        assert!(
            message.starts_with("no declared one-shot login for claude-code; "),
            "the refusal carries the contract phrasing: {message}"
        );
        assert!(
            message.contains("claude login keeps OAuth material"),
            "the own-login note rides the refusal as the instruction: {message}"
        );
    }

    #[test]
    fn an_unknown_harness_is_refused_naming_the_registered_surface() {
        let error = auth_face("definitely-not-a-harness").unwrap_err();
        assert_eq!(error.code(), "harness_auth.unknown_harness");
        assert!(error.message().contains("aikit client status"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn run_mode_spawns_a_fake_login_executable_and_propagates_its_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let script = dir.path().join("fake-login");
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf spawned > {}\nexit 7\n", marker.display()),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let code = run_login_argv(&[script.to_string_lossy().to_string()]).unwrap();

        assert_eq!(code, 7, "the child's exit code is ours to report");
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "spawned",
            "the fake login actually ran"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_login_plan_carries_the_declared_argv_verbatim() {
        let plan = plan_login("codex").unwrap();
        assert_eq!(plan.slug, "codex");
        assert_eq!(plan.argv, vec!["codex".to_string(), "login".to_string()]);
    }

    #[test]
    #[ignore = "requires an installed Codex executable; probes its real isolated login store"]
    fn a_real_codex_login_probe_refuses_an_empty_isolated_login_store() {
        let codex = crate::probe::which("codex").expect("installed Codex executable");
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path().join("aikit"));
        let isolated_codex_home = temp.path().join("codex-home");
        std::fs::create_dir_all(&isolated_codex_home).unwrap();
        let runner =
            SystemRunner::probe().with_env("CODEX_HOME", isolated_codex_home.to_string_lossy());
        assert!(!codex_chatgpt_login_ready(
            &runner,
            &home,
            codex.to_str().unwrap(),
            "provider:openai"
        )
        .unwrap());
        assert!(!codex_chatgpt_login_ready(
            &runner,
            &home,
            codex.to_str().unwrap(),
            "provider:anthropic"
        )
        .unwrap());
    }

    #[test]
    fn revoked_api_binding_refuses_codex_login_fallback_before_a_probe() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path().join("aikit"));
        let credential_ref = CredentialRef::new("credential:openai").unwrap();
        let provider = EnvironmentImportProvider::from_value(
            credential_ref.clone(),
            "AIKIT_CODEX_REVOKED_PROBE",
            Some("diagnostic-material".into()),
        )
        .unwrap();
        let mut binding = provider.binding_state(&credential_ref).unwrap().unwrap();
        binding.revoked = true;
        CredentialBindingStore::new(&home).save(&binding).unwrap();
        let error =
            codex_chatgpt_login_ready(&SystemRunner::probe(), &home, "codex", "provider:openai")
                .unwrap_err();
        assert_eq!(error.code(), "harness_auth.binding_revoked_or_expired");
    }

    #[test]
    #[ignore = "requires an installed Codex executable with a real ChatGPT login"]
    fn installed_codex_own_login_is_verified_by_its_native_status() {
        let codex = crate::probe::which("codex").expect("installed Codex executable");
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path().join("aikit"));
        assert!(codex_chatgpt_login_ready(
            &SystemRunner::probe(),
            &home,
            codex.to_str().unwrap(),
            "provider:openai",
        )
        .unwrap());
    }
}
