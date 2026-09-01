//! Persistent carriers for the AIKit Agency Gateway service body.
//!
//! Gateway semantics remain in `gateway_runtime`; this module only materialises
//! the already-versioned request/response protocol over durable process carriers.
//! Workcell can therefore start/observe/release `aikit-gateway` as an ordinary
//! managed service without importing AgentSession, ActuationStream or connector
//! semantics.
//!
//! Network carrier: RFC 6455 WebSocket with bearer authentication during the HTTP
//! upgrade. Same-host carrier: Unix-domain socket with owner-only filesystem
//! permissions. Both execute the same [`GatewayRequestEnvelope`] commands as the
//! existing stdio carrier.

use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};

use crate::gateway_runtime::{
    execute_gateway_command, AgencyGateway, GatewayRequestEnvelope, GatewayResponseEnvelope,
};

pub const GATEWAY_SERVICE_CARRIER_VERSION: &str = "aikit.gateway-service-carrier/v1";
pub const DEFAULT_GATEWAY_MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayServiceConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub websocket_bind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub websocket_bearer_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unix_socket: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_file: Option<PathBuf>,
    #[serde(default = "default_max_frame_bytes")]
    pub max_frame_bytes: usize,
}

fn default_max_frame_bytes() -> usize {
    DEFAULT_GATEWAY_MAX_FRAME_BYTES
}

impl GatewayServiceConfig {
    pub fn validate(&self) -> Result<()> {
        if self.websocket_bind.is_none() && self.unix_socket.is_none() {
            return Err(AikitError::new(
                "agency_gateway_service.no_carrier",
                "gateway service mode requires --ws and/or --unix",
            ));
        }
        if self.websocket_bind.is_some()
            && self
                .websocket_bearer_token
                .as_deref()
                .is_none_or(|token| token.trim().is_empty())
        {
            return Err(AikitError::new(
                "agency_gateway_service.websocket_auth_required",
                "network WebSocket carrier requires a non-empty bearer token",
            ));
        }
        if self
            .websocket_bind
            .as_deref()
            .is_some_and(|bind| bind.trim().is_empty())
        {
            return Err(AikitError::new(
                "agency_gateway_service.empty_websocket_bind",
                "WebSocket bind address must not be empty",
            ));
        }
        if self.max_frame_bytes == 0 {
            return Err(AikitError::new(
                "agency_gateway_service.invalid_frame_limit",
                "gateway WebSocket frame limit must be greater than zero",
            ));
        }
        Ok(())
    }
}

/// Load a previously persisted semantic gateway snapshot when present.
///
/// The persisted document is the canonical gateway snapshot itself. It contains
/// no PID, socket or Workcell allocation identity.
pub fn restore_gateway_state(
    fresh: AgencyGateway,
    state_file: Option<&Path>,
) -> Result<AgencyGateway> {
    let Some(path) = state_file else {
        return Ok(fresh);
    };
    if !path.exists() {
        return Ok(fresh);
    }
    let content = fs::read_to_string(path).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.state_read",
            format!("read gateway state {}: {error}", path.display()),
        )
    })?;
    let snapshot = serde_json::from_str(&content).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.state_decode",
            format!("decode gateway state {}: {error}", path.display()),
        )
    })?;
    AgencyGateway::from_snapshot(snapshot)
}

pub fn persist_gateway_state(gateway: &AgencyGateway, state_file: Option<&Path>) -> Result<()> {
    let Some(path) = state_file else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.state_directory",
                format!("create gateway state directory {}: {error}", parent.display()),
            )
        })?;
    }
    let encoded = serde_json::to_vec_pretty(&gateway.snapshot()).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.state_encode",
            format!("encode gateway state: {error}"),
        )
    })?;
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("json")
    ));
    {
        let mut file = fs::File::create(&tmp).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.state_write",
                format!("create gateway state {}: {error}", tmp.display()),
            )
        })?;
        file.write_all(&encoded).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.state_write",
                format!("write gateway state {}: {error}", tmp.display()),
            )
        })?;
        file.sync_all().map_err(|error| {
            AikitError::new(
                "agency_gateway_service.state_sync",
                format!("sync gateway state {}: {error}", tmp.display()),
            )
        })?;
    }
    fs::rename(&tmp, path).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.state_replace",
            format!(
                "replace gateway state {} from {}: {error}",
                path.display(),
                tmp.display()
            ),
        )
    })?;
    Ok(())
}

