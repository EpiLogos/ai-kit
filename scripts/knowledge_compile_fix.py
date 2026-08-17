from pathlib import Path
p = Path('crates/aikit-store/src/knowledge_application.rs')
text = p.read_text().replace('Ulid::new()', 'Ulid::generate()')
p.write_text(text)
