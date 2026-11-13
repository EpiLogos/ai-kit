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
use aikit_adapters::runner::{CommandRunner, Output};
use aikit_core::credential::CredentialRef;
use aikit_core::harness_profile::{HarnessProfile, ModelLoginStatus, ModelOwnLogin};
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
    /// The harness's declared native logged-in check, as the argv that asks
    /// it — named here, run only by `--check`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Vec<String>>,
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

/// One native logged-in check's answer (`aikit harness auth <harness>
/// --check`): the provider, the declared argv that asked, and whether the
/// harness says it is signed in; `signed_in` is absent when no answer came,
/// with why in `unread`. Only the declared answer is read: nothing else the
/// harness prints (an account, an email, a masked key) is carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginCheck {
    pub provider_ref: String,
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_in: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unread: Option<String>,
}

/// The harness's native logged-in checks, one per own-login fact that
/// declares one. Empty when the harness has none: whether a key is present
/// then stays with the credential store's leg.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginChecks {
    pub slug: String,
    pub checks: Vec<LoginCheck>,
}

/// Run each declared native logged-in check of the harness through the
/// runner (a bounded probe runner from the CLI) and read its declared answer.
pub fn check_logins(runner: &dyn CommandRunner, harness: &str) -> Result<LoginChecks> {
    let profile = profile_for(harness)?;
    let checks = profile
        .models
        .as_ref()
        .and_then(|models| models.key_delivery.as_ref())
        .map(|delivery| {
            delivery
                .own_login
                .iter()
                .filter_map(|fact| check_login(runner, fact))
                .collect()
        })
        .unwrap_or_default();
    Ok(LoginChecks {
        slug: profile.slug.clone(),
        checks,
    })
}

/// Run one own-login fact's declared native check and read its declared
/// answer; `None` when the fact declares no check. The one reading both
/// `harness auth --check` and `client status` use.
pub(crate) fn check_login(runner: &dyn CommandRunner, fact: &ModelOwnLogin) -> Option<LoginCheck> {
    let status = fact.status.as_ref()?;
    let (signed_in, unread) = match runner.run(&status.argv) {
        Ok(output) => match signed_in_by(status, &output) {
            Ok(answer) => (Some(answer), None),
            Err(why) => (None, Some(why)),
        },
        Err(error) => (
            None,
            Some(format!(
                "{} did not answer: {}",
                status.argv.join(" "),
                error.message()
            )),
        ),
    };
    Some(LoginCheck {
        provider_ref: fact.provider_ref.clone(),
        argv: status.argv.clone(),
        signed_in,
        unread,
    })
}

