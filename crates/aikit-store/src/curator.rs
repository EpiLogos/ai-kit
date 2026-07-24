//! The curator and the drift detector: the system observing its own capability
//! tree and **proposing** to the inbox — never archiving, deleting or editing on
//! its own (PRIOR-ART-ACTIONS L2, L4).
//!
//! Hermes ships a curator that archives on an idle timer. AIKit keeps the
//! lifecycle it proves is worth having and drops the one thing that makes it
//! dangerous: the automatic write. The curator here reads usage, derives a
//! lifecycle, and files a `ProcedureReport` the user can act on in one keystroke.
//! Nothing it finds changes the tree until a human says so.

use std::collections::BTreeMap;

use aikit_core::catalog::Catalog;
use aikit_core::id::{CapsuleId, ProjectId, Revision};
use aikit_core::lifecycle::LifecycleThresholds;
use aikit_core::{ResolvedView, Result};

use crate::channel::{Evidence, InboxChannel, InboxItem, InboxKind, NewItem};
use crate::events::Timestamp;
use crate::index::Index;

// ---------------------------------------------------------------------------
// L4: the lifecycle curator
// ---------------------------------------------------------------------------

/// One capability the curator judged stale — used before, then gone quiet for
/// longer than [`LifecycleThresholds::stale_after`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleCapability {
    pub id: CapsuleId,
    pub idle_days: u64,
    pub successful_runs: u32,
}

/// What a curation run found and did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurationReport {
    pub stale: Vec<StaleCapability>,
    /// The inbox item published, if anything was stale. `None` means nothing was
    /// stale, so nothing was filed — the curator does not nag about a healthy tree.
    pub published: Option<InboxItem>,
}

