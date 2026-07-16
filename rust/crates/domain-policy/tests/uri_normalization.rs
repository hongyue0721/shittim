use domain_policy::{normalize_uri, normalize_uri_pattern, PolicyErrorCode};
use serde_json::Value;

fn task_create_fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/kcp/task_create_normalized_hash.v1.json"
    ))
    .expect("task.create normalization fixture must be valid JSON")
}

#[test]
fn public_normalizers_reproduce_task_create_fixture_without_array_policy() {
    let fixture = task_create_fixture();
    let input = &fixture["command_envelope"]["payload"];
    let expected = &fixture["normalized_payload"];

    let origin = input["origin"]["source_uri"].as_str().unwrap();
    assert_eq!(
        normalize_uri(origin).unwrap(),
        expected["origin"]["source_uri"].as_str().unwrap()
    );

    for field in ["resource_patterns", "exclusions"] {
        let actual = input["task_scope"][field]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| normalize_uri_pattern(value.as_str().unwrap()))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let expected_patterns = expected["task_scope"][field]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected_patterns);
    }

    assert_eq!(
        expected["task_scope"]["resource_patterns"][0],
        expected["task_scope"]["resource_patterns"][1]
    );
    assert_eq!(
        expected["task_scope"]["exclusions"][0],
        expected["task_scope"]["exclusions"][1]
    );
}

#[test]
fn pattern_tokens_query_fragment_and_file_drive_have_defined_normalization() {
    assert_eq!(
        normalize_uri_pattern("HTTPS://Example.COM:443/a/*/**?x=%2f#Part").unwrap(),
        "https://example.com/a/*/**?x=%2F#Part"
    );
    assert_eq!(
        normalize_uri_pattern("file:///c:/Users/*/**").unwrap(),
        "file:///C:/Users/*/**"
    );
}

#[test]
fn concrete_uri_rejects_glob_tokens() {
    for value in [
        "https://example.com/*",
        "https://example.com/**",
        "https://example.com/a?q=*",
    ] {
        let error = normalize_uri(value).unwrap_err();
        assert_eq!(error.code, PolicyErrorCode::InvalidUriPattern, "{value}");
    }
}

#[test]
fn invalid_patterns_fail_closed_with_policy_error() {
    for pattern in [
        "https://example.com/foo*",
        "https://*.example.com/a",
        "https://example.com/a?q=*",
        "https://example.com/a#*",
        "file://server/share/*",
        "file:///C:\\Users\\*",
        "https://example.com/(foo)",
    ] {
        let error = normalize_uri_pattern(pattern).unwrap_err();
        assert_eq!(error.code, PolicyErrorCode::InvalidUriPattern, "{pattern}");
    }
}
