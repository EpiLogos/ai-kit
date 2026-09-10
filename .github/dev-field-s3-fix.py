from pathlib import Path

root = Path(__file__).resolve().parents[1]

# Correct generated executable-basis ownership.
app_path = root / "crates/aikit-cli/src/app/development_field.rs"
app = app_path.read_text()
old = """    let modality = match source_revision {\n        Some(_) if cfg!(debug_assertions) => DevelopmentFieldExecutableModality::Developer,\n        Some(_) => DevelopmentFieldExecutableModality::Source,\n        None => DevelopmentFieldExecutableModality::Installed,\n    };\n"""
new = """    let modality = match source_revision.as_ref() {\n        Some(_) if cfg!(debug_assertions) => DevelopmentFieldExecutableModality::Developer,\n        Some(_) => DevelopmentFieldExecutableModality::Source,\n        None => DevelopmentFieldExecutableModality::Installed,\n    };\n"""
if app.count(old) != 1:
    raise SystemExit("expected generated executable modality block once")
app_path.write_text(app.replace(old, new, 1))

# The generic helper in the first patch used the last newline-delimited brace,
# which can be the subject_reading function when the file has no terminal newline.
# Move the two generated tests into the existing #[cfg(test)] module explicitly.
core_path = root / "crates/aikit-core/src/resource/development_field.rs"
core = core_path.read_text()
start_marker = "\n    #[test]\n    fn a_present_resource_without_an_owner_binding_is_unknown_not_a_fabricated_carrier()"
start = core.find(start_marker)
if start < 0:
    raise SystemExit("generated Development Field tests were not found")
end_marker = "\n}\n\n#[cfg(test)]\nmod tests {"
end = core.find(end_marker, start)
if end < 0:
    raise SystemExit("subject_reading/test-module boundary was not found")
block = core[start:end]
core = core[:start] + core[end:]
anchor = """    fn executable() -> DevelopmentFieldExecutableBasis {\n        DevelopmentFieldExecutableBasis {\n            executable: \"/work/aikit/target/debug/aikit\".into(),\n            package_version: \"0.1.0\".into(),\n            modality: DevelopmentFieldExecutableModality::Developer,\n            source_revision: Some(VersionRevision::new(\"abc123\")),\n            source_dirty: false,\n        }\n    }\n"""
if core.count(anchor) != 1:
    raise SystemExit("test executable helper was not found exactly once")
core = core.replace(anchor, anchor + block + "\n", 1)
core_path.write_text(core)

Path(__file__).unlink()
