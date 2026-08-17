from pathlib import Path
p = Path('crates/aikit-cli/src/main.rs')
text = p.read_text()
old = '''fn cmd_knowledge(cwd: &std::path::Path, c: KnowledgeCmd) -> Result<Reply> {
    use aikit_core::resource::{ResourceRef, SourceRef};
    use aikit_core::{ForgetScope, KnowledgeAddress};'''
new = '''fn cmd_knowledge(cwd: &std::path::Path, c: KnowledgeCmd) -> Result<Reply> {
    use aikit_core::resource::ResourceRef;
    use aikit_core::ForgetScope;'''
if old not in text:
    raise SystemExit('cmd_knowledge import anchor missing')
p.write_text(text.replace(old, new, 1))
