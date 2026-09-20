//! Constructive operations over the existing okf-wiki/v1 document.
//!
//! A construction is a WikiFrame and its WikiConstellation, not a renderer
//! graph. Membership and role belong to a participation; sources, native
//! edges and Expression/Scene/Palace references remain independently owned.
//! This module performs no I/O. Central (or the native CLI) owns atomic save.
use std::collections::{BTreeMap, BTreeSet};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::{AikitError, Result, WikiObject, WikiFrame, WikiConstellation, WikiConstellationMember,
    WikiNode, WikiEdge, WikiEdgeOrigin, WikiProvenanceRef, WikiConstellationReturn, OKF_WIKI_PROFILE};
use crate::resource::ResourceRef;
use crate::knowledge_facets::{read_source_selector, TemporalFacet, PlaceFacet, TechneFacets,
    write_facets_to_extensions, parse_facets_from_extensions, TECHNE_FACET_EXTENSION};
use crate::knowledge_wiki_write::{apply_wiki_mutation, WikiDocument, WikiMutationLedger};

pub const CONSTRUCTION: &str = "aikit.constellation/v1";
pub const PARTICIPATION: &str = "aikit.constellation-participation/v1";
pub const RELATION: &str = "aikit.constellation-relation/v1";
pub const ACTION: &str = "aikit.constellation-action/v1";
const LIMIT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inquiry { pub question: String, #[serde(default)] pub purpose: String }

/// The QL owner validates grammar; the author, never grammar, supplies the
/// interpretation. Open roles are legitimate and are not fabricated members.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredFrame {
    pub shape_ref: String,
    pub contract_ref: String,
    pub roles: Vec<FrameRole>,
    pub provenance: Vec<WikiProvenanceRef>,
    pub standing: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameRole { pub role_ref: ResourceRef, pub label: String, pub address: Value }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WholeReference { pub whole_ref: ResourceRef, pub revision: u64, pub kind: String }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Participation {
    pub participation_ref: ResourceRef,
    #[serde(default)] pub role_ref: Option<ResourceRef>,
    /// Existing provenance + ql.techne/v1 SourceSelector, not another selector grammar.
    #[serde(default)] pub sources: Vec<WikiProvenanceRef>,
    #[serde(default)] pub nested: Option<WholeReference>,
    #[serde(default)] pub note: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberInput { pub subject_ref: ResourceRef, pub participation: Participation }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationInput {
    pub relation_ref: ResourceRef,
    /// None creates; an update must address the exact native edge revision.
    pub expected_revision: Option<u64>,
    pub from_participation_ref: ResourceRef,
    pub to_participation_ref: ResourceRef,
    pub relation: String,
    pub direction: String,
    pub standing: String,
    #[serde(default)] pub evidence: Vec<WikiProvenanceRef>,
    #[serde(default)] pub uncertainty: Option<String>,
    #[serde(default)] pub temporal: Vec<TemporalFacet>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionReference {
    pub reference: ResourceRef,
    pub revision: String,
    pub kind: String,
    pub source: WikiProvenanceRef,
    #[serde(default)] pub derivation_refs: Vec<ResourceRef>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedOperation {
    pub actor_ref: ResourceRef,
    pub request_digest: String,
    pub basis_revision: u64,
    pub result_revision: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Construction {
    pub title: String,
    pub inquiry: Inquiry,
    pub authored_by: ResourceRef,
    #[serde(default)] pub frame: Option<AuthoredFrame>,
    #[serde(default)] pub relation_refs: Vec<ResourceRef>,
    #[serde(default)] pub alternatives: Vec<WholeReference>,
    #[serde(default)] pub variant_of: Option<WholeReference>,
    #[serde(default)] pub compositions: Vec<CompositionReference>,
    #[serde(default)] pub applied: BTreeMap<String, AppliedOperation>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Create { anchor_ref: ResourceRef, title: String, inquiry: Inquiry,
        #[serde(default)] space_refs: Vec<ResourceRef>, #[serde(default)] frame: Option<AuthoredFrame>,
        #[serde(default)] variant_of: Option<WholeReference> },
    InquirySet { title: String, inquiry: Inquiry },
    FrameSet { frame: Option<AuthoredFrame> },
    MemberAdd { member: MemberInput },
    MemberRemove { participation_ref: ResourceRef },
    RoleSet { participation_ref: ResourceRef, role_ref: Option<ResourceRef> },
    SourcesSet { participation_ref: ResourceRef, sources: Vec<WikiProvenanceRef> },
    RelationPut { relation: RelationInput },
    RelationRetract { relation_ref: ResourceRef, expected_revision: u64, reason: String },
    AlternativeAdd { alternative: WholeReference },
    AlternativeRemove { whole_ref: ResourceRef },
    CompositionAttach { composition: CompositionReference },
    PlaceSet { participation_ref: ResourceRef, places: Vec<PlaceFacet> },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub frame_ref: ResourceRef,
    /// Zero means creation, never "apply on whatever revision is current".
    pub expected_revision: u64,
    pub actor_ref: ResourceRef,
    pub operation_ref: ResourceRef,
    pub changes: Vec<Change>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub schema: &'static str,
    pub frame_ref: ResourceRef,
    pub revision: u64,
    pub idempotent: bool,
    pub content: String,
    pub objects_changed: Vec<String>,
    pub warnings: Vec<String>,
}
fn err(message: impl Into<String>) -> AikitError { AikitError::new("knowledge.constellation_refused", message) }
fn encoded<T: Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| err(e.to_string()))
}
fn decoded<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|e| err(e.to_string()))
}
fn text(value: &str, field: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') { return Err(err(format!("invalid {field}"))); }
    Ok(())
}
fn core_metadata(frame: &WikiFrame) -> Result<Construction> {
    decoded(frame.extensions.get(CONSTRUCTION).ok_or_else(|| err("not a constructive WikiFrame; adopt it explicitly before editing"))?)
}
fn participation(member: &WikiConstellationMember) -> Result<Participation> {
    decoded(member.extensions.get(PARTICIPATION).ok_or_else(|| err("member has no contextual participation identity"))?)
}
fn selection_sources(sources: &[WikiProvenanceRef]) -> Result<()> {
    if sources.len() > LIMIT { return Err(err("source budget exceeded")); }
    for source in sources {
        crate::resource::SourceRef::parse(source.source_ref.as_str())?;
        if matches!(&source.source_revision, Some(crate::SemanticRevision::Number(0))) || matches!(&source.source_revision,Some(crate::SemanticRevision::Text(s)) if s.trim().is_empty()) {return Err(err("source revision is empty"));}
        if source.source_revision.is_none() { return Err(err("source participation requires its exact revision")); }
        // The native facet parser refuses unknown selector fields and malformed units.
        if let Some(selector) = read_source_selector(source)? {
            if let crate::knowledge_facets::SourceSelector::TextSpan {start,end,..} = selector {
                if start >= end { return Err(err("selected passage must have a non-empty forward span")); }
            }
        }
    }
    Ok(())
}
fn check_reference(reference: &WholeReference, target: &ResourceRef) -> Result<()> {
    ResourceRef::parse(reference.whole_ref.as_str())?;
    if reference.revision == 0 || &reference.whole_ref == target { return Err(err("a nested/alternative/variant reference requires another whole at an exact revision")); }
    if !["constellation","expression","journey","palace","source","place"].contains(&reference.kind.as_str()) { return Err(err("unsupported whole kind")); }
    Ok(())
}
fn frame_of(doc: &WikiDocument, reference: &ResourceRef) -> Result<WikiFrame> {
    match doc.object(reference) { Some(WikiObject::Frame(f)) if f.constellations.len()==1 => Ok(f.clone()), _ => Err(err("the native frame is absent")) }
}
fn member_index(frame: &WikiFrame, reference: &ResourceRef) -> Result<usize> {
    frame.constellations[0].members.iter().position(|m| participation(m).is_ok_and(|p| &p.participation_ref == reference))
        .ok_or_else(|| err(format!("participation {reference} is not in this construction")))
}
fn revise_member(frame: &mut WikiFrame, reference: &ResourceRef, change: impl FnOnce(&mut Participation)) -> Result<()> {
    let index = member_index(frame, reference)?;
    let member = &mut frame.constellations[0].members[index];
    let mut p = participation(member)?;
    change(&mut p);
    member.extensions.insert(PARTICIPATION.into(), encoded(&p)?);
    Ok(())
}
fn native_put(doc: &mut WikiDocument, ledger: &mut WikiMutationLedger, object: WikiObject, expected: Option<u64>) -> Result<()> {
    match doc.object(object.ref_id()) {
        Some(existing) if Some(existing.revision()) == expected => {
            if !doc.holds_equivalent(&object) { ledger.record(doc.update_object(object)?); }
        }
        None if expected.is_none() => ledger.record(doc.create_object(object)?),
        _ => return Err(AikitError::new("knowledge.constellation_revision_conflict", format!("native object {} changed or already exists", object.ref_id()))),
    }
    Ok(())
}
/// Read a whole without acquiring publication or mutation authority.
pub fn inspect(input: &str, reference: &ResourceRef) -> Result<Value> {
    let doc = WikiDocument::parse(input)?;
    let frame = frame_of(&doc,reference)?;
    let meta = core_metadata(&frame)?;
    let relations: Vec<_> = meta.relation_refs.iter().map(|r| match doc.object(r) {
        Some(WikiObject::Edge(edge)) if edge.origin_ref.as_ref()==Some(reference)=>encoded(edge),
        _=>Err(err("relation ref is absent or does not name this construction's native edge")),
    }).collect::<Result<_>>()?;
    Ok(json!({"schema":CONSTRUCTION,"frame":frame,"construction":meta,"relations":relations,
        "actions":["aikit.constellation.apply"],"native_owner":"ai-kit","semantic_whole_truncated":false}))
}

