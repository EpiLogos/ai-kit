//! Read facets over admitted source material and the existing SemanticWiki.
//! There is one Markdown parser and one authored-relation compiler. A document
//! facet is disposable presentation, not a source, index or semantic mutation.
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use aikit_core::knowledge::{KnowledgeReading, KnowledgeRelationView};
use aikit_core::knowledge_okf::AuthoredRelationResolution;
use aikit_core::knowledge_source_pool::SourceMaterial;
use aikit_core::knowledge_wiki::WikiObject;
use aikit_core::resource::{ResourceLocator, ResourceRef, SourceAuthority};
use aikit_core::Result;
use serde_json::{json, Value};

use crate::authored_wiki_source::{compile_authored_wiki_relations, parse_authored_wiki_source_with_authority, AuthoredWikiRelationCompilation};
use crate::markdown_document::{parse_markdown_document, MarkdownDocument, MarkdownNode};

pub const WIKI_DOCUMENT_FACET: &str = "aikit.markdown-reading/v1";

pub fn is_markdown(material: &SourceMaterial) -> bool {
    matches!(material.binding.media_type.split(';').next().unwrap_or("").trim(), "text/markdown" | "text/x-markdown")
}

fn normal_path(path: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {},
            Component::ParentDir => {if !result.pop() {return None;}},
            Component::Normal(value) => result.push(value),
            Component::RootDir => result.push(Path::new("/")),
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
        }
    }
    Some(result)
}

/// Pure lookup among already admitted locators. No filesystem probing, fallback
/// to another world, or title guessing for an explicit relative Markdown path.
pub fn compile_material_sources(material: &[SourceMaterial], objects: &[WikiObject], warnings: &mut Vec<String>) -> Result<AuthoredWikiRelationCompilation> {
    let paths: BTreeMap<PathBuf, ResourceRef> = material.iter().filter_map(|item| {
        let ResourceLocator::Path(path) = item.binding.locator.as_ref()? else {return None};
        Some((normal_path(path)?, ResourceRef::parse(item.binding.source.as_str()).ok()?))
    }).collect();
    let mut sources = Vec::new();
    for item in material.iter().filter(|item| is_markdown(item) && !item.body.is_empty()) {
        let locators = match &item.binding.locator {
            Some(ResourceLocator::Path(path)) => vec![path.to_string_lossy().into_owned()],
            _ => Vec::new(),
        };
        let authority = item.binding.metadata.get("authority").cloned().and_then(|value| serde_json::from_value(value).ok()).unwrap_or(SourceAuthority::Observed);
        let parsed = parse_authored_wiki_source_with_authority(
            ResourceRef::parse(item.binding.source.as_str())?, item.binding.source.clone(), authority,
            Some(item.binding.revision.clone()), locators, &item.body,
        );
        let mut source = match parsed {
            Ok(source) => source,
            Err(error) => {warnings.push(format!("Source {} has unreadable relation metadata: {error}", item.binding.source)); continue;}
        };
        if source.title.is_none() {source.title = Some(item.binding.title.clone());}
        if let Some(ResourceLocator::Path(path)) = &item.binding.locator {
            for relation in &mut source.relations {
                let raw = relation.raw_target.as_str();
                if raw.contains(':') || raw.starts_with('/') {continue;}
                let explicit_path = raw.contains('/') || raw.ends_with(".md") || raw.ends_with(".markdown");
                if !explicit_path {continue;}
                if let Some(target) = normal_path(&path.parent().unwrap_or(Path::new("")).join(raw)).and_then(|path| paths.get(&path)) {
                    relation.resolution = AuthoredRelationResolution::Resolved {target_ref: target.clone()};
                }
            }
        }
        sources.push(source);
    }
    compile_authored_wiki_relations(&sources, objects, &[])
}

fn node_text(node: &MarkdownNode) -> String {
    let mut value = node.text.clone().unwrap_or_default();
    for child in &node.children {value.push_str(&node_text(child));}
    value
}

