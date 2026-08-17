from pathlib import Path
p = Path('crates/aikit-cli/src/app/knowledge.rs')
text = p.read_text()
old = '''    FamiliarityContext, ForgetScope, KnowledgeAddress, KnowledgeApplication, KnowledgeExplanation,
    KnowledgeProviderStatus, KnowledgeSearchResult, KnowledgeSources, Result,
};'''
new = '''    FamiliarityContext, ForgetScope, KnowledgeAddress, KnowledgeApplication, KnowledgeExplanation,
    KnowledgeProviderStatus, KnowledgeRankingEvidence, KnowledgeSearchResult, KnowledgeSources,
    Result, DEFAULT_FAMILIARITY_HALF_LIFE_MS,
};'''
if old not in text:
    raise SystemExit('knowledge ranking import anchor missing')
p.write_text(text.replace(old, new, 1))
