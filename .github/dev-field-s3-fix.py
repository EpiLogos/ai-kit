from pathlib import Path

root = Path(__file__).resolve().parents[1]
path = root / "crates/aikit-cli/src/app/development_field.rs"
text = path.read_text()
old = """    let modality = match source_revision {\n        Some(_) if cfg!(debug_assertions) => DevelopmentFieldExecutableModality::Developer,\n        Some(_) => DevelopmentFieldExecutableModality::Source,\n        None => DevelopmentFieldExecutableModality::Installed,\n    };\n"""
new = """    let modality = match source_revision.as_ref() {\n        Some(_) if cfg!(debug_assertions) => DevelopmentFieldExecutableModality::Developer,\n        Some(_) => DevelopmentFieldExecutableModality::Source,\n        None => DevelopmentFieldExecutableModality::Installed,\n    };\n"""
if text.count(old) != 1:
    raise SystemExit("expected generated executable modality block once")
path.write_text(text.replace(old, new, 1))
Path(__file__).unlink()
