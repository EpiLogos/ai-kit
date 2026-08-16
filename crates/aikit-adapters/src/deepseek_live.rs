//! Live DeepSeek Harness/Cordis activation over the exact upstream process seam.
//!
//! The upstream source currently proves a real Cordis-hosted Web path through
//! `node --import tsx apps/cli/src/bin.ts web --patch examples/web-cordis/cordis.yml`.
//! AIKit does not reimplement Cordis. This adapter starts/stops that target-owned
//! process, waits for its target-owned Web endpoint, and only then reports a
//! SessionSpace Component as live.

use std::collections::BTreeSet;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use aikit_core::resource::ResourceRef;
use aikit_core::{
    resolve_harness_composition, AikitError, CompositionActivationMode, HarnessComposition, Result,
    SessionSpaceActivationDriver, SessionSpaceActivationObservation, SessionSpaceActivationRequest,
};

use crate::composition_topology::ComponentContainment;
use crate::deepseek_harness::{DeepSeekShellProvider, DEEPSEEK_HARNESS_UPSTREAM_REVISION};
use crate::deepseek_maximal::{deepseek_maximal_conformance, DeepSeekMaximalConformance};

pub const DEEPSEEK_CORDIS_WEB_PORT: u16 = 3081;

/// Components for which the current maximal conformance adapter has direct
/// target evidence inside the shipped Cordis/Web composition. The older thin
/// components remain at their resolver-declared activation mode unless separately
/// proven live; this avoids upgrading the whole graph by association.
pub const DEEPSEEK_LIVE_CORDIS_COMPONENTS: &[&str] = &[
    "component/deepseek/profile-root",
    "component/deepseek/client-ui-slots",
    "component/deepseek/client-ui-conversation",
    "component/deepseek/client-ui-commands",
    "component/deepseek/client-ui-permission",
    "component/deepseek/agent-loop",
];

pub struct DeepSeekLiveComposition {
    pub composition: HarnessComposition,
    pub containments: Vec<ComponentContainment>,
    pub live_components: BTreeSet<ResourceRef>,
}

/// Resolve the existing #65 specimen through the one canonical composition
/// resolver, then strengthen only target-owned lifecycle truth for the Cordis/Web
/// components the live adapter can actually materialise. Component/Surface/
/// provider identity and every resolver-owned binding remain unchanged.
pub fn deepseek_live_cordis_composition(
    shell: DeepSeekShellProvider,
) -> Result<DeepSeekLiveComposition> {
    let DeepSeekMaximalConformance {
        specimen,
        containments,
    } = deepseek_maximal_conformance(shell);
    let mut composition = resolve_harness_composition(&specimen.catalog, specimen.request)?;
    let live_components: BTreeSet<_> = DEEPSEEK_LIVE_CORDIS_COMPONENTS
        .iter()
        .map(|component| r(component))
        .collect();

    for binding in &mut composition.component_bindings {
        if live_components.contains(&binding.component) {
            binding.activation_mode = CompositionActivationMode::LiveMounted;
        }
    }
    for contribution in &mut composition.contributions {
        if live_components.contains(&contribution.component) {
            contribution.activation_mode = CompositionActivationMode::LiveMounted;
        }
    }

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
    /// Exact current upstream Cordis Web demo path. The caller supplies a checkout
    /// at the pinned revision; AIKit does not vendor or fork the target runtime.
    pub fn deepseek_web(source_checkout: impl AsRef<Path>) -> Self {
        Self {
            provider: r("provider/deepseek/cordis-web"),
            program: "node".into(),
            args: vec![
                "--import".into(),
                "tsx".into(),
                "apps/cli/src/bin.ts".into(),
                "web".into(),
                "--patch".into(),
                "examples/web-cordis/cordis.yml".into(),
            ],
            working_directory: source_checkout.as_ref().to_path_buf(),
            readiness: Some(SocketAddr::from(([127, 0, 0, 1], DEEPSEEK_CORDIS_WEB_PORT))),
            startup_timeout: Duration::from_secs(20),
            provenance: vec![
                format!("deepseek-ai/deepseek-harness@{DEEPSEEK_HARNESS_UPSTREAM_REVISION}"),
                "scripts/demo-cordis.mjs:web -> dsh web --patch examples/web-cordis/cordis.yml"
                    .into(),
            ],
        }
    }
}

/// Process-owning adapter for the target's real Cordis runtime. One process may
/// substantiate several AIKit Component readings, but it remains one target-owned
/// provider; ACP/AgentSession and SessionSpace identities stay separate.
pub struct CordisProcessActivationDriver {
    spec: CordisProcessSpec,
    child: Option<Child>,
    active_components: BTreeSet<ResourceRef>,
}

impl CordisProcessActivationDriver {
    pub fn new(spec: CordisProcessSpec) -> Self {
        Self {
            spec,
            child: None,
            active_components: BTreeSet::new(),
        }
    }

    pub fn deepseek_web(source_checkout: impl AsRef<Path>) -> Self {
        Self::new(CordisProcessSpec::deepseek_web(source_checkout))
    }

    pub fn provider(&self) -> &ResourceRef {
        &self.spec.provider
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
                format!("{} has no target-native binding", request.component.component),
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
}

impl SessionSpaceActivationDriver for CordisProcessActivationDriver {
    fn activate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> Result<SessionSpaceActivationObservation> {
        Self::validate_target(request)?;
        self.ensure_started()?;
        self.active_components
            .insert(request.component.component.clone());
        let mut provenance = self.spec.provenance.clone();
        provenance.push(format!(
            "provider process confirmed live while activating {}",
            request.component.component
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
        if !self.active_components.contains(&request.component.component) {
            return Ok(SessionSpaceActivationObservation::Unavailable {
                provider: self.spec.provider.clone(),
                reason: "Component was not active in the Cordis provider process".into(),
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

        self.active_components.remove(&request.component.component);
        self.stop_all()?;
        Ok(SessionSpaceActivationObservation::Unavailable {
            provider: self.spec.provider.clone(),
            reason: "Cordis provider stopped after final live Component retraction".into(),
            provenance: self.spec.provenance.clone(),
        })
    }
}

impl Drop for CordisProcessActivationDriver {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}

fn process_error(error: std::io::Error) -> AikitError {
    AikitError::new("cordis.process.io", error.to_string())
}

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).expect("DeepSeek live adapter uses static valid ResourceRefs")
}
