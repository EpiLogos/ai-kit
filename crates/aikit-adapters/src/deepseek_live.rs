//! Live DeepSeek Harness/Cordis activation over the exact current upstream process seam.
//!
//! At the pinned source revision the ordinary `dsh web` bundle already mounts
//! `cordis-host-runner` and `cordis-client-runner`. The repository's older
//! `examples/web-cordis/cordis.yml` overlay now duplicates `cordis-host-runner`
//! and fails fast, so AIKit deliberately follows the current bundle rather than
//! preserving that stale demo seam. This adapter starts/stops the target-owned
//! Web/Cordis process, waits for its target-owned endpoint, and only then reports
//! a SessionSpace Component as live.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aikit_core::resource::ResourceRef;
use aikit_core::{
    resolve_harness_composition, AikitError, CompositionActivationMode, HarnessComposition, Result,
    SessionSpaceActivationDriver, SessionSpaceActivationObservation, SessionSpaceActivationRequest,
    SessionSpaceRef,
};

use crate::composition_topology::ComponentContainment;
use crate::deepseek_harness::{DeepSeekShellProvider, DEEPSEEK_HARNESS_UPSTREAM_REVISION};
use crate::deepseek_maximal::{deepseek_maximal_conformance, DeepSeekMaximalConformance};

pub const DEEPSEEK_CORDIS_WEB_PORT: u16 = 3080;

/// Components for which the current maximal conformance adapter has direct
/// process-level evidence inside the shipped Web/Cordis composition. Agent-loop
/// remains a per-session/preset concern and therefore stays at its resolver
/// activation mode until an actual AgentSession activation proves it live.
pub const DEEPSEEK_LIVE_CORDIS_COMPONENTS: &[&str] = &[
    "component/deepseek/profile-root",
    "component/deepseek/client-ui-slots",
    "component/deepseek/client-ui-conversation",
    "component/deepseek/client-ui-commands",
    "component/deepseek/client-ui-permission",
];

pub struct DeepSeekLiveComposition {
    pub composition: HarnessComposition,
    pub containments: Vec<ComponentContainment>,
    pub live_components: BTreeSet<ResourceRef>,
}

/// Resolve the live-capable #65 specimen through the one canonical composition
/// resolver. Target evidence changes the adapter-owned catalogue/request inputs
/// *before* resolution so the canonical body fingerprint/history includes the
/// final activation modes. Resolver output is never rewritten afterward.
pub fn deepseek_live_cordis_composition(
    shell: DeepSeekShellProvider,
) -> Result<DeepSeekLiveComposition> {
    let DeepSeekMaximalConformance {
        mut specimen,
        containments,
    } = deepseek_maximal_conformance(shell);
    let live_components: BTreeSet<_> = DEEPSEEK_LIVE_CORDIS_COMPONENTS
        .iter()
        .map(|component| r(component))
        .collect();

    // The target adapter owns this conformance fact, but the canonical resolver
    // owns the resulting body. Advertise LiveMounted on descriptors/contributions
    // and select that mode before resolution; never post-mutate a resolved body.
    for component in &live_components {
        let mut descriptor =
            specimen
                .catalog
                .component(component)
                .cloned()
                .ok_or_else(|| {
                    AikitError::new(
                "cordis.composition.component_absent",
                format!("live Cordis component {component} is absent from the conformance catalog"),
            )
                })?;
        descriptor
            .activation_modes
            .insert(CompositionActivationMode::LiveMounted);
        for contribution in &mut descriptor.contributions {
            contribution.activation_mode = CompositionActivationMode::LiveMounted;
        }
        specimen.catalog.insert_component(descriptor);
    }
    for selection in &mut specimen.request.selections {
        if live_components.contains(&selection.component) {
            selection.activation_mode = CompositionActivationMode::LiveMounted;
        }
    }

    let composition = resolve_harness_composition(&specimen.catalog, specimen.request)?;

    Ok(DeepSeekLiveComposition {
        composition,
        containments,
        live_components,
    })
}

#[derive(Debug, Clone)]
pub struct CordisProcessSpec {
    pub provider: ResourceRef,
    pub program: String,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
    pub readiness: Option<SocketAddr>,
    pub startup_timeout: Duration,
    pub provenance: Vec<String>,
}