/// Whether the harness's answer says signed in, read by the declared answer
/// form alone: `says`, exit 0 and the exact line on one stream with the other
/// empty (anything else is not signed in); `json-true`, the field of the JSON
/// object on stdout, true or false — an answer that carries no such boolean is
/// no answer, said by why.
fn signed_in_by(status: &ModelLoginStatus, output: &Output) -> std::result::Result<bool, String> {
    if let Some(says) = status.says.as_deref() {
        let (stdout, stderr) = (output.stdout.trim(), output.stderr.trim());
        return Ok(output.status == 0
            && ((stdout == says && stderr.is_empty()) || (stderr == says && stdout.is_empty())));
    }
    let field = status.json_true.as_deref().unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(output.stdout.trim())
        .ok()
        .and_then(|answer| answer.get(field).and_then(serde_json::Value::as_bool))
        .ok_or_else(|| {
            format!(
                "{} answered no `{field}` that is true or false",
                status.argv.join(" ")
            )
        })
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
                status: fact.status.as_ref().map(|status| status.argv.clone()),
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
    // The declared native check: `codex login status` and its one answer.
    let Some(status) = declared.and_then(|fact| fact.status.as_ref()) else {
        return Ok(false);
    };
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
    // The declared check on the resolved binary: the program this launch
    // runs, then the harness's own words. Codex 0.155.1 reports its status on
    // stderr; the declared answer is accepted from one stream only, so
    // warnings or another login mode are not affirmative subscription
    // evidence.
    let argv: Vec<String> = std::iter::once(harness_program.to_owned())
        .chain(status.argv.iter().skip(1).cloned())
        .collect();
    let output = runner.run(&argv)?;
    Ok(signed_in_by(status, &output).unwrap_or(false))
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
    fn describe_names_each_declared_native_check_without_running_it() {
        let codex = auth_face("codex").unwrap();
        assert_eq!(
            codex.own_login[0].status.as_deref(),
            Some(
                [
                    "codex".to_string(),
                    "login".to_string(),
                    "status".to_string()
                ]
                .as_slice()
            )
        );
        let json = serde_json::to_value(&codex).unwrap();
        assert_eq!(json["own_login"][0]["status"][2], "status");
        let claude = auth_face("claude").unwrap();
        assert_eq!(
            claude.own_login[0]
                .status
                .as_deref()
                .map(|argv| argv.join(" ")),
            Some("claude auth status --json".to_string())
        );
        let opencode = auth_face("opencode").unwrap();
        assert!(opencode
            .own_login
            .iter()
            .all(|login| login.status.is_none()));
        assert!(
            serde_json::to_value(&opencode).unwrap()["own_login"][0]
                .get("status")
                .is_none(),
            "no check declared, none rendered"
        );
    }

    /// One scripted answer per argv, the way the real binary answers.
    struct Answers(
        Vec<(&'static str, Result<aikit_adapters::runner::Output>)>,
        std::sync::Mutex<Vec<String>>,
    );
    impl Answers {
        fn new(answers: Vec<(&'static str, Result<aikit_adapters::runner::Output>)>) -> Self {
            Self(answers, std::sync::Mutex::new(Vec::new()))
        }
        fn asked(&self) -> Vec<String> {
            self.1.lock().unwrap().clone()
        }
    }
    impl CommandRunner for Answers {
        fn run(&self, argv: &[String]) -> Result<aikit_adapters::runner::Output> {
            let line = argv.join(" ");
            self.1.lock().unwrap().push(line.clone());
            self.0
                .iter()
                .find(|(asked, _)| *asked == line)
                .map(|(_, answer)| answer.clone())
                .unwrap_or_else(|| Err(AikitError::new("test.unscripted", line)))
        }
    }
    fn said(status: i32, stdout: &str, stderr: &str) -> Result<aikit_adapters::runner::Output> {
        Ok(aikit_adapters::runner::Output {
            status,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        })
    }

    #[test]
    fn codex_is_signed_in_only_on_its_exact_answer() {
        let check = |answer| {
            let runner = Answers::new(vec![("codex login status", answer)]);
            let checks = check_logins(&runner, "codex").unwrap();
            assert_eq!(runner.asked(), vec!["codex login status".to_string()]);
            assert_eq!(checks.slug, "codex");
            assert_eq!(checks.checks.len(), 1);
            assert_eq!(checks.checks[0].provider_ref, "provider:openai");
            assert_eq!(checks.checks[0].argv.join(" "), "codex login status");
            checks.checks[0].clone()
        };
        // codex-cli 0.155.1 answers on stderr.
        assert_eq!(
            check(said(0, "", "Logged in using ChatGPT\n")).signed_in,
            Some(true)
        );
        assert_eq!(
            check(said(0, "Logged in using ChatGPT\n", "")).signed_in,
            Some(true)
        );
        assert_eq!(check(said(1, "", "Not logged in")).signed_in, Some(false));
        // Another login mode is not the declared answer, and its words (a
        // masked key) are not carried.
        let api_key = check(said(0, "", "Logged in using an API key - sk-proj-***abcd"));
        assert_eq!(api_key.signed_in, Some(false));
        assert!(!serde_json::to_string(&api_key).unwrap().contains("sk-proj"));
        // A warning beside the answer is not the answer.
        assert_eq!(
            check(said(0, "Logged in using ChatGPT", "WARNING: x")).signed_in,
            Some(false)
        );
        // No answer at all is unread, never "not signed in".
        let unread = check(Err(AikitError::new(
            "mux.command_timeout",
            "codex took too long",
        )));
        assert_eq!(unread.signed_in, None);
        assert!(
            unread
                .unread
                .as_deref()
                .unwrap()
                .contains("codex took too long"),
            "{unread:?}"
        );
    }

    #[test]
    fn claude_is_read_by_its_json_logged_in_field_alone() {
        let check = |answer| {
            let runner = Answers::new(vec![("claude auth status --json", answer)]);
            let checks = check_logins(&runner, "claude").unwrap();
            assert_eq!(checks.slug, "claude-code");
            assert_eq!(checks.checks.len(), 1);
            assert_eq!(checks.checks[0].provider_ref, "provider:anthropic");
            checks.checks[0].clone()
        };
        let signed_in = check(said(
            0,
            r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "person@example.com"}"#,
            "",
        ));
        assert_eq!(signed_in.signed_in, Some(true));
        assert!(
            !serde_json::to_string(&signed_in)
                .unwrap()
                .contains("example.com"),
            "nothing but the declared field is carried"
        );
        assert_eq!(
            check(said(1, r#"{"loggedIn": false}"#, "")).signed_in,
            Some(false)
        );
        let garbled = check(said(0, "Not JSON", ""));
        assert_eq!(garbled.signed_in, None);
        assert!(
            garbled.unread.as_deref().unwrap().contains("loggedIn"),
            "{garbled:?}"
        );
        assert_eq!(check(said(0, r#"{"loggedIn": "yes"}"#, "")).signed_in, None);
    }

    #[test]
    fn a_harness_without_a_native_check_runs_nothing() {
        let runner = Answers::new(Vec::new());
        let checks = check_logins(&runner, "opencode").unwrap();
        assert_eq!(checks.slug, "opencode");
        assert!(checks.checks.is_empty());
        assert!(
            runner.asked().is_empty(),
            "nothing is spawned for a harness with no check"
        );
        let json = serde_json::to_value(&checks).unwrap();
        assert_eq!(json["checks"], serde_json::json!([]));
        assert_eq!(
            check_logins(&runner, "definitely-not-a-harness")
                .unwrap_err()
                .code(),
            "harness_auth.unknown_harness"
        );
    }

    #[test]
    fn the_codex_subscription_gate_reads_the_declared_check_on_the_resolved_binary() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path().join("aikit"));
        let runner = Answers::new(vec![(
            "/opt/tools/codex login status",
            said(0, "", "Logged in using ChatGPT"),
        )]);
        assert!(
            codex_chatgpt_login_ready(&runner, &home, "/opt/tools/codex", "provider:openai")
                .unwrap()
        );
        assert_eq!(
            runner.asked(),
            vec!["/opt/tools/codex login status".to_string()]
        );
        let not = Answers::new(vec![(
            "/opt/tools/codex login status",
            said(1, "", "Not logged in"),
        )]);
        assert!(
            !codex_chatgpt_login_ready(&not, &home, "/opt/tools/codex", "provider:openai").unwrap()
        );
    }

    #[test]
    fn describe_reports_claude_runnable_login_and_its_check_without_executing() {
        // DLW build ruling 2026-11-04 (O-I #758 Sign in): Claude Code signs in
        // like Codex — its one-shot `claude auth login`, read back by
        // `claude auth status --json`. Describe runs neither.
        let face = auth_face("claude").unwrap();
        assert_eq!(face.slug, "claude-code");
        assert_eq!(face.env_var.len(), 1, "ANTHROPIC_API_KEY is declared");
        assert_eq!(face.own_login.len(), 1);
        let own_login = &face.own_login[0];
        assert!(own_login.runnable, "claude auth login is one-shot");
        assert_eq!(
            own_login.argv.as_deref().map(|argv| argv.join(" ")),
            Some("claude auth login".to_string())
        );
        assert!(
            !own_login.note.contains("in-TUI"),
            "the note says what the login does: {}",
            own_login.note
        );
        assert_eq!(
            plan_login("claude").unwrap().argv,
            vec![
                "claude".to_string(),
                "auth".to_string(),
                "login".to_string()
            ]
        );
    }

    #[test]
    fn run_mode_refuses_a_note_only_harness_printing_the_note_as_the_instruction() {
        let error = plan_login("gemini").unwrap_err();
        assert_eq!(error.code(), "harness_auth.no_login_declared");
        let message = error.message();
        assert!(
            message.starts_with("no declared one-shot login for gemini; "),
            "the refusal carries the contract phrasing: {message}"
        );
        assert!(
            message.contains("Login with Google"),
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
        #[cfg(unix)]
        {
            // The selected command may be a symlink named `codex` whose
            // executable target has another basename (npm global installs).
            // This is the actual native binary and login status, not a fake
            // response from a scripted runner.
            use std::os::unix::fs::symlink;
            let renamed = temp.path().join("renamed-native-codex");
            std::fs::hard_link(std::fs::canonicalize(&codex).unwrap(), &renamed).unwrap();
            let linked = temp.path().join("codex");
            symlink(&renamed, &linked).unwrap();
            assert!(codex_chatgpt_login_ready(
                &SystemRunner::probe(),
                &home,
                linked.to_str().unwrap(),
                "provider:openai",
            )
            .unwrap());
        }
    }
}
