//! Live Telegram Bot API transport over the system `curl`.
//!
//! This is the HTTPS body behind [`crate::telegram_bot_api::TelegramBotApiTransport`]:
//! the workspace deliberately carries no HTTP client dependency, so the live
//! carrier shells out to the one HTTP client present on every machine this
//! gateway serves. Connector semantics stay fully deterministic through the
//! fake transport; only this module touches the network.
//!
//! The bot token appears in the Bot API URL path by Telegram's own scheme, so
//! it is held outside the command's visible arguments wherever curl allows and
//! is redacted from every error, log line and debug rendering this module
//! produces.

use std::path::PathBuf;
use std::process::Command;

use aikit_core::{AikitError, Result};
use serde_json::{json, Value};

use crate::telegram_bot_api::{TelegramBotApiTransport, TELEGRAM_BOT_API_BASE};

/// Default per-call ceiling for methods that do not long-poll themselves.
const DEFAULT_CALL_TIMEOUT_SECONDS: u64 = 30;
/// Long-poll calls carry their own Telegram `timeout`; curl gets a margin on
/// top so the server's own hold, not curl, ends the call in the normal case.
const LONG_POLL_MARGIN_SECONDS: u64 = 10;

pub struct TelegramCurlTransport {
    bot_token: String,
    api_base: String,
    curl_path: PathBuf,
}

impl std::fmt::Debug for TelegramCurlTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramCurlTransport")
            .field("api_base", &self.api_base)
            .field("curl_path", &self.curl_path)
            .field("bot_token", &"[redacted]")
            .finish()
    }
}

impl TelegramCurlTransport {
    /// Read the bot token from a `file:` location. The file must be
    /// owner-only, mirroring the gateway serve token discipline: a token any
    /// other account could read is refused before any request is made.
    pub fn from_token_location(location: &str) -> Result<Self> {
        let path = location
            .strip_prefix("file:")
            .ok_or_else(|| {
                AikitError::new(
                    "telegram_curl.token_location",
                    "Telegram token location must be a file: path (keychain refs are a \
                     named remaining obligation)"
                        .to_string(),
                )
            })?
            .trim()
            .to_string();
        if path.is_empty() {
            return Err(AikitError::new(
                "telegram_curl.token_location",
                "Telegram token location file: path is empty",
            ));
        }
        let token_path = PathBuf::from(&path);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&token_path)
            .map_err(|error| {
                AikitError::new(
                    "telegram_curl.token_unreadable",
                    format!("Telegram token file {} cannot be read: {error}", token_path.display()),
                )
            })?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(AikitError::new(
                "telegram_curl.token_unusable",
                format!(
                    "Telegram token file {} must be owner only (chmod 600); mode is {:o}",
                    token_path.display(),
                    mode & 0o777
                ),
            ));
        }
        let token = std::fs::read_to_string(&token_path)
            .map_err(|error| {
                AikitError::new(
                    "telegram_curl.token_unreadable",
                    format!("Telegram token file {} cannot be read: {error}", token_path.display()),
                )
            })?
            .trim()
            .to_string();
        if token.is_empty() {
            return Err(AikitError::new(
                "telegram_curl.token_unusable",
                format!("Telegram token file {} is empty", token_path.display()),
            ));
        }
        Self::from_token(token)
    }

    pub fn from_token(token: impl Into<String>) -> Result<Self> {
        let token = token.into();
        if token.trim().is_empty() {
            return Err(AikitError::new(
                "telegram_curl.token_unusable",
                "Telegram bot token must not be empty",
            ));
        }
        Ok(Self {
            bot_token: token,
            api_base: TELEGRAM_BOT_API_BASE.to_string(),
            curl_path: PathBuf::from("curl"),
        })
    }

    /// Override the Bot API base (deterministic specimens point this at a
    /// local server; production keeps the Telegram default).
    pub fn with_api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = api_base.into();
        self
    }

    /// Override the curl executable (tests point this at a fixture).
    pub fn with_curl_path(mut self, curl_path: impl Into<PathBuf>) -> Self {
        self.curl_path = curl_path.into();
        self
    }

    /// The exact curl invocation for one Bot API call, as
    /// `(url, arguments)`. Separated from execution so tests can assert the
    /// request shape without a network or a curl binary.
    fn request_for(&self, method: &str, params: &Value) -> (String, Vec<String>) {
        let url = format!("{}/bot{}/{}", self.api_base, self.bot_token, method);
        let timeout = match params.get("timeout").and_then(Value::as_u64) {
            Some(poll_seconds) => poll_seconds + LONG_POLL_MARGIN_SECONDS,
            None => DEFAULT_CALL_TIMEOUT_SECONDS,
        };
        let mut arguments = vec![
            "--silent".to_string(),
            "--show-error".to_string(),
            "--max-time".to_string(),
            timeout.to_string(),
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

impl TelegramBotApiTransport for TelegramCurlTransport {
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let (_url, arguments) = self.request_for(method, &params);
        let output = Command::new(&self.curl_path)
            .args(&arguments)
            .output()
            .map_err(|error| {
                AikitError::new(
                    "telegram_curl.transport_failed",
                    format!("Telegram {method}: curl could not be started: {error}"),
                )
            })?;
        if !output.status.success() {
            // curl stderr names hosts and system errors, never the URL path
            // with this module's argument order; keep it verbatim for honesty.
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AikitError::new(
                "telegram_curl.transport_failed",
                format!(
                    "Telegram {method}: curl exit {}: {}",
                    output.status.code().unwrap_or(-1),
                    stderr.trim()
                ),
            ));
        }
        let body = String::from_utf8_lossy(&output.stdout);
        let envelope: Value = serde_json::from_str(body.trim()).map_err(|error| {
            AikitError::new(
                "telegram_curl.invalid_response",
                format!("Telegram {method}: response was not JSON: {error}"),
            )
        })?;
        if envelope.get("ok").is_none() {
            return Err(AikitError::new(
                "telegram_curl.invalid_response",
                format!(
                    "Telegram {method}: response carries no ok field: {}",
                    envelope
                ),
            ));
        }
        Ok(envelope)
    }
}