impl CordisProcessSpec {
    /// Exact current upstream Web/Cordis process path. The caller supplies a
    /// checkout at the pinned revision; AIKit does not vendor or fork Cordis.
    pub fn deepseek_web(source_checkout: impl AsRef<Path>) -> Self {
        Self {
            provider: r("provider/deepseek/cordis-web"),
            program: "node".into(),
            args: vec![
                "--import".into(),
                "tsx".into(),
                "apps/cli/src/bin.ts".into(),
                "web".into(),
            ],
            working_directory: source_checkout.as_ref().to_path_buf(),
            readiness: Some(SocketAddr::from(([127, 0, 0, 1], DEEPSEEK_CORDIS_WEB_PORT))),
            startup_timeout: Duration::from_secs(20),
            provenance: vec![
                format!("deepseek-ai/deepseek-harness@{DEEPSEEK_HARNESS_UPSTREAM_REVISION}"),
                "packages/bundle/web-app/cordis.patch.yml:cordis-host-runner+cordis-client-runner"
                    .into(),
                "apps/cli/src/bin.ts web (default target-owned Web/Cordis bundle)".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CordisActivationOperation {
    Activate,
    Deactivate,
}

impl CordisActivationOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Activate => "activate",
            Self::Deactivate => "deactivate",
        }
    }
}

/// Finite authority accepted by the actual target-process adapter.
///
/// The grant is issued/resolved outside this provider adapter. AIKit consumes it
/// at the last privileged seam before process start/stop and binds it to the exact
/// SessionSpace, AgentSession, Harness, Component, composition fingerprint and
/// pinned target implementation revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CordisActivationGrant {
    pub grant_ref: String,
    pub authority_ref: String,
    pub operation: CordisActivationOperation,
    pub space: SessionSpaceRef,
    pub agent_session: ResourceRef,
    pub harness: ResourceRef,
    pub component: ResourceRef,
    pub composition_fingerprint: String,
    pub implementation_revision: String,
    pub expires_at_unix_ms: u64,
    pub max_uses: u32,
}

#[derive(Debug, Clone)]
struct StoredCordisActivationGrant {
    grant: CordisActivationGrant,
    uses: u32,
    revoked: bool,
}

/// Process-owning adapter for the target's real Cordis runtime. One process may
/// substantiate several AIKit Component readings, but it remains one target-owned
/// provider; ACP/AgentSession and SessionSpace identities stay separate.
///
/// `LiveMounted` eligibility and SessionSpace admission are intentionally not
/// sufficient to reach `Command::spawn`: an exact finite activation grant must be
/// registered first by the authority-owning control path.
pub struct CordisProcessActivationDriver {
    spec: CordisProcessSpec,
    child: Option<Child>,
    active_components: BTreeSet<ResourceRef>,
    activation_grants: BTreeMap<String, StoredCordisActivationGrant>,
}

impl CordisProcessActivationDriver {
    pub fn new(spec: CordisProcessSpec) -> Self {
        Self {
            spec,
            child: None,
            active_components: BTreeSet::new(),
            activation_grants: BTreeMap::new(),
        }
    }

    pub fn deepseek_web(source_checkout: impl AsRef<Path>) -> Self {
        Self::new(CordisProcessSpec::deepseek_web(source_checkout))
    }

    pub fn provider(&self) -> &ResourceRef {
        &self.spec.provider
    }

    pub fn register_activation_grant(&mut self, grant: CordisActivationGrant) -> Result<()> {
        if grant.grant_ref.trim().is_empty() || grant.authority_ref.trim().is_empty() {
            return Err(AikitError::new(
                "cordis.activation.invalid_authority",
                "Cordis activation grant requires stable grant and authority refs",
            ));
        }
        if grant.max_uses == 0 || grant.expires_at_unix_ms <= now_unix_ms()? {
            return Err(AikitError::new(
                "cordis.activation.invalid_authority_lifetime",
                "Cordis activation grant must be live and have a non-zero use budget",
            ));
        }
        if grant.implementation_revision != DEEPSEEK_HARNESS_UPSTREAM_REVISION {
            return Err(AikitError::new(
                "cordis.activation.authority_revision_mismatch",
                format!(
                    "Cordis activation grant must bind pinned DeepSeek Harness revision {DEEPSEEK_HARNESS_UPSTREAM_REVISION}"
                ),
            ));
        }
        let key = activation_grant_key(grant.operation, &grant.component);
        if self.activation_grants.contains_key(&key) {
            return Err(AikitError::new(
                "cordis.activation.authority_already_registered",
                format!(
                    "authority for {} {} is already registered",
                    grant.operation.as_str(),
                    grant.component
                ),
            ));
        }
        self.activation_grants.insert(
            key,
            StoredCordisActivationGrant {
                grant,
                uses: 0,
                revoked: false,
            },
        );
        Ok(())
    }

