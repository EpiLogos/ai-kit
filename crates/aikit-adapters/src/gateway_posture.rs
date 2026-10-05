//! What a gateway process *is*, read from the process, never from its state
//! file: the build it is executing, when it started, which listeners it has
//! bound and how each is reachable, and which commands each carrier may issue.
//!
//! A saved configuration, a service definition and an installed binary are
//! three different facts from a running process. The gateway's state file
//! survives every restart; the executable it was started from does not. A
//! reading that comes from the process is the only one that can say whether an
//! update has reached the thing answering requests (`oi update` flips a
//! symlink; a resident keeps executing its old inode until it is restarted).
//!
//! Four separable questions live here, deliberately not fused:
//!
//! * **Listener binding** — [`ListenerClass`]: where a socket is bound
//!   (Unix path, loopback, tailnet address, LAN, every interface).
//! * **Carrier scope** — [`CarrierScope`]: what a peer that got through a
//!   carrier may *do* (an owner may drain and restore; a peer may relay and
//!   ask occupancy).
//! * **Lifecycle** — [`GatewayLifecycle`]: who keeps the process running
//!   (foreground, a supervisor, or an application).
//! * **Running build** — [`GatewayBuildIdentity`].
//!
//! Workcell placement, connector identity and session continuity are not
//! facts about the listener and are carried elsewhere; no connection mode
//! here creates another agent or another conversation.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The running process can say which build it is, where it runs from and since
/// when (`protocol`/`status` carry a [`GatewayBuildIdentity`]).
pub const GATEWAY_FEATURE_BUILD_IDENTITY: &str = "gateway-build-identity";
/// A binding-free drain: stop admitting turns, resolve in-flight work under a
/// bounded policy, record what could not finish, persist, and exit on request.
pub const GATEWAY_FEATURE_DRAIN: &str = "gateway-drain";
/// A network carrier does not grant every command: the owner-only set needs
/// the owner token (or the Unix socket).
pub const GATEWAY_FEATURE_CARRIER_SCOPE: &str = "gateway-carrier-scope";
/// A command this gateway does not know is answered as `unsupported_command`
/// (naming what it does support), never as malformed JSON.
pub const GATEWAY_FEATURE_UNSUPPORTED_COMMAND: &str = "gateway-unsupported-command";

/// This gateway relays a peer's Flow-conversation requests to its own
/// Workcell's encounter owner (`encounter-relay` command): the route a Flow
/// request to a recipient on another Workcell takes instead of an ssh command.
pub const GATEWAY_FEATURE_ENCOUNTER_RELAY: &str = "encounter-request-relay";

/// The service's configured gateway ref wins over the one a snapshot was
/// written under. A persisted ref is history, not configuration.
pub const GATEWAY_FEATURE_CONFIGURED_IDENTITY: &str = "gateway-configured-identity";

/// Which process is answering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayBuildIdentity {
    /// The source revision this executable was built from, full when known.
    pub revision: String,
    /// Built from a tree with uncommitted changes.
    #[serde(default)]
    pub dirty: bool,
    pub pid: u32,
    pub started_at_unix_ms: u64,
    /// The real path of the running executable (symlinks resolved).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
    /// SHA-256 of the executable as it was when the process started. This is
    /// what a managed install receipt records, so a resident can be compared
    /// with the binary now on disk without trusting a version string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_sha256: Option<String>,
    /// The Workcell this gateway serves, when it can say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workcell_ref: Option<String>,
    #[serde(default)]
    pub lifecycle: GatewayLifecycle,
}

/// Who keeps the gateway process running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayLifecycle {
    /// Started from a terminal or script: nothing restarts it if it exits.
    #[default]
    Foreground,
    /// A macOS LaunchAgent relaunches it.
    SupervisedLaunchd,
    /// A systemd user unit relaunches it.
    SupervisedSystemd,
    /// An application (the desktop, `oi`) started it and owns its lifetime.
    Application,
}

impl GatewayLifecycle {
    /// Detect from the environment the process was started in. A service
    /// definition names itself (`AIKIT_GATEWAY_LIFECYCLE`); launchd and
    /// systemd also leave their own markers.
    pub fn detect() -> Self {
        Self::detect_from(|name| std::env::var(name).ok())
    }

