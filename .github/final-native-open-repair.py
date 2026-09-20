from pathlib import Path
p=Path('crates/aikit-cli/src/encounter_service.rs');s=p.read_text()
old='let lane = host.open_session(crate::encounter_mcp::build_session_open_request('
assert s.count(old)==1;s=s.replace(old,'let lane = match host.open_session(crate::encounter_mcp::build_session_open_request(')
old='''            Some(agent_session.clone()),
        ))?;
        let native = lane.binding().native_session_id.clone();'''
assert s.count(old)==1
s=s.replace(old,'''            Some(agent_session.clone()),
        )) {
            Ok(lane) => lane,
            Err(failure) => {
                // The adapter can reject session/load before a SessionOpened
                // binding exists. Retain that actual failure and confirmed
                // cleanup without inventing a successful native continuation.
                let cleanup = host.shutdown();
                if cleanup.is_err() {
                    self.shutdown_requested
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
                self.store.append(&agent_session, &json!({
                    "kind":"native-open-refused",
                    "continuation_requested":reconnect,
                    "error_code":failure.code,
                    "cleanup_confirmed":cleanup.is_ok(),
                    "binding_recorded":false,
                    "turn_replayed":false
                }))?;
                return Err(failure);
            }
        };
        let native = lane.binding().native_session_id.clone();''')
p.write_text(s)
p=Path('tests/direct_agent_native_join.py');s=p.read_text()
old='''            assert refused["error"]["code"]=="encounter.native_identity_changed",refused
            assert "native-reconnect-identity-refused" in json.dumps(events(session))'''
assert s.count(old)==1
s=s.replace(old,'''            assert refused["error"]["code"]=="agent_session_host.open_failed",refused
            assert refused["error"]["message"]=="ACP load/resume contradicted the requested native identity",refused
            held=events(session)
            rejection=[row["event"] for row in held if row.get("event",{}).get("kind")=="native-open-refused"][-1]
            assert rejection=={"kind":"native-open-refused","continuation_requested":True,"error_code":"agent_session_host.open_failed","cleanup_confirmed":True,"binding_recorded":False,"turn_replayed":False},rejection
            bindings=[row["event"] for row in held if row.get("event",{}).get("kind")=="binding"]
            assert bindings and all(row["native_session_id"]==native for row in bindings),bindings''')
p.write_text(s);compile(s,str(p),'exec')
