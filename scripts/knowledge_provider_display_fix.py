from pathlib import Path
p = Path('crates/aikit-tui/src/application_service.rs')
text = p.read_text()
old = '''                let provider = hit
                    .provider
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "provider-neutral".into());'''
new = '''                let provider = hit.provider.to_string();'''
if old not in text:
    raise SystemExit('provider display anchor missing')
p.write_text(text.replace(old, new, 1))
