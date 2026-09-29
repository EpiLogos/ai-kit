//! The knowledge result cache: AIKit's knowledge answers, remembered in the
//! suite's own Redis under the `knowledge` key family.
//!
//! Knowledge materialisation rebuilds the whole read horizon — file maps,
//! Central wiki, entities, matrices, source pools, the wiki index — for every
//! CLI invocation, because no index state survives between processes, and a
//! Central-backed horizon is deliberately invalidated between calls within
//! one process. This cache lets a repeat read answer from Redis without that
//! rebuild. The caller keys each operation by what was asked *and* the basis
//! it was asked against; a basis change is a different key, so invalidation
//! is not a delete but a miss, and superseded entries age out under their TTL.
//!
//! This module owns transport only. What belongs in a basis is the caller's
//! judgement; the store never guesses it. It consumes the same
//! `aikit.redis-now-config/v1` document as the NOW context (same instance,
//! same auth, same bounded limits) and speaks the same hand-written RESP2 —
//! no new dependencies.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use aikit_core::{AikitError, Result, SecretValue};

use crate::now_context::RedisNowConfig;

/// The key family this cache owns inside the shared Redis namespace.
pub const KNOWLEDGE_CACHE_KEY_FAMILY: &str = "knowledge";

/// Bounds of one cached entry's lifetime. The basis does the real
/// invalidation; the TTL only bounds what an unseen input change can leave
/// stale (an input the caller's basis does not stat).
pub const MIN_TTL_SECONDS: u64 = 10;
pub const MAX_TTL_SECONDS: u64 = 24 * 60 * 60;

/// One operation key is a caller-composed string; it is never stored raw, so
/// its bound is about abuse rather than protocol limits.
const MAX_OPERATION_KEY_BYTES: usize = 4096;
/// A cached payload larger than this is refused rather than stored: knowledge
/// answers are bounded projections, and an oversized one means the caller
/// mis-keyed a corpus, not a projection.
const MAX_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;

fn fail(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message)
}

/// Availability of the knowledge cache, as `knowledge status` discloses it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KnowledgeCacheStatus {
    pub available: bool,
    pub address: String,
    pub database: u8,
    pub key_prefix: String,
}

#[derive(Clone, Debug)]
pub struct KnowledgeCacheStore {
    config: RedisNowConfig,
}

