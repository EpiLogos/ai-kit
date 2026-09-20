"""One-time bounded repair of the controlled native test; no product fixture."""
from pathlib import Path
p = Path('tests/direct_agent_native_join.py')
s = p.read_text()
def replace(old, new):
    global s
    assert s.count(old) == 1, old
    s = s.replace(old, new)
replace('def exercise(ctrl: Path, aikit: Path) -> dict:', 'def exercise(ctrl: Path, aikit: Path, evidence: Path | None = None) -> dict:')
replace('        prompt = json.dumps(request["params"]["prompt"])', '''        prompt = json.dumps(request["params"]["prompt"])
        skill_marker = (base / "skill-marker").read_text()
        assert skill_marker in prompt, "selected effective Skill never reached ACP prompt"
        effect({"kind":"skill-delivered", "digest":hashlib.sha256(skill_marker.encode()).hexdigest(), "model_consumption_observed":False})''')
replace('["state"]=="Idle"', '["state"]=="Resident"')
replace('"native turn did not reach Idle"', '"native turn did not reach the actual Resident state without a transport error"')
replace('            purpose="Read only my selected source and report its exact content."', '''            skill_ref="skill/test/native-join"
            capsule=base/"aikit/registries/personal/capsules"/skill_ref
            (capsule/"payload").mkdir(parents=True)
            (capsule/"manifest.toml").write_text('schema = 1\\nid = "skill/test/native-join"\\nkind = "skill"\\nname = "native-join"\\ndescription = "Controlled source return method."\\n[skill]\\nroot = "payload"\\n')
            marker="CONTROLLED_SKILL_"+secrets.token_hex(24)
            (base/"skill-marker").write_text(marker)
            skill_file=capsule/"payload/SKILL.md"
            skill_body='---\\nname: native-join\\ndescription: Controlled source return method.\\n---\\n\\n'+marker+'\\n'
            skill_file.write_text(skill_body)
            command([str(aikit),"--json","enable",skill_ref,"--scope","global"])
            skills=ai("agent-session-skills")
            assert any(row["ref"]==skill_ref and row["eligible"] is True for row in skills["rows"]), skills
            purpose="Read only my selected source and report its exact content."''')
replace('"skill_refs":[]})', '"skill_refs":[skill_ref]})')
replace('            checks.append("Real production handshake, fragmented stream and source-dependent result reach the native journal; human token is absent in the peer")', '''            assert any(e["kind"]=="skill-delivered" for e in effects())
            checks.append("Actual selected effective Skill bytes, fragmented ACP source return and hashed delivery evidence reach production handlers; no model-consumption claim or human-token leak")''')
replace('            (base/"wrong-load").unlink()', '''            (base/"wrong-load").unlink()
            skill_file.write_text(skill_body+"Changed after preparation\\n")
            count=sum(e["kind"]=="prompt" for e in effects())
            ai("agent-session-prepare","--request-json",json.dumps(preparation),expect=False)
            assert sum(e["kind"]=="prompt" for e in effects())==count
            skill_file.write_text(skill_body)
            checks.append("Changed effective Skill content is refused against the prepared digest before provider work")''')
replace('            logs.close()', '''            logs.flush()
            if evidence is not None:
                evidence.mkdir(parents=True,exist_ok=True)
                # This World is entirely controlled. Never copy authority files,
                # runtime stores, tokens, native profiles or caller environment.
                logs.seek(0)
                (evidence/"controlled-owner.log").write_text(logs.read().replace(env.get("CENTRAL_NATIVE_TOKEN","<absent>"),"[test credential redacted]"))
                (evidence/"completed-checks.json").write_text(json.dumps({"standing":"controlled-not-live", "checks":checks},indent=2))
                (evidence/"peer-effects.json").write_text(json.dumps(effects(),indent=2))
            logs.close()''')
replace('    parser.add_argument("--peer",type=Path)', '    parser.add_argument("--peer",type=Path)\n    parser.add_argument("--evidence",type=Path)')
replace('exercise(args.ctrl.resolve(),args.aikit.resolve())', 'exercise(args.ctrl.resolve(),args.aikit.resolve(),args.evidence)')
p.write_text(s)
compile(s,str(p),'exec')
