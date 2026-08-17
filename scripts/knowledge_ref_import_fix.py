from pathlib import Path

p = Path('crates/aikit-cli/src/main.rs')
text = p.read_text()
text = text.replace(
    'use aikit_core::{ForgetScope, KnowledgeAddress, ResourceRef, SourceRef};',
    'use aikit_core::resource::{ResourceRef, SourceRef};\n    use aikit_core::{ForgetScope, KnowledgeAddress};',
)
text = text.replace(
    'use aikit_core::{KnowledgeAddress, ResourceRef, SourceRef};',
    'use aikit_core::resource::{ResourceRef, SourceRef};\n    use aikit_core::KnowledgeAddress;',
)
p.write_text(text)

p = Path('crates/aikit-cli/tests/knowledge_application_v2.rs')
text = p.read_text()
text = text.replace(
    'use aikit_core::{ForgetScope, KnowledgeAddress, ResourceRef, DEFAULT_FAMILIARITY_HALF_LIFE_MS};',
    'use aikit_core::resource::ResourceRef;\nuse aikit_core::{ForgetScope, KnowledgeAddress, DEFAULT_FAMILIARITY_HALF_LIFE_MS};',
)
p.write_text(text)
