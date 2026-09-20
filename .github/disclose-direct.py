from pathlib import Path
p=Path('crates/aikit-cli/src/direct_agent_session.rs');s=p.read_text()
assert 'pub fn skills(service:' not in s
s=s.replace('pub fn scope(service:', '''/// Parent-session choices from the actual effective Skill catalogue.
/// No automatic activation or brokered-child grant is implied.
pub fn skills(service: &Service) -> Result<Value> {
    let view = service.resolved();
    let rows: Vec<Value> = view.catalog_index.iter()
        .filter(|(_, entry)| entry.kind == aikit_core::Kind::Skill)
        .map(|(id, entry)| {
            let material = service.effective_skill_markdown(id);
            json!({"ref":id,"name":entry.name,"description":entry.description,
                "revision":entry.revision,"source":view.active.get(id).and_then(|a|a.source.as_ref()),
                "eligible":material.is_ok(),"reason":material.err().map(|e|e.to_string())})
        }).collect();
    Ok(json!({"schema":"aikit.direct-agent-skills/v1","catalogue_revision":view.catalog_revision,
        "rows":rows,"activation_performed":false,"brokered_child_activation_observed":false}))
}

pub fn scope(service:''',1)
needle='    Ok(json!({"schema":"aikit.direct-agent-scope/v1", "project_ref":binding.project,'
assert s.count(needle)==1
s=s.replace(needle,'''    let executable = std::env::var_os("CENTRAL_CTRL_BIN").map(PathBuf::from).unwrap_or_else(||"ctrl".into());
    let mut warnings = Vec::new();
    let world = if cwd == central {
        aikit_adapters::central_world_sources::read_world_binding(&SystemRunner::new(), &executable, &central, "root", None, "control:root").map_err(|e|e.to_string())
    } else {
        let member = cwd.strip_prefix(central.join("Work")).ok().and_then(|p|p.to_str());
        member.and_then(|p|aikit_adapters::central_world_sources::read_project_binding(&SystemRunner::new(), &executable, &central, p, &mut warnings)).ok_or_else(||format!("Native World declaration unavailable: {}",warnings.join("; ")))
    };
    let readiness = match world {
        Ok(world) => json!({"ready":true,"world_ref":world.world_ref,"inherited_root_lineage":world.inherited_root_lineage}),
        Err(reason) => json!({"ready":false,"reason":reason,"owner":"central","action":"central.world-relations.save","requires_explicit_authored_source":true}),
    };
'''+needle)
s=s.replace('"cwd":cwd, "central_root":central, "binding":binding,','"cwd":cwd, "central_root":central, "binding":binding,"world_readiness":readiness,')
p.write_text(s)
p=Path('crates/aikit-cli/src/session_space_cli.rs');s=p.read_text();s=s.replace('    AgentSessionScope,','    AgentSessionScope,\n    /// Read native Skill eligibility; never activate or project on a read.\n    AgentSessionSkills,',1);s=s.replace('        Command::AgentSessionScope =>','        Command::AgentSessionSkills => emit(&crate::direct_agent_session::skills(&service)?),\n        Command::AgentSessionScope =>',1);p.write_text(s)
