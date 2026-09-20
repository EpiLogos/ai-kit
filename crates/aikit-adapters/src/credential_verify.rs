//! One explicit, operator-invoked live key check: `aikit credential verify`.
//!
//! The check answers a single question — *does this key actually work right
//! now* — with one minimal read against the credential's provider (normally a
//! GET of its models endpoint with the materialised key as the bearer
//! credential). It is never called from any other code path: no launch, no
//! resolution and no detection verifies keys automatically, because a live
//! check spends the key against a provider and belongs to the operator
//! alone.
//!
//! Mechanics mirror [`crate::provider_catalog_source`]: the one network call
//! in this repo shells out to `curl` through an injectable
//! [`CommandRunner`], so tests script responses and never touch a network.
//! The header carries the material only inside the child's argv for the one
//! check; it is never printed, logged, persisted or included in the outcome.
//!
//! A provider with no known check is an honest refusal naming that — never a
//! fake pass. The verdict records only what the status class proves: a 401/403
//! is a *definitive* refusal (the key is known bad and the check is recorded
//! as such), other 4xx codes refuse the request without proving the key bad,
//! and transport failures are unreachable. Only definitive outcomes are ever
//! stamped into the binding's lifecycle.

use aikit_core::credential::SecretValue;
use aikit_core::{AikitError, Result};

use crate::runner::CommandRunner;

/// The provider-specific auth convention of a live check. Most providers take
/// the standard bearer header; Anthropic and Google have their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialCheckAuth {
    /// `Authorization: Bearer <material>`.
    Bearer,
    /// Anthropic's `x-api-key` header plus its required version header.
    Anthropic,
    /// Google's `x-goog-api-key` header (the key never rides in the URL,
    /// where it could reach logs through the request line).
    Google,
}

/// One known provider check: the models endpoint whose GET answers "does this
/// key work" with a status code, and the auth convention the provider reads.
#[derive(Debug, Clone, Copy)]
pub struct CredentialCheck {
    pub provider: &'static str,
    pub url: &'static str,
    pub auth: CredentialCheckAuth,
}

/// The known provider checks. A credential whose provider is absent from this
/// table is refused honestly: no check is invented, and nothing is recorded.
pub const KNOWN_CHECKS: &[CredentialCheck] = &[
    CredentialCheck {
        provider: "openai",
        url: "https://api.openai.com/v1/models",
        auth: CredentialCheckAuth::Bearer,
    },
    CredentialCheck {
        provider: "anthropic",
        url: "https://api.anthropic.com/v1/models",
        auth: CredentialCheckAuth::Anthropic,
    },
    CredentialCheck {
        provider: "gemini",
        url: "https://generativelanguage.googleapis.com/v1beta/models",
        auth: CredentialCheckAuth::Google,
    },
    CredentialCheck {
        provider: "google",
        url: "https://generativelanguage.googleapis.com/v1beta/models",
        auth: CredentialCheckAuth::Google,
    },
    CredentialCheck {
        provider: "deepseek",
        url: "https://api.deepseek.com/models",
        auth: CredentialCheckAuth::Bearer,
    },
    // The models listing is public; the auth-key read is the endpoint that
    // actually answers for the key.
    CredentialCheck {
        provider: "openrouter",
        url: "https://openrouter.ai/api/v1/auth/key",
        auth: CredentialCheckAuth::Bearer,
    },
    CredentialCheck {
        provider: "moonshot",
        url: "https://api.moonshot.cn/v1/models",
        auth: CredentialCheckAuth::Bearer,
    },
    CredentialCheck {
        provider: "dashscope",
        url: "https://dashscope.aliyuncs.com/compatible-mode/v1/models",
        auth: CredentialCheckAuth::Bearer,
    },
    CredentialCheck {
        provider: "zai",
        url: "https://api.z.ai/api/paas/v4/models",
        auth: CredentialCheckAuth::Bearer,
    },
    CredentialCheck {
        provider: "zhipu",
        url: "https://open.bigmodel.cn/api/paas/v4/models",
        auth: CredentialCheckAuth::Bearer,
    },
    CredentialCheck {
        provider: "groq",
        url: "https://api.groq.com/openai/v1/models",
        auth: CredentialCheckAuth::Bearer,
    },
    CredentialCheck {
        provider: "mistral",
        url: "https://api.mistral.ai/v1/models",
        auth: CredentialCheckAuth::Bearer,
    },
    CredentialCheck {
        provider: "xai",
        url: "https://api.x.ai/v1/models",
        auth: CredentialCheckAuth::Bearer,
    },
];