impl KnowledgeCacheStore {
    pub fn new(config: RedisNowConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &RedisNowConfig {
        &self.config
    }

    /// The Redis key one operation answers under: the family and the
    /// operation key hashed into the configured namespace, so no caller
    /// string ever becomes a live key name.
    pub fn key(&self, operation_key: &str) -> String {
        format!(
            "{}:{}:{}",
            self.config.key_prefix,
            KNOWLEDGE_CACHE_KEY_FAMILY,
            blake3::hash(operation_key.as_bytes()).to_hex()
        )
    }

    pub fn status(&self, secret: Option<&SecretValue>) -> Result<KnowledgeCacheStatus> {
        expect_ok(self.command(secret, vec![b"PING".to_vec()])?)?;
        Ok(KnowledgeCacheStatus {
            available: true,
            address: self.config.address.clone(),
            database: self.config.database,
            key_prefix: self.config.key_prefix.clone(),
        })
    }

    /// The cached payload for one operation, `None` on a miss. A payload that
    /// fails to decode as UTF-8 is a corrupt entry, not a miss: it is named
    /// so the caller can disclose the degradation and recompute.
    pub fn get(&self, secret: Option<&SecretValue>, operation_key: &str) -> Result<Option<String>> {
        self.bounds_check(operation_key)?;
        let key = self.key(operation_key);
        match self.command(secret, vec![b"GET".to_vec(), key.into_bytes()])? {
            Resp::Bulk(None) => Ok(None),
            value => Ok(Some(bulk_utf8(value)?)),
        }
    }

    /// Remember one operation's payload under its operation key until the TTL
    /// elapses. Writing is idempotent; the last writer of a basis wins and
    /// the payloads agree because the inputs were the same basis.
    pub fn put(
        &self,
        secret: Option<&SecretValue>,
        operation_key: &str,
        payload: &str,
        ttl_seconds: u64,
    ) -> Result<()> {
        self.bounds_check(operation_key)?;
        if payload.is_empty() || payload.len() > MAX_PAYLOAD_BYTES {
            return Err(fail(
                "knowledge_cache.payload_bounds",
                format!(
                    "cached payload must be 1..={} bytes, got {}",
                    MAX_PAYLOAD_BYTES,
                    payload.len()
                ),
            ));
        }
        if !(MIN_TTL_SECONDS..=MAX_TTL_SECONDS).contains(&ttl_seconds) {
            return Err(fail(
                "knowledge_cache.ttl_bounds",
                format!(
                    "TTL must be {}..={} seconds, got {}",
                    MIN_TTL_SECONDS, MAX_TTL_SECONDS, ttl_seconds
                ),
            ));
        }
        let key = self.key(operation_key);
        expect_ok(self.command(
            secret,
            vec![
                b"SET".to_vec(),
                key.into_bytes(),
                payload.as_bytes().to_vec(),
                b"EX".to_vec(),
                ttl_seconds.to_string().into_bytes(),
            ],
        )?)?;
        Ok(())
    }

    fn bounds_check(&self, operation_key: &str) -> Result<()> {
        if operation_key.is_empty() || operation_key.len() > MAX_OPERATION_KEY_BYTES {
            return Err(fail(
                "knowledge_cache.operation_key_bounds",
                format!(
                    "operation key must be 1..={} bytes, got {}",
                    MAX_OPERATION_KEY_BYTES,
                    operation_key.len()
                ),
            ));
        }
        Ok(())
    }

    /// One stateless connection per command, exactly as the NOW context
    /// speaks: resolve, refuse remote without explicit allowance, authenticate
    /// through the materialised secret, select the database, and hand the
    /// caller a stream bounded by the configured I/O timeouts.
    fn connect(&self, secret: Option<&SecretValue>) -> Result<TcpStream> {
        let mut addresses = self
            .config
            .address
            .to_socket_addrs()
            .map_err(|e| fail("knowledge_cache.redis_address", e.to_string()))?
            .collect::<Vec<_>>();
        addresses.sort();
        addresses.dedup();
        let address = addresses.first().copied().ok_or_else(|| {
            fail(
                "knowledge_cache.redis_address",
                "Redis address resolved to no endpoints",
            )
        })?;
        if !self.config.allow_remote && addresses.iter().any(|a| !a.ip().is_loopback()) {
            return Err(fail(
                "knowledge_cache.redis_remote_denied",
                "Remote Redis requires explicit allow_remote configuration",
            ));
        }
        if addresses.iter().any(|a| !a.ip().is_loopback()) && self.config.credential_ref.is_none() {
            return Err(fail(
                "knowledge_cache.redis_auth_required",
                "Remote Redis requires a native credential reference",
            ));
        }
        let mut stream = TcpStream::connect_timeout(
            &address,
            Duration::from_millis(self.config.connect_timeout_ms),
        )
        .map_err(|e| fail("knowledge_cache.redis_unavailable", e.to_string()))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(self.config.io_timeout_ms)))
            .map_err(|e| fail("knowledge_cache.redis_io", e.to_string()))?;
        stream
            .set_write_timeout(Some(Duration::from_millis(self.config.io_timeout_ms)))
            .map_err(|e| fail("knowledge_cache.redis_io", e.to_string()))?;
        match (&self.config.credential_ref, secret) {
            (Some(_), Some(secret)) => {
                let mut args = vec![b"AUTH".to_vec()];
                if let Some(user) = &self.config.username {
                    args.push(user.as_bytes().to_vec());
                }
                args.push(secret.expose().as_bytes().to_vec());
                expect_ok(send(&mut stream, &args)?)?;
            }
            (Some(_), None) => {
                return Err(fail(
                    "knowledge_cache.redis_credential_missing",
                    "Configured Redis credential was not materialised",
                ))
            }
            (None, Some(_)) => {
                return Err(fail(
                    "knowledge_cache.redis_credential_unexpected",
                    "Redis secret material was supplied without a credential reference",
                ))
            }
            (None, None) => {}
        }
        if self.config.database != 0 {
            expect_ok(send(
                &mut stream,
                &[
                    b"SELECT".to_vec(),
                    self.config.database.to_string().into_bytes(),
                ],
            )?)?;
        }
        Ok(stream)
    }

    fn command(&self, secret: Option<&SecretValue>, args: Vec<Vec<u8>>) -> Result<Resp> {
        let mut stream = self.connect(secret)?;
        send(&mut stream, &args)
    }
}