    pub fn detect_from(get: impl Fn(&str) -> Option<String>) -> Self {
        match get("AIKIT_GATEWAY_LIFECYCLE").as_deref() {
            Some("application") => return Self::Application,
            Some("supervised-launchd") => return Self::SupervisedLaunchd,
            Some("supervised-systemd") => return Self::SupervisedSystemd,
            Some("foreground") => return Self::Foreground,
            _ => {}
        }
        if get("XPC_SERVICE_NAME").is_some_and(|name| name.starts_with("ai.aikit.gateway")) {
            return Self::SupervisedLaunchd;
        }
        if get("INVOCATION_ID").is_some() && get("JOURNAL_STREAM").is_some() {
            return Self::SupervisedSystemd;
        }
        Self::Foreground
    }

    /// A supervisor the platform provides (launchd, systemd) will start the
    /// gateway again after it exits. A foreground gateway will not, and an
    /// application-managed one is the application's to start and stop: draining
    /// it would turn it off under the application that holds it.
    pub fn restarts_itself(self) -> bool {
        matches!(self, Self::SupervisedLaunchd | Self::SupervisedSystemd)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Foreground => "foreground",
            Self::SupervisedLaunchd => "supervised-launchd",
            Self::SupervisedSystemd => "supervised-systemd",
            Self::Application => "application",
        }
    }
}

impl GatewayBuildIdentity {
    /// Read this process. The revision and Workcell come from the caller (the
    /// binary that owns the build stamp); everything else is the process's own.
    pub fn of_this_process(
        revision: impl Into<String>,
        dirty: bool,
        workcell_ref: Option<String>,
    ) -> Self {
        let executable = std::env::current_exe()
            .ok()
            .and_then(|path| std::fs::canonicalize(&path).ok().or(Some(path)));
        Self {
            revision: revision.into(),
            dirty,
            pid: std::process::id(),
            started_at_unix_ms: unix_ms_now(),
            executable_path: executable.map(|path| path.display().to_string()),
            // Hashing a whole executable is not free: a
            // [`GatewayProcessRecord`] fills this in off the start-up path.
            executable_sha256: None,
            workcell_ref,
            lifecycle: GatewayLifecycle::detect(),
        }
    }

    /// Whether `other` names the same executable image: the digest when both
    /// have one, otherwise the revision (and then only when neither is dirty).
    pub fn same_image_as(&self, other_sha256: Option<&str>, other_revision: Option<&str>) -> bool {
        self.image_match(other_sha256, other_revision) == ImageMatch::Same
    }

    /// The comparison with its third answer. A digest decides when both sides
    /// have one. Without a digest the revision can prove two builds *differ*,
    /// and can prove they are the same only for a clean build: a dirty build
    /// carries edits the revision does not name, so it stays `Unknown` until
    /// its digest has been read (the process reads its own executable in the
    /// background after it starts).
    pub fn image_match(
        &self,
        other_sha256: Option<&str>,
        other_revision: Option<&str>,
    ) -> ImageMatch {
        match (self.executable_sha256.as_deref(), other_sha256) {
            (Some(mine), Some(theirs)) => {
                if mine.eq_ignore_ascii_case(theirs) {
                    ImageMatch::Same
                } else {
                    ImageMatch::Different
                }
            }
            _ => match other_revision {
                Some(revision) if !self.revision.is_empty() && !revision.is_empty() => {
                    if !(revision.starts_with(&self.revision)
                        || self.revision.starts_with(revision))
                    {
                        ImageMatch::Different
                    } else if self.dirty {
                        ImageMatch::Unknown
                    } else {
                        ImageMatch::Same
                    }
                }
                _ => ImageMatch::Unknown,
            },
        }
    }
}

/// Whether two builds are the same executable image, as far as what each has
/// said about itself can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMatch {
    Same,
    Different,
    /// Not provable yet: a digest is still being read, or a dirty build is
    /// compared by revision alone.
    Unknown,
}

