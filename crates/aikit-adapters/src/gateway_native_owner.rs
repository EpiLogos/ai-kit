//! Transport to explicitly offered native World owners. This is an address
//! register, never a copy of their sources, sessions, documents or cursors.
//! Network access uses the gateway's existing authenticated carrier. The local
//! offer and socket are owner-only; each native owner checks its World and
//! incarnation before interpreting an operation. Uncertain effects are never
//! retried here.
use aikit_core::{AikitError, Result};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    time::Duration,
};

const MAX_BYTES: u64 = 1024 * 1024;
#[derive(Debug, Clone, Default)]
pub struct NativeOwnerRoutes {
    pub location: Option<PathBuf>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Register {
    schema: String,
    owners: Vec<Route>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    world_ref: String,
    socket: PathBuf,
}

impl NativeOwnerRoutes {
    pub fn from_env() -> Self {
        Self {
            location: std::env::var_os("AIKIT_GATEWAY_NATIVE_OWNERS").map(PathBuf::from),
        }
    }
    pub fn request(
        &self,
        world_ref: &str,
        generation: Option<&str>,
        request: Option<&Value>,
    ) -> Result<Value> {
        if !world_ref.starts_with("world:")
            || world_ref.len() <= 6
            || world_ref.trim() != world_ref
            || world_ref.len() > 512
        {
            return Err(fault(
                "gateway.native_owner.world_required",
                "An exact qualified World address is required",
            ));
        }
        if request.is_some() && generation.is_none_or(|v| v.is_empty()) {
            return Err(fault(
                "gateway.native_owner.generation_required",
                "Describe the native owner before operating on its exact generation",
            ));
        }
        let location = self.location.as_ref().ok_or_else(|| {
            fault(
                "gateway.native_owner.not_offered",
                "This gateway offers no native World owner transport",
            )
        })?;
        let metadata = fs::symlink_metadata(location).map_err(|_| {
            fault(
                "gateway.native_owner.offer_unavailable",
                "Native owner offer is unavailable",
            )
        })?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            return Err(fault(
                "gateway.native_owner.invalid_offer",
                "The native owner offer must be a bounded regular file",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.permissions().mode() & 0o077 != 0
                || metadata.uid() != rustix::process::geteuid().as_raw()
            {
                return Err(fault(
                    "gateway.native_owner.offer_permissions",
                    "The native owner offer must be owner-only",
                ));
            }
        }
        let register: Register = serde_json::from_slice(&fs::read(location).map_err(|_| {
            fault(
                "gateway.native_owner.offer_unavailable",
                "Native owner offer is unreadable",
            )
        })?)
        .map_err(|_| {
            fault(
                "gateway.native_owner.invalid_offer",
                "Native owner offer is invalid",
            )
        })?;
        if register.schema != "aikit.gateway-native-owners/v1" || register.owners.len() > 64 {
            return Err(fault(
                "gateway.native_owner.invalid_offer",
                "Unsupported native owner register",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for route in &register.owners {
            if !route.world_ref.starts_with("world:")
                || route.world_ref.len() <= 6
                || route.world_ref.len() > 512
                || route.world_ref.trim() != route.world_ref
                || !seen.insert(&route.world_ref)
                || !route.socket.is_absolute()
            {
                return Err(fault("gateway.native_owner.invalid_offer","Native owner offers require distinct qualified Worlds and absolute local sockets"));
            }
        }
        let route = register
            .owners
            .iter()
            .find(|r| r.world_ref == world_ref)
            .ok_or_else(|| {
                fault(
                    "gateway.native_owner.world_not_offered",
                    "This World has not offered a native operation route",
                )
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::{
                fs::{FileTypeExt, MetadataExt, PermissionsExt},
                net::UnixStream,
            };
            let socket_meta = fs::symlink_metadata(&route.socket)
                .map_err(|e| fault("gateway.native_owner.owner_unavailable", e.to_string()))?;
            if !socket_meta.file_type().is_socket()
                || socket_meta.permissions().mode() & 0o077 != 0
                || socket_meta.uid() != metadata.uid()
            {
                return Err(fault(
                    "gateway.native_owner.socket_permissions",
                    "The native owner socket must belong to the offer owner and be owner-only",
                ));
            }
            let mut socket = UnixStream::connect(&route.socket)
                .map_err(|e| fault("gateway.native_owner.owner_unavailable", e.to_string()))?;
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|e| fault("gateway.native_owner.owner_unavailable", e.to_string()))?;
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|e| fault("gateway.native_owner.owner_unavailable", e.to_string()))?;
            let command = match request {
                Some(request) => {
                    serde_json::json!({"operation":"apply","world_ref":world_ref,"expected_owner_generation":generation,"request":request})
                }
                None => serde_json::json!({"operation":"describe","world_ref":world_ref}),
            };
            let bytes = serde_json::to_vec(&command)
                .map_err(|e| fault("gateway.native_owner.request_encode", e.to_string()))?;
            if bytes.len() as u64 >= MAX_BYTES {
                return Err(fault(
                    "gateway.native_owner.request_limit",
                    "Native owner request exceeds the carrier bound",
                ));
            }
            let response_fault = |code: &'static str, message: &str| {
                if request.is_some() {
                    fault("gateway.native_owner.effect_uncertain",format!("{code}: {message}; an effect may have happened; reconcile with the native owner before replaying"))
                } else {
                    fault(code, message)
                }
            };
            socket
                .write_all(&bytes)
                .and_then(|_| socket.write_all(b"\n"))
                .map_err(|e| {
                    response_fault(
                        "gateway.native_owner.owner_unavailable",
                        &format!("Native request delivery failed: {e}"),
                    )
                })?;
            let mut line = String::new();
            BufReader::new(socket.take(MAX_BYTES + 1))
                .read_line(&mut line)
                .map_err(|e| {
                    response_fault(
                        "gateway.native_owner.owner_unavailable",
                        &format!("Native response was lost: {e}"),
                    )
                })?;
            if line.len() as u64 > MAX_BYTES || !line.ends_with('\n') {
                return Err(response_fault(
                    "gateway.native_owner.response_limit",
                    "Native response exceeds the bound or is incomplete",
                ));
            }
            let reply: Value = serde_json::from_str(&line).map_err(|_| {
                response_fault(
                    "gateway.native_owner.invalid_response",
                    "No complete native JSON envelope",
                )
            })?;
            if reply.get("ok").and_then(Value::as_bool) != Some(true) {
                let message = reply
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("No trustworthy native outcome");
                let pre_effect = [
                    "native_owner.wrong_world:",
                    "native_owner.stale_generation:",
                    "native_owner.operation_refused:",
                    "native_owner.grant_refused:",
                ]
                .iter()
                .any(|prefix| message.starts_with(prefix));
                return Err(if pre_effect {
                    fault("gateway.native_owner.owner_refused", message)
                } else {
                    response_fault("gateway.native_owner.owner_error", message)
                });
            }
            let value = reply.get("outcome").cloned().ok_or_else(|| {
                response_fault(
                    "gateway.native_owner.invalid_response",
                    "Native owner returned no outcome",
                )
            })?;
            if value.get("world_ref").and_then(Value::as_str) != Some(world_ref)
                || value
                    .get("owner_generation")
                    .and_then(Value::as_str)
                    .is_none_or(|v| v.is_empty())
            {
                return Err(response_fault(
                    "gateway.native_owner.wrong_owner",
                    "Response belongs to another or unqualified owner",
                ));
            }
            if let Some(expected) = generation {
                if value.get("owner_generation").and_then(Value::as_str) != Some(expected) {
                    return Err(response_fault(
                        "gateway.native_owner.wrong_generation",
                        "Response belongs to another owner generation",
                    ));
                }
            }
            Ok(value)
        }
        #[cfg(not(unix))]
        {
            let _ = (route, generation, request);
            Err(fault(
                "gateway.native_owner.unavailable",
                "Native owner transport requires the owner's local Unix socket",
            ))
        }
    }
}
fn fault(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message)
}
