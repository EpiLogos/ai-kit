from pathlib import Path
p=Path('crates/aikit-cli/src/encounter_service.rs');s=p.read_text()
needle='    pub fn apply(&self, request: EncounterRequest) -> Result<Value> {'
assert s.count(needle)==1
assert 'fn reconnect_native(' not in s
s=s.replace(needle,'''    /// Reconnect a failed body under the exclusive owner lease. A view-only
    /// reconnect cannot stop another session, replay a turn or mint a replacement.
    fn reconnect_native(&self, space: SessionSpaceRef, agent_session: ResourceRef, provider: String, cwd: PathBuf) -> Result<Value> {
        let mut lifecycle = self.lifecycle.write().map_err(error)?;
        if self.shutdown_requested.load(std::sync::atomic::Ordering::SeqCst)
            || !matches!(*lifecycle, Lifecycle::Running) {
            return Err(AikitError::new("encounter.owner_stopped", "Owner is stopping or requires cleanup repair"));
        }
        let cwd = std::fs::canonicalize(cwd).map_err(error)?;
        self.require_attached(&agent_session)?;
        crate::direct_agent_session::check(&self.home, &agent_session, &cwd)?;
        let mut residents = self.residents.lock().map_err(error)?;
        if let Some(held) = residents.get(&agent_session) {
            if held.space != space || held.provider != provider || held.cwd != cwd {
                return Err(AikitError::new("encounter.reconnect_basis", "Reconnect cannot change the native session's Project, Space or provider"));
            }
            if held.host.transport_error().is_none() {
                drop(residents);
                return self.open_native(space, agent_session, provider, cwd, true, None);
            }
            if !held.host.descriptor()?.capabilities.supports(SessionOpenMode::Load) {
                return Err(AikitError::new("encounter.load_unsupported", "This harness did not advertise native load; the failed session remains inspectable"));
            }
            self.store.append(&agent_session, &json!({"kind":"native-reconnect-requested", "native_session_id":held.lane.binding().native_session_id, "previous_turn_outcome":"unknown; not-replayed"}))?;
            let removed = residents.remove(&agent_session).expect("held resident");
            let removed = match Arc::try_unwrap(removed) {
                Ok(resident) => resident,
                Err(held) => {
                    residents.insert(agent_session.clone(), held);
                    return Err(AikitError::new("encounter.resident_in_use", "The failed body is still borrowed; inspect and explicitly retry after it settles"));
                }
            };
            if let Err(failure) = removed.host.shutdown() {
                let reason = format!("Failed body cleanup is uncertain: {failure}");
                *lifecycle = Lifecycle::Failed(reason.clone());
                let _ = self.store.append(&agent_session, &json!({"kind":"native-reconnect-cleanup-uncertain","reason":reason}));
                return Err(AikitError::new("encounter.cleanup_uncertain", reason));
            }
            self.permissions.lock().map_err(error)?.remove(&agent_session);
        }
        drop(residents);
        self.open_native(space, agent_session, provider, cwd, true, None)
    }

'''+needle)
needle='        if let EncounterRequest::Shutdown { expected_pid } = &request {'
assert s.count(needle)==1
s=s.replace(needle,'''        if let EncounterRequest::Reconnect { space, agent_session, provider, cwd } = request {
            return self.reconnect_native(space, agent_session, provider, cwd);
        }
'''+needle)
needle='        let native = lane.binding().native_session_id.clone();\n        let model_observation'
assert s.count(needle)==1
s=s.replace(needle,'''        let native = lane.binding().native_session_id.clone();
        if reconnect && previous.as_ref().and_then(|p| p["native_session_id"].as_str()) != Some(native.as_str()) {
            let cleanup = host.shutdown();
            self.store.append(&agent_session, &json!({"kind":"native-reconnect-identity-refused", "cleanup_confirmed":cleanup.is_ok(), "turn_replayed":false}))?;
            return Err(AikitError::new("encounter.native_identity_changed", "The harness returned another native identity to session/load; no binding or successful continuation was recorded"));
        }
        let model_observation''')
p.write_text(s)
p=Path('crates/aikit-cli/src/direct_agent_session.rs');s=p.read_text();needle='        || review["accepted"] != true';assert s.count(needle)==1;s=s.replace(needle,needle+'\n        || review["execution_authority_granted"] != false');p.write_text(s)
p=Path('crates/aikit-cli/tests/direct_agent_session.rs');s=p.read_text();s=s.replace('("/accepted", json!(false)),','("/accepted", json!(false)),\n        ("/execution_authority_granted", json!(true)),');s=s.replace('"accepted":true,"profile":profile','"accepted":true,"execution_authority_granted":false,"profile":profile');p.write_text(s)