    pub fn revoke_activation_grant(&mut self, grant_ref: &str) -> Result<()> {
        let stored = self
            .activation_grants
            .values_mut()
            .find(|stored| stored.grant.grant_ref == grant_ref)
            .ok_or_else(|| {
                AikitError::new(
                    "cordis.activation.authority_absent",
                    format!("unknown Cordis activation grant `{grant_ref}`"),
                )
            })?;
        stored.revoked = true;
        Ok(())
    }

    pub fn is_running(&mut self) -> Result<bool> {
        let Some(child) = self.child.as_mut() else {
            return Ok(false);
        };
        match child.try_wait().map_err(process_error)? {
            None => Ok(true),
            Some(_) => {
                self.child = None;
                self.active_components.clear();
                Ok(false)
            }
        }
    }

    pub fn stop_all(&mut self) -> Result<()> {
        let Some(mut child) = self.child.take() else {
            self.active_components.clear();
            return Ok(());
        };
        if child.try_wait().map_err(process_error)?.is_none() {
            child.kill().map_err(process_error)?;
        }
        child.wait().map_err(process_error)?;
        self.active_components.clear();
        Ok(())
    }

    fn ensure_started(&mut self) -> Result<()> {
        if self.is_running()? {
            return Ok(());
        }

        let mut command = Command::new(&self.spec.program);
        command
            .args(&self.spec.args)
            .current_dir(&self.spec.working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.child = Some(command.spawn().map_err(|error| {
            AikitError::new(
                "cordis.process.spawn_failed",
                format!(
                    "failed to start Cordis provider `{}` in {}: {error}",
                    self.spec.program,
                    self.spec.working_directory.display()
                ),
            )
        })?);

        self.wait_until_ready()
    }

    fn wait_until_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + self.spec.startup_timeout;
        loop {
            let Some(child) = self.child.as_mut() else {
                return Err(AikitError::new(
                    "cordis.process.missing",
                    "Cordis provider disappeared during activation",
                ));
            };
            if let Some(status) = child.try_wait().map_err(process_error)? {
                self.child = None;
                return Err(AikitError::new(
                    "cordis.process.exited_during_activation",
                    format!("Cordis provider exited during activation with {status}"),
                ));
            }

            match self.spec.readiness {
                None => return Ok(()),
                Some(address) => {
                    if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
                        return Ok(());
                    }
                }
            }

            if Instant::now() >= deadline {
                let _ = self.stop_all();
                return Err(AikitError::new(
                    "cordis.process.readiness_timeout",
                    format!(
                        "Cordis provider did not become reachable at {:?} within {:?}",
                        self.spec.readiness, self.spec.startup_timeout
                    ),
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn validate_target(request: &SessionSpaceActivationRequest) -> Result<()> {
        let Some(implementation) = &request.component.implementation else {
            return Err(AikitError::new(
                "cordis.activation.missing_target_binding",
                format!(
                    "{} has no target-native binding",
                    request.component.component
                ),
            ));
        };
        if implementation.implementation_target != "deepseek-ai/deepseek-harness" {
            return Err(AikitError::new(
                "cordis.activation.wrong_target",
                format!(
                    "{} belongs to {}, not deepseek-ai/deepseek-harness",
                    request.component.component, implementation.implementation_target
                ),
            ));
        }
        if implementation.revision.as_deref() != Some(DEEPSEEK_HARNESS_UPSTREAM_REVISION) {
            return Err(AikitError::new(
                "cordis.activation.revision_mismatch",
                format!(
                    "{} is not bound to the pinned DeepSeek Harness revision {}",
                    request.component.component, DEEPSEEK_HARNESS_UPSTREAM_REVISION
                ),
            ));
        }
        Ok(())
    }

    fn consume_activation_grant(
        &mut self,
        operation: CordisActivationOperation,
        request: &SessionSpaceActivationRequest,
    ) -> Result<String> {
        let key = activation_grant_key(operation, &request.component.component);
        let stored = self.activation_grants.get_mut(&key).ok_or_else(|| {
            AikitError::new(
                "cordis.activation.authority_required",
                format!(
                    "{} {} denied before target process side effect: exact activation authority is required",
                    operation.as_str(), request.component.component
                ),
            )
        })?;
        if stored.revoked {
            return Err(AikitError::new(
                "cordis.activation.authority_revoked",
                format!(
                    "Cordis activation grant `{}` is revoked",
                    stored.grant.grant_ref
                ),
            ));
        }
        if now_unix_ms()? >= stored.grant.expires_at_unix_ms {
            return Err(AikitError::new(
                "cordis.activation.authority_expired",
                format!(
                    "Cordis activation grant `{}` is expired",
                    stored.grant.grant_ref
                ),
            ));
        }
        let implementation_revision = request
            .component
            .implementation
            .as_ref()
            .and_then(|implementation| implementation.revision.as_deref());
        if stored.grant.space != request.space
            || stored.grant.agent_session != request.agent_session
            || stored.grant.harness != request.harness
            || stored.grant.component != request.component.component
            || stored.grant.composition_fingerprint != request.composition_fingerprint
            || implementation_revision != Some(stored.grant.implementation_revision.as_str())
        {
            return Err(AikitError::new(
                "cordis.activation.authority_target_mismatch",
                "Cordis activation authority is stale, substituted, or belongs to another runtime identity",
            ));
        }
        if stored.uses >= stored.grant.max_uses {
            return Err(AikitError::new(
                "cordis.activation.authority_exhausted",
                format!(
                    "Cordis activation grant `{}` is exhausted",
                    stored.grant.grant_ref
                ),
            ));
        }
        stored.uses += 1;
        Ok(stored.grant.authority_ref.clone())
    }
}

impl SessionSpaceActivationDriver for CordisProcessActivationDriver {
    fn activate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> Result<SessionSpaceActivationObservation> {
        Self::validate_target(request)?;
        let authority_ref =
            self.consume_activation_grant(CordisActivationOperation::Activate, request)?;
        self.ensure_started()?;
        self.active_components
            .insert(request.component.component.clone());
        let mut provenance = self.spec.provenance.clone();
        provenance.push(format!(
            "provider process confirmed live while activating {} under authority {}",
            request.component.component, authority_ref
        ));
        Ok(SessionSpaceActivationObservation::Active {
            provider: self.spec.provider.clone(),
            provenance,
        })
    }

    fn deactivate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> Result<SessionSpaceActivationObservation> {
        Self::validate_target(request)?;
        if !self
            .active_components
            .contains(&request.component.component)
        {
            return Ok(SessionSpaceActivationObservation::Deactivated {
                provider: self.spec.provider.clone(),
                provenance: self.spec.provenance.clone(),
            });
        }
        if self.active_components.len() > 1 {
            // The process seam proves composition activation and whole-provider teardown.
            // It does not prove arbitrary in-process Cordis Fiber disposal from Rust.
            // Refuse to counterfeit that stronger operation while siblings remain live.
            return Err(AikitError::new(
                "cordis.process.partial_retraction_unsupported",
                format!(
                    "cannot prove live retraction of {} through the process seam while {} sibling Components remain active",
                    request.component.component,
                    self.active_components.len() - 1
                ),
            ));
        }

        let authority_ref =
            self.consume_activation_grant(CordisActivationOperation::Deactivate, request)?;
        self.active_components.remove(&request.component.component);
        self.stop_all()?;
        let mut provenance = self.spec.provenance.clone();
        provenance.push(format!(
            "Cordis provider stopped after final live Component deactivation under authority {authority_ref}"
        ));
        Ok(SessionSpaceActivationObservation::Deactivated {
            provider: self.spec.provider.clone(),
            provenance,
        })
    }
}

impl Drop for CordisProcessActivationDriver {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}

fn activation_grant_key(operation: CordisActivationOperation, component: &ResourceRef) -> String {
    format!("{}|{}", operation.as_str(), component)
}

fn now_unix_ms() -> Result<u64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            AikitError::new(
                "cordis.activation.clock",
                format!("system clock error: {error}"),
            )
        })?;
    u64::try_from(duration.as_millis()).map_err(|_| {
        AikitError::new(
            "cordis.activation.clock",
            "system time exceeds u64 milliseconds",
        )
    })
}

fn process_error(error: std::io::Error) -> AikitError {
    AikitError::new("cordis.process.io", error.to_string())
}

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).expect("DeepSeek live adapter uses static valid ResourceRefs")
}
