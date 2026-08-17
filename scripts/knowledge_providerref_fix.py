from pathlib import Path
p = Path('crates/aikit-cli/src/app/knowledge.rs')
text = p.read_text()
text = text.replace('ResourceIndex, ResourceKind, ResourceRef, SourceAuthority, SourceRef', 'ProviderRef, ResourceIndex, ResourceKind, ResourceRef, SourceAuthority, SourceRef')
text = text.replace('    AikitError, FamiliarityContext, ForgetScope, KnowledgeAddress, KnowledgeApplication,\n', '    FamiliarityContext, ForgetScope, KnowledgeAddress, KnowledgeApplication,\n')
text = text.replace('aikit_core::ProviderRef::parse', 'ProviderRef::parse')
p.write_text(text)
