//! The health checks, and the Procedure that fixes what is fixable.
//!
//! Two rules shape this module. **Diff before write, always** (STANDARDS §5): a
//! `--fix` shows its plan and asks, and `--yes` answers the question in advance
//! rather than skipping it. And **every write outside `~/.aikit/state/` is a
//! Procedure**, so a fix is planned, staged, reviewable and undoable like any
//! other world mutation — `doctor --fix` is a front-end over the one engine, not a
//! second safety story.

use aikit_adapters::NativeSecureStoreProvider;
use aikit_core::credential::{CredentialRef, SecretProvider};
use aikit_core::procedure::{Inverse, Plan, Procedure, ProcedureKind, WorldEdit};
use aikit_core::{AikitError, Result};
use aikit_store::CredentialBindingStore;

use crate::app::Service;

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Something is broken now.
    Error,
    /// Something will bite later.
    Warning,
    /// Worth knowing; nothing is wrong.
    Note,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        }
    }
}

/// A fix a Procedure could apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// Create a missing directory AIKit owns.
    CreateDir { path: std::path::PathBuf },
}

/// One thing `doctor` noticed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub check: &'static str,
    pub severity: Severity,
    pub summary: String,
    pub detail: Option<String>,
    /// `None` when a human has to decide — most findings are of that kind, and
    /// pretending otherwise is how a "fix" becomes a surprise.
    pub fix: Option<Fix>,
}

impl Finding {
    fn new(check: &'static str, severity: Severity, summary: impl Into<String>) -> Self {
        Self {
            check,
            severity,
            summary: summary.into(),
            detail: None,
            fix: None,
        }
    }

    #[must_use]
    fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[must_use]
    fn fixable(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// Run every check against the live context.
pub fn run(service: &Service) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    let home = service.home();
    let view = service.resolved();

    // A registry that failed to load is the loudest thing here: the catalogue is
    // silently smaller than the user thinks.
    for problem in service.load_warnings() {
        findings.push(
            Finding::new("registry.load", Severity::Error, "a registry file did not load")
                .with_detail(problem),
        );
    }

    // Declared-but-unavailable: the user asked for something and did not get it.
    for (id, reason) in &view.unavailable {
        if view.is_declared_enabled(id) {
            findings.push(
                Finding::new(
                    "resolution.unavailable",
                    Severity::Warning,
                    format!("{id} is enabled here but cannot activate"),
                )
                .with_detail(reason.describe()),
            );
        }
    }

    // Unreviewed capsules that something is asking to activate.
    let unreviewed = view
        .unavailable
        .iter()
        .filter(|(_, r)| matches!(r, aikit_core::UnavailableReason::TrustRequired))
        .count();
    if unreviewed > 0 {
        findings.push(
            Finding::new(
                "trust.unreviewed",
                Severity::Warning,
                format!("{unreviewed} capabilit{} awaiting review", if unreviewed == 1 { "y is" } else { "ies are" }),
            )
            .with_detail("review them with `aikit inbox`, or run one ad hoc with `aikit run <id> --confirm`".to_string()),
        );
    }

    // The home layout. This one IS automatically fixable: the directories are
    // AIKit's own, so creating a missing one cannot surprise anybody.
    for (label, path) in [
        ("registries", home.registries()),
        ("profiles", home.profiles()),
        ("inbox", home.inbox()),
        ("state", home.state()),
        ("credential binding state", home.credentials()),
    ] {
        if !path.exists() {
            findings.push(
                Finding::new(
                    "home.layout",
                    Severity::Error,
                    format!("the {label} directory is missing"),
                )
                .with_detail(path.display().to_string())
                .fixable(Fix::CreateDir { path }),
            );
        }
    }

    // Credential provider visibility. Probe with an intentionally unbound
    // identity: NoEntry proves the native store initialized without asking doctor
    // to know any operator secret or model-specific credential requirements.
    let probe = CredentialRef::new("credential:aikit/doctor-probe")?;
    let native = NativeSecureStoreProvider::new();
    let descriptor = native.descriptor(&probe);
    findings.push(
        Finding::new(
            "credential.native-provider",
            if descriptor.available {
                Severity::Note
            } else {
                Severity::Warning
            },
            format!(
                "native credential provider {} is {}",
                descriptor.provider_kind,
                if descriptor.available { "available" } else { "unavailable" }
            ),
        )
        .with_detail(format!(
            "tier=os-secure-store; headless_capable={}; materialisation=provider-native-lease; provenance={}",
            descriptor.headless_capable, descriptor.binding_provenance
        )),
    );

    for binding in CredentialBindingStore::new(home).list()? {
        findings.push(
            Finding::new(
                "credential.binding",
                Severity::Note,
                format!(
                    "{} resolves through {:?}",
                    binding.credential_ref.as_str(), binding.provider_tier
                ),
            )
            .with_detail(format!(
                "provider={}; materialisation={:?}; provenance={}",
                binding.provider_ref.as_str(),
                binding.materialisation,
                binding.binding_provenance
            )),
        );
    }

    // Open bypasses are meant to be short-lived and visible.
    let bypasses = service.open_bypasses()?;
    if !bypasses.is_empty() {
        findings.push(
            Finding::new(
                "bypass.open",
                Severity::Warning,
                format!("{} hook bypass token(s) are open", bypasses.len()),
            )
            .with_detail("a bypass should outlive its reason by minutes, not days".to_string()),
        );
    }

    findings.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.check.cmp(b.check)));
    Ok(findings)
}

/// Plan a Procedure for the findings that carry a fix.
///
/// `Ok(None)` when nothing is automatically fixable — which is the common and
/// correct case, because most findings are decisions rather than chores.
pub fn plan_fixes(service: &Service, findings: &[Finding]) -> Result<Option<Procedure>> {
    let mut plan = Plan::new();
    let mut any = false;

    for finding in findings {
        match &finding.fix {
            Some(Fix::CreateDir { path }) => {
                any = true;
                plan = plan
                    .with_note(format!("create {}", path.display()))
                    // A directory AIKit owns, created with a marker file so the
                    // edit has a concrete inverse rather than a bare mkdir.
                    .with_edit(WorldEdit::WriteFile {
                        path: path.join(".aikit-keep"),
                        contents: b"Created by `aikit doctor --fix`.\n".to_vec(),
                        inverse: Inverse::Remove,
                    });
            }
            None => {}
        }
    }

    if !any {
        return Ok(None);
    }
    aikit_store::procedure::plan_procedure(service.home(), ProcedureKind::DoctorFix { checks: vec![] }, plan)
        .map(Some)
        .map_err(|e: AikitError| e)
}