pub fn unix_ms_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// SHA-256 of a file, hex. `None` when it cannot be read.
pub fn sha256_of_file(path: &Path) -> Option<String> {
    sha256_of_open_file(std::fs::File::open(path).ok()?)
}

/// SHA-256 of an already-open file, hex. An open handle keeps naming the inode
/// it was opened on: if the path is replaced afterwards (a managed install swaps
/// files by rename), the digest is still of the file that was opened.
pub fn sha256_of_open_file(mut file: std::fs::File) -> Option<String> {
    use std::io::Read;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

/// Where a listener is bound, and so who could possibly reach it. This is a
/// fact about the bind address alone — not about authentication, which is a
/// separate axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ListenerClass {
    /// A Unix-domain socket: only local processes with filesystem permission.
    LocalIpc,
    /// 127.0.0.0/8 or ::1: any local process of any user, and local browsers.
    Loopback,
    /// A Tailscale address (100.64.0.0/10, fd7a:115c:a1e0::/48): tailnet peers
    /// the tailnet policy admits, plus local processes.
    Tailnet,
    /// Another private range (RFC 1918, ULA, link-local): the local network.
    PrivateNetwork,
    /// 0.0.0.0 or ::: every interface, tailnet and LAN and anything routable.
    Wildcard,
    /// A routable address that is none of the above.
    Public,
    /// A host name, not an address: the address it resolves to is not known
    /// here, so nothing about reach can be claimed.
    Named,
}

impl ListenerClass {
    /// `HOST:PORT` (IPv6 in brackets) or a Unix path.
    pub fn classify_bind(bind: &str) -> Self {
        let bind = bind.trim();
        if bind.starts_with('/') || bind.starts_with("unix:") {
            return Self::LocalIpc;
        }
        let host = if let Some(rest) = bind.strip_prefix('[') {
            rest.split(']').next().unwrap_or("")
        } else {
            match bind.rsplit_once(':') {
                // A bare IPv6 literal has more than one ':' and no port split.
                Some((host, _)) if !host.contains(':') => host,
                _ => bind,
            }
        };
        match host.parse::<IpAddr>() {
            Ok(IpAddr::V4(ip)) => Self::classify_v4(ip),
            Ok(IpAddr::V6(ip)) => Self::classify_v6(ip),
            Err(_) if host.eq_ignore_ascii_case("localhost") => Self::Loopback,
            Err(_) => Self::Named,
        }
    }

    fn classify_v4(ip: Ipv4Addr) -> Self {
        let [a, b, ..] = ip.octets();
        if ip.is_unspecified() {
            Self::Wildcard
        } else if ip.is_loopback() {
            Self::Loopback
        } else if a == 100 && (64..=127).contains(&b) {
            Self::Tailnet
        } else if ip.is_private() || ip.is_link_local() {
            Self::PrivateNetwork
        } else {
            Self::Public
        }
    }

    fn classify_v6(ip: Ipv6Addr) -> Self {
        let segments = ip.segments();
        if ip.is_unspecified() {
            Self::Wildcard
        } else if ip.is_loopback() {
            Self::Loopback
        } else if segments[0] == 0xfd7a && segments[1] == 0x115c && segments[2] == 0xa1e0 {
            Self::Tailnet
        } else if (segments[0] & 0xfe00) == 0xfc00 || (segments[0] & 0xffc0) == 0xfe80 {
            Self::PrivateNetwork
        } else {
            Self::Public
        }
    }

    /// Reachable beyond this machine without a further hop.
    pub fn is_network_reachable(self) -> bool {
        !matches!(self, Self::LocalIpc | Self::Loopback)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalIpc => "local-ipc",
            Self::Loopback => "loopback",
            Self::Tailnet => "tailnet",
            Self::PrivateNetwork => "private-network",
            Self::Wildcard => "wildcard",
            Self::Public => "public",
            Self::Named => "named",
        }
    }
}

