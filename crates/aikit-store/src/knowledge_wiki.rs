//! Deletable SQLite materialisation of canonical Wiki objects.
//!
//! Register refs and object refs are copied from authored ground. SQLite row
//! identifiers never cross this boundary. Exact per-register content revisions
//! are the only cache-reuse key, and the core semantic rebuild validates both
//! writes and reads before this provider serves the projection.

use std::path::Path;

use aikit_core::knowledge::{KnowledgeReading, KnowledgeRelationView, RelationQuery};
use aikit_core::knowledge_wiki::{
    WikiEdge, WikiFrame, WikiNode, WikiObject, WikiProvenanceRef, WikiReading, WikiSpace,
};
use aikit_core::knowledge_wiki_index::{SemanticWikiIndex, WikiNeighbour, WikiSearchHit};
use aikit_core::knowledge_wiki_provider::{
    SemanticWikiProvider, SemanticWikiProviderStatus, WikiExplanation, WikiProvider,
    WikiRegisterRevision,
};
use aikit_core::resource::{ProviderRef, ResourceRef, SourceRef};
use aikit_core::{AikitError, Result};
use rusqlite::{params, Connection};

pub const SQLITE_WIKI_PROVIDER: &str = "provider/semantic-wiki/sqlite";
const SCHEMA_VERSION: &str = "aikit.sqlite-wiki/v1";

pub struct SqliteWikiProvider {
    index: SemanticWikiIndex,
    registers: Vec<WikiRegisterRevision>,
}

impl SqliteWikiProvider {
    pub fn index(&self) -> &SemanticWikiIndex {
        &self.index
    }

    pub fn contains(&self, resource: &ResourceRef) -> bool {
        self.index.contains(resource)
    }

    /// Replace the materialisation transactionally after core validation.
    pub fn rebuild(
        path: &Path,
        objects: impl IntoIterator<Item = WikiObject>,
        registers: impl IntoIterator<Item = WikiRegisterRevision>,
    ) -> Result<Self> {
        let objects = objects.into_iter().collect::<Vec<_>>();
        let index = SemanticWikiIndex::rebuild(objects.clone())?;
        let registers = normalise_registers(registers);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| store_error("create", error))?;
        }
        let mut connection = Connection::open(path).map_err(|error| store_error("open", error))?;
        initialise(&connection)?;
        let transaction = connection
            .transaction()
            .map_err(|error| store_error("begin", error))?;
        transaction
            .execute("DELETE FROM wiki_objects", [])
            .map_err(|error| store_error("clear objects", error))?;
        transaction
            .execute("DELETE FROM wiki_register_revisions", [])
            .map_err(|error| store_error("clear revisions", error))?;
        transaction
            .execute("DELETE FROM wiki_metadata", [])
            .map_err(|error| store_error("clear metadata", error))?;
        for object in &objects {
            let (kind, json) = encode_object(object)?;
            transaction
                .execute(
                    "INSERT INTO wiki_objects(resource_ref, object_kind, object_revision, json) VALUES (?1, ?2, ?3, ?4)",
                    params![object.ref_id().as_str(), kind, object.revision().to_string(), json],
                )
                .map_err(|error| store_error("write object", error))?;
        }
        for revision in &registers {
            transaction
                .execute(
                    "INSERT INTO wiki_register_revisions(register_ref, content_revision) VALUES (?1, ?2)",
                    params![revision.register.as_str(), revision.revision],
                )
                .map_err(|error| store_error("write register revision", error))?;
        }
        transaction
            .execute(
                "INSERT INTO wiki_metadata(key, value) VALUES ('schema', ?1), ('semantic_revision', ?2)",
                params![SCHEMA_VERSION, index.revision()],
            )
            .map_err(|error| store_error("write metadata", error))?;
        transaction
            .commit()
            .map_err(|error| store_error("commit", error))?;
        Ok(Self { index, registers })
    }

    /// Open only when every owner-authored register revision still matches.
    pub fn open_current(
        path: &Path,
        registers: impl IntoIterator<Item = WikiRegisterRevision>,
    ) -> Result<Option<Self>> {
        if !path.is_file() {
            return Ok(None);
        }
        let connection = Connection::open(path).map_err(|error| store_error("open", error))?;
        initialise(&connection)?;
        let expected = normalise_registers(registers);
        if read_registers(&connection)? != expected {
            return Ok(None);
        }
        let mut statement = connection
            .prepare("SELECT object_kind, json FROM wiki_objects ORDER BY resource_ref")
            .map_err(|error| store_error("prepare object read", error))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| store_error("read objects", error))?;
        let mut objects = Vec::new();
        for row in rows {
            let (kind, json) = row.map_err(|error| store_error("read object", error))?;
            objects.push(decode_object(&kind, &json)?);
        }
        let index = SemanticWikiIndex::rebuild(objects)?;
        Ok(Some(Self {
            index,
            registers: expected,
        }))
    }

    fn semantic(&self) -> SemanticWikiProvider<'_> {
        SemanticWikiProvider::new(&self.index)
            .with_provider_ref(
                ProviderRef::parse(SQLITE_WIKI_PROVIDER).expect("static provider ref"),
            )
            .with_register_revisions(self.registers.clone())
    }
}

