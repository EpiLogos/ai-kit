//! Default carrier resolution for the Agency Gateway commands.
//!
//! The gateway becomes part of the bootstrap/config posture by having one
//! well-known endpoint: `~/.aikit/state/gateway.sock` with semantic state in
//! `~/.aikit/state/gateway.json`. A bare `aikit gateway serve` is a complete
//! default posture, a bare `aikit gateway status` finds it, and `doctor`
//! reports it — so the terminal lane, the agent lane and the O:I desktop all
//! agree on where the gateway lives without a flag. Explicit carriers still
//! win over every default.

use aikit_adapters::{GatewayCarrierTarget, GatewayServiceConfig, DEFAULT_GATEWAY_MAX_FRAME_BYTES};
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;

use crate::cli::{GatewayQueryArgs, GatewayServeArgs};

/// Resolve the serve carriers. The state file always defaults to the home
/// file; the Unix carrier defaults to the home socket when no carrier is
/// named, so `serve` with no flags is same-host and discoverable. Naming
/// `--ws` alone is a deliberate network-only posture and binds no socket.
pub fn serve_config(home: &AikitHome, args: &GatewayServeArgs) -> Result<GatewayServiceConfig> {
    let websocket_bearer_token = args
        .websocket_token
        .clone()
        .or_else(gateway_token_from_env);
    #[cfg(unix)]
    let unix_socket = args.unix_socket.clone().or(match &args.websocket_bind {
        Some(_) => None,
        None => Some(home.gateway_socket()),
    });
    #[cfg(not(unix))]
    let unix_socket = args.unix_socket.clone();
    let config = GatewayServiceConfig {
        websocket_bind: args.websocket_bind.clone(),
        websocket_bearer_token,
        unix_socket,
        state_file: args
            .state_file
            .clone()
            .or_else(|| Some(home.gateway_state())),
        max_frame_bytes: DEFAULT_GATEWAY_MAX_FRAME_BYTES,
    };
    config.validate()?;
    Ok(config)
}

/// Resolve a query carrier: explicit `--unix`/`--ws` wins; with neither, the
/// well-known home socket.
pub fn carrier_target(home: &AikitHome, args: &GatewayQueryArgs) -> Result<GatewayCarrierTarget> {
    if args.unix_socket.is_some() && args.websocket_bind.is_some() {
        return Err(AikitError::new(
            "cli.gateway_carrier_conflict",
            "address one carrier: --unix PATH or --ws HOST:PORT, not both",
        ));
    }
    if let Some(bind) = &args.websocket_bind {
        let bearer_token = args
            .websocket_token
            .clone()
            .or_else(gateway_token_from_env)
            .ok_or_else(|| {
                AikitError::new(
                    "cli.gateway_token_required",
                    "WebSocket queries need --ws-token or AIKIT_GATEWAY_TOKEN",
                )
            })?;
        return Ok(GatewayCarrierTarget::WebSocket {
            bind: bind.clone(),
            path: args.websocket_path.clone(),
            bearer_token,
        });
    }
    #[cfg(unix)]
    {
        let path = args
            .unix_socket
            .clone()
            .unwrap_or_else(|| home.gateway_socket());
        Ok(GatewayCarrierTarget::UnixSocket(path))
    }
    #[cfg(not(unix))]
    {
        let _ = home;
        Err(AikitError::new(
            "cli.gateway_carrier_required",
            "this platform has no default Unix carrier; address a gateway with --ws HOST:PORT",
        ))
    }
}

/// Reframe a failed default-endpoint query so the common bootstrap case says
/// what to do instead of only naming the socket.
pub fn unreachable_hint(error: &AikitError) -> Option<AikitError> {
    if error.code() == "agency_gateway_client.unix_connect" {
        Some(
            AikitError::new(
                "cli.gateway_unreachable",
                format!("{error}; start one with `aikit gateway serve`"),
            )
            .with("hint", "aikit gateway serve"),
        )
    } else {
        None
    }
}

fn gateway_token_from_env() -> Option<String> {
    std::env::var("AIKIT_GATEWAY_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
}