/// Run every configured service carrier against one shared gateway state.
pub fn run_gateway_service(gateway: AgencyGateway, config: GatewayServiceConfig) -> Result<()> {
    config.validate()?;
    let gateway = restore_gateway_state(gateway, config.state_file.as_deref())?;
    let gateway = Arc::new(Mutex::new(gateway));
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::new();

    if let Some(bind) = config.websocket_bind.clone() {
        let token = config
            .websocket_bearer_token
            .clone()
            .expect("validated WebSocket bearer token");
        let listener = TcpListener::bind(&bind).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.websocket_bind",
                format!("bind WebSocket gateway at {bind}: {error}"),
            )
        })?;
        listener.set_nonblocking(true).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.websocket_nonblocking",
                format!("configure WebSocket listener {bind}: {error}"),
            )
        })?;
        let gateway = Arc::clone(&gateway);
        let shutdown = Arc::clone(&shutdown);
        let state_file = config.state_file.clone();
        let max_frame_bytes = config.max_frame_bytes;
        workers.push(thread::spawn(move || {
            serve_websocket_listener(
                listener,
                gateway,
                shutdown,
                token,
                state_file,
                max_frame_bytes,
            )
        }));
    }

    #[cfg(unix)]
    if let Some(path) = config.unix_socket.clone() {
        let gateway = Arc::clone(&gateway);
        let shutdown = Arc::clone(&shutdown);
        let state_file = config.state_file.clone();
        workers.push(thread::spawn(move || {
            serve_unix_socket(path, gateway, shutdown, state_file)
        }));
    }

    #[cfg(not(unix))]
    if config.unix_socket.is_some() {
        return Err(AikitError::new(
            "agency_gateway_service.unix_unsupported",
            "Unix-domain gateway carrier is unavailable on this platform",
        ));
    }

    let mut first_error = None;
    for worker in workers {
        match worker.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                shutdown.store(true, Ordering::SeqCst);
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            Err(_) => {
                shutdown.store(true, Ordering::SeqCst);
                if first_error.is_none() {
                    first_error = Some(AikitError::new(
                        "agency_gateway_service.worker_panic",
                        "gateway service carrier worker panicked",
                    ));
                }
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    let gateway = gateway.lock().map_err(|_| {
        AikitError::new(
            "agency_gateway_service.poisoned",
            "gateway state lock was poisoned",
        )
    })?;
    persist_gateway_state(&gateway, config.state_file.as_deref())
}

fn serve_websocket_listener(
    listener: TcpListener,
    gateway: Arc<Mutex<AgencyGateway>>,
    shutdown: Arc<AtomicBool>,
    token: String,
    state_file: Option<PathBuf>,
    max_frame_bytes: usize,
) -> Result<()> {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _peer)) => {
                let gateway = Arc::clone(&gateway);
                let shutdown = Arc::clone(&shutdown);
                let token = token.clone();
                let state_file = state_file.clone();
                thread::spawn(move || {
                    let _ = handle_websocket_connection(
                        stream,
                        gateway,
                        shutdown,
                        &token,
                        state_file.as_deref(),
                        max_frame_bytes,
                    );
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                return Err(AikitError::new(
                    "agency_gateway_service.websocket_accept",
                    format!("accept gateway WebSocket connection: {error}"),
                ));
            }
        }
    }
    Ok(())
}