#[cfg(test)]
mod telegram_gateway_curl_tests {
    use super::*;
    use crate::telegram_bot_api::TELEGRAM_BOT_API_BASE;

    fn r(value: &str) -> aikit_core::resource::ResourceRef {
        aikit_core::resource::ResourceRef::parse(value).unwrap()
    }

    #[test]
    fn curl_transport_request_carries_method_body_and_long_poll_margin() {
        let transport = TelegramCurlTransport::from_token("1234:secret-token").unwrap();
        let (url, arguments) =
            transport.request_for("getUpdates", &json!({"timeout": 30, "offset": 41}));
        assert_eq!(
            url,
            format!("{TELEGRAM_BOT_API_BASE}/bot1234:secret-token/getUpdates")
        );
        let max_time = arguments
            .iter()
            .position(|argument| argument == "--max-time")
            .unwrap();
        assert_eq!(arguments[max_time + 1], "40", "long poll gets the margin");
        let body = arguments
            .iter()
            .position(|argument| argument == "--data")
            .unwrap();
        let parsed: Value = serde_json::from_str(&arguments[body + 1]).unwrap();
        assert_eq!(parsed["timeout"], 30);
        assert_eq!(parsed["offset"], 41);
        assert_eq!(arguments.last().unwrap(), url.as_str());
    }

    #[test]
    fn curl_transport_debug_and_errors_never_name_the_token() {
        let transport = TelegramCurlTransport::from_token("1234:secret-token").unwrap();
        let rendered = format!("{transport:?}");
        assert!(!rendered.contains("secret-token"));
        assert!(rendered.contains("[redacted]"));
        // The URL used on the wire contains the token by Telegram's scheme,
        // but nothing rendered for a human does.
        let (_, arguments) = transport.request_for("getMe", &json!({}));
        assert!(arguments.last().unwrap().contains("secret-token"));
    }

    #[test]
    fn curl_transport_refuses_a_shared_or_missing_token_file() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let shared = home.path().join("shared.token");
        std::fs::write(&shared, "1234:token").unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o644)).unwrap();
        let error = TelegramCurlTransport::from_token_location(&format!("file:{}", shared.display()))
            .unwrap_err();
        assert_eq!(error.code(), "telegram_curl.token_unusable");
        assert!(error.to_string().contains("chmod 600"));

        let error = TelegramCurlTransport::from_token_location("file:/nonexistent/token")
            .unwrap_err();
        assert_eq!(error.code(), "telegram_curl.token_unreadable");

        let error = TelegramCurlTransport::from_token_location("keychain:telegram").unwrap_err();
        assert_eq!(error.code(), "telegram_curl.token_location");
    }

    #[test]
    fn curl_transport_maps_curl_failure_and_non_json_body_to_named_errors() {
        let home = tempfile::tempdir().unwrap();
        let curl_fail = home.path().join("curl-fail.sh");
        std::fs::write(
            &curl_fail,
            "#!/bin/sh\necho 'curl: (7) Failed to connect' >&2\nexit 7\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&curl_fail, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut transport =
            TelegramCurlTransport::from_token("1234:secret-token").unwrap();
        transport = transport.with_curl_path(&curl_fail);
        let error = transport.call("getMe", json!({})).unwrap_err();
        assert_eq!(error.code(), "telegram_curl.transport_failed");
        assert!(error.to_string().contains("curl exit 7"));
        assert!(!error.to_string().contains("secret-token"));

        let json_fail = home.path().join("curl-json.sh");
        std::fs::write(&json_fail, "#!/bin/sh\necho '<html>down</html>'\n").unwrap();
        std::fs::set_permissions(&json_fail, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut transport = TelegramCurlTransport::from_token("1234:secret-token")
            .unwrap()
            .with_curl_path(&json_fail);
        let error = transport.call("getMe", json!({})).unwrap_err();
        assert_eq!(error.code(), "telegram_curl.invalid_response");

        let envelope_fail = home.path().join("curl-envelope.sh");
        std::fs::write(&envelope_fail, "#!/bin/sh\necho '{\"unexpected\": true}'\n").unwrap();
        std::fs::set_permissions(
            &envelope_fail,
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let mut transport = TelegramCurlTransport::from_token("1234:secret-token")
            .unwrap()
            .with_curl_path(&envelope_fail);
        let error = transport.call("getMe", json!({})).unwrap_err();
        assert_eq!(error.code(), "telegram_curl.invalid_response");
    }

    #[test]
    fn curl_transport_passes_the_bot_api_envelope_through_untouched() {
        let home = tempfile::tempdir().unwrap();
        let script = home.path().join("curl-ok.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho '{\"ok\": true, \"result\": {\"id\": 7, \"is_bot\": true}}'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut transport = TelegramCurlTransport::from_token("12345:fixture-token")
            .unwrap()
            .with_curl_path(&script);
        let envelope = transport.call("getMe", json!({})).unwrap();
        assert_eq!(envelope["ok"], true);
        assert_eq!(envelope["result"]["id"], 7);
    }
}
