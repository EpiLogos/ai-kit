//! Query-only consumption of Central's persistent bkmr-backed file maps.
//! The adapter never creates, rebuilds, opens, or deletes the owner's database.
use crate::runner::{CommandRunner,SystemRunner};
use aikit_core::{AikitError,Result};
use aikit_core::knowledge_source_pool::*;
use aikit_core::resource::{ProviderRef,SourceRef,SourceRevision};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::{BTreeMap,BTreeSet},path::{Path,PathBuf}};
const SCHEMA:&str="central.file-map/v1";
fn invalid(message:impl Into<String>)->AikitError{AikitError::new("central.file_map_invalid",message)}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct CentralLocation { pub root_hint:PathBuf, pub source_ref:String, pub world_ref:String }

pub fn executable()->PathBuf{std::env::var_os("CENTRAL_CTRL_BIN").or_else(||std::env::var_os("OI_CENTRAL_CTRL_BIN")).map(PathBuf::from).unwrap_or_else(||"ctrl".into())}
pub fn discover_root(anchor:&Path)->Option<PathBuf>{
    std::env::var_os("CENTRAL_ROOT").or_else(||std::env::var_os("OI_CENTRAL_ROOT")).map(PathBuf::from)
        .or_else(||anchor.ancestors().find(|p|p.join("Control").is_dir()&&p.join("Work").is_dir()).map(Path::to_path_buf))
}
pub struct CentralMap<R> { runner:R, executable:PathBuf, root:PathBuf, scope:Value, owned:BTreeSet<String>, metadata:Vec<SourceMaterial> }
impl<R:CommandRunner> CentralMap<R>{
    pub fn attach(runner:R,executable:PathBuf,root:PathBuf,scope:Value)->Self{
        let mut client=Self{runner,executable,root,scope,owned:BTreeSet::new(),metadata:vec![]};
        // Attachment is read-only. Failure is retained by the live status call,
        // never repaired by creating an alternative local authoritative index.
        if let Ok(reading)=client.call("inspect",json!({})){
            if let Some(scopes)=reading["scopes"].as_array(){for s in scopes{if let Some(rs)=s["resources"].as_array(){for r in rs{
                if let Ok(material)=material(r){client.owned.insert(material.binding.source.to_string());client.metadata.push(material);}
            }}}}
        }
        client
    }
    pub fn metadata(&self)->&[SourceMaterial]{&self.metadata}
    pub fn call(&self,op:&str,input:Value)->Result<Value>{
        let mut args=self.scope.as_object().cloned().unwrap_or_default();
        args.extend(input.as_object().ok_or_else(||invalid("Map input must be an object"))?.clone());
        let argv=vec![self.executable.display().to_string(),"--json".into(),"--root".into(),self.root.display().to_string(),"action".into(),"run".into(),format!("central.map.{op}"),Value::Object(args).to_string()];
        let out=self.runner.run(&argv)?;
        let envelope:Value=serde_json::from_str(&out.stdout).map_err(|_|AikitError::new("central.file_map_unavailable",format!("Central {op} returned no valid owner envelope")))?;
        if !out.ok() || envelope["ok"]!=true {return Err(AikitError::new("central.file_map_unavailable",envelope["error"]["message"].as_str().unwrap_or("Central map operation failed")));}
        let data=envelope.get("data").ok_or_else(||invalid("Missing owner reading"))?;
        if data["schema"]!=SCHEMA {return Err(invalid("Unsupported Central file-map reading"));}Ok(data.clone())
    }
    pub fn read_source(&self,source:&SourceRef)->Result<SourceMaterial>{
        let reading=self.call("read",json!({"source_ref":source.as_str()}))?;
        if reading["source"]["ref"]!=source.as_str(){return Err(invalid("Owner returned another source"));}
        material(&reading)
    }
}
fn material(v:&Value)->Result<SourceMaterial>{
    let source=&v["source"];
    let reference=source["ref"].as_str().ok_or_else(||invalid("Missing SourceRef"))?;
    let revision=v["revision"].as_str().ok_or_else(||invalid("Missing source revision"))?;
    let mut metadata=BTreeMap::new();metadata.insert("central".into(),json!({"world_ref":v["world_ref"],"scope_root":v["scope_root"],"location":v["location"],"source":source}));
    Ok(SourceMaterial{binding:SourceBinding{source:SourceRef::parse(reference)?,revision:SourceRevision::parse(revision)?,title:source["title"].as_str().filter(|v|!v.is_empty()).or_else(||source["path"].as_str()).unwrap_or(reference).into(),tags:serde_json::from_value(source["tags"].clone()).unwrap_or_default(),visibility:SourceVisibility::Team,owners:vec![],media_type:"text/plain".into(),locator:None,metadata},body:v["content"].as_str().unwrap_or_default().into()})
}
impl<R:CommandRunner> SourcePoolProvider for CentralMap<R>{
    fn capabilities(&self)->SourceProviderCapabilities{
        let reading=self.call("inspect",json!({}));
        let fulltext=reading.as_ref().ok().and_then(|v|v["scopes"].as_array()).is_some_and(|ss|ss.iter().any(|s|s["capabilities"]["fulltext"]==true));
        let mut reasons=BTreeMap::new();if let Err(e)=reading{reasons.insert("provider".into(),e.message().into());}
        reasons.insert("semantic".into(),"Owner has not published semantic index readiness".into());
        reasons.insert("hybrid".into(),"Owner has not published hybrid index readiness".into());
        SourceProviderCapabilities{provider:ProviderRef::parse("provider/source-pool/central-bkmr").unwrap(),version:Some(SCHEMA.into()),fulltext,fuzzy_interactive:false,semantic:false,hybrid:false,tags:true,structured_output:true,reasons}
    }
    fn rebuild(&mut self,_:&[SourceMaterial])->Result<()>{Err(AikitError::new("central.file_map_owner_only","Central owns persistent map refresh; a consumer cannot rebuild it"))}
    fn owns_source(&self,source:&SourceRef)->bool{self.owned.contains(source.as_str())||source.as_str().starts_with("central:source:")}
    fn read_owned_source(&self,source:&SourceRef)->Result<SourceMaterial>{self.read_source(source)}
    fn search(&self,query:&str,mode:SourceSearchMode,tags:&[String],limit:usize)->Result<Vec<SourceHit>>{
        if limit==0{return Ok(vec![]);}
        let v=self.call("search",json!({"query":query,"mode":mode.as_str(),"tags":tags,"limit":limit}))?;
        if v["absences"].as_array().is_some_and(|a|!a.is_empty()) && v["hits"].as_array().is_none_or(|h|h.is_empty()){
            return Err(AikitError::new("central.file_map_unavailable",format!("Central map has no complete answer: {}",v["absences"])));
        }
        v["hits"].as_array().ok_or_else(||invalid("Missing native hit array"))?.iter().enumerate().map(|(rank,h)|{
            let reference=h["ref"].as_str().ok_or_else(||invalid("Hit lacks source identity"))?;
            if h["source"]["source"]["ref"]!=reference{return Err(invalid("Hit source provenance disagrees"));}
            Ok(SourceHit{source:SourceRef::parse(reference)?,provider:ProviderRef::parse("provider/source-pool/central-bkmr").unwrap(),score:Some(1.0/(rank as f64+1.0)),title:h["title"].as_str().unwrap_or(reference).into(),snippet:h["snippet"].as_str().unwrap_or("").into(),tags:serde_json::from_value(h["tags"].clone()).map_err(|e|invalid(e.to_string()))?,provider_binding:Some(format!("{}:{}",h["world_ref"].as_str().ok_or_else(||invalid("Missing owning World"))?,h["native_id"].as_i64().ok_or_else(||invalid("Missing native row id"))?)),retrieval_mode:mode})
        }).collect()
    }
}

