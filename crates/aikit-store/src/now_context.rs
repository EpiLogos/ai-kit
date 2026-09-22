//! Redis-backed hot operative context for the existing NOW relation.
//!
//! Canonical sources, Wiki knowledge, Factory evidence and SessionSpace state
//! remain with their native owners. This module stores only prepared participant
//! views, replayable change cursors and last-delivery state. It speaks RESP2
//! directly so the Redis boundary adds no second runtime/service dependency.
use aikit_core::context_source::{AgentVisibility, ExternalEgress};
use aikit_core::secret_ref::SecretRef;
use aikit_core::{AikitError, ResourceRef, Result, SecretValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const NOW_REDIS_CONFIG_SCHEMA: &str = "aikit.redis-now-config/v1";
pub const NOW_PREPARED_SCHEMA: &str = "aikit.prepared-now-context/v1";
pub const NOW_DELIVERY_SCHEMA: &str = "aikit.now-context-delivery/v1";
const MAX_JSON: usize = 1024 * 1024;
const MAX_ITEMS: usize = 64;
const MAX_NEIGHBOURS: usize = 64;
const MAX_CHANGES: usize = 64;

fn fail(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message.into())
}
fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}
fn digest<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|e| fail("now_context.encode", e.to_string()))?;
    Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedisNowConfig {
    pub schema: String,
    /// host:port only. Secrets never appear in an endpoint string.
    pub address: String,
    #[serde(default)]
    pub database: u8,
    pub key_prefix: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub credential_ref: Option<SecretRef>,
    #[serde(default)]
    pub allow_remote: bool,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_ms: u64,
    #[serde(default = "default_io_timeout")]
    pub io_timeout_ms: u64,
    #[serde(default = "default_prepared_ttl")]
    pub prepared_ttl_seconds: u64,
    #[serde(default = "default_coordination_retention")]
    pub coordination_retention_seconds: u64,
}
fn default_connect_timeout() -> u64 {
    500
}
fn default_io_timeout() -> u64 {
    1000
}
fn default_prepared_ttl() -> u64 {
    6 * 60 * 60
}
fn default_coordination_retention() -> u64 {
    14 * 24 * 60 * 60
}
impl RedisNowConfig {
    pub fn validate(&self) -> Result<()> {
        if self.schema != NOW_REDIS_CONFIG_SCHEMA {
            return Err(fail(
                "now_context.config_schema",
                "Unsupported Redis NOW configuration schema",
            ));
        }
        if !bounded(&self.address, 512)
            || self.address.contains("://")
            || !bounded(&self.key_prefix, 128)
            || !self
                .key_prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-:".contains(&b))
            || self.username.as_ref().is_some_and(|v| !bounded(v, 256))
            || !(50..=30_000).contains(&self.connect_timeout_ms)
            || !(50..=30_000).contains(&self.io_timeout_ms)
            || !(60..=7 * 24 * 60 * 60).contains(&self.prepared_ttl_seconds)
            || !(60..=90 * 24 * 60 * 60).contains(&self.coordination_retention_seconds)
        {
            return Err(fail(
                "now_context.config_invalid",
                "Redis NOW configuration is outside bounded native limits",
            ));
        }
        if self.username.is_some() && self.credential_ref.is_none() {
            return Err(fail(
                "now_context.config_invalid",
                "Redis ACL username requires a credential reference",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NowContextBasis {
    #[serde(default)]
    pub source_revisions: BTreeMap<String, String>,
    #[serde(default)]
    pub dependency_revisions: BTreeMap<String, String>,
    pub disclosure_revision: String,
    #[serde(default)]
    pub factory_revision: Option<String>,
    #[serde(default)]
    pub change_cursor: u64,
}
impl NowContextBasis {
    pub fn digest(&self) -> Result<String> {
        digest(self)
    }
    fn validate(&self) -> Result<()> {
        if !bounded(&self.disclosure_revision, 4096)
            || self
                .factory_revision
                .as_ref()
                .is_some_and(|v| !bounded(v, 4096))
            || self.source_revisions.len() > 256
            || self.dependency_revisions.len() > 256
            || self
                .source_revisions
                .iter()
                .chain(self.dependency_revisions.iter())
                .any(|(k, v)| !bounded(k, 4096) || !bounded(v, 4096))
        {
            return Err(fail(
                "now_context.basis_invalid",
                "Prepared NOW basis is malformed or unbounded",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NowContextItem {
    pub source_ref: ResourceRef,
    pub source_revision: String,
    pub title: String,
    pub excerpt: String,
    #[serde(default)]
    pub route: Option<String>,
    pub agent_visibility: AgentVisibility,
    pub external_egress: ExternalEgress,
}
impl NowContextItem {
    fn validate(&self, external_provider: bool) -> Result<()> {
        ResourceRef::parse(self.source_ref.as_str())?;
        if !bounded(&self.source_revision, 4096)
            || !bounded(&self.title, 4096)
            || !bounded(&self.excerpt, 256 * 1024)
            || self.route.as_ref().is_some_and(|v| !bounded(v, 16 * 1024))
        {
            return Err(fail(
                "now_context.item_invalid",
                "Prepared source material is malformed or unbounded",
            ));
        }
        if self.agent_visibility != AgentVisibility::Payload {
            return Err(fail(
                "now_context.disclosure_denied",
                "Prepared material is not payload-visible to this agent",
            ));
        }
        if external_provider && self.external_egress != ExternalEgress::Allowed {
            return Err(fail(
                "now_context.egress_denied",
                "Prepared material is not permitted to leave the local egress boundary",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NowNeighbour {
    pub participant_ref: ResourceRef,
    pub task_ref: ResourceRef,
    pub relation: String,
    #[serde(default)]
    pub dependency_revision: Option<String>,
    #[serde(default)]
    pub write_scope: Vec<String>,
    #[serde(default)]
    pub returned_refs: Vec<ResourceRef>,
}
impl NowNeighbour {
    fn validate(&self) -> Result<()> {
        ResourceRef::parse(self.participant_ref.as_str())?;
        ResourceRef::parse(self.task_ref.as_str())?;
        if !bounded(&self.relation, 256)
            || self
                .dependency_revision
                .as_ref()
                .is_some_and(|v| !bounded(v, 4096))
            || self.write_scope.len() > 64
            || self.write_scope.iter().any(|v| !bounded(v, 4096))
            || self.returned_refs.len() > 64
        {
            return Err(fail(
                "now_context.neighbour_invalid",
                "NOW neighbour relation is malformed or unbounded",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedNowContext {
    pub schema: String,
    pub project_ref: ResourceRef,
    pub now_ref: ResourceRef,
    pub participant_ref: ResourceRef,
    pub agent_session: ResourceRef,
    pub version: u64,
    pub basis: NowContextBasis,
    pub basis_digest: String,
    pub concern: String,
    #[serde(default)]
    pub practice_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub items: Vec<NowContextItem>,
    #[serde(default)]
    pub neighbours: Vec<NowNeighbour>,
    #[serde(default)]
    pub continuation: Option<String>,
    #[serde(default)]
    pub jev_invocation_ref: Option<ResourceRef>,
    pub prepared_at_unix_ms: u64,
}
impl PreparedNowContext {
    pub fn validate(&self, external_provider: bool) -> Result<()> {
        if self.schema != NOW_PREPARED_SCHEMA || self.version == 0 {
            return Err(fail(
                "now_context.prepared_schema",
                "Unsupported or unversioned prepared NOW context",
            ));
        }
        for id in [
            &self.project_ref,
            &self.now_ref,
            &self.participant_ref,
            &self.agent_session,
        ] {
            ResourceRef::parse(id.as_str())?;
        }
        self.basis.validate()?;
        if self.basis_digest != self.basis.digest()?
            || !bounded(&self.concern, 64 * 1024)
            || self.practice_refs.len() > 64
            || self.items.len() > MAX_ITEMS
            || self.neighbours.len() > MAX_NEIGHBOURS
            || self
                .continuation
                .as_ref()
                .is_some_and(|v| !bounded(v, 128 * 1024))
        {
            return Err(fail(
                "now_context.prepared_invalid",
                "Prepared NOW context failed its exact basis or size checks",
            ));
        }
        for item in &self.items {
            item.validate(external_provider)?;
        }
        for relation in &self.neighbours {
            relation.validate()?;
        }
        let encoded =
            serde_json::to_vec(self).map_err(|e| fail("now_context.encode", e.to_string()))?;
        if encoded.len() > MAX_JSON {
            return Err(fail(
                "now_context.prepared_too_large",
                "Prepared NOW context exceeds 1 MiB",
            ));
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<String> {
        digest(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NowContextChange {
    pub change_id: String,
    pub kind: String,
    pub source_ref: ResourceRef,
    pub source_revision: String,
    pub detail: String,
    pub observed_at_unix_ms: u64,
}
impl NowContextChange {
    fn validate(&self) -> Result<()> {
        ResourceRef::parse(self.source_ref.as_str())?;
        if !bounded(&self.change_id, 512)
            || !bounded(&self.kind, 256)
            || !bounded(&self.source_revision, 4096)
            || !bounded(&self.detail, 128 * 1024)
        {
            return Err(fail(
                "now_context.change_invalid",
                "NOW change is malformed or unbounded",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorChange {
    pub cursor: u64,
    pub change: NowContextChange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NowDeliveryReceipt {
    pub schema: String,
    pub participant_ref: ResourceRef,
    pub agent_session: ResourceRef,
    pub prepared_version: u64,
    pub prepared_digest: String,
    pub basis_digest: String,
    pub change_cursor: u64,
    pub delivered_at_unix_ms: u64,
}
impl NowDeliveryReceipt {
    pub fn validate(&self) -> Result<()> {
        if self.schema != NOW_DELIVERY_SCHEMA
            || self.prepared_version == 0
            || !bounded(&self.prepared_digest, 128)
            || !bounded(&self.basis_digest, 128)
        {
            return Err(fail(
                "now_context.delivery_invalid",
                "NOW delivery receipt is malformed",
            ));
        }
        ResourceRef::parse(self.participant_ref.as_str())?;
        ResourceRef::parse(self.agent_session.as_str())?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedisNowStatus {
    pub available: bool,
    pub redis_version: Option<String>,
    pub address: String,
    pub database: u8,
    pub key_prefix: String,
}

#[derive(Clone, Debug)]
pub struct RedisNowStore {
    config: RedisNowConfig,
}
impl RedisNowStore {
    pub fn new(config: RedisNowConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }
    pub fn config(&self) -> &RedisNowConfig {
        &self.config
    }
    fn key(&self, family: &str, participant: &ResourceRef) -> String {
        format!(
            "{}:{}:{}",
            self.config.key_prefix,
            family,
            blake3::hash(participant.as_str().as_bytes()).to_hex()
        )
    }
    fn change_key_prefix(&self, participant: &ResourceRef) -> String {
        format!("{}:change:", self.key("changes", participant))
    }
    fn connect(&self, secret: Option<&SecretValue>) -> Result<TcpStream> {
        let mut addresses = self
            .config
            .address
            .to_socket_addrs()
            .map_err(|e| fail("now_context.redis_address", e.to_string()))?
            .collect::<Vec<_>>();
        addresses.sort();
        addresses.dedup();
        let address = addresses.first().copied().ok_or_else(|| {
            fail(
                "now_context.redis_address",
                "Redis address resolved to no endpoints",
            )
        })?;
        if !self.config.allow_remote && addresses.iter().any(|a| !a.ip().is_loopback()) {
            return Err(fail(
                "now_context.redis_remote_denied",
                "Remote Redis requires explicit allow_remote configuration",
            ));
        }
        if addresses.iter().any(|a| !a.ip().is_loopback()) && self.config.credential_ref.is_none() {
            return Err(fail(
                "now_context.redis_auth_required",
                "Remote Redis requires a native credential reference",
            ));
        }
        let mut stream = TcpStream::connect_timeout(
            &address,
            Duration::from_millis(self.config.connect_timeout_ms),
        )
        .map_err(|e| fail("now_context.redis_unavailable", e.to_string()))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(self.config.io_timeout_ms)))
            .map_err(|e| fail("now_context.redis_io", e.to_string()))?;
        stream
            .set_write_timeout(Some(Duration::from_millis(self.config.io_timeout_ms)))
            .map_err(|e| fail("now_context.redis_io", e.to_string()))?;
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
                    "now_context.redis_credential_missing",
                    "Configured Redis credential was not materialised",
                ))
            }
            (None, Some(_)) => {
                return Err(fail(
                    "now_context.redis_credential_unexpected",
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
    pub fn status(&self, secret: Option<&SecretValue>) -> Result<RedisNowStatus> {
        expect_ok(self.command(secret, vec![b"PING".to_vec()])?)?;
        let info = bulk_utf8(self.command(secret, vec![b"INFO".to_vec(), b"server".to_vec()])?)?;
        let version = info.lines().find_map(|line| {
            line.strip_prefix("redis_version:")
                .map(|v| v.trim().to_owned())
        });
        Ok(RedisNowStatus {
            available: true,
            redis_version: version,
            address: self.config.address.clone(),
            database: self.config.database,
            key_prefix: self.config.key_prefix.clone(),
        })
    }
    pub fn current_version(
        &self,
        participant: &ResourceRef,
        secret: Option<&SecretValue>,
    ) -> Result<u64> {
        let meta = self.key("meta", participant);
        match self.command(secret, vec![b"GET".to_vec(), meta.into_bytes()])? {
            Resp::Nil => Ok(0),
            value => {
                let raw = bulk_utf8(value)?;
                let v: serde_json::Value = serde_json::from_str(&raw)
                    .map_err(|e| fail("now_context.redis_corrupt", e.to_string()))?;
                v.get("version").and_then(|v| v.as_u64()).ok_or_else(|| {
                    fail(
                        "now_context.redis_corrupt",
                        "Prepared NOW metadata has no version",
                    )
                })
            }
        }
    }
    pub fn publish(
        &self,
        view: &PreparedNowContext,
        expected_version: u64,
        secret: Option<&SecretValue>,
    ) -> Result<u64> {
        view.validate(false)?;
        if view.version
            != expected_version.checked_add(1).ok_or_else(|| {
                fail(
                    "now_context.version_exhausted",
                    "Prepared NOW version exhausted",
                )
            })?
        {
            return Err(fail(
                "now_context.version_invalid",
                "Published prepared version must be exactly expected + 1",
            ));
        }
        let participant = &view.participant_ref;
        let prepared = self.key("prepared", participant);
        let meta = self.key("meta", participant);
        let revoked = self.key("revoked", participant);
        let body =
            serde_json::to_vec(view).map_err(|e| fail("now_context.encode", e.to_string()))?;
        let meta_body=serde_json::to_vec(&serde_json::json!({"version":view.version,"basis_digest":view.basis_digest,"prepared_digest":view.digest()?})).map_err(|e|fail("now_context.encode",e.to_string()))?;
        let script = r#"local m=redis.call('GET',KEYS[1]); local v=0; if m then local ok,o=pcall(cjson.decode,m); if not ok or not o.version then return redis.error_reply('CORRUPT') end; v=tonumber(o.version) end; if v~=tonumber(ARGV[1]) then return redis.error_reply('STALE') end; local r=redis.call('GET',KEYS[2]); if r and r==ARGV[2] then return redis.error_reply('REVOKED') end; redis.call('SET',KEYS[3],ARGV[3],'EX',ARGV[4]); redis.call('SET',KEYS[1],ARGV[5]); return tonumber(ARGV[6])"#;
        let result = self.command(
            secret,
            vec![
                b"EVAL".to_vec(),
                script.as_bytes().to_vec(),
                b"3".to_vec(),
                meta.into_bytes(),
                revoked.into_bytes(),
                prepared.into_bytes(),
                expected_version.to_string().into_bytes(),
                view.basis.disclosure_revision.as_bytes().to_vec(),
                body,
                self.config.prepared_ttl_seconds.to_string().into_bytes(),
                meta_body,
                view.version.to_string().into_bytes(),
            ],
        );
        map_eval_version(result)
    }
    pub fn read_prepared(
        &self,
        participant: &ResourceRef,
        external_provider: bool,
        secret: Option<&SecretValue>,
    ) -> Result<Option<PreparedNowContext>> {
        let prepared = self.key("prepared", participant);
        let revoked = self.key("revoked", participant);
        let revoked_value =
            match self.command(secret, vec![b"GET".to_vec(), revoked.into_bytes()])? {
                Resp::Bulk(Some(value)) => Some(value),
                Resp::Nil => None,
                other => {
                    return Err(fail(
                        "now_context.redis_protocol",
                        format!("Unexpected revocation response: {other:?}"),
                    ))
                }
            };
        let raw = match self.command(secret, vec![b"GET".to_vec(), prepared.into_bytes()])? {
            Resp::Nil if revoked_value.is_some() => {
                return Err(fail(
                    "now_context.disclosure_revoked",
                    "Prepared NOW material was revoked and removed from the hot cache",
                ))
            }
            Resp::Nil => return Ok(None),
            v => bulk_utf8(v)?,
        };
        let view: PreparedNowContext = serde_json::from_str(&raw)
            .map_err(|e| fail("now_context.redis_corrupt", e.to_string()))?;
        view.validate(external_provider)?;
        if &view.participant_ref != participant {
            return Err(fail(
                "now_context.redis_corrupt",
                "Prepared NOW participant key and payload disagree",
            ));
        }
        if revoked_value
            .as_ref()
            .is_some_and(|value| value.as_slice() == view.basis.disclosure_revision.as_bytes())
        {
            return Err(fail(
                "now_context.disclosure_revoked",
                "Prepared NOW disclosure was revoked after preparation",
            ));
        }
        Ok(Some(view))
    }
    pub fn revoke(
        &self,
        participant: &ResourceRef,
        disclosure_revision: &str,
        secret: Option<&SecretValue>,
    ) -> Result<()> {
        if !bounded(disclosure_revision, 4096) {
            return Err(fail(
                "now_context.disclosure_invalid",
                "Disclosure revision is invalid",
            ));
        }
        let revoked = self.key("revoked", participant);
        let prepared = self.key("prepared", participant);
        expect_ok(self.command(
            secret,
            vec![
                b"SET".to_vec(),
                revoked.into_bytes(),
                disclosure_revision.as_bytes().to_vec(),
            ],
        )?)?;
        let _ = self.command(secret, vec![b"DEL".to_vec(), prepared.into_bytes()])?;
        Ok(())
    }
    pub fn append_change(
        &self,
        participant: &ResourceRef,
        change: &NowContextChange,
        secret: Option<&SecretValue>,
    ) -> Result<u64> {
        change.validate()?;
        let dedup = format!(
            "{}:{}",
            self.key("change-id", participant),
            blake3::hash(change.change_id.as_bytes()).to_hex()
        );
        let cursor = self.key("cursor", participant);
        let prefix = self.change_key_prefix(participant);
        let body =
            serde_json::to_vec(change).map_err(|e| fail("now_context.encode", e.to_string()))?;
        let script = r#"local p=redis.call('GET',KEYS[1]); if p then return tonumber(p) end; local c=redis.call('INCR',KEYS[2]); redis.call('SET',ARGV[1]..c,ARGV[2],'EX',ARGV[3]); redis.call('SET',KEYS[1],c,'EX',ARGV[3]); return c"#;
        let result = self.command(
            secret,
            vec![
                b"EVAL".to_vec(),
                script.as_bytes().to_vec(),
                b"2".to_vec(),
                dedup.into_bytes(),
                cursor.into_bytes(),
                prefix.into_bytes(),
                body,
                self.config
                    .coordination_retention_seconds
                    .to_string()
                    .into_bytes(),
            ],
        );
        map_eval_version(result)
    }
    pub fn read_changes(
        &self,
        participant: &ResourceRef,
        after: u64,
        limit: usize,
        secret: Option<&SecretValue>,
    ) -> Result<Vec<CursorChange>> {
        let limit = limit.clamp(1, MAX_CHANGES);
        let cursor_key = self.key("cursor", participant);
        let current = match self.command(secret, vec![b"GET".to_vec(), cursor_key.into_bytes()])? {
            Resp::Nil => 0,
            v => bulk_utf8(v)?
                .parse::<u64>()
                .map_err(|_| fail("now_context.redis_corrupt", "NOW cursor is not an integer"))?,
        };
        let end = current.min(after.saturating_add(limit as u64));
        let mut out = Vec::new();
        let prefix = self.change_key_prefix(participant);
        for cursor in after.saturating_add(1)..=end {
            let key = format!("{prefix}{cursor}");
            let raw =
                match self.command(secret, vec![b"GET".to_vec(), key.into_bytes()])? {
                    Resp::Nil => return Err(fail(
                        "now_context.change_gap",
                        format!(
                            "NOW change cursor {cursor} is unavailable; do not silently advance"
                        ),
                    )),
                    v => bulk_utf8(v)?,
                };
            let change = serde_json::from_str::<NowContextChange>(&raw)
                .map_err(|e| fail("now_context.redis_corrupt", e.to_string()))?;
            change.validate()?;
            out.push(CursorChange { cursor, change });
        }
        Ok(out)
    }
    pub fn ack_changes(
        &self,
        participant: &ResourceRef,
        cursor: u64,
        secret: Option<&SecretValue>,
    ) -> Result<u64> {
        let key = self.key("ack", participant);
        let script = r#"local v=redis.call('GET',KEYS[1]); if v and tonumber(v)>tonumber(ARGV[1]) then return tonumber(v) end; redis.call('SET',KEYS[1],ARGV[1]); return tonumber(ARGV[1])"#;
        map_eval_version(self.command(
            secret,
            vec![
                b"EVAL".to_vec(),
                script.as_bytes().to_vec(),
                b"1".to_vec(),
                key.into_bytes(),
                cursor.to_string().into_bytes(),
            ],
        ))
    }
    pub fn mark_delivered(
        &self,
        receipt: &NowDeliveryReceipt,
        secret: Option<&SecretValue>,
    ) -> Result<()> {
        receipt.validate()?;
        let meta = self.key("meta", &receipt.participant_ref);
        let delivery = self.key("delivery", &receipt.participant_ref);
        let body =
            serde_json::to_vec(receipt).map_err(|e| fail("now_context.encode", e.to_string()))?;
        let script = r#"local m=redis.call('GET',KEYS[1]); if not m then return redis.error_reply('NO_META') end; local ok,o=pcall(cjson.decode,m); if not ok or tonumber(o.version)~=tonumber(ARGV[1]) or o.prepared_digest~=ARGV[2] then return redis.error_reply('STALE') end; local p=redis.call('GET',KEYS[2]); if p then local ok2,d=pcall(cjson.decode,p); if not ok2 then return redis.error_reply('CORRUPT') end; if tonumber(d.prepared_version)>tonumber(ARGV[1]) then return redis.error_reply('STALE') end; if tonumber(d.prepared_version)==tonumber(ARGV[1]) and d.prepared_digest~=ARGV[2] then return redis.error_reply('CONFLICT') end end; redis.call('SET',KEYS[2],ARGV[3]); return 1"#;
        map_eval_version(self.command(
            secret,
            vec![
                b"EVAL".to_vec(),
                script.as_bytes().to_vec(),
                b"2".to_vec(),
                meta.into_bytes(),
                delivery.into_bytes(),
                receipt.prepared_version.to_string().into_bytes(),
                receipt.prepared_digest.as_bytes().to_vec(),
                body,
            ],
        ))?;
        Ok(())
    }
    pub fn last_delivery(
        &self,
        participant: &ResourceRef,
        secret: Option<&SecretValue>,
    ) -> Result<Option<NowDeliveryReceipt>> {
        let key = self.key("delivery", participant);
        match self.command(secret, vec![b"GET".to_vec(), key.into_bytes()])? {
            Resp::Nil => Ok(None),
            v => {
                let raw = bulk_utf8(v)?;
                let receipt = serde_json::from_str::<NowDeliveryReceipt>(&raw)
                    .map_err(|e| fail("now_context.redis_corrupt", e.to_string()))?;
                receipt.validate()?;
                Ok(Some(receipt))
            }
        }
    }
}

fn map_eval_version(result: Result<Resp>) -> Result<u64> {
    match result {
        Ok(Resp::Integer(v)) if v >= 0 => Ok(v as u64),
        Ok(v) => bulk_utf8(v)?.parse().map_err(|_| {
            fail(
                "now_context.redis_protocol",
                "Redis script returned a non-integer",
            )
        }),
        Err(e) => Err(e),
    }
}
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
            "now_context.redis_protocol",
            format!("Unexpected Redis response: {other:?}"),
        )),
    }
}
fn bulk_utf8(value: Resp) -> Result<String> {
    match value {
        Resp::Bulk(Some(v)) => {
            String::from_utf8(v).map_err(|e| fail("now_context.redis_protocol", e.to_string()))
        }
        Resp::Simple(v) => Ok(v),
        Resp::Integer(v) => Ok(v.to_string()),
        Resp::Bulk(None) => Err(fail("now_context.redis_missing", "Redis returned nil")),
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
        .map_err(|e| fail("now_context.redis_io", e.to_string()))?;
    stream
        .flush()
        .map_err(|e| fail("now_context.redis_io", e.to_string()))?;
    read_resp(stream)
}
fn read_resp(stream: &mut TcpStream) -> Result<Resp> {
    let mut prefix = [0u8; 1];
    stream
        .read_exact(&mut prefix)
        .map_err(|e| fail("now_context.redis_io", e.to_string()))?;
    match prefix[0] {
        b'+' => Ok(Resp::Simple(read_line(stream, 64 * 1024)?)),
        b'-' => {
            let message = read_line(stream, 64 * 1024)?;
            let code = if message.contains("STALE") {
                "now_context.stale"
            } else if message.contains("REVOKED") {
                "now_context.disclosure_revoked"
            } else if message.contains("CORRUPT") {
                "now_context.redis_corrupt"
            } else if message.contains("CONFLICT") {
                "now_context.delivery_conflict"
            } else {
                "now_context.redis_error"
            };
            Err(fail(code, message))
        }
        b':' => {
            let n = read_line(stream, 64)?
                .parse::<i64>()
                .map_err(|_| fail("now_context.redis_protocol", "Invalid Redis integer"))?;
            Ok(Resp::Integer(n))
        }
        b'$' => {
            let n = read_line(stream, 64)?
                .parse::<i64>()
                .map_err(|_| fail("now_context.redis_protocol", "Invalid Redis bulk length"))?;
            if n < 0 {
                return Ok(Resp::Bulk(None));
            }
            let n = n as usize;
            if n > MAX_JSON * 2 {
                return Err(fail(
                    "now_context.redis_protocol",
                    "Redis response exceeds bound",
                ));
            }
            let mut bytes = vec![0; n];
            stream
                .read_exact(&mut bytes)
                .map_err(|e| fail("now_context.redis_io", e.to_string()))?;
            let mut crlf = [0; 2];
            stream
                .read_exact(&mut crlf)
                .map_err(|e| fail("now_context.redis_io", e.to_string()))?;
            if crlf != *b"\r\n" {
                return Err(fail(
                    "now_context.redis_protocol",
                    "Redis bulk response lacks CRLF",
                ));
            }
            Ok(Resp::Bulk(Some(bytes)))
        }
        _ => Err(fail(
            "now_context.redis_protocol",
            "Unsupported Redis response type",
        )),
    }
}
fn read_line(stream: &mut TcpStream, max: usize) -> Result<String> {
    let mut out = Vec::new();
    loop {
        if out.len() > max {
            return Err(fail(
                "now_context.redis_protocol",
                "Redis line exceeds bound",
            ));
        }
        let mut b = [0; 1];
        stream
            .read_exact(&mut b)
            .map_err(|e| fail("now_context.redis_io", e.to_string()))?;
        if b[0] == b'\r' {
            let mut lf = [0; 1];
            stream
                .read_exact(&mut lf)
                .map_err(|e| fail("now_context.redis_io", e.to_string()))?;
            if lf[0] != b'\n' {
                return Err(fail("now_context.redis_protocol", "Redis line lacks LF"));
            }
            break;
        }
        out.push(b[0]);
    }
    String::from_utf8(out).map_err(|e| fail("now_context.redis_protocol", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_requires_explicit_remote_boundary_and_bounded_ttls() {
        let mut c = RedisNowConfig {
            schema: NOW_REDIS_CONFIG_SCHEMA.into(),
            address: "127.0.0.1:6379".into(),
            database: 0,
            key_prefix: "aikit-now".into(),
            username: None,
            credential_ref: None,
            allow_remote: false,
            connect_timeout_ms: 500,
            io_timeout_ms: 1000,
            prepared_ttl_seconds: 3600,
            coordination_retention_seconds: 86400,
        };
        assert!(c.validate().is_ok());
        c.prepared_ttl_seconds = 1;
        assert_eq!(
            c.validate().unwrap_err().code(),
            "now_context.config_invalid"
        );
    }
    #[test]
    fn prepared_view_binds_exact_basis_and_egress() {
        let source = ResourceRef::parse("context-source/test").unwrap();
        let mut basis = NowContextBasis {
            source_revisions: BTreeMap::from([(source.to_string(), "r1".into())]),
            dependency_revisions: BTreeMap::new(),
            disclosure_revision: "d1".into(),
            factory_revision: Some("f1".into()),
            change_cursor: 0,
        };
        let mut view = PreparedNowContext {
            schema: NOW_PREPARED_SCHEMA.into(),
            project_ref: ResourceRef::parse("project/test").unwrap(),
            now_ref: ResourceRef::parse("now/test").unwrap(),
            participant_ref: ResourceRef::parse("agent/test").unwrap(),
            agent_session: ResourceRef::parse("agent-session/test").unwrap(),
            version: 1,
            basis_digest: basis.digest().unwrap(),
            basis: basis.clone(),
            concern: "implement".into(),
            practice_refs: vec![],
            items: vec![NowContextItem {
                source_ref: source,
                source_revision: "r1".into(),
                title: "source".into(),
                excerpt: "essential passage".into(),
                route: Some("wiki/node/test".into()),
                agent_visibility: AgentVisibility::Payload,
                external_egress: ExternalEgress::Denied,
            }],
            neighbours: vec![],
            continuation: None,
            jev_invocation_ref: None,
            prepared_at_unix_ms: 1,
        };
        assert!(view.validate(false).is_ok());
        assert_eq!(
            view.validate(true).unwrap_err().code(),
            "now_context.egress_denied"
        );
        basis.disclosure_revision = "d2".into();
        view.basis = basis;
        assert_eq!(
            view.validate(false).unwrap_err().code(),
            "now_context.prepared_invalid"
        );
    }
}