fn handle_websocket_connection(
    stream: TcpStream,
    gateway: Arc<Mutex<AgencyGateway>>,
    shutdown: Arc<AtomicBool>,
    token: &str,
    state_file: Option<&Path>,
    max_frame_bytes: usize,
) -> Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(io_error("configure WebSocket read timeout"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(io_error("configure WebSocket write timeout"))?;
    let writer = stream
        .try_clone()
        .map_err(io_error("clone WebSocket stream"))?;
    let mut reader = BufReader::new(stream);
    let mut writer = writer;
    websocket_handshake(&mut reader, &mut writer, token)?;

    loop {
        let frame = match read_websocket_frame(&mut reader, max_frame_bytes) {
            Ok(frame) => frame,
            Err(error) if error.code() == "agency_gateway_service.websocket_eof" => return Ok(()),
            Err(error) => {
                let _ = write_websocket_close(&mut writer, 1002, "protocol error");
                return Err(error);
            }
        };
        match frame.opcode {
            0x1 => {
                let text = String::from_utf8(frame.payload).map_err(|error| {
                    AikitError::new(
                        "agency_gateway_service.websocket_utf8",
                        format!("WebSocket text frame is not UTF-8: {error}"),
                    )
                })?;
                let (response, should_shutdown) =
                    execute_serialized_request(&gateway, &text, state_file)?;
                write_websocket_text(&mut writer, response.as_bytes())?;
                if should_shutdown {
                    shutdown.store(true, Ordering::SeqCst);
                    write_websocket_close(&mut writer, 1000, "gateway shutdown")?;
                    return Ok(());
                }
            }
            0x8 => {
                write_websocket_close(&mut writer, 1000, "closing")?;
                return Ok(());
            }
            0x9 => write_websocket_frame(&mut writer, 0xA, &frame.payload)?,
            0xA => {}
            _ => {
                write_websocket_close(&mut writer, 1003, "unsupported frame")?;
                return Ok(());
            }
        }
    }
}

#[cfg(unix)]
fn serve_unix_socket(
    path: PathBuf,
    gateway: Arc<Mutex<AgencyGateway>>,
    shutdown: Arc<AtomicBool>,
    state_file: Option<PathBuf>,
) -> Result<()> {
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};

    if path.exists() {
        fs::remove_file(&path).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.unix_remove_stale",
                format!("remove stale Unix gateway socket {}: {error}", path.display()),
            )
        })?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.unix_directory",
                format!("create Unix gateway socket directory {}: {error}", parent.display()),
            )
        })?;
    }
    let listener = UnixListener::bind(&path).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.unix_bind",
            format!("bind Unix gateway socket {}: {error}", path.display()),
        )
    })?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.unix_permissions",
            format!("set Unix gateway socket permissions {}: {error}", path.display()),
        )
    })?;
    listener.set_nonblocking(true).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.unix_nonblocking",
            format!("configure Unix gateway socket {}: {error}", path.display()),
        )
    })?;

    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _address)) => {
                let gateway = Arc::clone(&gateway);
                let shutdown = Arc::clone(&shutdown);
                let state_file = state_file.clone();
                thread::spawn(move || {
                    let _ = handle_line_connection(stream, gateway, shutdown, state_file.as_deref());
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                let _ = fs::remove_file(&path);
                return Err(AikitError::new(
                    "agency_gateway_service.unix_accept",
                    format!("accept Unix gateway connection: {error}"),
                ));
            }
        }
    }
    drop(listener);
    let _ = fs::remove_file(path);
    Ok(())
}

fn handle_line_connection<S>(
    stream: S,
    gateway: Arc<Mutex<AgencyGateway>>,
    shutdown: Arc<AtomicBool>,
    state_file: Option<&Path>,
) -> Result<()>
where
    S: Read + Write + Send + 'static,
{
    // UnixStream/TcpStream cloning is intentionally avoided in this generic
    // carrier helper. Split reading/writing through one BufReader and its inner
    // stream so line framing remains deterministic.
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        let count = reader.read_line(&mut line).map_err(io_error("read gateway line"))?;
        if count == 0 {
            return Ok(());
        }
        if line.trim().is_empty() {
            continue;
        }
        let (response, should_shutdown) =
            execute_serialized_request(&gateway, line.trim_end(), state_file)?;
        {
            let stream = reader.get_mut();
            stream
                .write_all(response.as_bytes())
                .map_err(io_error("write gateway line response"))?;
            stream
                .write_all(b"\n")
                .map_err(io_error("write gateway line terminator"))?;
            stream.flush().map_err(io_error("flush gateway line response"))?;
        }
        if should_shutdown {
            shutdown.store(true, Ordering::SeqCst);
            return Ok(());
        }
    }
}