impl WikiProvider for SqliteWikiProvider {
    fn status(&self) -> SemanticWikiProviderStatus {
        self.semantic().status()
    }
    fn discover(&self) -> Vec<ResourceRef> {
        self.semantic().discover()
    }
    fn search(&self, query: &str, limit: usize) -> Vec<WikiSearchHit> {
        self.semantic().search(query, limit)
    }
    fn resolve(&self, resource: &ResourceRef) -> Option<WikiObject> {
        self.semantic().resolve(resource)
    }
    fn read(&self, resource: &ResourceRef) -> Result<KnowledgeReading> {
        self.semantic().read(resource)
    }
    fn neighbours(&self, resource: &ResourceRef, limit: usize) -> Vec<WikiNeighbour> {
        self.semantic().neighbours(resource, limit)
    }
    fn relations(&self, query: RelationQuery) -> Result<KnowledgeRelationView> {
        self.semantic().relations(query)
    }
    fn frame(&self, resource: &ResourceRef) -> Option<WikiFrame> {
        self.semantic().frame(resource)
    }
    fn sources(&self, resource: &ResourceRef) -> Vec<SourceRef> {
        self.semantic().sources(resource)
    }

    fn citing_nodes(&self, source: &SourceRef) -> Vec<ResourceRef> {
        self.semantic().citing_nodes(source)
    }
    fn provenance(&self, resource: &ResourceRef) -> Vec<WikiProvenanceRef> {
        self.semantic().provenance(resource)
    }
    fn explain(&self, resource: &ResourceRef) -> Result<WikiExplanation> {
        self.semantic().explain(resource)
    }
}

fn initialise(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS wiki_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS wiki_register_revisions (register_ref TEXT PRIMARY KEY, content_revision TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS wiki_objects (resource_ref TEXT PRIMARY KEY, object_kind TEXT NOT NULL, object_revision TEXT NOT NULL, json TEXT NOT NULL);",
    ).map_err(|error| store_error("initialise", error))
}

fn normalise_registers(
    registers: impl IntoIterator<Item = WikiRegisterRevision>,
) -> Vec<WikiRegisterRevision> {
    let mut values = registers.into_iter().collect::<Vec<_>>();
    values.sort_by(|left, right| left.register.cmp(&right.register));
    values.dedup_by(|left, right| left.register == right.register);
    values
}

