//! The existing Workcell control client owns material effects. This consumer
//! correlates its actual receipt/observation with the admitted Central clearing;
//! a declaration, a healthy endpoint or a past receipt is not an attachment.
use super::{error, OwnerRunner};
use aikit_adapters::central_placement::AllocatedCentralTask;
use aikit_adapters::runner::CommandRunner;
use aikit_core::{ResourceRef, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, net::SocketAddr, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MaterialHost {
    pub workcell_bin: PathBuf,
    pub endpoint: String,
    pub workcell_ref: ResourceRef,
    /// An explicit material attempt, not a provider-generated Agent/Run id.
    pub demand_ref: ResourceRef,
    #[serde(default)]
    pub required_services: Vec<ResourceRef>,
    /// When supplied, this service must be the actual encounter owner process.
    #[serde(default)]
    pub encounter_service: Option<ResourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct MaterialBinding {
    pub host: MaterialHost,
    pub demand: Value,
    pub world: Value,
    /// Captured at preparation. Every launch/turn obtains a fresh observation.
    pub prepared_observation: Value,
}

impl MaterialHost {
    fn check(&self) -> Result<()> {
        // This caller launches a local resident; claiming remote material as
        // that resident's host would invent relocation. Tunnels/physical host
        // identity still require the separately approved material arrangement.
        let endpoint: SocketAddr = self.endpoint.parse().map_err(error)?;
        if !endpoint.ip().is_loopback() || !self.workcell_bin.is_absolute()
            || !self.workcell_bin.is_file() || self.required_services.len() > 32 {
            return Err(error("Local resident material needs an explicit native Workcell executable and loopback control endpoint; remote resident hosting is not inferred"));
        }
        let mut services = std::collections::BTreeSet::new();
        if self.required_services.iter().any(|r| !services.insert(r)) {
            return Err(error("Duplicate required material service"));
        }
        if self.encounter_service.as_ref().is_some_and(|r| !self.required_services.contains(r)) {
            return Err(error("Encounter owner service must be an explicit required service"));
        }
        Ok(())
    }

    fn call(&self, operation: &str, input: Option<&Value>) -> Result<Value> {
        self.check()?;
        let staging = tempfile::tempdir().map_err(error)?;
        let receipt = staging.path().join("world.json");
        let mut argv = vec![self.workcell_bin.display().to_string(),
            "--endpoint".into(), self.endpoint.clone(), "--json".into(),
            "--state-root".into(), staging.path().display().to_string()];
        if let Some(input) = input {
            argv.extend(["--receipt".into(), receipt.display().to_string()]);
            if operation == "prepare" {
                let demand = staging.path().join("demand.json");
                fs::write(&demand, input.to_string()).map_err(error)?;
                argv.extend([operation.into(), "--demand-json".into(), demand.display().to_string()]);
            } else {
                fs::write(&receipt, input.to_string()).map_err(error)?;
                argv.push(operation.into());
            }
        } else {
            argv.push(operation.into());
        }
        // WORKCELL_CONTROL_TOKEN travels only through the owner process's
        // environment; no credential is stored in requests, records or argv.
        let output = OwnerRunner.run(&argv)?;
        if !output.ok() {
            return Err(error(format!("Native Workcell {operation} refused or was unavailable; keep this demand and inspect uncertain effects before retry")));
        }
        let mut value: Value = serde_json::from_str(&output.stdout).map_err(error)?;
        if value["ok"] != true {
            return Err(error("Native Workcell returned an unsuccessful material response"));
        }
        value.as_object_mut().ok_or_else(|| error("Native material response must be an object"))?.remove("ok");
        Ok(value)
    }

    pub fn preflight(&self) -> Result<()> {
        if self.call("status", None)?["workcell_ref"] != json!(self.workcell_ref) {
            return Err(error("Control endpoint is not the deliberately selected Workcell"));
        }
        Ok(())
    }

    pub fn prepare(&self, task: &AllocatedCentralTask, subjects: Value) -> Result<MaterialBinding> {
        self.preflight()?;
        let empty = json!({"required":[], "preferred":[], "optional":[]});
        let demand = json!({
            "demand_ref":self.demand_ref, "subjects":subjects,
            "affordances":empty, "connectivity":{"required":self.required_services, "preferred":[], "optional":[]},
            "exposure":empty, "outputs":empty,
            "storage":{"required":[task.storage_requirement()?], "preferred":[], "optional":[]},
            "workspace":null, "project_runtime":null, "resources":[],
            "persistence":null, "isolation_trust":null, "retention":"release", "extensions":{}
        });
        // Workcell already owns exact-demand replay and durable uncertainty.
        // Do not create a second demand or a consumer-side material journal.
        let reply = self.call("prepare", Some(&demand))?;
        let world = reply.get("world").filter(|v| v.is_object()).cloned()
            .ok_or_else(|| error("Native prepare omitted its material world"))?;
        let mut binding = MaterialBinding { host:self.clone(), demand, world, prepared_observation:Value::Null };
        binding.prepared_observation = binding.validate(task)?;
        Ok(binding)
    }
}

impl MaterialBinding {
    /// Called in the resident owner, not in its protocol child or a configure
    /// client. Fresh validate() must precede this process correlation.
    pub fn check_encounter_owner(&self) -> Result<()> {
        let Some(service) = &self.host.encounter_service else { return Ok(()); };
        let bindings = self.world["binding_graph"]["bindings"].as_array()
            .ok_or_else(|| error("Missing owner material bindings"))?;
        let binding = bindings.iter().find(|b| b["port"] == "service" && b["properties"]["logical_ref"] == json!(service))
            .ok_or_else(|| error("Required encounter owner service is absent"))?;
        if binding["properties"]["pid"].as_str().and_then(|p| p.parse::<u32>().ok()) != Some(std::process::id()) {
            return Err(error("The current encounter owner is not the process hosted by the selected Workcell binding"));
        }
        Ok(())
    }

    fn check_world(&self, task: &AllocatedCentralTask, world: &Value) -> Result<()> {
        if world["version"] != "workcell.material-world/v1"
            || world["workcell_ref"] != json!(self.host.workcell_ref)
            || world["demand_ref"] != json!(self.host.demand_ref)
            || world["subjects"] != self.demand["subjects"]
            || world["state"] != "healthy"
            || world["provenance"].get("superseded_by").is_some()
            || world["world_ref"].as_str().is_none_or(str::is_empty) {
            return Err(error("Material world is not the active selected task/attempt/Workcell; explicit re-resolution is required"));
        }
        let bindings = world["binding_graph"]["bindings"].as_array()
            .ok_or_else(|| error("Material world omitted its binding graph"))?;
        if bindings.iter().any(|b| b["necessity"] == "required"
            && (b["presence"] != "present" || b["health"] != "healthy")) {
            return Err(error("A required native material binding is not present and healthy"));
        }
        let now_ref = &task.allocation["now_ref"];
        let stores: Vec<_> = bindings.iter().filter(|b| b["port"] == "storage"
            && b["properties"]["logical_ref"] == *now_ref).collect();
        if stores.len() != 1 { return Err(error("Native task NOW must have exactly one actual storage binding")); }
        let store = stores[0];
        let now = task.now_directory()?;
        if store["necessity"] != "required" || store["presence"] != "present"
            || store["health"] != "healthy" || store["properties"]["path"] != json!(now)
            || store["properties"]["access"] != "writable" || store["properties"]["sharing"] != "shared"
            || store["properties"]["object_identity"] != local_identity(&now)? {
            return Err(error("Native storage is not the exact admitted NOW directory and required attachment"));
        }
        for service in &self.host.required_services {
            // A connectivity binding role is not the native service identity.
            if bindings.iter().filter(|b| b["port"] == "service" && b["properties"]["logical_ref"] == json!(service)
                && b["necessity"] == "required" && b["presence"] == "present" && b["health"] == "healthy").count() != 1 {
                return Err(error("Required native service was not actually prepared"));
            }
        }
        Ok(())
    }

    pub fn validate(&self, task: &AllocatedCentralTask) -> Result<Value> {
        self.check_world(task, &self.world)?;
        let current = self.host.call("inspect", Some(&self.world))?;
        self.check_world(task, &current)?;
        if current["world_ref"] != self.world["world_ref"]
            || current["binding_graph"] != self.world["binding_graph"]
            || current["provenance"]["demand_fingerprint"] != self.world["provenance"]["demand_fingerprint"] {
            return Err(error("Material binding changed; recovery is a new explicit binding, not opening another view"));
        }
        let observation = self.host.call("observe", Some(&self.world))?;
        if observation["world_ref"] != self.world["world_ref"] {
            return Err(error("Observation belongs to a different material world"));
        }
        let observations = observation["observations"].as_array()
            .ok_or_else(|| error("Material observation omitted provider observations"))?;
        for binding in current["binding_graph"]["bindings"].as_array().expect("checked graph") {
            if binding["necessity"] != "required" { continue; }
            let matches: Vec<_> = observations.iter().filter(|o| o["logical_ref"] == binding["logical_ref"]).collect();
            if matches.len() != 1 || matches[0]["state"] != "healthy"
                || matches[0]["detail"]["material_ref"] != binding["material_ref"]
                || matches[0]["detail"]["provider_ref"] != binding["provider_ref"] {
                return Err(error("Fresh provider observation does not confirm every required binding"));
            }
            if binding["port"] == "storage" && (matches[0]["detail"]["path"] != binding["properties"]["path"]
                || matches[0]["detail"]["object_identity"] != binding["properties"]["object_identity"]) {
                return Err(error("Observed NOW attachment changed path or filesystem identity"));
            }
        }
        Ok(observation)
    }
}

#[cfg(unix)]
fn local_identity(path: &std::path::Path) -> Result<String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).map_err(error)?;
    if !metadata.is_dir() { return Err(error("Task storage is no longer a directory")); }
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}
#[cfg(not(unix))]
fn local_identity(_path: &std::path::Path) -> Result<String> {
    Err(error("Same-object local storage correlation is unsupported on this platform"))
}