fn execute_serialized_request(
    gateway: &Arc<Mutex<AgencyGateway>>,
    input: &str,
    state_file: Option<&Path>,
) -> Result<(String, bool)> {
    let request = match serde_json::from_str::<GatewayRequestEnvelope>(input) {
        Ok(request) => request,
        Err(error) => {
            let response = serde_json::json!({
                "request_id": null,
                "ok": false,
                "error": {
                    "code": "agency_gateway.invalid_request_json",
                    "message": error.to_string()
                }
            });
            return Ok((response.to_string(), false));
        }
    };
    let should_shutdown = request.command.is_shutdown();
    let mut gateway = gateway.lock().map_err(|_| {
        AikitError::new(
            "agency_gateway_service.poisoned",
            "gateway state lock was poisoned",
        )
    })?;
    let response = GatewayResponseEnvelope::from_result(
        request.request_id,
        execute_gateway_command(&mut gateway, request.command),
    );
    if response.ok {
        persist_gateway_state(&gateway, state_file)?;
    }
    let encoded = serde_json::to_string(&response).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.response_encode",
            format!("encode gateway response: {error}"),
        )
    })?;
    Ok((encoded, should_shutdown))
}

fn websocket_handshake<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    expected_token: &str,
) -> Result<()> {
    let mut request_line = String::new();
    let mut consumed = reader
        .read_line(&mut request_line)
        .map_err(io_error("read WebSocket request line"))?;
    if consumed == 0 {
        return Err(AikitError::new(
            "agency_gateway_service.websocket_eof",
            "WebSocket peer closed before upgrade",
        ));
    }
    if !request_line.starts_with("GET ") || !request_line.contains(" HTTP/1.1") {
        write_http_error(writer, 400, "Bad Request")?;
        return Err(AikitError::new(
            "agency_gateway_service.websocket_request",
            "WebSocket upgrade must use HTTP/1.1 GET",
        ));
    }

    let mut upgrade = None;
    let mut connection = None;
    let mut version = None;
    let mut key = None;
    let mut authorization = None;
    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(io_error("read WebSocket upgrade header"))?;
        if bytes == 0 {
            return Err(AikitError::new(
                "agency_gateway_service.websocket_eof",
                "WebSocket peer closed during upgrade",
            ));
        }
        consumed += bytes;
        if consumed > MAX_HTTP_HEADER_BYTES {
            write_http_error(writer, 431, "Request Header Fields Too Large")?;
            return Err(AikitError::new(
                "agency_gateway_service.websocket_headers_too_large",
                "WebSocket upgrade headers exceed gateway limit",
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        match name.as_str() {
            "upgrade" => upgrade = Some(value),
            "connection" => connection = Some(value),
            "sec-websocket-version" => version = Some(value),
            "sec-websocket-key" => key = Some(value),
            "authorization" => authorization = Some(value),
            _ => {}
        }
    }

    let authorised = authorization
        .as_deref()
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| constant_time_eq(value.as_bytes(), expected_token.as_bytes()));
    if !authorised {
        write_http_error(writer, 401, "Unauthorized")?;
        return Err(AikitError::new(
            "agency_gateway_service.websocket_unauthorised",
            "gateway WebSocket bearer authentication failed",
        ));
    }
    if upgrade
        .as_deref()
        .is_none_or(|value| !value.eq_ignore_ascii_case("websocket"))
        || connection.as_deref().is_none_or(|value| {
            !value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        })
        || version.as_deref() != Some("13")
    {
        write_http_error(writer, 426, "Upgrade Required")?;
        return Err(AikitError::new(
            "agency_gateway_service.websocket_upgrade",
            "invalid WebSocket upgrade headers",
        ));
    }
    let key = key.ok_or_else(|| {
        AikitError::new(
            "agency_gateway_service.websocket_key",
            "WebSocket upgrade has no Sec-WebSocket-Key",
        )
    })?;
    let accept = websocket_accept(&key);
    write!(
        writer,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )
    .map_err(io_error("write WebSocket upgrade response"))?;
    writer
        .flush()
        .map_err(io_error("flush WebSocket upgrade response"))?;
    Ok(())
}

fn write_http_error<W: Write>(writer: &mut W, status: u16, reason: &str) -> Result<()> {
    write!(
        writer,
        "HTTP/1.1 {status} {reason}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    )
    .map_err(io_error("write HTTP error response"))?;
    writer.flush().map_err(io_error("flush HTTP error response"))?;
    Ok(())
}

#[derive(Debug)]
struct WebSocketFrame {
    opcode: u8,
    payload: Vec<u8>,
}

fn read_websocket_frame<R: Read>(reader: &mut R, max_frame_bytes: usize) -> Result<WebSocketFrame> {
    let mut head = [0u8; 2];
    match reader.read_exact(&mut head) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(AikitError::new(
                "agency_gateway_service.websocket_eof",
                "WebSocket peer closed",
            ))
        }
        Err(error) => return Err(io_error("read WebSocket frame header")(error)),
    }
    if head[0] & 0x70 != 0 || head[0] & 0x80 == 0 {
        return Err(AikitError::new(
            "agency_gateway_service.websocket_fragmentation",
            "gateway WebSocket accepts only final frames with no RSV extensions",
        ));
    }
    let opcode = head[0] & 0x0f;
    let masked = head[1] & 0x80 != 0;
    if !masked {
        return Err(AikitError::new(
            "agency_gateway_service.websocket_unmasked_client",
            "client WebSocket frames must be masked",
        ));
    }
    let mut length = (head[1] & 0x7f) as u64;
    if length == 126 {
        let mut bytes = [0u8; 2];
        reader
            .read_exact(&mut bytes)
            .map_err(io_error("read WebSocket 16-bit length"))?;
        length = u16::from_be_bytes(bytes) as u64;
    } else if length == 127 {
        let mut bytes = [0u8; 8];
        reader
            .read_exact(&mut bytes)
            .map_err(io_error("read WebSocket 64-bit length"))?;
        if bytes[0] & 0x80 != 0 {
            return Err(AikitError::new(
                "agency_gateway_service.websocket_length",
                "WebSocket payload length has invalid high bit",
            ));
        }
        length = u64::from_be_bytes(bytes);
    }
    if length > max_frame_bytes as u64 {
        return Err(AikitError::new(
            "agency_gateway_service.websocket_frame_too_large",
            format!(
                "WebSocket frame {length} bytes exceeds gateway limit {max_frame_bytes}"
            ),
        ));
    }
    if matches!(opcode, 0x8..=0xA) && length > 125 {
        return Err(AikitError::new(
            "agency_gateway_service.websocket_control_length",
            "WebSocket control frame payload exceeds 125 bytes",
        ));
    }
    let mut mask = [0u8; 4];
    reader
        .read_exact(&mut mask)
        .map_err(io_error("read WebSocket mask"))?;
    let mut payload = vec![0u8; length as usize];
    reader
        .read_exact(&mut payload)
        .map_err(io_error("read WebSocket payload"))?;
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
    Ok(WebSocketFrame { opcode, payload })
}

