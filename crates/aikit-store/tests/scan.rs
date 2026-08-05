//! The built-in secret scanner.
//!
//! Two failure modes, and only one of them is obvious. Missing a secret lets a
//! credential into a registry and, eventually, into a git history. Crying wolf
//! is quieter but just as fatal: a scanner that quarantines ordinary code teaches
//! people to click past it, and after that it may as well not exist. So the false
//! positive tests below are not padding — they are half the specification.
//!
//! Note also what the scanner is *not* allowed to be: a capsule. It cannot depend
//! on anything that has itself gone through the capture pipeline, because the
//! thing deciding whether unreviewed content is safe must not be unreviewed
//! content.

use aikit_store::scan::{redact, scan, Family, Scanner};

fn families(text: &str) -> Vec<Family> {
    let mut out: Vec<Family> = scan(text).into_iter().map(|f| f.family).collect();
    out.sort();
    out.dedup();
    out
}

fn finds(text: &str) -> bool {
    !scan(text).is_empty()
}

// ---------------------------------------------------------------------------
// Known token shapes
// ---------------------------------------------------------------------------

#[test]
fn a_github_personal_access_token_is_found() {
    let text = "git remote set-url origin https://ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8@github.com/me/repo.git";
    let findings = scan(text);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].family, Family::TokenPrefix);
    assert_eq!(&text[findings[0].range.clone()], "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8");
}

#[test]
fn the_fine_grained_github_token_shape_is_found_too() {
    assert!(finds(
        "GH_TOKEN=github_pat_11ABCDEFG0aBcDeFgHiJkL_zYxWvUtSrQpOnMlKjIhGfEdCbA9876543210"
    ));
}

#[test]
fn an_openai_style_key_is_found() {
    let findings = scan("client = OpenAI(api_key=\"sk-proj-9aZbYcXdWeVfUgThSiRjQkPlOmNn\")");
    assert!(findings.iter().any(|f| f.family == Family::TokenPrefix));
}

#[test]
fn every_slack_token_prefix_is_found() {
    for prefix in ["xoxb", "xoxa", "xoxp", "xoxr", "xoxs"] {
        let text = format!("SLACK={prefix}-2314567890-2314567890-aBcDeFgHiJkLmNoPqRsT");
        assert!(finds(&text), "{prefix} should be recognized");
    }
}

#[test]
fn aws_access_key_ids_are_found_in_both_flavours() {
    assert!(finds("aws_access_key_id = AKIAIOSFODNN7EXAMPLE"));
    assert!(finds("aws_access_key_id = ASIAY34FZKBOKMUTVV7A"));
}

#[test]
fn a_google_api_key_is_found() {
    assert!(finds(
        "https://maps.googleapis.com/maps/api/js?key=AIzaSyD-9tSrke72PouQMnMX-a7eZSW0jkFMBWY"
    ));
}

#[test]
fn gitlab_npm_and_docker_tokens_are_found() {
    assert!(finds("CI_JOB_TOKEN=glpat-ABCdefGHIjklMNOpqrST"));
    assert!(finds(
        "//registry.npmjs.org/:_authToken=npm_bQ8kR2vN5xL7yT1wZ3cF6hJ9mP4sD0gA8eK2"
    ));
    assert!(finds("dckr_pat_L3sT9vQ2xR7mB1nK4jH8gF6dS0aZ"));
}

// ---------------------------------------------------------------------------
// Private keys
// ---------------------------------------------------------------------------

#[test]
fn a_pem_private_key_block_is_found_whole() {
    let text = "here is the deploy key:\n\
        -----BEGIN OPENSSH PRIVATE KEY-----\n\
        b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gt\n\
        ZWQyNTUxOQAAACBQ0Q0dPZ0hRSFRwaXR0cmlja3lidXR0cnVlAAAAEAAAAAtzc2gtZWQy\n\
        -----END OPENSSH PRIVATE KEY-----\n\
        keep this safe";

    let findings = scan(text);
    let key = findings
        .iter()
        .find(|f| f.family == Family::PrivateKey)
        .expect("a PEM block must be found");
    let matched = &text[key.range.clone()];
    assert!(matched.starts_with("-----BEGIN"));
    assert!(
        matched.ends_with("-----END OPENSSH PRIVATE KEY-----"),
        "the whole block has to be covered, not just the marker line"
    );
}

