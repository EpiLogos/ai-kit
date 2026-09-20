"""Apply the bounded Wiki upgrade to the inspected native source. Temporary.
Every replacement fails closed if the expected source no longer matches.
The resulting production files, not this script, are the review target.
"""
from pathlib import Path
import re

def patch(path, old, new):
    p=Path(path); text=p.read_text(); assert text.count(old)==1, (path, old[:60],text.count(old)); p.write_text(text.replace(old,new))

p=Path('crates/aikit-adapters/src/markdown_document.rs');s=p.read_text();s=s.replace('fn inline_tags(text: &str) -> Vec<String> {','fn inline_tags(markdown: &str, span: Range<usize>) -> Vec<String> {\n    let text = &markdown[span.clone()];').replace('let mut previous = None;','let mut previous = markdown[..span.start].chars().next_back();');a=s.index('tags.extend(inline_tags(');b=s.index('));',a)+3;s=s[:a]+'tags.extend(inline_tags(markdown, node.start_byte..node.end_byte));'+s[b:];p.write_text(s)
patch('crates/aikit-adapters/src/lib.rs','pub mod workcell_instance_intake;','pub mod wiki_document;\npub mod workcell_instance_intake;')
patch('crates/aikit-core/src/knowledge.rs','    pub containment: Option<ContainmentRole>,','''    pub containment: Option<ContainmentRole>,
    /// Exact native edge identity. Equal endpoints do not collapse occurrences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<ResourceRef>,
    /// Explicit source occurrence, never inferred from proximity or a label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_relation: Option<crate::knowledge_okf::AuthoredRelationEvidence>,''')
patch('crates/aikit-core/src/knowledge.rs','            containment: None,','            containment: None,\n            reference: None,\n            authored_relation: None,')
patch('crates/aikit-core/src/knowledge_wiki_provider.rs','''                view.push_edge(RelationEdge::new(
                    from,
                    to,
                    neighbour.relation.clone(),
                    direction,
                    RelationOrigin::new(authority_for_neighbour(&neighbour))
                        .from_provider(self.provider.clone())
                        .in_lens("semantic-wiki"),
                ))?;''','''                let mut relation = RelationEdge::new(
                    from, to, neighbour.relation.clone(), direction,
                    RelationOrigin::new(authority_for_neighbour(&neighbour))
                        .from_provider(self.provider.clone()).in_lens("semantic-wiki"),
                );
                if let Some(WikiObject::Edge(edge)) = self.index.resolve(&neighbour.edge_ref) {
                    relation.reference = Some(edge.ref_id);
                    relation.origin.revision = Some(edge.revision.to_string());
                    relation.authored_relation = edge.extensions.get("authored_relation")
                        .cloned().and_then(|value| serde_json::from_value(value).ok());
                }
                view.push_edge(relation)?;''')
p=Path('crates/aikit-adapters/src/authored_wiki_source.rs');s=p.read_text();s=s.replace('use crate::okf::{parse_authored_markdown_relations, parse_okf_markdown};','use crate::okf::parse_okf_markdown;');s=s.replace('''    let mut relations =
        parse_authored_markdown_relations(&source_ref, source_revision.as_ref(), markdown);''','''    let document = crate::markdown_document::parse_markdown_document(markdown);
    let mut relations = crate::markdown_document::authored_relations_from_document(
        &source_ref, source_revision.as_ref(), &document,
    );''');s=s.replace('''        None => (None, None, Vec::new(), Vec::new()),''','''        None => (None,
            document.properties.get("title").and_then(Value::as_str).map(str::to_owned),
            aliases_from_extensions(&document.properties), Vec::new()),''');p.write_text(s)
patch('crates/aikit-cli/src/app/knowledge.rs','''    pub fn knowledge_relations(
''','''    /// Additive native document facet; plain KnowledgeReading users keep their API.
    pub fn knowledge_read_document(&self, address: &KnowledgeAddress) -> Result<serde_json::Value> {
        self.with_knowledge(|runtime, application| {
            let reading = application.read(address)?;
            let relations = application.relations(address, 1, 256, 512).ok();
            aikit_adapters::wiki_document::reading_document(&reading, relations.as_ref(), &runtime.material)
        })
    }

    pub fn knowledge_relations(
''')
patch('crates/aikit-cli/src/main.rs','jval!(service.knowledge_read(&address)?)','jval!(service.knowledge_read_document(&address)?)')
p=Path('crates/aikit-cli/src/app/knowledge.rs');s=p.read_text();a=s.index('        let wiki = if discovered.wiki.is_empty()');b=s.index('        let central =',a);block=s[a:b];s=s[:a]+s[b:];anchor='        native_source.rebuild(&material)?;';assert s.count(anchor)==1
insert='''
        let ordinary = aikit_adapters::wiki_document::compile_material_sources(&material, &discovered.wiki, &mut absences)?;
        aikit_adapters::central_entities::adopt_into(&mut discovered.wiki,
            ordinary.edges.into_iter().map(WikiObject::Edge).collect());
'''+block
s=s.replace(anchor,anchor+'\n'+insert);p.write_text(s)