/// The verdict of one live check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialVerdict {
    /// The provider answered success: the key works right now.
    Working,
    /// The provider refused the request.
    Refused,
    /// The check could not reach a verdict: transport failure or a
    /// provider-side error. The key is neither proven good nor bad.
    Unreachable,
}

/// What one live check yielded. No field can carry material: the key and the
/// Authorization header are never part of an outcome.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CredentialCheckOutcome {
    pub provider: String,
    pub verdict: CredentialVerdict,
    /// The HTTP status class (`2xx`, `4xx`, …), or `None` when no HTTP
    /// response was received at all.
    pub http_status_class: Option<String>,
    /// Whether the outcome definitively establishes key quality: a success
    /// (working) or a 401/403 refusal (known bad). Only definitive outcomes
    /// are recorded into the binding's lifecycle.
    pub definitive: bool,
}

pub fn known_check(provider: &str) -> Option<&'static CredentialCheck> {
    KNOWN_CHECKS.iter().find(|check| check.provider == provider)
}

/// Run the one live check for a provider key. The material is passed as a
/// [`SecretValue`] and consumed only here, in the single header of the single
/// child command.
pub fn check_provider_key(
    runner: &dyn CommandRunner,
    provider: &str,
    material: &SecretValue,
) -> Result<CredentialCheckOutcome> {
    let check = known_check(provider).ok_or_else(|| {
        AikitError::new(
            "credential.verify_provider_unknown",
            format!(
                "no live check is known for provider {provider:?}; AIKit refuses to fake \
                 a pass — verify is available for: {}",
                KNOWN_CHECKS
                    .iter()
                    .map(|check| check.provider)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
    })?;
    let mut argv = vec![
        "curl".to_string(),
        "-sS".to_string(),
        "--max-time".to_string(),
        "20".to_string(),
        "-o".to_string(),
        "/dev/null".to_string(),
        "-w".to_string(),
        "%{http_code}".to_string(),
    ];
    match check.auth {
        CredentialCheckAuth::Bearer => {
            argv.push("-H".to_string());
            argv.push(format!("Authorization: Bearer {}", material.expose()));
        }
        CredentialCheckAuth::Anthropic => {
            argv.push("-H".to_string());
            argv.push(format!("x-api-key: {}", material.expose()));
            argv.push("-H".to_string());
            argv.push("anthropic-version: 2023-06-01".to_string());
        }
        CredentialCheckAuth::Google => {
            argv.push("-H".to_string());
            argv.push(format!("x-goog-api-key: {}", material.expose()));
        }
    }
    argv.push(check.url.to_string());
    let outcome = runner.run(&argv).map_err(|error| {
        // The error path carries argv only as the program that failed to
        // spawn; CommandRunner errors name the command, so rebuild the
        // disclosure without the credential header.
        AikitError::new(
            "credential.verify_spawn_failed",
            format!("could not run the {provider} live check: {error}"),
        )
    })?;
    let unreachable = |definitive: bool, class: Option<String>| CredentialCheckOutcome {
        provider: provider.to_string(),
        verdict: CredentialVerdict::Unreachable,
        http_status_class: class,
        definitive,
    };
    if outcome.status != 0 {
        // curl failed below HTTP: DNS, connection, timeout. Not a verdict on
        // the key.
        let _ = outcome.stderr;
        return Ok(unreachable(false, None));
    }
    let code: u16 = outcome.stdout.trim().parse().map_err(|_| {
        AikitError::new(
            "credential.verify_status_unparsable",
            "the live check did not return an HTTP status code",
        )
    })?;
    let class = format!("{}xx", code / 100);
    let outcome = match code {
        200..=299 => CredentialCheckOutcome {
            provider: provider.to_string(),
            verdict: CredentialVerdict::Working,
            definitive: true,
            http_status_class: Some(class),
        },
        401 | 403 => CredentialCheckOutcome {
            provider: provider.to_string(),
            verdict: CredentialVerdict::Refused,
            definitive: true,
            http_status_class: Some(class),
        },
        400..=499 => CredentialCheckOutcome {
            provider: provider.to_string(),
            verdict: CredentialVerdict::Refused,
            definitive: false,
            http_status_class: Some(class),
        },
        _ => unreachable(false, Some(class)),
    };
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Output, ScriptedRunner};

    fn working_runner() -> ScriptedRunner {
        ScriptedRunner::new().on("curl", "200")
    }

    fn refused_runner() -> ScriptedRunner {
        ScriptedRunner::new().on("curl", "401")
    }

    fn broken_runner() -> ScriptedRunner {
        ScriptedRunner::new().failing("curl", 7, "curl: (7) Failed to connect")
    }

    fn material() -> SecretValue {
        SecretValue::new("fixture-material-not-a-real-key").unwrap()
    }

    #[test]
    fn a_success_status_is_a_working_definitive_verdict() {
        let outcome = check_provider_key(&working_runner(), "openai", &material()).unwrap();
        assert_eq!(outcome.verdict, CredentialVerdict::Working);
        assert_eq!(outcome.http_status_class.as_deref(), Some("2xx"));
        assert!(outcome.definitive);
        assert_eq!(outcome.provider, "openai");
    }

    #[test]
    fn a_401_is_a_definitive_refusal_and_the_header_carries_the_material() {
        let runner = refused_runner();
        let outcome = check_provider_key(&runner, "groq", &material()).unwrap();
        assert_eq!(outcome.verdict, CredentialVerdict::Refused);
        assert_eq!(outcome.http_status_class.as_deref(), Some("4xx"));
        assert!(outcome.definitive, "401 proves the key bad");
        let lines = runner.call_lines();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("Authorization: Bearer fixture-material-not-a-real-key"),
            "the materialised key must be the bearer credential: {lines:?}"
        );
        assert!(lines[0].contains("https://api.groq.com/openai/v1/models"));
    }

    #[test]
    fn a_transport_failure_is_unreachable_and_never_a_verdict_on_the_key() {
        let outcome = check_provider_key(&broken_runner(), "deepseek", &material()).unwrap();
        assert_eq!(outcome.verdict, CredentialVerdict::Unreachable);
        assert!(outcome.http_status_class.is_none());
        assert!(!outcome.definitive);
    }

    #[test]
    fn a_rate_limit_refuses_the_request_without_proving_the_key_bad() {
        let runner = ScriptedRunner::new().on("curl", "429");
        let outcome = check_provider_key(&runner, "moonshot", &material()).unwrap();
        assert_eq!(outcome.verdict, CredentialVerdict::Refused);
        assert!(!outcome.definitive);
    }

    #[test]
    fn a_provider_side_error_is_unreachable() {
        let runner = ScriptedRunner::new().on("curl", "503");
        let outcome = check_provider_key(&runner, "xai", &material()).unwrap();
        assert_eq!(outcome.verdict, CredentialVerdict::Unreachable);
        assert_eq!(outcome.http_status_class.as_deref(), Some("5xx"));
        assert!(!outcome.definitive);
    }

    #[test]
    fn an_unknown_provider_is_an_honest_refusal_and_runs_no_check() {
        let runner = working_runner();
        let error = check_provider_key(&runner, "some-unknown-vendor", &material()).unwrap_err();
        assert_eq!(error.code(), "credential.verify_provider_unknown");
        assert!(error.message().contains("some-unknown-vendor"));
        assert!(
            error.message().contains("openai"),
            "the refusal names the known providers: {error}"
        );
        assert!(
            runner.calls().is_empty(),
            "an unknown provider must not spend a network call"
        );
    }

    #[test]
    fn anthropic_and_gemini_use_their_own_auth_conventions() {
        let runner = working_runner();
        check_provider_key(&runner, "anthropic", &material()).unwrap();
        let line = &runner.call_lines()[0];
        assert!(line.contains("x-api-key: fixture-material-not-a-real-key"));
        assert!(line.contains("anthropic-version: 2023-06-01"));
        assert!(!line.contains("Authorization: Bearer"));

        let runner = working_runner();
        check_provider_key(&runner, "gemini", &material()).unwrap();
        let line = &runner.call_lines()[0];
        assert!(line.contains("x-goog-api-key: fixture-material-not-a-real-key"));
        assert!(line.contains("generativelanguage.googleapis.com"));
    }

    #[test]
    fn the_outcome_render_never_carries_material() {
        let outcome = check_provider_key(&working_runner(), "openai", &material()).unwrap();
        let rendered = format!("{outcome:?} {}", serde_json::to_string(&outcome).unwrap());
        assert!(!rendered.contains("fixture-material-not-a-real-key"));
        assert!(!rendered.contains("Authorization"));
    }

    #[test]
    fn every_known_provider_name_is_unique_and_https() {
        let mut names: Vec<&str> = KNOWN_CHECKS.iter().map(|check| check.provider).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "provider names must be unique");
        for check in KNOWN_CHECKS {
            assert!(check.url.starts_with("https://"));
        }
    }

    #[test]
    fn a_curl_success_with_a_garbled_status_is_an_error_not_a_pass() {
        let runner = ScriptedRunner::new().on("curl", "gateway-timeout");
        let error = check_provider_key(&runner, "openai", &material()).unwrap_err();
        assert_eq!(error.code(), "credential.verify_status_unparsable");
    }

    #[test]
    fn output_success_helper_remains_compatible_with_catalogue_usage() {
        let output = Output::success("200");
        assert_eq!(output.status, 0);
    }
}
