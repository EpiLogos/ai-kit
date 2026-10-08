//! Central team preparation and addressed work consume the existing Agency,
//! SessionSpace, task, Workcell-bound launcher and delivery journals. There is
//! no team mailbox or process supervisor here.
use super::*;
use crate::encounter_service::{
    self, EncounterAgencyBinding, EncounterContextAdmission, EncounterRequest,
    EncounterRequiredSource, EncounterService,
};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
};
use aikit_core::{ResourceRef, SourceRevision};
use aikit_store::SessionSpaceApplicationStore;

fn invalid(message: impl Into<String>) -> AikitError {
    AikitError::new("inhabit.team_operation_refused", message)
}
fn required<'a>(input: &'a Value, key: &str) -> Result<&'a str> {
    input[key]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid(format!("Missing bounded team operation field {key}")))
}

fn member(home: &AikitHome, generation: &str, agent: &str) -> Result<(Value, Value)> {
    let receipt = existing(home, generation)
        .ok_or_else(|| invalid("This tenure has no retained team projection"))?;
    if receipt["generation_ref"] != generation {
        return Err(invalid("Team generation changed"));
    }
    let mut selected = None;
    for file in receipt["files"].as_array().into_iter().flatten() {
        let path = required(file, "path")?;
        if !path.ends_with("/team.json") {
            continue;
        }
        let bytes = std::fs::read(path).map_err(|error| invalid(error.to_string()))?;
        let text = std::str::from_utf8(&bytes).map_err(|error| invalid(error.to_string()))?;
        if file["digest"] != digest(text) {
            return Err(invalid("Retained team manifest material changed"));
        }
        let team: Value =
            serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
        for entry in team["members"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["agent_ref"] == agent)
        {
            if selected.replace(entry.clone()).is_some() {
                return Err(invalid("Team member is ambiguous across selected sets"));
            }
        }
    }
    Ok((
        receipt,
        selected.ok_or_else(|| {
            invalid("The selected Agent is not a member of this tenure's Central-authored team")
        })?,
    ))
}

#[cfg(unix)]
fn call(home: &AikitHome, request: &EncounterRequest) -> Result<Value> {
    let response = encounter_service::request(&encounter_service::socket_path(home), request)?;
    if response["ok"] != true {
        return Err(
            invalid(format!("Native encounter refused: {}", response["error"]))
                .with("native_receipt", response.to_string()),
        );
    }
    Ok(response["data"].clone())
}
#[cfg(not(unix))]
fn call(_: &AikitHome, _: &EncounterRequest) -> Result<Value> {
    Err(invalid(
        "Native team encounter transport is unavailable on this host",
    ))
}

fn check_child_binding_revision(
    task: &Value,
    binding: &EncounterAgencyBinding,
    expected: &str,
) -> Result<()> {
    if task["agency_revision"] != json!(binding.revision) || binding.revision.as_str() != expected {
        return Err(invalid(
            "Selected child Agency basis changed; reconcile retained preparation before read, cancellation or release",
        ));
    }
    Ok(())
}

