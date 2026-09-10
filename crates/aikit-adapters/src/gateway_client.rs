//! One-shot client for the AIKit Agency Gateway carriers.
//!
//! Speaks the same [`GatewayRequestEnvelope`]/[`GatewayResponseEnvelope`]
//! protocol as the persistent service carriers in [`crate::gateway_service`]:
//! newline-delimited JSON over the owner-only Unix-domain socket, or an
//! authenticated RFC 6455 WebSocket exchange over TCP. Each call opens one
//! connection, sends one request, reads one response and closes — a query
//! posture, not a session. Consumers that need continuous presence keep their
//! own connection; the gateway keeps no client affinity.

use std::{
    io::{self, BufRead, BufReader, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aikit_core::{AikitError, Result};

use crate::gateway_runtime::{GatewayCommand, GatewayRequestEnvelope, GatewayResponse};
use crate::gateway_runtime::GatewayResponseEnvelope;
use crate::gateway_service::{base64_encode, sha1, WEBSOCKET_GUID};

pub const GATEWAY_CLIENT_VERSION: &str = "aikit.gateway-client/v1";
const CARRIER_TIMEOUT: Duration = Duration::from_secs(10);
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Where a gateway query is addressed: the two carriers the service exposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayCarrierTarget {
    #[cfg(unix)]
    UnixSocket(PathBuf),
    WebSocket {
        /// `HOST:PORT`, the same shape `serve --ws` binds.
        bind: String,
        /// Request target of the HTTP upgrade; `/` by default.
        path: String,
        bearer_token: String,
    },
}

impl GatewayCarrierTarget {
    /// Build the WebSocket target, defaulting the upgrade path to `/`.
    pub fn websocket(bind: impl Into<String>, bearer_token: impl Into<String>) -> Self {
        Self::WebSocket {
            bind: bind.into(),
            path: "/".into(),
            bearer_token: bearer_token.into(),
        }
    }
}

/// Send one command, return one response envelope. Connection-per-request.
pub fn gateway_request(
    target: &GatewayCarrierTarget,
    command: GatewayCommand,
    request_id: Option<String>,
) -> Result<GatewayResponseEnvelope> {
    let request = GatewayRequestEnvelope {
        request_id,
        command,
    };
    let encoded = serde_json::to_string(&request).map_err(|error| {
        AikitError::new(
            "agency_gateway_client.request_encode",
            format!("encode gateway request: {error}"),
        )
    })?;
    match target {
        #[cfg(unix)]
        GatewayCarrierTarget::UnixSocket(path) => {
            unix_line_request(path, &encoded)
        }
        GatewayCarrierTarget::WebSocket {
            bind,
            path,
            bearer_token,
        } => websocket_request(bind, path, bearer_token, &encoded),
    }
}

/// Run a request and require an `ok` envelope, mapping gateway errors into
/// AIKit errors so CLI/agent surfaces see the usual error shape.
pub fn gateway_command(
    target: &GatewayCarrierTarget,
    command: GatewayCommand,
    request_id: Option<String>,
) -> Result<GatewayResponse> {
    let envelope = gateway_request(target, command, request_id)?;
    if !envelope.ok {
        return Err(envelope
            .error
            .map(|error| {
                AikitError::new(
                    "agency_gateway_client.gateway_refused",
                    format!("gateway refused the command: {}", error.message),
                )
                .with("gateway_error_code", error.code)
                .with("gateway_error_message", error.message)
            })
            .unwrap_or_else(|| {
                AikitError::new(
                    "agency_gateway_client.envelope_refused",
                    "gateway returned a failure envelope without an error block",
                )
            }));
    }
    envelope.response.ok_or_else(|| {
        AikitError::new(
            "agency_gateway_client.empty_response",
            "gateway returned an ok envelope without a response",
        )
    })
}

#[cfg(unix)]
fn unix_line_request(path: &std::path::Path, encoded_request: &str) -> Result<GatewayResponseEnvelope> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(path).map_err(|error| {
        AikitError::new(
            "agency_gateway_client.unix_connect",
            format!("connect gateway socket {}: {error}", path.display()),
        )
    })?;
    stream
        .set_read_timeout(Some(CARRIER_TIMEOUT))
        .map_err(|error| {
            AikitError::new(
                "agency_gateway_client.unix_timeout",
                format!("set gateway socket read timeout: {error}"),
            )
        })?;
    write_request_line(&mut stream, encoded_request)?;
    read_response_line(&mut stream)
}

fn write_request_line<W: Write>(stream: &mut W, encoded_request: &str) -> Result<()> {
    stream
        .write_all(encoded_request.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .map_err(|error| {
            AikitError::new(
                "agency_gateway_client.write",
                format!("write gateway request: {error}"),
            )
        })
}

fn read_response_line<S: io::Read>(stream: &mut S) -> Result<GatewayResponseEnvelope> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|error| {
        AikitError::new(
            "agency_gateway_client.read",
            format!("read gateway response: {error}"),
        )
    })?;
    if line.trim().is_empty() {
        return Err(AikitError::new(
            "agency_gateway_client.empty_response",
            "gateway closed the carrier before answering",
        ));
    }
    decode_response(line.trim().as_bytes())
}