/// Native fragment keys. The renderer only matches keys supplied here; it never
/// invents a heading dialect or rebases a stale span onto a different passage.
pub fn document_selectors(document: &MarkdownDocument) -> Vec<Value> {
    fn collect(nodes: &[MarkdownNode], result: &mut Vec<Value>) {
        for node in nodes {
            if node.kind == "heading" {
                let label = node_text(node);
                let slug = label.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_').collect::<String>().split_whitespace().collect::<Vec<_>>().join("-");
                result.push(json!({"kind":"heading","keys":[label,slug],"id":node.attributes.get("id"),"start_byte":node.start_byte,"end_byte":node.end_byte}));
            } else if let Some(block) = node.attributes.get("block_id").and_then(Value::as_str) {
                result.push(json!({"kind":"block","keys":[format!("^{block}")],"id":node.attributes.get("id"),"start_byte":node.start_byte,"end_byte":node.end_byte}));
            }
            collect(&node.children, result);
        }
    }
    let mut result = Vec::new(); collect(&document.blocks, &mut result); result
}

/// The original KnowledgeReading is kept byte-for-byte inside its normal wire
/// fields. The optional facet carries the same source/revision and native edges.
pub fn reading_document(reading: &KnowledgeReading, relations: Option<&KnowledgeRelationView>, material: &[SourceMaterial]) -> Result<Value> {
    let mut value = serde_json::to_value(reading).map_err(|error| aikit_core::AikitError::new("knowledge.document_serialize", error.to_string()))?;
    let Some(source) = material.iter().find(|item| item.binding.source.as_str() == reading.resource.as_str() && is_markdown(item)) else {return Ok(value)};
    let Some(content) = reading.content.as_deref() else {return Ok(value)};
    let document = parse_markdown_document(content);
    let mut occurrences = Vec::new();
    for link in &document.links {
        let native = relations.and_then(|view| view.edges.iter().find(|edge| {
            edge.from == reading.resource && edge.authored_relation.as_ref().is_some_and(|evidence| {
                evidence.source_ref.as_str() == source.binding.source.as_str()
                && evidence.source_revision.as_ref().map(ToString::to_string) == reading.revision
                && evidence.anchor.start_byte == Some(link.start_byte)
                && evidence.anchor.end_byte == Some(link.end_byte)
                && evidence.raw_token == link.raw_token
            })
        }));
        let target = native.map(|edge| {
            let source = material.iter().any(|item| item.binding.source.as_str() == edge.to.as_str());
            json!({"kind":if source {"source"} else {"wiki"},"value":edge.to})
        });
        occurrences.push(json!({"start_byte":link.start_byte,"end_byte":link.end_byte,"target":target,"reference":native.and_then(|edge|edge.reference.as_ref()),"evidence":native.and_then(|edge|edge.authored_relation.as_ref()),"state":if link.external {"external"} else if native.is_some() {"resolved"} else {"unresolved"}}));
    }
    let incoming: Vec<Value> = relations.into_iter().flat_map(|view| &view.edges).filter(|edge| edge.to == reading.resource).map(|edge| {
        let source = material.iter().find(|item| item.binding.source.as_str() == edge.from.as_str());
        json!({"reference":edge.reference,"from":edge.from,"relation":edge.relation,"origin":edge.origin,"evidence":edge.authored_relation,"label":source.map(|item|item.binding.title.as_str()).unwrap_or(edge.from.as_str()),"address":{"kind":if source.is_some() {"source"} else {"wiki"},"value":edge.from}})
    }).collect();
    value["document"] = json!({"schema":WIKI_DOCUMENT_FACET,"source_ref":source.binding.source,"source_revision":reading.revision,"syntax":document,"selectors":document_selectors(&document),"occurrences":occurrences,"incoming":incoming,"relations_truncated":relations.is_some_and(|view|view.truncated),"relations_available":relations.is_some()});
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::knowledge_wiki_provider::SemanticWikiProvider;
    use aikit_core::knowledge::RelationQuery;
    use aikit_core::knowledge_source_pool::{SourceBinding, SourceVisibility};
    use aikit_core::resource::{SourceRef, SourceRevision};
    fn material(reference:&str, title:&str, path:&str, body:&str)->SourceMaterial {
        SourceMaterial {binding:SourceBinding {source:SourceRef::parse(reference).unwrap(),revision:SourceRevision::parse("r1").unwrap(),title:title.into(),tags:Vec::new(),visibility:SourceVisibility::Public,owners:Vec::new(),media_type:"text/markdown".into(),locator:Some(ResourceLocator::Path(path.into())),metadata:BTreeMap::new()},body:body.into()}
    }
    #[test]
    fn plain_material_and_exact_occurrences_share_one_index_and_backlinks() {
        let material=vec![material("source:a","Alpha","/world/a.md","🌱 [[Beta]] and [again](b.md#Part)."),material("source:b","Beta","/world/b.md","# Part\n\nBody ^claim\n")];
        let compilation=compile_material_sources(&material,&[], &mut Vec::new()).unwrap();
        assert_eq!(compilation.edges.len(),2);
        let index=crate::authored_wiki_source::rebuild_semantic_wiki_with_authored_relations(&[],&compilation).unwrap();
        let provider=SemanticWikiProvider::new(&index);
        let resource=ResourceRef::parse("source:a").unwrap();
        let view=provider.relations(RelationQuery::local(resource.clone())).unwrap();
        assert_eq!(view.edges.len(),2);
        assert_ne!(view.edges[0].reference,view.edges[1].reference);
        assert!(view.edges.iter().all(|edge|edge.authored_relation.is_some()));
        let reading=KnowledgeReading {resource,provider:None,lens:None,revision:Some("r1".into()),freshness:None,authority:SourceAuthority::Observed,content:Some(material[0].body.clone()),evidence:Vec::new(),why_selected:"explicit".into()};
        let wire=reading_document(&reading,Some(&view),&material).unwrap();
        assert_eq!(wire["content"],reading.content.unwrap());
        assert_eq!(wire["document"]["occurrences"].as_array().unwrap().len(),2);
        assert!(wire["document"]["occurrences"].as_array().unwrap().iter().all(|o|o["state"]=="resolved"));
        assert_eq!(index.backlinks(&ResourceRef::parse("source:b").unwrap()).len(),2);
    }
    #[test]
    fn ordinary_properties_resolve_aliases_but_duplicate_titles_stay_ambiguous() {
        let material=vec![material("source:a","A","/world/a.md","[[Named]] [[Same]]"),material("source:b","B","/world/b.md","---\naliases: [Named]\n---\n"),material("source:c","Same","/world/c.md","text"),material("source:d","Same","/world/d.md","text")];
        let compilation=compile_material_sources(&material,&[], &mut Vec::new()).unwrap();
        assert_eq!(compilation.edges.len(),1);assert_eq!(compilation.pending.len(),1);
        assert!(matches!(compilation.pending[0].evidence.resolution,AuthoredRelationResolution::Ambiguous {..}));
    }
    #[test]
    fn absent_targets_do_not_probe_disk_and_selectors_preserve_native_spans() {
        let material=vec![material("source:a","A","/world/a.md","[private](../private/secret.md)\n\n# A heading\n\nText ^claim\n")];
        let compilation=compile_material_sources(&material,&[], &mut Vec::new()).unwrap();
        assert!(compilation.edges.is_empty());assert_eq!(compilation.pending.len(),1);
        let selectors=document_selectors(&parse_markdown_document(&material[0].body));
        assert_eq!(selectors.len(),2);assert_eq!(selectors[0]["keys"][1],"a-heading");assert_eq!(selectors[1]["keys"][0],"^claim");
    }
}
