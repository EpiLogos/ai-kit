#!/usr/bin/env python3
from pathlib import Path

path = Path("crates/aikit-core/src/resource/operative.rs")
source = path.read_text()
old = '''    if trimmed.starts_with('"') || trimmed.starts_with('\\'') {\n        return match parse_resolve_expression(trimmed)? {\n'''
new = '''    if trimmed.starts_with('"') || trimmed.starts_with('\\'') || trimmed.contains('\\\\') {\n        return match parse_resolve_expression(trimmed)? {\n'''
if old not in source:
    raise SystemExit("operative quoted/escaped subject anchor changed; re-inspect before editing")
path.write_text(source.replace(old, new, 1))
