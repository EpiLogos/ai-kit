//! Reuse an existing Workcell material run. This operation never allocates a
//! workspace or chooses a weaker boundary than Central's actual requirements.
use super::{error, OwnerRunner};
use aikit_adapters::runner::CommandRunner;
use aikit_core::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub run_slug: String,
    pub expected_demand_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub executable: PathBuf,
    pub boundary_executable: PathBuf,
    pub state_root: PathBuf,
    pub scope: Value,
}

fn resolve_executable(name: &str) -> Result<PathBuf> {
    let path = std::env::var_os("PATH").ok_or_else(|| error("Owner PATH is not configured"))?;
    for directory in std::env::split_paths(&path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(name);
        if !candidate.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if candidate.metadata().map_err(error)?.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        return candidate.canonicalize().map_err(error);
    }
    Err(error(format!(
        "The native owner environment does not expose {name}"
    )))
}
fn call(program: &Path, state_root: &Path, args: &[String]) -> Result<Value> {
    let mut argv = vec![
        program.display().to_string(),
        "--state-root".into(),
        state_root.display().to_string(),
        "--json".into(),
    ];
    argv.extend_from_slice(args);
    let output = OwnerRunner.run(&argv)?;
    let value: Value = serde_json::from_str(&output.stdout).map_err(error)?;
    if !output.ok() || value["ok"] != true {
        return Err(error(
            "Workcell refused the selected material run operation",
        ));
    }
    Ok(value)
}
fn validate_run(value: &Value, request: &Request) -> Result<()> {
    if value["schema"] != "workcell.run/v1"
        || value["run_slug"] != request.run_slug
        || value["demand_digest"] != request.expected_demand_digest
        || !matches!(
            value["execution_status"].as_str(),
            Some("running" | "blocked")
        )
    {
        return Err(error(
            "Selected material run changed or is not live; no replacement is allocated",
        ));
    }
    Ok(())
}
impl Binding {
    pub fn resolve(request: &Request) -> Result<Self> {
        if request.run_slug.is_empty()
            || request.run_slug.len() > 128
            || !request
                .run_slug
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !request.expected_demand_digest.starts_with("sha256:")
        {
            return Err(error(
                "Expected an exact Workcell run slug and demand digest",
            ));
        }
        let executable = resolve_executable("workcell")?;
        let boundary_executable = resolve_executable("workcell-write-boundary")?;
        // Both programs belong to the selected installed/candidate owner. A
        // similarly named unrelated executable is not a fallback.
        if executable.parent() != boundary_executable.parent() {
            return Err(error(
                "Workcell and its boundary executable must come from the same owner installation",
            ));
        }
        let state_root = std::env::var_os("WORKCELL_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".workcell")))
            .ok_or_else(|| error("Native Workcell state root is unavailable"))?
            .canonicalize()
            .map_err(error)?;
        let run = call(
            &executable,
            &state_root,
            &[
                "run".into(),
                "show".into(),
                "--run".into(),
                request.run_slug.clone(),
            ],
        )?;
        validate_run(&run["run"], request)?;
        Ok(Self {
            executable,
            boundary_executable,
            state_root,
            scope: Value::Null,
        })
    }
    pub fn prepare(
        &mut self,
        request: &Request,
        cwd: &Path,
        requirements: &Value,
        agency: &Value,
    ) -> Result<Value> {
        let file = tempfile::NamedTempFile::new().map_err(error)?;
        fs::write(file.path(), requirements.to_string()).map_err(error)?;
        let result = call(
            &self.executable,
            &self.state_root,
            &[
                "run".into(),
                "scope".into(),
                "--run".into(),
                request.run_slug.clone(),
                "--expected-demand-digest".into(),
                request.expected_demand_digest.clone(),
                "--write-boundary".into(),
                file.path().display().to_string(),
                "--agency-ref".into(),
                agency["agency_ref"]
                    .as_str()
                    .ok_or_else(|| error("Admitted Agency reference absent"))?
                    .into(),
                "--agency-source".into(),
                agency["source"]
                    .as_str()
                    .ok_or_else(|| error("Admitted Agency source absent"))?
                    .into(),
                "--agency-rev".into(),
                agency["revision"]
                    .as_str()
                    .ok_or_else(|| error("Admitted Agency revision absent"))?
                    .into(),
                "--expected-agency-digest".into(),
                agency["digest"]
                    .as_str()
                    .ok_or_else(|| error("Admitted Agency digest absent"))?
                    .into(),
            ],
        )?;
        let scope = &result["scope"];
        if scope["schema"] != "workcell.prepared-run-scope/v1"
            || scope["run_slug"] != request.run_slug
            || scope["demand_digest"] != request.expected_demand_digest
            || scope["worktree_path"] != json!(cwd)
            || scope["agency"]["agency_ref"] != agency["agency_ref"]
            || scope["agency"]["agency_rev"] != agency["revision"]
            || scope["agency"]["source_digest"] != agency["digest"]
            || scope["run_revision"].as_str().is_none_or(str::is_empty)
            || scope["prepared_write_boundary"]["requirements"] != *requirements
            || scope["prepared_write_boundary"]["state"] != "prepared-not-executed"
        {
            return Err(error("Workcell did not return the exact selected worktree and owner boundary; no fallback"));
        }
        self.scope = scope.clone();
        self.revalidate(request)?;
        Ok(scope["prepared_write_boundary"].clone())
    }
    pub fn revalidate(&self, request: &Request) -> Result<()> {
        let reading = call(
            &self.executable,
            &self.state_root,
            &[
                "run".into(),
                "show".into(),
                "--run".into(),
                request.run_slug.clone(),
            ],
        )?;
        let run = &reading["run"];
        validate_run(run, request)?;
        if reading["run_revision"] != self.scope["run_revision"]
            || run["boundary_digest"]
                != self.scope["prepared_write_boundary"]["requirements_digest"]
            || run["agency"] != self.scope["agency"]
            || run["operative"] != self.scope["operative"]
        {
            return Err(error("Material run's admitted scope changed; explicitly reconfigure before starting another resident"));
        }
        Ok(())
    }
}