/// What a peer that reached a carrier may do with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CarrierScope {
    /// The machine's owner: everything, including draining, restoring state
    /// and binding conversations. The Unix socket is owner scope by filesystem
    /// permission; a network carrier is owner scope only with the owner token.
    Owner,
    /// Another gateway or an operator reading through `--at`: relay,
    /// occupancy, contact, reads. Never the commands that rewrite the
    /// gateway's own state or stop it.
    #[default]
    Peer,
}

impl CarrierScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Peer => "peer",
        }
    }
}

/// How one carrier of this process is bound, as the process itself sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayListenerReading {
    /// `unix` or `websocket`.
    pub carrier: String,
    /// The requested Unix path or `HOST:PORT` while waiting; the actual bound
    /// coordinate once the listener is bound. Port zero is an allocation request.
    pub bind: String,
    pub class: ListenerClass,
    /// The scope a client of this carrier gets with its ordinary credential.
    pub scope: CarrierScope,
    pub state: ListenerState,
    /// Why a listener is not bound (the OS error and what retries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ListenerState {
    Bound,
    /// Not bound yet and being retried (a tailnet address that does not exist
    /// until Tailscale is up). Other carriers keep serving meanwhile.
    Waiting,
}

/// The process's own record of itself, shared by the carrier threads.
#[derive(Debug)]
pub struct GatewayProcessRecord {
    build: GatewayBuildIdentity,
    /// The executable's digest, filled in by a thread started with the record
    /// so service start-up never waits on reading the whole image.
    digest: std::sync::Arc<std::sync::OnceLock<Option<String>>>,
    listeners: Mutex<Vec<GatewayListenerReading>>,
}

impl GatewayProcessRecord {
    pub fn new(build: GatewayBuildIdentity) -> Self {
        let digest = std::sync::Arc::new(std::sync::OnceLock::new());
        if build.executable_sha256.is_none() {
            let cell = std::sync::Arc::clone(&digest);
            // The image is OPENED here, at start, and read later in the
            // background: `/proc/self/exe` where there is one (it names the image
            // this process runs even after its file was replaced or removed), else
            // the executable's path opened now — a handle that keeps the inode this
            // process started from if the path is later swapped by rename.
            let own_image = Path::new("/proc/self/exe");
            let handle = if own_image.exists() {
                std::fs::File::open(own_image).ok()
            } else {
                build
                    .executable_path
                    .as_deref()
                    .map(Path::new)
                    .and_then(|path| std::fs::File::open(path).ok())
            };
            std::thread::spawn(move || {
                let _ = cell.set(handle.and_then(sha256_of_open_file));
            });
        }
        Self {
            build,
            digest,
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// This process's identity, with the executable digest once it is known.
    pub fn build(&self) -> GatewayBuildIdentity {
        let mut build = self.build.clone();
        if build.executable_sha256.is_none() {
            build.executable_sha256 = self.digest.get().cloned().flatten();
        }
        build
    }

    /// Record or update one listener (matched by carrier and bind).
    pub fn set_listener(&self, reading: GatewayListenerReading) {
        if let Ok(mut listeners) = self.listeners.lock() {
            match listeners.iter_mut().find(|existing| {
                existing.carrier == reading.carrier && existing.bind == reading.bind
            }) {
                Some(existing) => *existing = reading,
                None => listeners.push(reading),
            }
        }
    }

    /// Record the actual bound listener, retiring only its exact configured
    /// waiting coordinate in the same update. Other carrier/bind readings remain.
    /// A non-bound reading never removes a pending configured coordinate.
    /// Production wiring lands with the listener-reading reconciliation; the
    /// posture contract itself is exercised by the tests below.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_bound_listener(&self, requested_bind: &str, reading: GatewayListenerReading) {
        if let Ok(mut listeners) = self.listeners.lock() {
            if reading.state == ListenerState::Bound {
                listeners.retain(|existing| {
                    !(existing.carrier == reading.carrier
                        && existing.bind == requested_bind
                        && existing.state == ListenerState::Waiting)
                });
            }
            match listeners.iter_mut().find(|existing| {
                existing.carrier == reading.carrier && existing.bind == reading.bind
            }) {
                Some(existing) => *existing = reading,
                None => listeners.push(reading),
            }
        }
    }

    pub fn listeners(&self) -> Vec<GatewayListenerReading> {
        self.listeners
            .lock()
            .map(|listeners| listeners.clone())
            .unwrap_or_default()
    }
}

/// The path a Unix listener is bound at, as a reading.
pub fn unix_listener_reading(path: &Path, state: ListenerState) -> GatewayListenerReading {
    GatewayListenerReading {
        carrier: "unix".into(),
        bind: path.display().to_string(),
        class: ListenerClass::LocalIpc,
        scope: CarrierScope::Owner,
        state,
        detail: None,
    }
}

/// Resolve `PATH`'s real target — what a service manager would exec after a
/// symlink flip — for comparison with a running image.
pub fn resolved_executable(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_of_an_open_handle_is_of_the_file_that_was_opened_not_of_what_the_path_names_later()
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("aikit");
        std::fs::write(&path, b"the image this process started from").unwrap();
        let expected = sha256_of_file(&path).unwrap();
        let handle = std::fs::File::open(&path).unwrap();
        // A managed install swaps the file by rename: the path now names a new image.
        let replacement = dir.path().join("aikit.new");
        std::fs::write(&replacement, b"a newer image").unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        assert_ne!(sha256_of_file(&path).unwrap(), expected);
        assert_eq!(sha256_of_open_file(handle).unwrap(), expected);
    }