fn read_registers(connection: &Connection) -> Result<Vec<WikiRegisterRevision>> {
    let mut statement = connection.prepare(
        "SELECT register_ref, content_revision FROM wiki_register_revisions ORDER BY register_ref",
    ).map_err(|error| store_error("prepare revisions", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| store_error("read revisions", error))?;
    let mut revisions = Vec::new();
    for row in rows {
        let (register, revision) = row.map_err(|error| store_error("read revision", error))?;
        revisions.push(WikiRegisterRevision {
            register: ResourceRef::parse(register)?,
            revision,
        });
    }
    Ok(revisions)
}

fn encode_object(object: &WikiObject) -> Result<(&'static str, String)> {
    let (kind, value) = match object {
        WikiObject::Space(value) => ("space", serde_json::to_string(value)),
        WikiObject::Node(value) => ("node", serde_json::to_string(value)),
        WikiObject::Edge(value) => ("edge", serde_json::to_string(value)),
        WikiObject::Frame(value) => ("frame", serde_json::to_string(value)),
        WikiObject::Reading(value) => ("reading", serde_json::to_string(value)),
    };
    value
        .map(|json| (kind, json))
        .map_err(|error| store_error("serialize object", error))
}

fn decode_object(kind: &str, json: &str) -> Result<WikiObject> {
    let invalid = |error| store_error("decode object", error);
    match kind {
        "space" => serde_json::from_str::<WikiSpace>(json)
            .map(WikiObject::Space)
            .map_err(invalid),
        "node" => serde_json::from_str::<WikiNode>(json)
            .map(WikiObject::Node)
            .map_err(invalid),
        "edge" => serde_json::from_str::<WikiEdge>(json)
            .map(WikiObject::Edge)
            .map_err(invalid),
        "frame" => serde_json::from_str::<WikiFrame>(json)
            .map(WikiObject::Frame)
            .map_err(invalid),
        "reading" => serde_json::from_str::<WikiReading>(json)
            .map(WikiObject::Reading)
            .map_err(invalid),
        _ => Err(AikitError::new(
            "knowledge.wiki_materialisation_invalid",
            "SQLite Wiki projection contains an unknown object kind",
        )
        .with("kind", kind)),
    }
}

fn store_error(action: &str, error: impl std::fmt::Display) -> AikitError {
    AikitError::new(
        "knowledge.wiki_materialisation_store",
        format!("could not {action} SQLite Wiki projection: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use aikit_core::knowledge_wiki::parse_wiki_objects;
    use aikit_core::knowledge_wiki_index::{SemanticWikiIndex, WikiSearchAddress};
    use aikit_core::knowledge_wiki_provider::SemanticWikiProvider;

    use super::*;

    fn objects() -> Vec<WikiObject> {
        parse_wiki_objects(
            r#"{"objects":[
              {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:test","revision":1,"node_refs":["wiki:node:one"]},
              {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:one","revision":18446744073709551615,"type":"concept","title":"One","space_refs":["wiki:space:test"],"source_refs":["source:test:one"]}
            ]}"#,
        )
        .unwrap()
    }

    fn revisions(value: &str) -> Vec<WikiRegisterRevision> {
        vec![WikiRegisterRevision {
            register: ResourceRef::parse("wiki:space:test").unwrap(),
            revision: value.into(),
        }]
    }

    #[test]
    fn exact_register_revisions_gate_reuse_without_becoming_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wiki.sqlite3");
        let written = SqliteWikiProvider::rebuild(&path, objects(), revisions("blake3:a")).unwrap();
        assert_eq!(written.status().provider.as_str(), SQLITE_WIKI_PROVIDER);

        let reopened = SqliteWikiProvider::open_current(&path, revisions("blake3:a"))
            .unwrap()
            .expect("matching canonical revisions reuse the projection");
        assert_eq!(reopened.discover(), written.discover());
        assert_eq!(
            reopened.search("One", 8)[0]
                .address
                .as_curated()
                .unwrap()
                .as_str(),
            "wiki:node:one"
        );
        assert!(
            SqliteWikiProvider::open_current(&path, revisions("blake3:b"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_deleted_projection_is_rebuilt_from_canonical_objects() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wiki.sqlite3");
        SqliteWikiProvider::rebuild(&path, objects(), revisions("blake3:a")).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(
            SqliteWikiProvider::open_current(&path, revisions("blake3:a"))
                .unwrap()
                .is_none()
        );
        let rebuilt = SqliteWikiProvider::rebuild(&path, objects(), revisions("blake3:a")).unwrap();
        assert_eq!(
            rebuilt
                .resolve(&ResourceRef::parse("wiki:node:one").unwrap())
                .unwrap()
                .revision(),
            u64::MAX
        );
    }

    /// The SQLite projection rebuilds `SemanticWikiIndex` from stored objects,
    /// so the authored-source facet must reconstruct identically to the
    /// native in-memory index — this is what makes CASE 19 findability parity
    /// come free rather than needing its own SQLite-side search logic.
    #[test]
    fn native_and_sqlite_search_parity_includes_authored_source_hits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wiki.sqlite3");
        let sqlite = SqliteWikiProvider::rebuild(&path, objects(), revisions("blake3:a")).unwrap();

        let native_index = SemanticWikiIndex::rebuild(objects()).unwrap();
        let native = SemanticWikiProvider::new(&native_index);

        for query in ["One", "test:one", "wiki:node:one", ""] {
            assert_eq!(
                native.search(query, 16),
                sqlite.search(query, 16),
                "native/sqlite search parity diverged for query {query:?}"
            );
        }

        // The authored source is independently findable through the SQLite
        // projection too, and it is not the same hit as the curated node
        // that cites it.
        let hits = sqlite.search("test:one", 16);
        assert!(hits.iter().any(|hit| hit.address
            == WikiSearchAddress::AuthoredSource {
                source: SourceRef::parse("source:test:one").unwrap()
            }));
        assert!(!hits
            .iter()
            .any(|hit| hit.address.as_curated().is_some_and(
                |resource| resource.as_str() == "source:test:one"
            )));
    }
}