fn write_websocket_text<W: Write>(writer: &mut W, payload: &[u8]) -> Result<()> {
    write_websocket_frame(writer, 0x1, payload)
}

fn write_websocket_close<W: Write>(writer: &mut W, code: u16, reason: &str) -> Result<()> {
    let mut payload = code.to_be_bytes().to_vec();
    payload.extend_from_slice(reason.as_bytes());
    write_websocket_frame(writer, 0x8, &payload)
}

fn write_websocket_frame<W: Write>(writer: &mut W, opcode: u8, payload: &[u8]) -> Result<()> {
    writer
        .write_all(&[0x80 | (opcode & 0x0f)])
        .map_err(io_error("write WebSocket frame opcode"))?;
    match payload.len() {
        length if length < 126 => writer
            .write_all(&[length as u8])
            .map_err(io_error("write WebSocket frame length"))?,
        length if length <= u16::MAX as usize => {
            writer
                .write_all(&[126])
                .map_err(io_error("write WebSocket frame length marker"))?;
            writer
                .write_all(&(length as u16).to_be_bytes())
                .map_err(io_error("write WebSocket 16-bit length"))?;
        }
        length => {
            writer
                .write_all(&[127])
                .map_err(io_error("write WebSocket frame length marker"))?;
            writer
                .write_all(&(length as u64).to_be_bytes())
                .map_err(io_error("write WebSocket 64-bit length"))?;
        }
    }
    writer
        .write_all(payload)
        .map_err(io_error("write WebSocket frame payload"))?;
    writer.flush().map_err(io_error("flush WebSocket frame"))?;
    Ok(())
}