    #[test]
    fn a_bind_is_classified_by_who_could_reach_it() {
        use ListenerClass::*;
        let cases = [
            ("/Users/x/.aikit/state/gateway.sock", LocalIpc),
            ("127.0.0.1:7788", Loopback),
            ("localhost:7788", Loopback),
            ("[::1]:7788", Loopback),
            ("100.109.102.82:7788", Tailnet),
            ("100.64.0.1:1", Tailnet),
            ("100.127.255.254:1", Tailnet),
            ("100.128.0.1:1", Public),
            ("100.63.255.255:1", Public),
            ("[fd7a:115c:a1e0::1a35:3e66]:7788", Tailnet),
            ("192.168.4.90:7788", PrivateNetwork),
            ("10.1.2.3:7788", PrivateNetwork),
            ("[fe80::1]:7788", PrivateNetwork),
            ("0.0.0.0:7788", Wildcard),
            ("[::]:7788", Wildcard),
            ("203.0.113.9:443", Public),
            ("admins-mac.tail7e55a2.ts.net:7788", Named),
        ];
        for (bind, expected) in cases {
            assert_eq!(ListenerClass::classify_bind(bind), expected, "{bind}");
        }
        assert!(Tailnet.is_network_reachable());
        assert!(!Loopback.is_network_reachable());
        assert!(!LocalIpc.is_network_reachable());
    }

