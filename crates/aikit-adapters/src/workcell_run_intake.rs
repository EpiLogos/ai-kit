//! One bounded native Workcell reading joins the existing ResourceRef field.
//! A material run projection never replaces Factory's canonical Run identity,
//! and observing a binding never confers eligibility or execution authority.
use crate::runner::CommandRunner;
use aikit_core::resource::{
    OwnerRef, ProviderOffer, ProviderRef, ProviderState, ResourceDescriptor, ResourceKind,
    ResourceLocator, ResourceRecord, ResourceRef, ResourceSource, SourceAuthority, SourceRef,
    SourceRevision, SourceState,
};
use aikit_core::{AikitError, Result};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

fn error(message: impl Into<String>) -> AikitError {
    AikitError::new("workcell.run_intake_unavailable", message)
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| error(format!("Workcell run has no {key}")))
}

pub fn read<R: CommandRunner>(runner: &R, executable: &str) -> Result<Vec<ResourceRecord>> {
    let argv = vec![
        executable.into(),
        "--json".into(),
        "run".into(),
        "list".into(),
        "--full".into(),
    ];
    let output = runner
        .run_with_timeout(&argv, Duration::from_secs(3))?
        .require(&argv, "workcell.run_intake_unavailable")?;
    if output.stdout.len() > 8 * 1024 * 1024 {
        return Err(error("Workcell run reading exceeds the 8 MiB intake limit"));
    }
    let value: Value =
        serde_json::from_str(&output.stdout).map_err(|failure| error(failure.to_string()))?;
    if value["ok"] != true {
        return Err(error("Workcell refused the full native run reading"));
    }
    project(&value)
}

fn add(
    records: &mut BTreeMap<ResourceRef, ResourceRecord>,
    reference: &str,
    kind: ResourceKind,
    name: &str,
    description: &str,
    source: ResourceSource,
    annotations: BTreeMap<String, String>,
) -> Result<()> {
    let id = ResourceRef::parse(reference)?;
    if let Some(existing) = records.get_mut(&id) {
        if existing.descriptor.kind != kind {
            return Err(error(format!(
                "Workcell binding {reference} has conflicting resource kinds"
            )));
        }
        if !existing.descriptor.sources.contains(&source) {
            existing.descriptor.sources.push(source);
        }
        return Ok(());
    }
    let mut descriptor = ResourceDescriptor::new(id.clone(), kind, name, description);
    if kind == ResourceKind::Run {
        descriptor.owner = Some(OwnerRef::parse("workcell")?);
    }
    descriptor.sources.push(source);
    descriptor.annotations = annotations;
    let mut record = ResourceRecord::new(descriptor);
    record.providers.push(ProviderOffer {
        provider: ProviderRef::parse("provider/workcell/run-ledger")?,
        locator: Some(ResourceLocator::Opaque(reference.into())),
        state: ProviderState::Available,
    });
    records.insert(id, record);
    Ok(())
}

fn project(reading: &Value) -> Result<Vec<ResourceRecord>> {
    let runs = reading["runs"]
        .as_array()
        .ok_or_else(|| error("Workcell did not return full run records"))?;
    if runs.len() > 512 {
        return Err(error(
            "Workcell run reading exceeds the 512-record intake limit",
        ));
    }
    let mut records = BTreeMap::new();
    for run in runs {
        if run["schema"] != "workcell.run/v1" {
            return Err(error("Workcell returned an unknown run schema"));
        }
        let slug = text(run, "run_slug")?;
        if slug.len() > 128
            || !slug
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(error("Workcell returned an invalid run slug"));
        }
        let demand = text(run, "demand_ref")?;
        let status = text(run, "execution_status")?;
        let source = ResourceSource {
            source: SourceRef::parse(demand)?,
            authority: Some(SourceAuthority::Observed),
            revision: run["demand_digest"]
                .as_str()
                .map(SourceRevision::parse)
                .transpose()?,
            locator: None,
            state: SourceState::Unresolved,
        };
        let mut annotations = BTreeMap::from([
            (
                "workcell.identity_standing".into(),
                "material run projection; native demand and Factory Run identities remain distinct"
                    .into(),
            ),
            ("workcell.demand_ref".into(), demand.into()),
            ("workcell.execution_status".into(), status.into()),
        ]);
        for key in ["canonical_run_ref", "world_ref", "rung"] {
            if let Some(value) = run[key].as_str() {
                annotations.insert(format!("workcell.{key}"), value.into());
            }
        }
        if let Some(agency) = run["agency"].as_object() {
            let agency = Value::Object(agency.clone());
            let reference = text(&agency, "agency_ref")?;
            annotations.insert("workcell.agency_ref".into(), reference.into());
            let agency_source = ResourceSource {
                source: SourceRef::parse(text(&agency, "source_ref")?)?,
                authority: Some(SourceAuthority::Observed),
                revision: agency["agency_rev"]
                    .as_str()
                    .map(SourceRevision::parse)
                    .transpose()?,
                locator: None,
                state: SourceState::Unresolved,
            };
            add(
                &mut records,
                reference,
                ResourceKind::Agency,
                reference,
                &format!(
                    "Agency recorded on material run {slug}; current admission is not inferred"
                ),
                agency_source,
                BTreeMap::new(),
            )?;
        }
        for (key, kind) in [
            ("harness_ref", ResourceKind::Harness),
            ("connection_ref", ResourceKind::Connection),
            ("model_ref", ResourceKind::Model),
            ("agent_profile_ref", ResourceKind::Profile),
        ] {
            if let Some(reference) = run["operative"][key]
                .as_str()
                .filter(|reference| !reference.is_empty())
            {
                annotations.insert(format!("workcell.{key}"), reference.into());
                add(&mut records, reference, kind, reference, &format!("{key} recorded on material run {slug}; current eligibility is not inferred"), source.clone(), BTreeMap::new())?;
            }
        }
        add(
            &mut records,
            &format!("run/{slug}"),
            ResourceKind::Run,
            slug,
            &format!("Workcell material run · {status} · {demand}"),
            source,
            annotations,
        )?;
    }
    Ok(records.into_values().collect())
}
