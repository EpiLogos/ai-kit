//! Live-Redis proof for the knowledge result cache: put/get round trip,
//! overwrite, namespace isolation, and status. Gated on
//! `AIKIT_TEST_REDIS_ADDR` exactly like the NOW-context Redis proofs.

use aikit_store::knowledge_cache::{KnowledgeCacheStore, KnowledgeCacheStatus};
use aikit_store::now_context::{RedisNowConfig, NOW_REDIS_CONFIG_SCHEMA};

fn config(address: String, prefix: String) -> RedisNowConfig {
    RedisNowConfig {
        schema: NOW_REDIS_CONFIG_SCHEMA.into(),
        address,
        database: 0,
        key_prefix: prefix,
        username: None,
        credential_ref: None,
        allow_remote: false,
        connect_timeout_ms: 1000,
        io_timeout_ms: 1000,
        prepared_ttl_seconds: 3600,
        coordination_retention_seconds: 3600,
    }
}

#[test]
fn knowledge_cache_round_trips_overwrites_and_isolates_namespaces() {
    let Ok(address) = std::env::var("AIKIT_TEST_REDIS_ADDR") else {
        eprintln!("AIKIT_TEST_REDIS_ADDR absent; real Redis integration is exercised by the dedicated workflow");
        return;
    };
    let run = ulid::Ulid::generate().to_string();
    let first = KnowledgeCacheStore::new(config(address.clone(), format!("aikit-kc-test-{run}-a")))
        .unwrap();
    let second = KnowledgeCacheStore::new(config(address, format!("aikit-kc-test-{run}-b"))).unwrap();

    let status = first.status(None).unwrap();
    assert_eq!(
        status,
        KnowledgeCacheStatus {
            available: true,
            address: status.address.clone(),
            database: 0,
            key_prefix: format!("aikit-kc-test-{run}-a"),
        }
    );

    let operation = "relations\x1fbasis-1\x1fsource:git/demo\x1f2\x1f64\x1f256";
    assert_eq!(first.get(None, operation).unwrap(), None);

    first
        .put(None, operation, r#"{"nodes":[1,2]}"#, 600)
        .unwrap();
    assert_eq!(
        first.get(None, operation).unwrap().as_deref(),
        Some(r#"{"nodes":[1,2]}"#)
    );

    // A later writer of the same basis wins; the payloads agree because the
    // inputs were the same basis.
    first
        .put(None, operation, r#"{"nodes":[1,2,3]}"#, 600)
        .unwrap();
    assert_eq!(
        first.get(None, operation).unwrap().as_deref(),
        Some(r#"{"nodes":[1,2,3]}"#)
    );

    // A different key prefix is a different namespace: no cross-family read.
    assert_eq!(second.get(None, operation).unwrap(), None);

    // A different basis is a different key: the old entry is not served.
    assert_eq!(first.get(None, "relations\x1fbasis-2\x1fsource:git/demo\x1f2\x1f64\x1f256").unwrap(), None);
}