/// The RESP2 responses this cache reads. Kept local to the family: the NOW
/// context maps server errors to NOW-specific codes, and this family must
/// name its own.
#[derive(Debug)]
enum Resp {
    Simple(String),
    Bulk(Option<Vec<u8>>),
    Integer(i64),
}

fn expect_ok(value: Resp) -> Result<()> {
    match value {
        Resp::Simple(v) if v == "OK" || v == "PONG" => Ok(()),
        other => Err(fail(
            "knowledge_cache.redis_protocol",
            format!("Unexpected Redis response: {other:?}"),
        )),
    }
}

fn bulk_utf8(value: Resp) -> Result<String> {
    match value {
        Resp::Bulk(Some(v)) => {
            String::from_utf8(v).map_err(|e| fail("knowledge_cache.redis_protocol", e.to_string()))
        }
        Resp::Simple(v) => Ok(v),
        Resp::Integer(v) => Ok(v.to_string()),
        Resp::Bulk(None) => Err(fail("knowledge_cache.redis_missing", "Redis returned nil")),
    }
}

fn send(stream: &mut TcpStream, args: &[Vec<u8>]) -> Result<Resp> {
    let mut request = format!("*{}\r\n", args.len()).into_bytes();
    for arg in args {
        request.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
        request.extend_from_slice(arg);
        request.extend_from_slice(b"\r\n");
    }
    stream
        .write_all(&request)
        .map_err(|e| fail("knowledge_cache.redis_io", e.to_string()))?;
    stream
        .flush()
        .map_err(|e| fail("knowledge_cache.redis_io", e.to_string()))?;
    read_resp(stream)
}

fn read_resp(stream: &mut TcpStream) -> Result<Resp> {
    let mut prefix = [0u8; 1];
    stream
        .read_exact(&mut prefix)
        .map_err(|e| fail("knowledge_cache.redis_io", e.to_string()))?;
    match prefix[0] {
        b'+' => Ok(Resp::Simple(read_line(stream, 64 * 1024)?)),
        b'-' => Err(fail(
            "knowledge_cache.redis_error",
            read_line(stream, 64 * 1024)?,
        )),
        b':' => {
            let n = read_line(stream, 64)?
                .parse::<i64>()
                .map_err(|_| fail("knowledge_cache.redis_protocol", "Invalid Redis integer"))?;
            Ok(Resp::Integer(n))
        }
        b'$' => {
            let len = read_line(stream, 64)?
                .parse::<i64>()
                .map_err(|_| fail("knowledge_cache.redis_protocol", "Invalid bulk length"))?;
            if len < 0 {
                // RESP2 spells a null bulk as `$-1`: a miss, not a fault.
                return Ok(Resp::Bulk(None));
            }
            if len as u64 > MAX_PAYLOAD_BYTES as u64 + 1024 {
                return Err(fail(
                    "knowledge_cache.redis_protocol",
                    format!("Bulk response of {len} bytes exceeds bounded payload size"),
                ));
            }
            let len = len as usize;
            let mut buffer = vec![0u8; len + 2];
            stream
                .read_exact(&mut buffer)
                .map_err(|e| fail("knowledge_cache.redis_io", e.to_string()))?;
            buffer.truncate(len);
            Ok(Resp::Bulk(Some(buffer)))
        }
        other => Err(fail(
            "knowledge_cache.redis_protocol",
            format!("Unsupported RESP2 prefix {other:?}"),
        )),
    }
}

