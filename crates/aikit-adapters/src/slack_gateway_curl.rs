//! Live Slack Web API transport over the system `curl`.
//!
//! This is the HTTPS body behind [`crate::slack_bot_api::SlackBotApiTransport`]:
//! the workspace deliberately carries no HTTP client dependency, so the live
//! carrier shells out to the one HTTP client present on every machine this
//! gateway serves. Connector semantics stay fully deterministic through the
//! fake transport; only this module touches the network.
//!
//! Unlike Telegram, Slack keeps the token out of the URL entirely: every call
//! is a JSON POST to `{base}/{method}` with `Authorization: Bearer <token>`.
//! The token travels in a header argument, so it is redacted from every
//! error, log line and debug rendering this module produces.

use std::path::PathBuf;
use std::process::Command;

use aikit_core::{AikitError, Result};
use serde_json::Value;

use crate::slack_bot_api::{SlackBotApiTransport, SLACK_WEB_API_BASE};

/// Per-call ceiling; Slack Web API answers are synchronous.
const DEFAULT_CALL_TIMEOUT_SECONDS: u64 = 30;

pub struct SlackCurlTransport {
    bot_token: String,
    api_base: String,
    curl_path: PathBuf,
}

impl std::fmt::Debug for SlackCurlTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlackCurlTransport")
            .field("api_base", &self.api_base)
            .field("curl_path", &self.curl_path)
            .field("bot_token", &"[redacted]")
            .finish()
    }
}

impl SlackCurlTransport {
    /// Read the bot token from a `file:` location. The file must be
    /// owner-only, mirroring the gateway serve token discipline: a token any
    /// other account could read is refused before any request is made.
    pub fn from_token_location(location: &str) -> Result<Self> {
        let path = location
            .strip_prefix("file:")
            .ok_or_else(|| {
                AikitError::new(
                    "slack_curl.token_location",
                    "Slack token location must be a file: path (keychain refs are a \
                     named remaining obligation)"
                        .to_string(),
                )
            })?
            .trim()
            .to_string();
        if path.is_empty() {
            return Err(AikitError::new(
                "slack_curl.token_location",
                "Slack token location file: path is empty",
            ));
        }
        let token_path = PathBuf::from(&path);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&token_path)
            .map_err(|error| {
                AikitError::new(
                    "slack_curl.token_unreadable",
                    format!(
                        "Slack token file {} cannot be read: {error}",
                        token_path.display()
                    ),
                )
            })?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(AikitError::new(
                "slack_curl.token_unusable",
                format!(
                    "Slack token file {} must be owner only (chmod 600); mode is {:o}",
                    token_path.display(),
                    mode & 0o777
                ),
            ));
        }
        let token = std::fs::read_to_string(&token_path)
            .map_err(|error| {
                AikitError::new(
                    "slack_curl.token_unreadable",
                    format!(
                        "Slack token file {} cannot be read: {error}",
                        token_path.display()
                    ),
                )
            })?
            .trim()
            .to_string();
        if token.is_empty() {
            return Err(AikitError::new(
                "slack_curl.token_unusable",
                format!("Slack token file {} is empty", token_path.display()),
            ));
        }
        Self::from_token(token)
    }

    pub fn from_token(token: impl Into<String>) -> Result<Self> {
        let token = token.into();
        if token.trim().is_empty() {
            return Err(AikitError::new(
                "slack_curl.token_unusable",
                "Slack bot token must not be empty",
            ));
        }
        Ok(Self {
            bot_token: token,
            api_base: SLACK_WEB_API_BASE.to_string(),
            curl_path: PathBuf::from("curl"),
        })
    }

    /// Override the Web API base (deterministic specimens point this at a
    /// local server; production keeps the Slack default).
    pub fn with_api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = api_base.into();
        self
    }

    /// Override the curl executable (tests point this at a fixture).
    pub fn with_curl_path(mut self, curl_path: impl Into<PathBuf>) -> Self {
        self.curl_path = curl_path.into();
        self
    }

    /// The exact curl invocation for one Web API call, as
    /// `(url, arguments)`. Separated from execution so tests can assert the
    /// request shape without a network or a curl binary.
    fn request_for(&self, method: &str, params: &Value) -> (String, Vec<String>) {
        let url = format!("{}/{}", self.api_base, method);
        let mut arguments = vec![
            "--silent".to_string(),
            "--show-error".to_string(),
            "--max-time".to_string(),
            DEFAULT_CALL_TIMEOUT_SECONDS.to_string(),
            "--header".to_string(),
            format!("Authorization: Bearer {}", self.bot_token),
            "--header".to_string(),
            "Content-Type: application/json".to_string(),
            "--request".to_string(),
            "POST".to_string(),
            "--data".to_string(),
            params.to_string(),
        ];
        arguments.push(url.clone());
        (url, arguments)
    }
}