#[test]
fn every_common_private_key_header_is_recognized() {
    for header in [
        "-----BEGIN RSA PRIVATE KEY-----",
        "-----BEGIN EC PRIVATE KEY-----",
        "-----BEGIN DSA PRIVATE KEY-----",
        "-----BEGIN PRIVATE KEY-----",
        "-----BEGIN PGP PRIVATE KEY BLOCK-----",
    ] {
        assert!(finds(header), "{header} should be recognized");
    }
}

#[test]
fn a_public_key_is_not_a_private_key() {
    assert!(!finds("-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkq\n-----END PUBLIC KEY-----"));
}

// ---------------------------------------------------------------------------
// Authorization headers
// ---------------------------------------------------------------------------

#[test]
fn an_authorization_header_is_found_however_it_is_spelled() {
    for text in [
        "curl -H 'Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.QWxhZGRpbjpvcGVuc2VzYW1l'",
        "authorization: Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==",
        "Proxy-Authorization: Bearer abcdefghijklmnopqrstuvwx",
    ] {
        assert!(
            families(text).contains(&Family::AuthorizationHeader),
            "should flag: {text}"
        );
    }
}

#[test]
fn the_word_authorization_on_its_own_is_not_a_finding() {
    assert!(!finds("See the Authorization section of the README for how to log in."));
    assert!(!finds("Authorization: see docs"));
}

// ---------------------------------------------------------------------------
// .env style assignments
// ---------------------------------------------------------------------------

#[test]
fn a_dot_env_secret_assignment_is_found_even_when_the_value_is_dull() {
    let text = "DATABASE_URL=postgres://localhost/app\nDATABASE_PASSWORD=hunter2please\nPORT=8080";
    let findings = scan(text);
    let env: Vec<_> = findings
        .iter()
        .filter(|f| f.family == Family::EnvAssignment)
        .collect();

    assert_eq!(env.len(), 1, "only the password line is a secret: {findings:#?}");
    assert!(text[env[0].range.clone()].contains("DATABASE_PASSWORD"));
}

#[test]
fn ordinary_environment_variables_are_left_alone() {
    let text = "AWS_REGION=us-east-1\nRUST_LOG=debug\nNODE_ENV=production\nPORT=3000\nHOME=/home/me";
    assert!(!finds(text), "{:#?}", scan(text));
}

#[test]
fn an_obvious_placeholder_is_not_reported_as_a_secret() {
    // A quarantined `.env.example` would be a daily annoyance and would train
    // people to ignore the warning.
    for text in [
        "API_KEY=changeme",
        "SECRET_KEY=your-secret-here",
        "PASSWORD=xxxxxxxx",
        "TOKEN=",
        "API_TOKEN=<your token>",
        "AUTH_TOKEN=${AUTH_TOKEN}",
    ] {
        assert!(!finds(text), "{text} should not be reported");
    }
}

// ---------------------------------------------------------------------------
// High entropy — the rule most likely to misfire
// ---------------------------------------------------------------------------

#[test]
fn a_high_entropy_value_assigned_to_a_secret_sounding_name_is_found() {
    let text = "let session_secret = \"Zq7Z+kP3nW9xR2vTbLcF8yJmA4sD6gHu\";";
    assert!(families(text).contains(&Family::HighEntropy), "{:#?}", scan(text));
}

#[test]
fn a_json_credential_field_is_found() {
    let text = r#"{"client_id": "aikit", "client_secret": "9vX2pQ7wR4zT6yU1iO3pA5sD8fG0hJ2k"}"#;
    assert!(families(text).contains(&Family::HighEntropy), "{:#?}", scan(text));
}

#[test]
fn a_long_hex_commit_sha_in_a_url_does_not_trip_the_entropy_rule() {
    // Hex cannot carry more than four bits per character, and the threshold sits
    // above that on purpose — so no commit sha, blob id or checksum can ever
    // reach it, whatever its length.
    let text = "See https://github.com/EpiLogos/ai-kit/commit/9f2b7c1e4a6d8035bf19ce27a4d0e5b83c716d92 \
                for the fix.\nsha256 = \"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"";
    assert!(!finds(text), "{:#?}", scan(text));
}