fn check_child(
    home: &AikitHome,
    session: &ResourceRef,
    member: &str,
    parent: &ResourceRef,
    expected_task: &Value,
    expected_binding_revision: &str,
) -> Result<EncounterAgencyBinding> {
    let binding = EncounterService::read_agency_binding(home, session)?
        .ok_or_else(|| invalid("Child has no operative Agency; prepare this team member first"))?;
    let task = EncounterService::read_task(home, session)?;
    check_child_binding_revision(&task, &binding, expected_binding_revision)?;
    if !binding.active {
        return Err(invalid("Selected child Agency is inactive"));
    }
    binding
        .context
        .as_ref()
        .ok_or_else(|| invalid("Selected child has no retained scoped context"))?
        .verify()?;

    let exact_pairs = [
        ("/revision", "revision"),
        ("/request/central/task_ref", "task_ref"),
        ("/allocation/allocation/now_ref", "now_ref"),
        ("/allocation/allocation/revision/revision", "now_revision"),
        ("/allocation/allocation/policy/revision", "policy_revision"),
        ("/request/cwd", "cwd"),
    ];
    if binding.agent_ref.as_str() != member
        || !binding.allowed_senders.contains(parent)
        || task["ready"] != true
        || exact_pairs
            .iter()
            .any(|(path, key)| task.pointer(path) != expected_task.get(key))
        || json!(binding.agent_ref) != expected_task["agent_ref"]
        || json!(binding.agency_ref) != expected_task["agency_ref"]
        || json!(binding.world_binding_ref) != expected_task["world_binding_ref"]
        || json!(binding.agency_source.source_ref) != expected_task["source_ref"]
        || json!(binding.agency_source.revision) != expected_task["source_revision"]
        || binding.agency_source.content_digest != expected_task["source_digest"]
        || !task["request"]["central"]["participant_refs"]
            .as_array()
            .is_some_and(|refs| refs.contains(&json!(parent)))
    {
        return Err(invalid(
            "Exact child Agent, parent relation or selected task changed; reconcile retained preparation before another effect",
        ));
    }
    EncounterService::verify_task_admission(home, session)?;
    Ok(binding)
}

fn resolve_team_repertoire(
    home: &AikitHome,
    repertoire: &mut crate::app::Service,
    old_task: &Value,
    request: &Value,
    binding: &EncounterAgencyBinding,
    reviewed: crate::app::repertoire::RepertoirePreview,
) -> Result<crate::app::repertoire::RepertoireApplication> {
    if !old_task.is_null() {
        let mut retained_request = old_task["request"].clone();
        let selected = retained_request.as_object_mut()
                    .and_then(|request| request.remove("repertoire"))
                    .ok_or_else(|| invalid("Existing child Task has no retained repertoire; reconcile the original admission"))?;
        let mut requested_basis = request.clone();
        requested_basis
            .as_object_mut()
            .ok_or_else(|| invalid("Task request must be an object"))?
            .remove("repertoire");
        if retained_request != requested_basis
            || old_task["agency_revision"] != json!(binding.revision)
            || selected["procedure"] != json!(reviewed.procedure.id)
        {
            return Err(invalid(
                "Retained Task request, Agency or selected Procedure differs; reconcile before another composition or execution effect",
            ));
        }
        let reading: aikit_core::repertoire::RepertoireReading =
            serde_json::from_value(selected["reading"].clone())
                .map_err(|error| invalid(error.to_string()))?;
        let procedure: aikit_core::ProcedureId =
            serde_json::from_value(selected["procedure"].clone())
                .map_err(|error| invalid(error.to_string()))?;
        repertoire.verify_repertoire_procedure(&reading, &procedure)?;
        Ok(crate::app::repertoire::RepertoireApplication {
            reading,
            procedure: procedure.clone(),
            applied_edits: 0,
            reused_generation: true,
            recovered: true,
            observation: None,
            undo: format!(
                "AIKIT_HOME={} aikit procedure undo {procedure}",
                crate::app::repertoire::shell_word(&home.root().display().to_string())
            ),
        })
    } else {
        repertoire.apply_repertoire(reviewed)
    }
}