pub fn client(root:PathBuf)->CentralMap<SystemRunner>{CentralMap::attach(SystemRunner::new(),executable(),root,json!({"scope":"all"}))}
pub fn resolve_location(location:&CentralLocation)->Result<(CentralMap<SystemRunner>,Value)>{
    let cwd=std::env::current_dir().map_err(|e|invalid(e.to_string()))?;
    let root=discover_root(&cwd).unwrap_or_else(||location.root_hint.clone());
    let c=client(root);let v=c.call("resolve",json!({"source_ref":location.source_ref}))?;
    if v["world_ref"]!=location.world_ref{return Err(invalid("Skill source resolved into another owning World"));}Ok((c,v))
}
/// An explicit skill-source add delegates location registration to Central. The
/// human operation is declared, not authentication of arbitrary agent callers.
pub fn bind_directory(root:PathBuf,path:&Path)->Result<CentralLocation>{
    let c=client(root.clone());let map=c.call("inspect",json!({}))?;
    let scopes=map["scopes"].as_array().ok_or_else(||invalid("Missing owner scopes"))?;
    let s=scopes.iter().filter(|s|s["root"].as_str().is_some_and(|r|path.starts_with(r))).max_by_key(|s|s["root"].as_str().unwrap().len()).ok_or_else(||invalid("No owning Central scope for this skill directory"))?;
    let owner=Path::new(s["root"].as_str().unwrap());let relative=path.strip_prefix(owner).map_err(|e|invalid(e.to_string()))?.to_str().ok_or_else(||invalid("Non UTF-8 skill source"))?;
    if let Some(r)=s["resources"].as_array().and_then(|rs|rs.iter().find(|r|r["source"]["path"]==relative)){
        return Ok(CentralLocation{root_hint:root,source_ref:r["source"]["ref"].as_str().unwrap().into(),world_ref:s["world_ref"].as_str().unwrap().into()});
    }
    let mut input=json!({"scope":"root","path":relative,"expected_revision":s["ground_revision"],"actor":"aikit:source-add","actor_kind":"human"});
    if s["world_ref"]!="control:root"{input["project_path"]=json!(owner);}
    let out=c.call("register",input)?;
    Ok(CentralLocation{root_hint:root,source_ref:out["source"]["ref"].as_str().ok_or_else(||invalid("Registration lacks SourceRef"))?.into(),world_ref:out["world_ref"].as_str().unwrap().into()})
}