#[test]
fn a_base64_asset_does_not_trip_the_entropy_rule() {
    let blob = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk".repeat(6);
    let text = format!("const LOGO = \"data:image/png;base64,{blob}\";");
    assert!(!finds(&text), "{:#?}", scan(&text));
}

#[test]
fn ordinary_source_code_is_not_flagged() {
    let text = r#"
use std::collections::BTreeMap;

/// Compute the canonical form used for hashing.
pub fn canonical(table: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for (key, value) in table {
        out.push_str(key);
        out.push('=');
        out.push_str(value);
        out.push(';');
    }
    out
}

const DEFAULT_TIMEOUT_SECONDS: u64 = 90;
let url = "https://example.com/api/v2/resources?include=metadata&limit=100";
"#;
    assert!(!finds(text), "{:#?}", scan(text));
}

#[test]
fn a_uuid_is_not_a_secret() {
    assert!(!finds("request_id = \"3f2504e0-4f89-11d3-9a0c-0305e82c3301\""));
    assert!(!finds("container_id: 4a1e2b7c8d9f0a1b2c3d4e5f60718293"));
}

// ---------------------------------------------------------------------------
// Custom rules
// ---------------------------------------------------------------------------

#[test]
fn a_user_configured_pattern_is_honoured() {
    let scanner = Scanner::new()
        .with_custom_rule("acme-key", r"ACME-[0-9]{8}")
        .unwrap();
    let findings = scanner.scan("deploy with ACME-12345678 today");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].family, Family::Custom);
    assert_eq!(findings[0].rule, "acme-key");
}

#[test]
fn an_invalid_custom_pattern_is_refused_with_a_stable_code() {
    let error = Scanner::new()
        .with_custom_rule("broken", r"([unclosed")
        .unwrap_err();
    assert_eq!(error.code(), "scan.invalid_pattern");
}

// ---------------------------------------------------------------------------
// Findings and redaction
// ---------------------------------------------------------------------------

#[test]
fn a_finding_never_carries_the_secret_it_found() {
    let secret = "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8";
    let findings = scan(&format!("token: {secret}"));
    let rendered = format!("{findings:?}");
    assert!(
        !rendered.contains(secret),
        "a finding is passed around and logged; it must not be a copy of the secret"
    );
    assert!(!findings[0].preview.contains("A1b2C3d4E5f6"));
}

#[test]
fn redaction_removes_the_secret_and_keeps_everything_else() {
    let text = "export GITHUB_TOKEN=ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8 # for CI";
    let redacted = redact(text);

    assert!(!redacted.contains("ghp_A1b2C3d4E5f6"));
    assert!(redacted.contains("export"));
    assert!(redacted.contains("# for CI"));
    assert!(redacted.contains("redacted"));
}

#[test]
fn redacting_text_with_no_secrets_returns_it_unchanged() {
    let text = "cargo nextest run --workspace";
    assert_eq!(redact(text), text);
}

#[test]
fn overlapping_findings_redact_once_rather_than_corrupting_the_text() {
    // A token inside an assignment matches both the prefix rule and the env rule.
    let text = "AWS_SECRET_ACCESS_KEY=AKIAIOSFODNN7EXAMPLE";
    let redacted = redact(text);
    assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
    assert_eq!(redacted.matches("redacted").count(), 1, "got: {redacted}");
}

#[test]
fn redaction_is_stable_under_multibyte_text() {
    let text = "clé — ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8 — fin";
    let redacted = redact(text);
    assert!(redacted.starts_with("clé — "));
    assert!(redacted.ends_with(" — fin"));
    assert!(!redacted.contains("ghp_"));
}

#[test]
fn findings_come_back_in_document_order() {
    let text = "first ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8 then AKIAIOSFODNN7EXAMPLE";
    let findings = scan(text);
    assert!(findings.len() >= 2);
    for pair in findings.windows(2) {
        assert!(pair[0].range.start <= pair[1].range.start);
    }
}