fn decode_response(payload: &[u8]) -> Result<GatewayResponseEnvelope> {
    serde_json::from_slice(payload).map_err(|error| {
        AikitError::new(
            "agency_gateway_client.response_decode",
            format!("decode gateway response: {error}"),
        )
    })
}

fn websocket_request(
    bind: &str,
    path: &str,
    bearer_token: &str,
    encoded_request: &str,
) -> Result<GatewayResponseEnvelope> {
    let stream = tcp_connect(bind)?;
    let mut writer = stream.try_clone().map_err(|error| {
        AikitError::new(
            "agency_gateway_client.stream_clone",
            format!("clone gateway carrier stream: {error}"),
        )
    })?;
    let mut reader = BufReader::new(stream);
    websocket_handshake(&mut reader, &mut writer, bind, path, bearer_token)?;
    write_masked_text_frame(&mut writer, encoded_request.as_bytes())?;
    let payload = read_server_text_frame(&mut reader)?;
    decode_response(&payload)
}

fn tcp_connect(bind: &str) -> Result<TcpStream> {
    let address = bind
        .to_socket_addrs()
        .map_err(|error| {
            AikitError::new(
                "agency_gateway_client.resolve",
                format!("resolve gateway bind {bind}: {error}"),
            )
        })?
        .next()
        .ok_or_else(|| {
            AikitError::new(
                "agency_gateway_client.resolve",
                format!("gateway bind {bind} resolved to no address"),
            )
        })?;
    let stream = TcpStream::connect_timeout(&address, CARRIER_TIMEOUT).map_err(|error| {
        AikitError::new(
            "agency_gateway_client.connect",
            format!("connect gateway at {bind}: {error}"),
        )
    })?;
    stream
        .set_read_timeout(Some(CARRIER_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(CARRIER_TIMEOUT)))
        .map_err(|error| {
            AikitError::new(
                "agency_gateway_client.timeout",
                format!("set gateway carrier timeouts: {error}"),
            )
        })?;
    Ok(stream)
}

fn websocket_handshake<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    bind: &str,
    path: &str,
    bearer_token: &str,
) -> Result<()> {
    let key = websocket_key();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {bind}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {key}\r\nAuthorization: Bearer {bearer_token}\r\n\r\n"
    );
    writer
        .write_all(request.as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|error| {
            AikitError::new(
                "agency_gateway_client.handshake_write",
                format!("send gateway WebSocket upgrade: {error}"),
            )
        })?;

    let mut status = String::new();
    reader
        .read_line(&mut status)
        .map_err(|error| {
            AikitError::new(
                "agency_gateway_client.handshake_read",
                format!("read gateway upgrade status: {error}"),
            )
        })?;
    if !status.starts_with("HTTP/1.1 101") {
        return Err(AikitError::new(
            "agency_gateway_client.handshake_refused",
            format!(
                "gateway refused the WebSocket upgrade: {}",
                status.trim_end()
            ),
        ));
    }

    let mut accept = None;
    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|error| {
                AikitError::new(
                    "agency_gateway_client.handshake_read",
                    format!("read gateway upgrade header: {error}"),
                )
            })?;
        if bytes == 0 {
            return Err(AikitError::new(
                "agency_gateway_client.handshake_eof",
                "gateway closed during the WebSocket upgrade",
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("sec-websocket-accept") {
                accept = Some(value.trim().to_string());
            }
        }
    }
    let accept = accept.ok_or_else(|| {
        AikitError::new(
            "agency_gateway_client.handshake_accept",
            "gateway upgrade carried no Sec-WebSocket-Accept",
        )
    })?;
    let mut handshake_input = key.as_bytes().to_vec();
    handshake_input.extend_from_slice(WEBSOCKET_GUID.as_bytes());
    let expected = base64_encode(&sha1(&handshake_input));
    if !constant_time_eq(accept.as_bytes(), expected.as_bytes()) {
        return Err(AikitError::new(
            "agency_gateway_client.handshake_accept_mismatch",
            "gateway Sec-WebSocket-Accept does not complete the RFC 6455 handshake",
        ));
    }
    Ok(())
}

