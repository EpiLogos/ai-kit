#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use aikit_adapters::{QlCliClient, QlOperativeProvider};
    use aikit_core::ql::{
        QlClientSubject, QlOperation, QlProviderClient, QlProviderState, QlRefractionRequest,
    };
    use aikit_core::resource::{OperativeSemanticProvider, OperativeSemanticProviderStatus, ResourceRef};
    use tempfile::TempDir;

    fn ql_fixture() -> (TempDir, std::path::PathBuf) {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("ql");
        fs::write(
            &path,
            r#"#!/bin/sh
if [ "$1" = "capabilities" ]; then
  cat <<'JSON'
{"version":"8.0.0-test","kernel":{"supportedForms":["C","CPrime"]},"service":{"providerState":"available","operations":[{"operation":"refract","supported":true,"deterministic":true},{"operation":"relate","supported":false,"deterministic":false}]}}
JSON
  exit 0
fi
if [ "$1" = "service" ] && [ "$2" = "negotiate" ] && [ "$3" = "refract" ]; then
  printf '%s\n' '{"supported":true,"deterministic":true}'
  exit 0
fi
printf '%s\n' '{"error":"unexpected fixture invocation"}' >&2
exit 2
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        (temp, path)
    }

    #[test]
    fn installed_ql_owner_is_observed_through_existing_client_contract() {
        let (_temp, path) = ql_fixture();
        let client = QlCliClient::new(path);
        let capabilities = client.capabilities();
        assert_eq!(capabilities.health.state, QlProviderState::Available);
        assert_eq!(capabilities.provider.version, "8.0.0-test");
        assert!(capabilities.operations.contains(&QlOperation::Refract));
        assert!(capabilities.deterministic_operations.contains(&QlOperation::Refract));
        assert_eq!(capabilities.supported_forms, vec!["C", "CPrime"]);

        let provider = QlOperativeProvider::new(client);
        assert!(matches!(
            provider.descriptor().status,
            OperativeSemanticProviderStatus::Available
        ));
    }

    #[test]
    fn cli_never_fabricates_refract_when_owner_has_not_exposed_dispatch() {
        let (_temp, path) = ql_fixture();
        let client = QlCliClient::new(path);
        let request = QlRefractionRequest::new(
            QlClientSubject::new(ResourceRef::parse("project/one").unwrap(), Some("r1".into())),
            "ql/interpretation/c-prime",
        );
        let failure = client.refract(&request).unwrap_err();
        assert_eq!(failure.code, "ql.cli_refract_transport_unexposed");
    }

    #[test]
    fn absent_ql_keeps_provider_unavailable_without_affecting_native_resolution() {
        let client = QlCliClient::new("/definitely/not/an/aikit-ql-provider");
        let capabilities = client.capabilities();
        assert_eq!(capabilities.health.state, QlProviderState::Absent);
        assert!(capabilities.operations.is_empty());
        let provider = QlOperativeProvider::new(client);
        assert!(matches!(
            provider.descriptor().status,
            OperativeSemanticProviderStatus::Unavailable { .. }
        ));
    }
}