fn read_line(stream: &mut TcpStream, limit: usize) -> Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream
            .read_exact(&mut byte)
            .map_err(|e| fail("knowledge_cache.redis_io", e.to_string()))?;
        if byte[0] == b'\n' {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return String::from_utf8(line)
                .map_err(|e| fail("knowledge_cache.redis_protocol", e.to_string()));
        }
        line.push(byte[0]);
        if line.len() > limit {
            return Err(fail(
                "knowledge_cache.redis_protocol",
                format!("Redis line exceeded {limit} bytes"),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::now_context::NOW_REDIS_CONFIG_SCHEMA;

    fn config() -> RedisNowConfig {
        RedisNowConfig {
            schema: NOW_REDIS_CONFIG_SCHEMA.into(),
            address: "127.0.0.1:6381".into(),
            database: 0,
            key_prefix: format!("aikit-knowledge-test-{}", ulid::Ulid::generate()),
            username: None,
            credential_ref: None,
            allow_remote: false,
            connect_timeout_ms: 1000,
            io_timeout_ms: 1000,
            prepared_ttl_seconds: 3600,
            coordination_retention_seconds: 3600,
        }
    }

    #[test]
    fn a_key_is_namespaced_deterministically_and_never_carries_the_operation_key() {
        let store = KnowledgeCacheStore::new(config()).unwrap();
        let first = store.key("resolve\x1fsubject/gateway\x1f7");
        let second = store.key("resolve\x1fsubject/gateway\x1f7");
        assert_eq!(first, second);
        assert!(
            !first.contains("gateway"),
            "raw operation key material must not become a live key name"
        );
        assert!(first.starts_with("aikit-knowledge-test-"));
        assert!(first.contains(":knowledge:"));
        assert_ne!(store.key("other"), first);
    }

    #[test]
    fn out_of_bounds_requests_are_refused_before_any_socket_is_opened() {
        let store = KnowledgeCacheStore::new(config()).unwrap();
        let long = "x".repeat(MAX_OPERATION_KEY_BYTES + 1);
        assert_eq!(
            store.get(None, "").unwrap_err().code(),
            "knowledge_cache.operation_key_bounds"
        );
        assert_eq!(
            store.get(None, &long).unwrap_err().code(),
            "knowledge_cache.operation_key_bounds"
        );
        assert_eq!(
            store
                .put(None, "k", "payload", MIN_TTL_SECONDS - 1)
                .unwrap_err()
                .code(),
            "knowledge_cache.ttl_bounds"
        );
        assert_eq!(
            store
                .put(None, "k", "payload", MAX_TTL_SECONDS + 1)
                .unwrap_err()
                .code(),
            "knowledge_cache.ttl_bounds"
        );
        assert_eq!(
            store
                .put(None, "k", "", MIN_TTL_SECONDS)
                .unwrap_err()
                .code(),
            "knowledge_cache.payload_bounds"
        );
        let oversized = "x".repeat(MAX_PAYLOAD_BYTES + 1);
        assert_eq!(
            store
                .put(None, "k", &oversized, MIN_TTL_SECONDS)
                .unwrap_err()
                .code(),
            "knowledge_cache.payload_bounds"
        );
    }

    #[test]
    fn an_unreachable_redis_is_a_named_failure_not_a_hang() {
        // Port 1 on the loopback refuses immediately; the store must surface
        // the refusal under the family's own code.
        let mut cfg = config();
        cfg.address = "127.0.0.1:1".into();
        let store = KnowledgeCacheStore::new(cfg).unwrap();
        let error = store.get(None, "k").unwrap_err();
        assert_eq!(error.code(), "knowledge_cache.redis_unavailable");
    }
}