/// 16 non-secret bytes, base64: the RFC 6455 `Sec-WebSocket-Key` shape. The
/// value only has to be unique per handshake; the accept check binds it.
fn websocket_key() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    let seed = nanos ^ ((std::process::id() as u128) << 64) ^ (counter << 96);
    let bytes = seed.to_be_bytes();
    base64_encode(&bytes)
}

fn client_mask() -> [u8; 4] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mixed = (nanos as u32)
        ^ ((nanos >> 32) as u32)
        ^ std::process::id()
        ^ (REQUEST_COUNTER.load(Ordering::Relaxed) as u32);
    mixed.to_be_bytes()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

fn write_masked_text_frame<W: Write>(writer: &mut W, payload: &[u8]) -> Result<()> {
    let mask = client_mask();
    writer
        .write_all(&[0x81])
        .map_err(frame_io_error("write client frame opcode"))?;
    let length = payload.len();
    if length < 126 {
        writer
            .write_all(&[0x80 | length as u8])
            .map_err(frame_io_error("write client frame length"))?;
    } else if length <= u16::MAX as usize {
        writer
            .write_all(&[0x80 | 126])
            .and_then(|()| writer.write_all(&(length as u16).to_be_bytes()))
            .map_err(frame_io_error("write client frame length"))?;
    } else {
        writer
            .write_all(&[0x80 | 127])
            .and_then(|()| writer.write_all(&(length as u64).to_be_bytes()))
            .map_err(frame_io_error("write client frame length"))?;
    }
    writer
        .write_all(&mask)
        .map_err(frame_io_error("write client frame mask"))?;
    let masked = payload
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ mask[index % 4])
        .collect::<Vec<_>>();
    writer
        .write_all(&masked)
        .and_then(|()| writer.flush())
        .map_err(frame_io_error("write client frame payload"))?;
    Ok(())
}

fn read_server_text_frame<R: io::Read>(reader: &mut R) -> Result<Vec<u8>> {
    let mut head = [0u8; 2];
    reader
        .read_exact(&mut head)
        .map_err(|error| {
            AikitError::new(
                "agency_gateway_client.frame_read",
                format!("read gateway frame: {error}"),
            )
        })?;
    let opcode = head[0] & 0x0f;
    if head[1] & 0x80 != 0 {
        return Err(AikitError::new(
            "agency_gateway_client.server_masked",
            "server WebSocket frames must arrive unmasked",
        ));
    }
    let mut length = (head[1] & 0x7f) as u64;
    if length == 126 {
        let mut bytes = [0u8; 2];
        reader
            .read_exact(&mut bytes)
            .map_err(frame_io_error("read gateway frame length"))?;
        length = u16::from_be_bytes(bytes) as u64;
    } else if length == 127 {
        let mut bytes = [0u8; 8];
        reader
            .read_exact(&mut bytes)
            .map_err(frame_io_error("read gateway frame length"))?;
        length = u64::from_be_bytes(bytes);
    }
    let mut payload = vec![0u8; length as usize];
    reader
        .read_exact(&mut payload)
        .map_err(frame_io_error("read gateway frame payload"))?;
    match opcode {
        0x1 => Ok(payload),
        0x8 => Err(AikitError::new(
            "agency_gateway_client.closed",
            "gateway closed the WebSocket before answering",
        )),
        other => Err(AikitError::new(
            "agency_gateway_client.unexpected_frame",
            format!("gateway sent unexpected WebSocket opcode {other}"),
        )),
    }
}

