from pathlib import Path
import subprocess
p=Path('crates/aikit-cli/src/encounter_service.rs')
if 'fn reconnect_native(' not in p.read_text():
    subprocess.run(['python3','.github/repair-direct.py'],check=True)
s=p.read_text()
needle='            let cleanup = host.shutdown();\n            self.store.append(&agent_session'
if needle in s:
    s=s.replace(needle,'''            let cleanup = host.shutdown();
            if cleanup.is_err() {
                // Refuse later effects if this body may still be live.
                self.shutdown_requested.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            self.store.append(&agent_session''')
    p.write_text(s)
p=Path('crates/aikit-cli/src/direct_agent_session.rs');s=p.read_text()
if 'pub fn skills(service:' not in s:
    s=s.replace('pub fn scope(service:', '''/// Read actual effective parent-Skill eligibility without activating anything.
pub fn skills(service: &Service) -> Result<Value> {
    let view = service.resolved();
    let rows: Vec<Value> = view.catalog_index.iter()
        .filter(|(_, entry)| entry.kind == aikit_core::Kind::Skill)
        .map(|(id, entry)| {
            let material = service.effective_skill_markdown(id);
            json!({"ref":id,"name":entry.name,"description":entry.description,
                "revision":entry.revision,"source":view.active.get(id).and_then(|a|a.source.as_ref()),
                "eligible":material.is_ok(),"reason_code":material.err().map(|e|e.code().to_owned())})
        }).collect();
    Ok(json!({"schema":"aikit.direct-agent-skills/v1","catalogue_revision":view.catalog_revision,
        "rows":rows,"activation_performed":false,"brokered_child_activation_observed":false}))
}

pub fn scope(service:''',1)
    needle='    Ok(\n        json!({"schema":"aikit.direct-agent-scope/v1", "project_ref":binding.project,'
    assert s.count(needle)==1
    s=s.replace(needle,'''    let executable = std::env::var_os("CENTRAL_CTRL_BIN").map(PathBuf::from).unwrap_or_else(||"ctrl".into());
    let mut warnings = Vec::new();
    let world = if cwd == central {
        aikit_adapters::central_world_sources::read_world_binding(&SystemRunner::new(), &executable, &central, "root", None, "control:root").ok()
    } else {
        cwd.strip_prefix(central.join("Work")).ok().and_then(|p|p.to_str())
            .and_then(|p|aikit_adapters::central_world_sources::read_project_binding(&SystemRunner::new(), &executable, &central, p, &mut warnings))
    };
    let readiness = match world {
        Some(world) => json!({"ready":true,"world_ref":world.world_ref,"inherited_root_lineage":world.inherited_root_lineage}),
        None => json!({"ready":false,"owner":"central","action":"central.world-relations.save",
            "requires_explicit_authored_source":true,"reason":"The effective native World declaration is absent or unavailable; inspect Central source before creating or changing it."}),
    };
'''+needle)
    s=s.replace('"cwd":cwd, "central_root":central, "binding":binding,','"cwd":cwd, "central_root":central, "binding":binding,"world_readiness":readiness,')
    old='''    let state = SessionSpaceApplicationStore::new(home.clone()).load(&binding.space)?;
    let ready = state.agent_sessions.contains_key(session)
        && state.project_contexts.get(&binding.project_context.project)
            == Some(&binding.project_context.context);'''
    new='''    // A published binding can precede Space creation/attachment. Read that
    // partial outcome without creating anything, so the same request can be
    // explicitly continued rather than minting a replacement session.
    let state = match SessionSpaceApplicationStore::new(home.clone()).load(&binding.space) {
        Ok(state) => Some(state),
        Err(e) if e.code() == "session_space.not_found" => None,
        Err(e) => return Err(e),
    };
    let ready = state.as_ref().is_some_and(|state| state.agent_sessions.contains_key(session)
        && state.project_contexts.get(&binding.project_context.project)
            == Some(&binding.project_context.context));'''
    assert s.count(old)==1;s=s.replace(old,new)
    s=s.replace('"prepared":ready,"provider_started":false','"prepared":ready,"resume_preparation_allowed":!ready,"provider_started":false')
    p.write_text(s)
p=Path('crates/aikit-cli/src/session_space_cli.rs');s=p.read_text()
if 'AgentSessionSkills' not in s:
    s=s.replace('    AgentSessionScope,','    AgentSessionScope,\n    /// Read effective parent-Skill choices without activating or projecting.\n    AgentSessionSkills,',1)
    s=s.replace('        Command::AgentSessionScope =>','        Command::AgentSessionSkills => emit(&crate::direct_agent_session::skills(&service)?),\n        Command::AgentSessionScope =>',1)
    p.write_text(s)