/// The native operation shared by Pi's team tool and structured CLI consumers.
/// Preparation takes explicit existing owner bases, and may use the standing
/// Agency mint when one was authored. Lack of an actual grant is a native
/// refusal, never a request to hand-copy commands or an unprotected spawn.
pub fn team_operation(home: &AikitHome, cwd: &Path, input: Value) -> Result<Value> {
    let owners = crate::gateway_owners::ProcessOwners::from_env();
    let occupant = crate::communique_turn::turn_occupant(&owners)
        .map_err(invalid)?
        .ok_or_else(|| invalid("Team operations require the actual current occupying parent"))?;
    let member_ref = required(&input, "member_ref")?;
    let (receipt, member) = member(home, &occupant.generation_ref, member_ref)?;
    let parent = ResourceRef::parse(required(&receipt, "orchestrator_agent_ref")?)?;
    let session = ResourceRef::parse(required(&input, "agent_session")?)?;
    if !session.as_str().starts_with("agent-session/") {
        return Err(invalid("Child must have a canonical AgentSession identity"));
    }
    match required(&input, "action")? {
        "prepare" => {
            let prep = &input["preparation"];
            let space = SessionSpaceRef::parse(required(prep, "space")?)?;
            let mut request = prep["task_request"].clone();
            let parent_now = required(&request["central"], "parent_now_ref")?;
            let workcell = required(&request["central"], "workcell_ref")?;
            if occupant.workcell_ref.as_deref() != Some(workcell) {
                return Err(invalid(
                    "Task must stand on the current parent's actual Workcell; remote preparation needs the admitted remote owner route",
                ));
            }
            let root_now = owners
                .run_ctrl_action("central.now.read", &json!({"now_ref":parent_now}))
                .map_err(|error| invalid(format!("Parent NOW unavailable: {error:?}")))?;
            if root_now["record"]["now_ref"] != parent_now
                || root_now["record"]["workcell_ref"] != workcell
                || root_now["record"]["lifecycle"] != "active"
                || root_now["revision"]["revision"] != required(prep, "parent_now_revision")?
            {
                return Err(invalid(
                    "Parent NOW, Workcell or exact source revision changed; reconcile before child preparation",
                ));
            }
            let task_cwd = PathBuf::from(required(&request, "cwd")?);
            let participants = request["central"]["participant_refs"]
                .as_array()
                .ok_or_else(|| invalid("Task has no explicit participants"))?;
            if !participants.contains(&json!(parent)) || !participants.contains(&json!(member_ref))
            {
                return Err(invalid(
                    "Task must retain both the actual parent and selected member",
                ));
            }
            let source = required(&member, "expression_ref")?;
            let root = PathBuf::from(required(&request["central"], "central_root")?);
            let relative = source
                .strip_prefix(CENTRAL_ROOT_SOURCE_PREFIX)
                .ok_or_else(|| invalid("Member expression is outside this Central root"))?;
            let path = root
                .join(relative)
                .canonicalize()
                .map_err(|error| invalid(error.to_string()))?;
            if !path.starts_with(
                root.canonicalize()
                    .map_err(|error| invalid(error.to_string()))?,
            ) {
                return Err(invalid("Member source escaped Central"));
            }
            let bytes = std::fs::read(&path).map_err(|error| invalid(error.to_string()))?;
            let source_digest = format!("blake3:{}", blake3::hash(&bytes).to_hex());
            if member["expression_digest"] != source_digest {
                return Err(invalid(
                    "The selected authored member expression changed since this tenure was composed",
                ));
            }
            let store = SessionSpaceApplicationStore::new(home.clone());
            // Reuse a compatible retained place rather than allocating another
            // workspace or launching a second parent process.
            let found = store.list()?.into_iter().find(|state| state.id() == &space);
            if found.is_none() {
                let preview = store.stage(
                    None,
                    SessionSpaceMutation::Create {
                        id: space.clone(),
                        label: Some("Central native team".into()),
                    },
                )?;
                store.apply(&preview)?;
            }
            let state = store.load(&space)?;
            if !state.agent_sessions.contains_key(&session) {
                let preview = store.stage(
                    Some(&space),
                    SessionSpaceMutation::AttachAgentSession {
                        attachment: SessionSpaceAgentAttachmentIntent {
                            agent_session: session.clone(),
                            purpose: request["central"]["purpose"].as_str().map(str::to_owned),
                            provenance: vec![format!(
                                "Central member {member_ref}; parent {parent}; generation {}",
                                occupant.generation_ref
                            )],
                        },
                    },
                )?;
                store.apply(&preview)?;
            }
            let mut existing_binding = EncounterService::read_agency_binding(home, &session)?;
            let mut minted = false;
            let mut binding = if let Some(current) = &existing_binding {
                current.clone()
            } else if !prep["agency_binding"].is_null() {
                serde_json::from_value(prep["agency_binding"].clone())
                    .map_err(|error| invalid(error.to_string()))?
            } else {
                minted = true;
                encounter_service::mint_task_from_cli(
                    home,
                    &task_cwd,
                    &session,
                    Some(ResourceRef::parse(member_ref)?),
                )?;
                existing_binding = EncounterService::read_agency_binding(home, &session)?;
                existing_binding
                    .clone()
                    .ok_or_else(|| invalid("Native Agency mint produced no binding"))?
            };
            if minted && binding.agent_ref.as_str() == member_ref {
                // Native team preparation is the owner-side admission, bounded
                // by this actual authored parent/member/task relation. It changes
                // one sender and one packet-source disclosure, never the grant.
                EncounterService::admit_agency_disclosure(
                    home,
                    &session,
                    &parent,
                    &ResourceRef::parse(required(&request["central"], "task_ref")?)?,
                )?;
                existing_binding = EncounterService::read_agency_binding(home, &session)?;
                binding = existing_binding
                    .clone()
                    .ok_or_else(|| invalid("Native child admission was not retained"))?;
            }
            if binding.agent_ref.as_str() != member_ref
                || !binding.allowed_senders.contains(&parent)
            {
                return Err(invalid(
                    "Actual child grant must bind the selected member and permit this parent; team membership grants no authority",
                ));
            }
            let selected = EncounterRequiredSource {
                source: ResourceRef::parse(source)?,
                revision: SourceRevision::parse(format!("rev/{}", &source_digest[7..23]))?,
                path,
                content_digest: source_digest,
            };
            let allowed: std::collections::BTreeSet<String> = member["governance_refs"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(member["skill_refs"].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .chain(std::iter::once(source.to_owned()))
                .chain(
                    request["central"]["source_refs"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned),
                )
                .collect();
            let context = binding.context.get_or_insert(EncounterContextAdmission {
                sources: vec![],
                source_activations: vec![],
                projection: None,
                activation: None,
            });
            if context
                .sources
                .iter()
                .any(|source| !allowed.contains(source.source.as_str()))
            {
                return Err(invalid(
                    "Child context contains source outside its selected member and task basis",
                ));
            }
            if !context
                .sources
                .iter()
                .any(|source| source.source == selected.source)
            {
                context.sources.push(selected);
            }
            for reference in member["governance_refs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let Some(relative) = reference.strip_prefix(CENTRAL_ROOT_SOURCE_PREFIX) else {
                    continue;
                };
                let path = root
                    .join(relative)
                    .canonicalize()
                    .map_err(|error| invalid(error.to_string()))?;
                if !path.starts_with(
                    root.canonicalize()
                        .map_err(|error| invalid(error.to_string()))?,
                ) {
                    return Err(invalid("Selected member governance escaped its World root"));
                }
                let bytes = std::fs::read(&path).map_err(|error| invalid(error.to_string()))?;
                let digest = format!("blake3:{}", blake3::hash(&bytes).to_hex());
                if member["governance_digests"][reference] != digest {
                    return Err(invalid(
                        "Selected member governance changed since native team composition",
                    ));
                }
                let admitted = EncounterRequiredSource {
                    source: ResourceRef::parse(reference)?,
                    revision: SourceRevision::parse(format!("rev/{}", &digest[7..23]))?,
                    path,
                    content_digest: digest,
                };
                if let Some(existing) = context
                    .sources
                    .iter()
                    .find(|source| source.source == admitted.source)
                {
                    if existing != &admitted {
                        return Err(invalid(
                            "Retained child governance differs from the exact selected source",
                        ));
                    }
                } else {
                    context.sources.push(admitted);
                }
            }
            context.verify()?;
            if existing_binding.as_ref() != Some(&binding) {
                let expected = existing_binding.as_ref().map(|current| &current.revision);
                if expected.is_some() {
                    binding.revision =
                        SourceRevision::parse(format!("team-binding/{}", ulid::Ulid::generate()))?;
                }
                EncounterService::configure_agency(home, &session, &binding, expected)?;
            }
            // The reviewed A Procedure operates in the actual child's own
            // Session/Context; no parent overlay or trust grant is inherited.
            let mut repertoire = EncounterService::open_task_repertoire(home, &task_cwd, &session)?;
            let reviewed: crate::app::repertoire::RepertoirePreview =
                serde_json::from_value(prep["repertoire_preview"].clone())
                    .map_err(|error| invalid(error.to_string()))?;
            if reviewed.request.scope != aikit_core::scope::ScopeKind::Session
                || reviewed.reading.context_id
                    != EncounterService::task_repertoire_context_id(&session).as_str()
            {
                return Err(invalid(
                    "Team repertoire must be reviewed for this exact child Session",
                ));
            }
            if reviewed
                .reading
                .members
                .iter()
                .any(|capsule| !allowed.contains(&capsule.id.to_string()))
            {
                return Err(invalid(
                    "Selected repertoire includes source outside this actual member and explicit task basis",
                ));
            }
            // Reconcile a retained native application before another effect.
            // The original preview predates its published generation, so repeating
            // it must not mint a replacement Procedure or widen the Task request.
            let old_task = EncounterService::read_task(home, &session)?;
            let application = resolve_team_repertoire(
                home,
                &mut repertoire,
                &old_task,
                &request,
                &binding,
                reviewed,
            )?;
            repertoire.verify_repertoire_procedure(&application.reading, &application.procedure)?;
            request["repertoire"] =
                json!({"reading":application.reading,"procedure":application.procedure});
            let task = if old_task["ready"] == true
                && old_task["request"] == request
                && old_task["agency_revision"] == json!(binding.revision)
            {
                old_task
            } else {
                if !old_task.is_null() && old_task["request"] != request {
                    return Err(invalid(
                        "Existing child task differs; do not replace uncertain or unrelated work",
                    ));
                }
                let expected = old_task["revision"]
                    .as_str()
                    .map(SourceRevision::parse)
                    .transpose()?;
                EncounterService::configure_task(home, &session, request, expected.as_ref())?
            };
            #[cfg(unix)]
            encounter_service::start(home, cwd)?;
            let opened = call(
                home,
                &EncounterRequest::Open {
                    space,
                    agent_session: session.clone(),
                    provider: required(&task["launcher"], "id")?.into(),
                    cwd: task_cwd,
                },
            )?;
            let expected_task = json!({"revision":task["revision"],"task_ref":task["request"]["central"]["task_ref"],
                "now_ref":task["allocation"]["allocation"]["now_ref"],"now_revision":task["allocation"]["allocation"]["revision"]["revision"],
                "policy_revision":task["allocation"]["allocation"]["policy"]["revision"],"cwd":task["request"]["cwd"],
                "agent_ref":binding.agent_ref,"agency_ref":binding.agency_ref,"world_binding_ref":binding.world_binding_ref,
                "source_ref":binding.agency_source.source_ref,"source_revision":binding.agency_source.revision,"source_digest":binding.agency_source.content_digest});
            Ok(
                json!({"schema":"aikit.team-operation/v1","action":"prepare","member_ref":member_ref,"parent_ref":parent,
                "agent_session":session,"generation_ref":occupant.generation_ref,"expected_binding_revision":binding.revision,
                "expected_task":expected_task,"repertoire":application,"task":task,"native":opened,"return_to":occupant.position_ref,"parent_now":root_now}),
            )
        }
        "delegate" => {
            let mut turn: crate::encounter_service::EncounterAddressedTurn =
                serde_json::from_value(input["turn"].clone())
                    .map_err(|error| invalid(error.to_string()))?;
            let expected = turn.expected_task.as_ref().ok_or_else(|| {
                invalid("Delegated work requires the exact prepared child task basis")
            })?;
            let binding = check_child(
                home,
                &session,
                member_ref,
                &parent,
                &serde_json::to_value(expected).map_err(|error| invalid(error.to_string()))?,
                turn.expected_binding_revision.as_str(),
            )?;
            if turn.sender != parent
                || turn.expected_binding_revision != binding.revision
                || !turn.packet.audience.contains(&binding.agent_ref)
            {
                return Err(invalid(
                    "Delegation changed parent, audience or admitted child generation",
                ));
            }
            // Existing Send owns duplicate/uncertain delivery reconciliation;
            // never clear a human draft or drive the provider a second way.
            turn.sender = parent.clone();
            let delivery_ref = turn.delivery_ref.clone();
            let result = call(
                home,
                &EncounterRequest::Send {
                    agent_session: session.clone(),
                    turn,
                },
            )?;
            Ok(
                json!({"schema":"aikit.team-operation/v1","action":"delegate","member_ref":member_ref,"parent_ref":parent,"agent_session":session,"delivery_ref":delivery_ref,"result":result,"return_to":occupant.position_ref}),
            )
        }
        action @ ("read" | "cancel" | "release") => {
            check_child(
                home,
                &session,
                member_ref,
                &parent,
                &input["expected_task"],
                required(&input, "expected_binding_revision")?,
            )?;
            let request = match action {
                "read" => EncounterRequest::DeliveryReply {
                    agent_session: session.clone(),
                    delivery_ref: ResourceRef::parse(required(&input, "delivery_ref")?)?,
                },
                "cancel" => EncounterRequest::Cancel {
                    agent_session: session.clone(),
                    reason: Some(required(&input, "reason")?.into()),
                },
                _ => EncounterRequest::ReleaseNative {
                    agent_session: session.clone(),
                    expected_native_session_id: required(&input, "expected_native_session_id")?
                        .into(),
                    expected_generation: required(&input, "expected_generation")?.into(),
                },
            };
            Ok(
                json!({"schema":"aikit.team-operation/v1","action":action,"member_ref":member_ref,"agent_session":session,"result":call(home,&request)?}),
            )
        }
        _ => Err(invalid(
            "Use prepare, delegate, read, cancel or exact idle release",
        )),
    }
}

#[cfg(test)]
mod child_binding_revision_tests {
    use super::*;
    #[test]
    fn native_owner_cas_binding_advance_refuses_prior_preparation_basis() {
        let directory = tempfile::tempdir().unwrap();
        let home = AikitHome::at(directory.path());
        home.ensure_layout().unwrap();
        let session = ResourceRef::parse("agent-session/revision-guard-child").unwrap();
        // Inactive metadata exercises the actual native CAS/persist/read owner,
        // without asserting an operative grant, prepared process or live Return.
        let before:EncounterAgencyBinding=serde_json::from_value(json!({
            "revision":"rev/guard-before","active":false,"agent_ref":"agent/revision-guard-child",
            "agency_ref":"agency/revision-guard","world_ref":"world/revision-guard","world_binding_ref":"world-binding/revision-guard",
            "agency_source":{"source_ref":"source/revision-guard","revision":"rev/source","path":"/unused-inactive-source","content_digest":format!("blake3:{}","0".repeat(64))},
            "actuation_bin":"/unused-no-live-grant","allowed_senders":["agent/revision-guard-parent"],
            "allowed_packet_sources":[],"context":null})).unwrap();
        EncounterService::configure_agency(&home, &session, &before, None).unwrap();
        let retained_before = EncounterService::read_agency_binding(&home, &session)
            .unwrap()
            .unwrap();
        let mut after = before.clone();
        after.revision = SourceRevision::parse("rev/guard-after").unwrap();
        EncounterService::configure_agency(&home, &session, &after, Some(&before.revision))
            .unwrap();
        let retained_after = EncounterService::read_agency_binding(&home, &session)
            .unwrap()
            .unwrap();
        assert_eq!(retained_after.revision, after.revision);
        assert!(
            check_child_binding_revision(
                &json!({"agency_revision":retained_before.revision}),
                &retained_after,
                retained_after.revision.as_str()
            )
            .is_err(),
            "a current callback cannot bless the old task binding"
        );
        assert!(
            check_child_binding_revision(
                &json!({"agency_revision":retained_after.revision}),
                &retained_after,
                retained_before.revision.as_str()
            )
            .is_err(),
            "the old prepared receipt cannot address a successor binding"
        );
        assert!(check_child_binding_revision(
            &json!({"agency_revision":retained_after.revision}),
            &retained_after,
            retained_after.revision.as_str()
        )
        .is_ok());
    }
}

#[cfg(test)]
mod retained_repertoire_tests {
    use super::*;
    use aikit_core::catalog::Catalog;
    use aikit_core::{CapsuleId, ProfileId, TrustKey, TrustState};
    use std::{collections::BTreeMap, fs, path::PathBuf, time::SystemTime};
    fn write(path: &Path, material: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, material).unwrap();
    }
    fn material(root: &Path) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
        let mut files = BTreeMap::new();
        fn visit(root: &Path, files: &mut BTreeMap<PathBuf, (Vec<u8>, SystemTime)>) {
            for entry in fs::read_dir(root).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                if metadata.is_dir() {
                    visit(&path, files);
                } else if metadata.is_file()
                    && matches!(
                        path.extension().and_then(|e| e.to_str()),
                        Some("toml" | "md" | "json")
                    )
                {
                    files.insert(
                        path,
                        (
                            fs::read(entry.path()).unwrap(),
                            metadata.modified().unwrap(),
                        ),
                    );
                }
            }
        }
        visit(root, &mut files);
        files
    }
    #[test]
    fn native_application_replay_retains_procedure_and_refuses_changed_task_selection_or_material()
    {
        let owned = tempfile::tempdir().unwrap();
        let home = AikitHome::at(owned.path().join("home with space"));
        let project = owned.path().join("project");
        let capsule = home
            .root()
            .join("registries/personal/capsules/skill/development/plain");
        write(
            &capsule.join("manifest.toml"),
            "schema = 1\nid = \"skill/development/plain\"\nkind = \"skill\"\nname = \"plain\"\n[skill]\nroot = \"payload\"\n",
        );
        write(
            &capsule.join("payload/SKILL.md"),
            "---\nname: plain\ndescription: Actual accepted source material\n---\nCarry the exact task.\n",
        );
        write(
            &home
                .root()
                .join("registries/personal/profiles/development.toml"),
            "schema = 1\nid = \"profile/development\"\nenable = [\"skill/development/plain\"]\n",
        );
        write(&project.join(".aikit/profile.toml"), "schema = 1\n");
        aikit_store::skillsets::create(
            &home,
            "member",
            &[CapsuleId::parse("skill/development/plain").unwrap()],
            &[],
        )
        .unwrap();
        home.ensure_layout().unwrap();
        let catalog = crate::app::load_catalog(&home, Some(&project)).unwrap();
        let index = aikit_store::index::Index::open(&home.database()).unwrap();
        for capsule in catalog.catalog.capsules() {
            let key = TrustKey::new(
                capsule.source.clone().unwrap(),
                capsule.id.clone(),
                capsule.revision.clone().unwrap(),
            );
            aikit_store::trust::TrustStore::new(&index)
                .record(
                    &key,
                    TrustState::Reviewed,
                    Some("exact regression source review"),
                )
                .unwrap();
        }
        let session = ResourceRef::parse("agent-session/actual-repertoire-replay").unwrap();
        let mut service =
            EncounterService::open_task_repertoire(&home, &project, &session).unwrap();
        let preview = service
            .preview_repertoire(crate::app::repertoire::RepertoireRequest {
                scope: aikit_core::scope::ScopeKind::Session,
                profile: Some(ProfileId::parse("profile/development").unwrap()),
                skill_sets: vec!["member".into()],
            })
            .unwrap();
        // Inactive Agency metadata is only this material function's selected
        // revision input; it never claims actual task admission or a live body.
        let binding:EncounterAgencyBinding=serde_json::from_value(json!({
            "revision":"rev/retained-basis","active":false,"agent_ref":"agent/retained-child",
            "agency_ref":"agency/retained-child","world_ref":"world/retained-test","world_binding_ref":"world-binding/retained-test",
            "agency_source":{"source_ref":"source/retained-test","revision":"rev/source","path":"/unused-inactive-source","content_digest":format!("blake3:{}","0".repeat(64))},
            "actuation_bin":"/unused-no-live-grant","allowed_senders":[],"allowed_packet_sources":[],"context":null})).unwrap();
        let request = json!({"central":{"task_ref":"task/selected-material"},"cwd":project});
        let first = resolve_team_repertoire(
            &home,
            &mut service,
            &Value::Null,
            &request,
            &binding,
            preview.clone(),
        )
        .unwrap();
        let mut retained_request = request.clone();
        retained_request["repertoire"] =
            json!({"reading":first.reading,"procedure":first.procedure});
        let retained = json!({"request":retained_request,"agency_revision":binding.revision});
        let retained_file = home.root().join("state/retained-material-regression.json");
        write(&retained_file, &retained.to_string());
        let retained: Value = serde_json::from_slice(&fs::read(&retained_file).unwrap()).unwrap();
        let before = material(home.root());
        let replay = resolve_team_repertoire(
            &home,
            &mut service,
            &retained,
            &request,
            &binding,
            preview.clone(),
        )
        .unwrap();
        assert_eq!(replay.procedure, first.procedure);
        assert_eq!(replay.reading, first.reading);
        assert!(replay.recovered && replay.reused_generation && replay.applied_edits == 0);
        assert_eq!(
            replay.undo, first.undo,
            "exact home quoting belongs to the existing A format owner"
        );
        let mut other_request = request.clone();
        other_request["central"]["task_ref"] = json!("task/unrelated-material");
        assert!(resolve_team_repertoire(
            &home,
            &mut service,
            &retained,
            &other_request,
            &binding,
            preview.clone()
        )
        .is_err());
        let mut other_preview = preview.clone();
        other_preview.procedure.id = aikit_core::ProcedureId::generate();
        assert!(resolve_team_repertoire(
            &home,
            &mut service,
            &retained,
            &request,
            &binding,
            other_preview
        )
        .is_err());
        let mut other_binding = binding.clone();
        other_binding.revision = SourceRevision::parse("rev/changed-basis").unwrap();
        assert!(resolve_team_repertoire(
            &home,
            &mut service,
            &retained,
            &request,
            &other_binding,
            preview.clone()
        )
        .is_err());
        assert_eq!(
            material(home.root()),
            before,
            "replay/refusal must preserve overlay, Procedure, source and generation bytes/mtimes"
        );
        let context = home.context_dir(&EncounterService::task_repertoire_context_id(&session));
        let pi = context
            .join("generations")
            .join(first.reading.generation.as_ref().unwrap().as_str())
            .join("projections/pi/.pi/skills");
        fn first_managed_skill(root: &Path) -> PathBuf {
            for entry in fs::read_dir(root).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                if metadata.file_type().is_symlink() {
                    return path;
                }
                if metadata.is_dir() {
                    return first_managed_skill(&path);
                }
            }
            panic!("actual Pi generation has no managed skill link")
        }
        let source_before = fs::read(capsule.join("payload/SKILL.md")).unwrap();
        let managed = first_managed_skill(&pi);
        assert!(fs::symlink_metadata(&managed)
            .unwrap()
            .file_type()
            .is_symlink());
        fs::remove_file(&managed).unwrap();
        assert_eq!(
            fs::read(capsule.join("payload/SKILL.md")).unwrap(),
            source_before,
            "missing target repair must preserve accepted source material"
        );
        assert!(
            resolve_team_repertoire(&home, &mut service, &retained, &request, &binding, preview)
                .is_err(),
            "native generation drift must not bless retained execution"
        );
    }
}