/// Scan the catalogued capabilities, derive each one's lifecycle from usage, and —
/// if any are stale — publish **one** `ProcedureReport` to the inbox proposing a
/// review. It archives nothing.
///
/// "Stale" here is the clearest, false-positive-resistant signal: a capability
/// that *was* useful (it has a successful run on record) and has since been idle
/// past the stale threshold. Capabilities that were never used at all are a
/// different question, answered by `aikit unused`, not nagged about here.
pub fn curate(
    index: &Index,
    thresholds: &LifecycleThresholds,
    now: Timestamp,
) -> Result<CurationReport> {
    let mut stale: Vec<StaleCapability> = Vec::new();
    for row in index.capsules()? {
        let usage = index.usage_at(&row.id, now)?;
        if let Some(age) = usage.last_success_age {
            if age >= thresholds.stale_after {
                stale.push(StaleCapability {
                    id: row.id.clone(),
                    idle_days: age.as_secs() / (24 * 60 * 60),
                    successful_runs: usage.successful_runs,
                });
            }
        }
    }
    stale.sort_by(|a, b| a.id.cmp(&b.id));

    if stale.is_empty() {
        return Ok(CurationReport {
            stale,
            published: None,
        });
    }

    let mut body = format!(
        "{} capabilit{} have been quiet past the stale threshold and may be worth a review:\n\n",
        stale.len(),
        if stale.len() == 1 { "y" } else { "ies" }
    );
    for cap in &stale {
        body.push_str(&format!(
            "- `{}` — idle {} days ({} successful run{})\n",
            cap.id,
            cap.idle_days,
            cap.successful_runs,
            if cap.successful_runs == 1 { "" } else { "s" }
        ));
    }
    body.push_str(
        "\nThe curator only proposes. Nothing is archived or disabled until you decide.",
    );

    // Dedup on the exact set + newest idle bucket, so re-running the curator over
    // an unchanged tree returns the existing item instead of filing another.
    let dedup = format!(
        "curator:stale:{}",
        stale
            .iter()
            .map(|c| c.id.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let evidence: Vec<Evidence> = stale
        .iter()
        .map(|c| Evidence::Summary {
            text: format!("{} idle {} days", c.id, c.idle_days),
        })
        .collect();

    let item = InboxChannel::new(index).publish(
        NewItem::new(
            InboxKind::ProcedureReport,
            format!("{} capabilities have gone stale", stale.len()),
            body,
        )
        .with_evidence(evidence)
        .deduped_by(dedup),
    )?;

    Ok(CurationReport {
        stale,
        published: Some(item),
    })
}

// ---------------------------------------------------------------------------
// L2: content-hash tamper / drift detection
// ---------------------------------------------------------------------------

/// One capability whose payload has changed underneath an applied generation: the
/// revision the generation was built against no longer matches the current one on
/// disk (PRIOR-ART-ACTIONS L2 — the `.bundled_manifest` tamper check, surfaced as
/// a notice rather than a silent serve).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    pub id: CapsuleId,
    pub applied: Revision,
    pub current: Option<Revision>,
}

/// Compare an applied view (read back from a generation lock) against the current
/// catalog, and report every active capability whose payload has drifted.
///
/// Pure: it computes the diff, it does not write anything. `report_drift` files
/// the notices.
pub fn detect_drift(applied: &ResolvedView, catalog: &dyn Catalog) -> Vec<Drift> {
    let mut drifts = Vec::new();
    for (id, capability) in &applied.active {
        let Some(applied_rev) = &capability.revision else {
            continue; // an unstamped applied capsule can't be checked for drift
        };
        let current = catalog.get(id).and_then(|c| c.revision.clone());
        if current.as_ref() != Some(applied_rev) {
            drifts.push(Drift {
                id: id.clone(),
                applied: applied_rev.clone(),
                current,
            });
        }
    }
    drifts
}

/// File a `DriftNotice` for each drift, deduped so a re-check does not nag. Returns
/// the items that were published (or found already pending).
pub fn report_drift(
    index: &Index,
    drifts: &[Drift],
    project: Option<&ProjectId>,
) -> Result<Vec<InboxItem>> {
    let channel = InboxChannel::new(index);
    let mut items = Vec::new();
    for drift in drifts {
        let current = drift
            .current
            .as_ref()
            .map(|r| r.short().to_string())
            .unwrap_or_else(|| "gone from the catalog".to_string());
        let body = format!(
            "`{}` was applied at revision `{}`, but its payload on disk is now `{}`.\n\n\
             An out-of-band edit to a projected payload is not served silently. Re-apply to \
             pick up the change deliberately, or review what changed.",
            drift.id,
            drift.applied.short(),
            current,
        );
        let dedup = format!(
            "drift:{}:{}->{}",
            drift.id,
            drift.applied.short(),
            drift
                .current
                .as_ref()
                .map(|r| r.short().to_string())
                .unwrap_or_else(|| "none".to_string())
        );
        let item = channel.publish(
            NewItem::new(
                InboxKind::DriftNotice,
                format!("{} changed since it was applied", drift.id),
                body,
            )
            .in_project(project.cloned())
            .with_evidence(vec![Evidence::Hash {
                label: "applied revision".to_string(),
                value: drift.applied.as_str().to_string(),
            }])
            .deduped_by(dedup),
        )?;
        items.push(item);
    }
    Ok(items)
}

/// Convenience used by tests and callers that hold both an applied view and the
/// live catalog: detect and report in one call.
pub fn detect_and_report_drift(
    index: &Index,
    applied: &ResolvedView,
    catalog: &dyn Catalog,
    project: Option<&ProjectId>,
) -> Result<Vec<InboxItem>> {
    let drifts = detect_drift(applied, catalog);
    report_drift(index, &drifts, project)
}

/// A tiny helper so a caller can build the "was there anything?" summary without
/// re-deriving it. (Kept generic — a `BTreeMap` of counts by kind.)
pub fn count_by_kind(items: &[InboxItem]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for item in items {
        *counts.entry(item.kind.as_str()).or_insert(0) += 1;
    }
    counts
}