fn frame_io_error(context: &'static str) -> impl FnOnce(io::Error) -> AikitError {
    move |error| {
        AikitError::new(
            "agency_gateway_client.frame_io",
            format!("{context}: {error}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway_connector::ConnectorDescriptor;
    use crate::gateway_runtime::AgencyGateway;
    use crate::gateway_service::{run_gateway_service, GatewayServiceConfig};
    use aikit_core::resource::ResourceRef;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    fn r(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn wait_until(deadline_seconds: u64, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(deadline_seconds);
        while !condition() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(condition(), "condition not reached within {deadline_seconds}s");
    }

    fn descriptor() -> ConnectorDescriptor {
        use crate::gateway_connector::ConnectorCapabilities;
        use std::collections::BTreeSet;
        ConnectorDescriptor {
            version: crate::gateway_connector::GATEWAY_CONNECTOR_SDK_VERSION.into(),
            connector_ref: r("gateway-connector/fixture/main"),
            platform: "fixture".into(),
            implementation: "gateway-client-test".into(),
            capabilities: ConnectorCapabilities {
                operations: BTreeSet::from([crate::gateway_connector::ConnectorOperation::Send]),
                max_text_bytes: Some(1024),
                max_media_bytes: None,
                media_types: BTreeSet::new(),
                provenance: vec!["fixture".into()],
            },
            configuration_ref: None,
            provenance: vec!["fixture".into()],
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_carrier_speaks_the_full_protocol_one_shot() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("gateway.sock");
        let state = root.path().join("gateway.json");
        let config = GatewayServiceConfig {
            websocket_bind: None,
            websocket_bearer_token: None,
            unix_socket: Some(socket.clone()),
            state_file: Some(state.clone()),
            max_frame_bytes: crate::gateway_service::DEFAULT_GATEWAY_MAX_FRAME_BYTES,
        };
        thread::spawn(move || {
            let _ = run_gateway_service(AgencyGateway::new(r("agency-gateway/local")), config);
        });
        wait_until(3, || socket.exists());
        let target = GatewayCarrierTarget::UnixSocket(socket.clone());

        let protocol = gateway_command(
            &target,
            GatewayCommand::Protocol,
            Some("protocol-1".into()),
        )
        .unwrap();
        let GatewayResponse::Protocol { gateway_version, .. } = protocol else {
            panic!("protocol command should answer with versions");
        };
        assert_eq!(gateway_version, crate::gateway_runtime::AGENCY_GATEWAY_VERSION);

        gateway_command(
            &target,
            GatewayCommand::RegisterConnector {
                descriptor: descriptor(),
            },
            None,
        )
        .unwrap();
        let empty = gateway_command(&target, GatewayCommand::Ecology, None).unwrap();
        let GatewayResponse::Ecology { ecology } = empty else {
            panic!("expected ecology");
        };
        assert!(ecology.agencies.is_empty());
        assert_eq!(ecology.authority, "presence-does-not-imply-authority");

        gateway_command(&target, GatewayCommand::Shutdown, None).unwrap();
        wait_until(3, || !socket.exists());
        assert!(state.exists());
    }

    #[test]
    fn websocket_carrier_speaks_the_full_protocol_one_shot() {
        // Probe an ephemeral loopback port, release it, and let the service
        // claim it; the wait_until connect loop absorbs the rebind window.
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let bind = format!("127.0.0.1:{port}");

        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("gateway.json");
        let config = GatewayServiceConfig {
            websocket_bind: Some(bind.clone()),
            websocket_bearer_token: Some("secret".into()),
            unix_socket: None,
            state_file: Some(state),
            max_frame_bytes: crate::gateway_service::DEFAULT_GATEWAY_MAX_FRAME_BYTES,
        };
        let service = thread::spawn(move || {
            run_gateway_service(AgencyGateway::new(r("agency-gateway/local")), config)
        });
        let target = Arc::new(GatewayCarrierTarget::websocket(bind.clone(), "secret"));
        let reachable = target.clone();
        wait_until(5, || {
            gateway_request(&reachable, GatewayCommand::Protocol, None).is_ok()
        });

        let ecology = gateway_command(&target, GatewayCommand::Ecology, Some("eco-1".into()))
            .unwrap();
        let GatewayResponse::Ecology { ecology } = ecology else {
            panic!("expected ecology");
        };
        assert_eq!(ecology.gateway_ref, r("agency-gateway/local"));
        assert_eq!(ecology.authority, "presence-does-not-imply-authority");

        // A wrong bearer never completes the upgrade.
        let refused = GatewayCarrierTarget::websocket(bind.clone(), "wrong");
        let error = gateway_request(&refused, GatewayCommand::Protocol, None).unwrap_err();
        assert_eq!(error.code(), "agency_gateway_client.handshake_refused");

        gateway_request(&target, GatewayCommand::Shutdown, None).unwrap();
        service.join().unwrap().unwrap();
    }
}