    #[test]
    fn the_lifecycle_is_read_from_the_environment_the_process_started_in() {
        use GatewayLifecycle::*;
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_owned())
            }
        };
        assert_eq!(GatewayLifecycle::detect_from(env(&[])), Foreground);
        assert_eq!(
            GatewayLifecycle::detect_from(env(&[("XPC_SERVICE_NAME", "ai.aikit.gateway")])),
            SupervisedLaunchd
        );
        assert_eq!(
            GatewayLifecycle::detect_from(env(&[("XPC_SERVICE_NAME", "0")])),
            Foreground
        );
        assert_eq!(
            GatewayLifecycle::detect_from(env(&[
                ("INVOCATION_ID", "abc"),
                ("JOURNAL_STREAM", "8:1")
            ])),
            SupervisedSystemd
        );
        assert_eq!(
            GatewayLifecycle::detect_from(env(&[
                ("AIKIT_GATEWAY_LIFECYCLE", "application"),
                ("XPC_SERVICE_NAME", "ai.aikit.gateway")
            ])),
            Application,
            "an explicit declaration outranks inference"
        );
        assert!(!Foreground.restarts_itself());
        assert!(SupervisedSystemd.restarts_itself());
        assert!(SupervisedLaunchd.restarts_itself());
        assert!(
            !Application.restarts_itself(),
            "the application holds its process; an upgrade does not stop it"
        );
    }

    #[test]
    fn this_process_reports_its_own_image_and_a_matching_digest_means_the_same_build() {
        let identity = GatewayBuildIdentity::of_this_process(
            "0123456789abcdef",
            false,
            Some("workcell:t".into()),
        );
        assert_eq!(identity.pid, std::process::id());
        assert!(identity.started_at_unix_ms > 0);
        let record = GatewayProcessRecord::new(identity.clone());
        let mut identity = record.build();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while identity.executable_sha256.is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
            identity = record.build();
        }
        let digest = identity
            .executable_sha256
            .clone()
            .expect("the test binary can be read");
        assert_eq!(digest.len(), 64);
        assert!(identity.same_image_as(Some(&digest.to_uppercase()), None));
        assert!(!identity.same_image_as(Some(&"0".repeat(64)), Some("0123456789abcdef")));
        // Without digests the revision decides, and a dirty build never claims
        // to be a revision.
        let mut undigested = identity.clone();
        undigested.executable_sha256 = None;
        assert!(undigested.same_image_as(None, Some("0123456789abcdef0123")));
        assert!(!undigested.same_image_as(None, Some("fedcba")));
        undigested.dirty = true;
        assert!(!undigested.same_image_as(None, Some("0123456789abcdef0123")));
        assert_eq!(
            undigested.image_match(None, Some("0123456789abcdef0123")),
            ImageMatch::Unknown,
            "a dirty build matching a revision is not yet proven"
        );
        assert_eq!(
            undigested.image_match(None, Some("fedcba")),
            ImageMatch::Different,
            "a different revision is different whatever the digest"
        );
    }

    #[test]
    fn a_bound_listener_replaces_only_its_exact_waiting_coordinate() {
        // This is the record's pure state contract, not a native bind receipt.
        // No background executable-digest worker is needed for listener state.
        let record = GatewayProcessRecord {
            build: GatewayBuildIdentity::of_this_process("listener-state-contract", true, None),
            digest: std::sync::Arc::new(std::sync::OnceLock::new()),
            listeners: Mutex::new(Vec::new()),
        };
        let reading = |carrier: &str, bind: &str, state| GatewayListenerReading {
            carrier: carrier.into(),
            bind: bind.into(),
            class: ListenerClass::classify_bind(bind),
            scope: CarrierScope::Peer,
            state,
            detail: (state == ListenerState::Waiting).then(|| "pending coordinate".into()),
        };
        let requested = "127.0.0.1:0";
        let actual = "127.0.0.1:43123";
        let waiting = reading("websocket", requested, ListenerState::Waiting);
        let unrelated = reading("websocket", "127.0.0.1:43124", ListenerState::Waiting);
        let other_carrier = reading("unix", requested, ListenerState::Waiting);
        let current_actual = reading("websocket", actual, ListenerState::Waiting);
        let bound = reading("websocket", actual, ListenerState::Bound);
        record.set_listener(waiting.clone());
        record.set_listener(unrelated.clone());
        record.set_listener(other_carrier.clone());
        record.set_listener(current_actual);

        record.set_bound_listener(requested, bound.clone());
        assert_eq!(
            record.listeners(),
            vec![unrelated.clone(), other_carrier.clone(), bound.clone()]
        );
        record.set_bound_listener(requested, bound.clone());
        assert_eq!(
            record.listeners(),
            vec![unrelated.clone(), other_carrier.clone(), bound.clone()],
            "updating the exact actual coordinate must not add another reading"
        );

        let established = reading("websocket", "127.0.0.1:43125", ListenerState::Bound);
        record.set_listener(established.clone());
        record.set_bound_listener(&established.bind, bound.clone());
        assert_eq!(
            record.listeners(),
            vec![
                unrelated.clone(),
                other_carrier.clone(),
                bound.clone(),
                established.clone(),
            ],
            "a bound requested coordinate is not an obsolete waiting reading"
        );

        record.set_listener(waiting.clone());
        record.set_bound_listener(requested, unrelated.clone());
        assert_eq!(
            record.listeners(),
            vec![unrelated, other_carrier, bound, established, waiting],
            "a non-bound observation cannot retire a pending coordinate"
        );
    }
}