fn websocket_accept(key: &str) -> String {
    let mut input = Vec::with_capacity(key.len() + WEBSOCKET_GUID.len());
    input.extend_from_slice(key.as_bytes());
    input.extend_from_slice(WEBSOCKET_GUID.as_bytes());
    base64_encode(&sha1(&input))
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

// RFC 3174 SHA-1. SHA-1 is required by the RFC 6455 WebSocket handshake; it is
// not used here for credential hashing or any security decision.
fn sha1(input: &[u8]) -> [u8; 20] {
    let bit_len = (input.len() as u64) * 8;
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    let mut h0 = 0x67452301u32;
    let mut h1 = 0xEFCDAB89u32;
    let mut h2 = 0x98BADCFEu32;
    let mut h3 = 0x10325476u32;
    let mut h4 = 0xC3D2E1F0u32;

    for chunk in message.chunks_exact(64) {
        let mut words = [0u32; 80];
        for (index, bytes) in chunk.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes(bytes.try_into().expect("four-byte SHA-1 word"));
        }
        for index in 16..80 {
            words[index] = (words[index - 3]
                ^ words[index - 8]
                ^ words[index - 14]
                ^ words[index - 16])
                .rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;
        for (index, word) in words.iter().enumerate() {
            let (function, constant) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(function)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut output = [0u8; 20];
    for (index, word) in [h0, h1, h2, h3, h4].iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    output
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(c & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn io_error(context: &'static str) -> impl FnOnce(io::Error) -> AikitError {
    move |error| {
        AikitError::new(
            "agency_gateway_service.io",
            format!("{context}: {error}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        net::SocketAddr,
        sync::mpsc,
        time::{Duration, Instant},
    };

    use aikit_core::resource::ResourceRef;
    use serde_json::Value;

    fn gateway() -> AgencyGateway {
        AgencyGateway::new(ResourceRef::parse("agency-gateway/test").unwrap())
    }

    #[test]
    fn websocket_accept_matches_rfc_6455_reference_vector() {
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn sha1_matches_reference_digest() {
        assert_eq!(
            sha1(b"abc"),
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71,
                0x78, 0x50, 0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d,
            ]
        );
    }

    #[test]
    fn bearer_comparison_rejects_length_and_value_drift() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"sage"));
        assert!(!constant_time_eq(b"same", b"same-longer"));
    }

    #[test]
    fn semantic_state_round_trips_through_atomic_file_without_material_identity() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("gateway.json");
        let initial_gateway = gateway();
        persist_gateway_state(&initial_gateway, Some(&state)).unwrap();
        let encoded = fs::read_to_string(&state).unwrap();
        assert!(!encoded.contains("pid"));
        assert!(!encoded.contains("socket"));
        assert!(!encoded.contains("workcell"));
        let restored = restore_gateway_state(gateway(), Some(&state)).unwrap();
        assert_eq!(restored.status(), initial_gateway.status());
    }

    #[test]
    fn invalid_websocket_auth_is_rejected_before_upgrade() {
        let request = b"GET / HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: Bearer wrong\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let mut response = Vec::new();
        let error = websocket_handshake(&mut reader, &mut response, "correct").unwrap_err();
        assert_eq!(error.code(), "agency_gateway_service.websocket_unauthorised");
        assert!(String::from_utf8(response).unwrap().starts_with("HTTP/1.1 401"));
    }

    #[test]
    fn websocket_upgrade_accepts_authenticated_reference_handshake() {
        let request = b"GET /gateway HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: Bearer secret\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let mut response = Vec::new();
        websocket_handshake(&mut reader, &mut response, "secret").unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 101 Switching Protocols"));
        assert!(response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
    }

    #[test]
    fn masked_client_frame_decodes_and_server_frame_remains_unmasked() {
        let payload = br#"{"hello":"world"}"#;
        let mask = [1u8, 2, 3, 4];
        let mut frame = vec![0x81, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        for (index, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[index % 4]);
        }
        let decoded = read_websocket_frame(&mut &frame[..], 1024).unwrap();
        assert_eq!(decoded.opcode, 1);
        assert_eq!(decoded.payload, payload);

        let mut server = Vec::new();
        write_websocket_text(&mut server, payload).unwrap();
        assert_eq!(server[0], 0x81);
        assert_eq!(server[1] & 0x80, 0);
    }

    #[test]
    fn oversized_frame_is_rejected_before_payload_allocation() {
        let mut frame = vec![0x81, 0x80 | 126];
        frame.extend_from_slice(&5000u16.to_be_bytes());
        frame.extend_from_slice(&[1, 2, 3, 4]);
        let error = read_websocket_frame(&mut &frame[..], 1024).unwrap_err();
        assert_eq!(error.code(), "agency_gateway_service.websocket_frame_too_large");
    }

    fn masked_text_frame(text: &str) -> Vec<u8> {
        let payload = text.as_bytes();
        let mask = [5u8, 7, 11, 13];
        let mut frame = vec![0x81];
        if payload.len() < 126 {
            frame.push(0x80 | payload.len() as u8);
        } else {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        for (index, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[index % 4]);
        }
        frame
    }

    fn read_server_text(stream: &mut TcpStream) -> Value {
        let mut head = [0u8; 2];
        stream.read_exact(&mut head).unwrap();
        assert_eq!(head[0] & 0x0f, 1);
        assert_eq!(head[1] & 0x80, 0);
        let mut length = (head[1] & 0x7f) as usize;
        if length == 126 {
            let mut bytes = [0u8; 2];
            stream.read_exact(&mut bytes).unwrap();
            length = u16::from_be_bytes(bytes) as usize;
        }
        let mut payload = vec![0u8; length];
        stream.read_exact(&mut payload).unwrap();
        serde_json::from_slice(&payload).unwrap()
    }

    #[test]
    fn websocket_service_executes_protocol_and_shutdown_on_one_shared_gateway() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address: SocketAddr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let gateway = Arc::new(Mutex::new(gateway()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_gateway = Arc::clone(&gateway);
        let server_shutdown = Arc::clone(&shutdown);
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            let result = serve_websocket_listener(
                listener,
                server_gateway,
                server_shutdown,
                "secret".into(),
                None,
                64 * 1024,
            );
            done_tx.send(result).unwrap();
        });

        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(
            stream,
            "GET / HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: Bearer secret\r\n\r\n"
        )
        .unwrap();
        stream.flush().unwrap();
        let mut handshake = Vec::new();
        let mut last_four = [0u8; 4];
        while last_four != *b"\r\n\r\n" {
            let mut byte = [0u8; 1];
            stream.read_exact(&mut byte).unwrap();
            handshake.push(byte[0]);
            if handshake.len() >= 4 {
                last_four.copy_from_slice(&handshake[handshake.len() - 4..]);
            }
        }
        assert!(String::from_utf8(handshake).unwrap().starts_with("HTTP/1.1 101"));

        let protocol = serde_json::json!({
            "request_id":"p1",
            "command":{"type":"protocol"}
        })
        .to_string();
        stream.write_all(&masked_text_frame(&protocol)).unwrap();
        let response = read_server_text(&mut stream);
        assert_eq!(response["ok"], true);
        assert_eq!(response["request_id"], "p1");
        assert_eq!(response["response"]["type"], "protocol");

        let shutdown_request = serde_json::json!({
            "request_id":"stop",
            "command":{"type":"shutdown"}
        })
        .to_string();
        stream
            .write_all(&masked_text_frame(&shutdown_request))
            .unwrap();
        let response = read_server_text(&mut stream);
        assert_eq!(response["ok"], true);
        assert_eq!(response["response"]["type"], "shutdown");

        let deadline = Instant::now() + Duration::from_secs(3);
        while !shutdown.load(Ordering::SeqCst) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(shutdown.load(Ordering::SeqCst));
        done_rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_carrier_is_owner_only_and_executes_same_protocol() {
        use std::os::unix::{fs::PermissionsExt, net::UnixStream};

        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("gateway.sock");
        let state = root.path().join("gateway.json");
        let config = GatewayServiceConfig {
            websocket_bind: None,
            websocket_bearer_token: None,
            unix_socket: Some(socket.clone()),
            state_file: Some(state.clone()),
            max_frame_bytes: DEFAULT_GATEWAY_MAX_FRAME_BYTES,
        };
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || done_tx.send(run_gateway_service(gateway(), config)).unwrap());

        let deadline = Instant::now() + Duration::from_secs(3);
        while !socket.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(socket.exists());
        assert_eq!(fs::metadata(&socket).unwrap().permissions().mode() & 0o777, 0o600);

        let mut stream = UnixStream::connect(&socket).unwrap();
        writeln!(
            stream,
            "{}",
            serde_json::json!({"request_id":"p1","command":{"type":"protocol"}})
        )
        .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        let response: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(response["response"]["type"], "protocol");

        writeln!(
            stream,
            "{}",
            serde_json::json!({"request_id":"stop","command":{"type":"shutdown"}})
        )
        .unwrap();
        done_rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap();
        assert!(state.exists());
        assert!(!socket.exists());
    }
}