impl SlackBotApiTransport for SlackCurlTransport {
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let (_url, arguments) = self.request_for(method, &params);
        let output = Command::new(&self.curl_path)
            .args(&arguments)
            .output()
            .map_err(|error| {
                AikitError::new(
                    "slack_curl.transport_failed",
                    format!("Slack {method}: curl could not be started: {error}"),
                )
            })?;
        if !output.status.success() {
            // curl stderr names hosts and system errors; the token rides a
            // header argument curl never echoes to stderr.
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AikitError::new(
                "slack_curl.transport_failed",
                format!(
                    "Slack {method}: curl exit {}: {}",
                    output.status.code().unwrap_or(-1),
                    stderr.trim()
                ),
            ));
        }
        let body = String::from_utf8_lossy(&output.stdout);
        let envelope: Value = serde_json::from_str(body.trim()).map_err(|error| {
            AikitError::new(
                "slack_curl.invalid_response",
                format!("Slack {method}: response was not JSON: {error}"),
            )
        })?;
        if envelope.get("ok").is_none() {
            return Err(AikitError::new(
                "slack_curl.invalid_response",
                format!("Slack {method}: response carries no ok field: {}", envelope),
            ));
        }
        Ok(envelope)
    }
}

#[cfg(test)]
mod slack_gateway_curl_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn curl_transport_carries_the_token_in_a_header_not_the_url() {
        let transport = SlackCurlTransport::from_token("xoxb-fixture-token").unwrap();
        let (url, arguments) =
            transport.request_for("chat.postMessage", &json!({"channel": "C1", "text": "hi"}));
        assert_eq!(url, format!("{SLACK_WEB_API_BASE}/chat.postMessage"));
        assert!(
            !url.contains("xoxb-fixture-token"),
            "the Slack token never appears in the URL"
        );
        let header = arguments
            .iter()
            .position(|argument| argument == "--header")
            .unwrap();
        assert_eq!(
            arguments[header + 1],
            "Authorization: Bearer xoxb-fixture-token"
        );
        let body = arguments
            .iter()
            .position(|argument| argument == "--data")
            .unwrap();
        let parsed: Value = serde_json::from_str(&arguments[body + 1]).unwrap();
        assert_eq!(parsed["channel"], "C1");
        assert_eq!(arguments.last().unwrap(), url.as_str());
    }

    #[test]
    fn curl_transport_debug_and_errors_never_name_the_token() {
        let transport = SlackCurlTransport::from_token("xoxb-fixture-token").unwrap();
        let rendered = format!("{transport:?}");
        assert!(!rendered.contains("xoxb-fixture-token"));
        assert!(rendered.contains("[redacted]"));
    }

    #[test]
    fn curl_transport_refuses_a_shared_or_missing_token_file() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let shared = home.path().join("shared.token");
        std::fs::write(&shared, "xoxb-token").unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o644)).unwrap();
        let error = SlackCurlTransport::from_token_location(&format!("file:{}", shared.display()))
            .unwrap_err();
        assert_eq!(error.code(), "slack_curl.token_unusable");
        assert!(error.to_string().contains("chmod 600"));

        let error = SlackCurlTransport::from_token_location("file:/nonexistent/token").unwrap_err();
        assert_eq!(error.code(), "slack_curl.token_unreadable");

        let error = SlackCurlTransport::from_token_location("keychain:slack").unwrap_err();
        assert_eq!(error.code(), "slack_curl.token_location");
    }

    #[test]
    fn curl_transport_maps_curl_failure_and_non_json_body_to_named_errors() {
        let home = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        let curl_fail = home.path().join("curl-fail.sh");
        std::fs::write(
            &curl_fail,
            "#!/bin/sh\necho 'curl: (7) Failed to connect' >&2\nexit 7\n",
        )
        .unwrap();
        std::fs::set_permissions(&curl_fail, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut transport = SlackCurlTransport::from_token("xoxb-fixture-token")
            .unwrap()
            .with_curl_path(&curl_fail);
        let error = transport.call("auth.test", json!({})).unwrap_err();
        assert_eq!(error.code(), "slack_curl.transport_failed");
        assert!(error.to_string().contains("curl exit 7"));
        assert!(!error.to_string().contains("xoxb-fixture-token"));

        let json_fail = home.path().join("curl-json.sh");
        std::fs::write(&json_fail, "#!/bin/sh\necho '<html>down</html>'\n").unwrap();
        std::fs::set_permissions(&json_fail, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut transport = SlackCurlTransport::from_token("xoxb-fixture-token")
            .unwrap()
            .with_curl_path(&json_fail);
        let error = transport.call("auth.test", json!({})).unwrap_err();
        assert_eq!(error.code(), "slack_curl.invalid_response");

        let envelope_fail = home.path().join("curl-envelope.sh");
        std::fs::write(&envelope_fail, "#!/bin/sh\necho '{\"unexpected\": true}'\n").unwrap();
        std::fs::set_permissions(&envelope_fail, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut transport = SlackCurlTransport::from_token("xoxb-fixture-token")
            .unwrap()
            .with_curl_path(&envelope_fail);
        let error = transport.call("auth.test", json!({})).unwrap_err();
        assert_eq!(error.code(), "slack_curl.invalid_response");
    }

    #[test]
    fn curl_transport_passes_the_web_api_envelope_through_untouched() {
        let home = tempfile::tempdir().unwrap();
        let script = home.path().join("curl-ok.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho '{\"ok\": true, \"team_id\": \"T0TEAM\", \"bot_id\": \"B0BOT\"}'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut transport = SlackCurlTransport::from_token("xoxb-fixture-token")
            .unwrap()
            .with_curl_path(&script);
        let envelope = transport.call("auth.test", json!({})).unwrap();
        assert_eq!(envelope["ok"], true);
        assert_eq!(envelope["team_id"], "T0TEAM");
    }
}