pub fn apply(input: &str, request: &Request) -> Result<Applied> {
    for reference in [&request.frame_ref,&request.actor_ref,&request.operation_ref] { ResourceRef::parse(reference.as_str())?; }
    if request.schema != ACTION || request.changes.is_empty() || request.changes.len()>256 { return Err(err("unsupported request schema or change budget")); }
    let digest = blake3::hash(serde_json::to_string(request).map_err(|e| err(e.to_string()))?.as_bytes()).to_hex().to_string();
    let original = WikiDocument::parse(input)?;
    if let Some(WikiObject::Frame(frame)) = original.object(&request.frame_ref) {
        let meta = core_metadata(frame)?;
        if let Some(prior) = meta.applied.get(request.operation_ref.as_str()) {
            if prior.request_digest != digest { return Err(err("operation identity was reused with different content")); }
            return Ok(Applied{schema:CONSTRUCTION,frame_ref:request.frame_ref.clone(),revision:frame.revision,idempotent:true,
                content:input.into(),objects_changed:vec![],warnings:vec![]});
        }
    }
    let (content,outcome) = apply_wiki_mutation(input, |doc,ledger| {
        let mut frame;
        let mut meta;
        if request.expected_revision == 0 {
            let Some(Change::Create{anchor_ref,title,inquiry,space_refs,frame:form,variant_of}) = request.changes.first() else { return Err(err("new construction must start with create")); };
            if doc.holds(&request.frame_ref) || doc.holds(anchor_ref) { return Err(err("construction/anchor identity already exists")); }
            if anchor_ref == &request.frame_ref { return Err(err("the anchor and frame are distinct native objects")); }
            meta = Construction {title:title.clone(),inquiry:inquiry.clone(),authored_by:request.actor_ref.clone(),frame:form.clone(),
                relation_refs:vec![],alternatives:vec![],variant_of:variant_of.clone(),compositions:vec![],applied:BTreeMap::new()};
            frame = WikiFrame {profile:OKF_WIKI_PROFILE.into(),ref_id:request.frame_ref.clone(),revision:1,provenance:vec![],inquiry_ref:None,
                space_refs:space_refs.clone(),member_refs:vec![anchor_ref.clone()],external_refs:vec![],
                constellations:vec![WikiConstellation{anchor_ref:anchor_ref.clone(),members:vec![],returns:vec![],conjugate_ref:None,extensions:BTreeMap::new()}],extensions:BTreeMap::new()};
            if let Some(variant) = variant_of {
                check_reference(variant,&request.frame_ref)?;
                let base=frame_of(doc,&variant.whole_ref)?;
                if base.revision!=variant.revision { return Err(err("variant basis is stale")); }
                // Copy contextual organisation deliberately, never native source identity.
                let base_meta=core_metadata(&base)?;
                if meta.frame.is_none(){ meta.frame=base_meta.frame; }
                let mut remapped=BTreeMap::new();
                for (i,m) in base.constellations[0].members.iter().enumerate() {
                    let mut cloned=m.clone();let mut p=participation(m)?;
                    let prior=p.participation_ref.clone();
                    p.participation_ref=ResourceRef::parse(format!("{}:participation:{i}",request.frame_ref))?;
                    remapped.insert(prior,p.participation_ref.clone());
                    cloned.extensions.insert(PARTICIPATION.into(),encoded(&p)?);
                    frame.constellations[0].members.push(cloned);
                }
                // A variant has independent relation identities while retaining
                // the source edge, evidence and remapped contextual endpoints.
                for (i,edge_ref) in base_meta.relation_refs.iter().enumerate() {
                    let Some(WikiObject::Edge(prior))=doc.object(edge_ref) else {return Err(err("variant relation is absent"));};
                    let mut edge=prior.clone();
                    let mut relation:RelationInput=decoded(edge.extensions.get(RELATION).ok_or_else(||err("variant relation has no participation basis"))?)?;
                    if relation.standing=="retracted" {continue;}
                    relation.relation_ref=ResourceRef::parse(format!("{}:relation:{i}",request.frame_ref))?;
                    relation.expected_revision=None;
                    relation.from_participation_ref=remapped.get(&relation.from_participation_ref).ok_or_else(||err("variant relation endpoint is absent"))?.clone();
                    relation.to_participation_ref=remapped.get(&relation.to_participation_ref).ok_or_else(||err("variant relation endpoint is absent"))?.clone();
                    edge.ref_id=relation.relation_ref.clone();edge.revision=1;edge.origin_ref=Some(request.frame_ref.clone());
                    edge.extensions.insert(RELATION.into(),encoded(&relation)?);
                    edge.extensions.insert("derived_from_relation".into(),json!({"reference":edge_ref,"revision":prior.revision}));
                    meta.relation_refs.push(edge.ref_id.clone());
                    ledger.record(doc.create_object(WikiObject::Edge(edge))?);
                }
            }
            let anchor=WikiNode{profile:OKF_WIKI_PROFILE.into(),ref_id:anchor_ref.clone(),revision:1,provenance:vec![],node_type:"Constellation".into(),
                title:Some(title.clone()),space_refs:space_refs.clone(),source_refs:vec![],local_space_ref:None,
                extensions:BTreeMap::from([("frame_ref".into(),encoded(&request.frame_ref)?)])};
            ledger.record(doc.create_object(WikiObject::Node(anchor.clone()))?);
            ledger.record(doc.sync_space_memberships(&anchor)?);
        } else {
            frame=frame_of(doc,&request.frame_ref)?;
            if frame.revision!=request.expected_revision { return Err(AikitError::new("knowledge.constellation_revision_conflict","construction changed; inspect and explicitly reconcile")); }
            if frame.extensions.get("read_only")==Some(&Value::Bool(true)) || frame.extensions.contains_key("shared_projection_ref") { return Err(err("read-only shared material requires an explicit local derivative")); }
            meta=core_metadata(&frame)?;
            if frame.constellations.len()!=1 { return Err(err("this operation requires one addressed constellation per native frame")); }
        }
        for (index,change) in request.changes.iter().enumerate() {
            match change {
                Change::Create{..} if request.expected_revision==0 && index==0 => {},
                Change::Create{..} => return Err(err("create cannot replace an existing construction")),
                Change::InquirySet{title,inquiry} => {meta.title=title.clone();meta.inquiry=inquiry.clone();},
                Change::FrameSet{frame:form} => meta.frame=form.clone(),
                Change::MemberAdd{member} => {
                    ResourceRef::parse(member.subject_ref.as_str())?;
                    ResourceRef::parse(member.participation.participation_ref.as_str())?;
                    if frame.constellations[0].members.iter().any(|m|participation(m).is_ok_and(|p|p.participation_ref==member.participation.participation_ref)) {return Err(err("participation identity already exists"));}
                    if member.subject_ref==frame.constellations[0].anchor_ref {return Err(err("whole anchor is not an extra positional member"));}
                    frame.constellations[0].members.push(WikiConstellationMember{ref_id:member.subject_ref.clone(),position:None,conjugate:false,
                        extensions:BTreeMap::from([(PARTICIPATION.into(),encoded(&member.participation)?)])});
                },
                Change::MemberRemove{participation_ref} => {
                    let index=member_index(&frame,participation_ref)?;
                    for edge_ref in &meta.relation_refs {
                        if let Some(WikiObject::Edge(edge))=doc.object(edge_ref) {
                            if let Some(relation)=edge.extensions.get(RELATION) {
                                if relation["standing"]!="retracted" && (relation["from_participation_ref"]==encoded(participation_ref)? || relation["to_participation_ref"]==encoded(participation_ref)?) {
                                    return Err(err("retract or reconnect active relations in the same transaction before removing their member"));
                                }
                            }
                        }
                    }
                    frame.constellations[0].members.remove(index);
                },
                Change::RoleSet{participation_ref,role_ref}=>revise_member(&mut frame,participation_ref,|p|p.role_ref=role_ref.clone())?,
                Change::SourcesSet{participation_ref,sources}=>revise_member(&mut frame,participation_ref,|p|p.sources=sources.clone())?,
                Change::RelationPut{relation:r} => {
                    let a=member_index(&frame,&r.from_participation_ref)?;
                    let b=member_index(&frame,&r.to_participation_ref)?;
                    ResourceRef::parse(r.relation_ref.as_str())?;
                    text(&r.relation,"relation type",256)?;
                    if !["directed","undirected","bidirectional"].contains(&r.direction.as_str()) || !["proposed","asserted","contested","uncertain"].contains(&r.standing.as_str()) {return Err(err("invalid relation direction/standing"));}
                    selection_sources(&r.evidence)?;
                    if let Some(WikiObject::Edge(existing))=doc.object(&r.relation_ref) {
                        if existing.origin_ref.as_ref()!=Some(&request.frame_ref) {return Err(err("a construction cannot rewrite a foreign native relation"));}
                    }
                    let mut extensions=BTreeMap::from([(RELATION.into(),encoded(r)?)]);
                    extensions.insert("independent_corroboration".into(),Value::Bool(false));
                    if !r.temporal.is_empty(){write_facets_to_extensions(&mut extensions,&TechneFacets{temporal:r.temporal.clone(),spatial:vec![]})?;}
                    let edge=WikiEdge{profile:OKF_WIKI_PROFILE.into(),ref_id:r.relation_ref.clone(),revision:1,provenance:r.evidence.clone(),
                        from_ref:frame.constellations[0].members[a].ref_id.clone(),to_ref:frame.constellations[0].members[b].ref_id.clone(),relation:r.relation.clone(),
                        origin:WikiEdgeOrigin::Authored,origin_ref:Some(request.frame_ref.clone()),extensions};
                    native_put(doc,ledger,WikiObject::Edge(edge),r.expected_revision)?;
                    if !meta.relation_refs.contains(&r.relation_ref){meta.relation_refs.push(r.relation_ref.clone());}
                },
                Change::RelationRetract{relation_ref,expected_revision,reason}=> {
                    text(reason,"retraction reason",4096)?;
                    let Some(WikiObject::Edge(existing))=doc.object(relation_ref) else{return Err(err("relation is absent"));};
                    if existing.origin_ref.as_ref()!=Some(&request.frame_ref){return Err(err("foreign relation cannot be retracted by this construction"));}
                    let mut edge=existing.clone();
                    let details=edge.extensions.get_mut(RELATION).and_then(Value::as_object_mut).ok_or_else(||err("missing native relation participation basis"))?;
                    details.insert("standing".into(),json!("retracted"));
                    edge.extensions.insert("retraction".into(),json!({"actor_ref":request.actor_ref,"reason":reason,"operation_ref":request.operation_ref}));
                    native_put(doc,ledger,WikiObject::Edge(edge),Some(*expected_revision))?;
                },
                Change::AlternativeAdd{alternative}=>{check_reference(alternative,&request.frame_ref)?;if !meta.alternatives.contains(alternative){meta.alternatives.push(alternative.clone());}},
                Change::AlternativeRemove{whole_ref}=>meta.alternatives.retain(|r|&r.whole_ref!=whole_ref),
                Change::CompositionAttach{composition}=>{
                    if !["expression","journey","palace","artifact"].contains(&composition.kind.as_str()){return Err(err("unsupported composition kind"));}
                    ResourceRef::parse(composition.reference.as_str())?;
                    for reference in &composition.derivation_refs {ResourceRef::parse(reference.as_str())?;}
                    text(&composition.revision,"composition revision",1024)?;
                    selection_sources(std::slice::from_ref(&composition.source))?;
                    if composition.derivation_refs.is_empty(){return Err(err("returned composition/artifact must retain its derivation"));}
                    if let Some(i)=meta.compositions.iter().position(|c|c.reference==composition.reference){meta.compositions[i]=composition.clone();}else{meta.compositions.push(composition.clone());}
                },
                Change::PlaceSet{participation_ref,places}=>{
                    let index=member_index(&frame,participation_ref)?;
                    if places.iter().any(|p|p.source_ref.as_deref().is_none_or(str::is_empty)){return Err(err("a place requires a native source basis, not invented coordinates"));}
                    let extensions=&mut frame.constellations[0].members[index].extensions;
                    let mut facets=parse_facets_from_extensions(extensions)?;
                    facets.spatial=places.clone();
                    extensions.remove(TECHNE_FACET_EXTENSION);
                    write_facets_to_extensions(extensions,&facets)?;
                }
            }
        }
        validate_construction(doc,&frame,&meta)?;
        if request.expected_revision>0 && !ledger.outcome().changed {
            let previous=frame_of(doc,&request.frame_ref)?;
            if core_metadata(&previous)?==meta && previous.constellations==frame.constellations {return Ok(());}
        }
        if meta.applied.len()>=LIMIT{return Err(err("operation receipt budget reached; preserve history before continuing"));}
        let revision=if request.expected_revision==0{1}else{request.expected_revision.checked_add(1).ok_or_else(||err("revision exhausted"))?};
        meta.applied.insert(request.operation_ref.to_string(),AppliedOperation{actor_ref:request.actor_ref.clone(),request_digest:digest.clone(),basis_revision:request.expected_revision,result_revision:revision});
        frame.extensions.insert(CONSTRUCTION.into(),encoded(&meta)?);
        let mut members=BTreeSet::from([frame.constellations[0].anchor_ref.clone()]);
        members.extend(frame.constellations[0].members.iter().map(|m|m.ref_id.clone()));
        // Native Return remains a whole/ground relation, never an all-to-all clique.
        let through=frame.constellations[0].anchor_ref.clone();
        frame.constellations[0].returns=meta.compositions.iter().map(|composition| WikiConstellationReturn {
            through_anchor_ref:through.clone(),ground_ref:composition.reference.clone(),ground_kind:Some("own".into()),
            extensions:BTreeMap::from([("derivation_refs".into(),json!(composition.derivation_refs)),("revision".into(),json!(composition.revision)),("kind".into(),json!(composition.kind))]),
        }).collect();
        frame.member_refs=members.into_iter().collect();
        let mut external:BTreeSet<_>=frame.member_refs.iter().filter(|r|!doc.holds(r)).cloned().collect();
        external.extend(meta.compositions.iter().filter(|c|!doc.holds(&c.reference)).map(|c|c.reference.clone()));
        frame.external_refs=external.into_iter().collect();
        let anchor_ref=frame.constellations[0].anchor_ref.clone();
        if let Some(WikiObject::Node(anchor))=doc.object(&anchor_ref){
            let mut anchor=anchor.clone();let expected=anchor.revision;
            anchor.title=Some(meta.title.clone());
            native_put(doc,ledger,WikiObject::Node(anchor),Some(expected))?;
        }
        native_put(doc,ledger,WikiObject::Frame(frame),if request.expected_revision==0{None}else{Some(request.expected_revision)})?;
        Ok(())
    })?;
    let content=if outcome.changed{content}else{input.into()};
    let stored=WikiDocument::parse(&content)?;
    let frame=frame_of(&stored,&request.frame_ref)?;
    Ok(Applied{schema:CONSTRUCTION,frame_ref:request.frame_ref.clone(),revision:frame.revision,idempotent:!outcome.changed,content,
        objects_changed:outcome.touched.iter().map(|t|t.resource.clone()).collect(),warnings:outcome.warnings})
}
fn validate_construction(doc:&WikiDocument,frame:&WikiFrame,meta:&Construction)->Result<()> {
    text(&meta.title,"construction title",1024)?;text(&meta.inquiry.question,"inquiry question",16384)?;
    if meta.inquiry.purpose.len()>16384{return Err(err("purpose budget exceeded"));}
    let mut roles=BTreeSet::new();
    if let Some(form)=&meta.frame{
        text(&form.shape_ref,"shape ref",1024)?;text(&form.contract_ref,"shape contract",1024)?;
        if !["proposed","established"].contains(&form.standing.as_str()){return Err(err("frame must retain its proposed/established standing"));}
        if form.standing=="established" && form.provenance.is_empty(){return Err(err("an established QL reading requires its warrant; proposed authorship does not"));}
        selection_sources(&form.provenance)?;
        if form.roles.len()>LIMIT{return Err(err("role budget exceeded"));}
        for role in &form.roles{ResourceRef::parse(role.role_ref.as_str())?;if !roles.insert(role.role_ref.clone()){return Err(err("duplicate frame role"));}text(&role.label,"role label",1024)?;if role.address.is_null(){return Err(err("role lacks its QL address"));}}
    }
    let mut ids=BTreeSet::new();
    if frame.constellations[0].members.len()>LIMIT{return Err(err("membership budget exceeded"));}
    for member in &frame.constellations[0].members{
        let p=participation(member)?;
        if !ids.insert(p.participation_ref.clone()){return Err(err("duplicate participation identity"));}
        if p.role_ref.as_ref().is_some_and(|r|!roles.contains(r)){return Err(err("membership role is outside the authored frame"));}
        selection_sources(&p.sources)?;
        if let Some(nested)=&p.nested{
            check_reference(nested,&frame.ref_id)?;
            if nested.kind=="constellation" {
                let mut seen=BTreeSet::new();check_nested(doc,&nested.whole_ref,&frame.ref_id,&mut seen,0)?;
            }
        }
    }
    Ok(())
}
fn check_nested(doc:&WikiDocument,reference:&ResourceRef,target:&ResourceRef,seen:&mut BTreeSet<ResourceRef>,depth:usize)->Result<()> {
    if reference==target{return Err(err("nested whole cycle"));}
    if depth>=64{return Err(err("nested disclosure depth exceeded"));}
    if !seen.insert(reference.clone()){return Ok(());}
    if let Some(WikiObject::Frame(frame))=doc.object(reference){
        for constellation in &frame.constellations{for member in &constellation.members{
            if let Ok(p)=participation(member){if let Some(n)=p.nested{if n.kind=="constellation"{check_nested(doc,&n.whole_ref,target,seen,depth+1)?;}}}
        }}
    }
    Ok(())
}

#[cfg(test)]
include!("knowledge_construction_tests.rs");