/// Materialise owner-returned bytes into AIKit's private staging directory.
/// No upstream file, symlink or generated destination is edited in place.
pub fn stage_skill_tree(location:&CentralLocation,staging:&Path)->Result<PathBuf>{
    use base64::Engine;
    use std::os::unix::fs::PermissionsExt;
    let (client,_)=resolve_location(location)?;
    let tree=client.call("skill-tree",json!({"source_ref":location.source_ref}))?;
    if tree["world_ref"]!=location.world_ref || tree["source_ref"]!=location.source_ref {return Err(invalid("Skill tree identity differs from its binding"));}
    let checkout=staging.join("checkout");std::fs::create_dir(&checkout).map_err(|e|invalid(e.to_string()))?;
    let mut seen=BTreeSet::new();let mut total=0usize;
    for f in tree["files"].as_array().ok_or_else(||invalid("Missing owner tree"))? {
        let rel=f["path"].as_str().ok_or_else(||invalid("Missing owner tree path"))?;
        if rel.is_empty() || !Path::new(rel).components().all(|c|matches!(c,std::path::Component::Normal(_))) || !seen.insert(rel.to_owned()) || seen.len()>4096{return Err(invalid("Invalid or duplicate owner tree path"));}
        if f["encoding"]!="base64"{return Err(invalid("Unknown owner tree encoding"));}
        let bytes=base64::engine::general_purpose::STANDARD.decode(f["content"].as_str().ok_or_else(||invalid("Missing tree content"))?).map_err(|e|invalid(e.to_string()))?;
        total+=bytes.len();if bytes.len()>4*1024*1024||total>32*1024*1024{return Err(invalid("Owner tree exceeds bounds"));}
        let path=checkout.join(rel);std::fs::create_dir_all(path.parent().unwrap()).map_err(|e|invalid(e.to_string()))?;
        std::fs::write(&path,bytes).map_err(|e|invalid(e.to_string()))?;
        let mode=f["mode"].as_u64().ok_or_else(||invalid("Missing owner file mode"))? as u32 & 0o777;
        std::fs::set_permissions(path,std::fs::Permissions::from_mode(mode)).map_err(|e|invalid(e.to_string()))?;
    }
    let revision=tree["revision"].as_str().ok_or_else(||invalid("Missing owner tree revision"))?;
    std::fs::write(staging.join("central-tree-revision"),revision).map_err(|e|invalid(e.to_string()))?;
    Ok(checkout)
}
pub fn tree_revision(location:&CentralLocation)->Result<String>{
    let (client,_)=resolve_location(location)?;
    let tree=client.call("skill-tree",json!({"source_ref":location.source_ref}))?;
    tree["revision"].as_str().map(str::to_owned).ok_or_else(||invalid("Missing owner tree revision"))
}
