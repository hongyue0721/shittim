use crate::{PolicyError, PolicyErrorCode};
use std::cmp::Ordering;
use url::{Host, Url};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedUri {
    pub value: String,
    scheme: String,
    authority: String,
    path_segments: Vec<String>,
    query: Option<String>,
    fragment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UriPatternScore {
    pub exact: i32,
    pub literal_components: i32,
    pub single_globs: i32,
    pub multi_globs: i32,
    pub pattern: String,
}

impl UriPatternScore {
    fn local_cmp(&self, other: &Self) -> Ordering {
        (
            self.exact,
            self.literal_components,
            -self.single_globs,
            -self.multi_globs,
        )
            .cmp(&(
                other.exact,
                other.literal_components,
                -other.single_globs,
                -other.multi_globs,
            ))
            .then_with(|| other.pattern.as_bytes().cmp(self.pattern.as_bytes()))
    }
}

/// Normalizes a concrete URI according to the policy matching contract.
///
/// This accepts no glob tokens. Call [`normalize_uri_pattern`] for Policy URI patterns.
pub fn normalize_uri(value: &str) -> Result<String, PolicyError> {
    Ok(parse_uri(value, false)?.value)
}

/// Normalizes one Policy URI pattern according to the segment-glob contract.
///
/// Only complete path segments `*` and `**` are accepted. Scheme and authority never support
/// globs, while query and fragment are normalized but remain exact strings and therefore also
/// reject glob tokens. This function deliberately handles one pattern at a time: callers retain
/// ownership of array ordering, duplicate preservation, and error aggregation.
pub fn normalize_uri_pattern(pattern: &str) -> Result<String, PolicyError> {
    Ok(parse_uri(pattern, true)?.value)
}

pub(crate) fn normalize_uri_value(value: &str) -> Result<NormalizedUri, PolicyError> {
    parse_uri(value, false)
}

pub(crate) fn best_uri_pattern(
    patterns: &[String],
    value: &NormalizedUri,
) -> Result<Option<UriPatternScore>, PolicyError> {
    let mut best = None;
    let mut unique: Vec<&String> = patterns.iter().collect();
    unique.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    unique.dedup();
    for pattern in unique {
        let parsed = parse_uri(pattern, true)?;
        if uri_pattern_matches(&parsed, value) {
            let single_globs = parsed.path_segments.iter().filter(|s| *s == "*").count() as i32;
            let multi_globs = parsed.path_segments.iter().filter(|s| *s == "**").count() as i32;
            let literal_paths = parsed
                .path_segments
                .iter()
                .filter(|s| *s != "*" && *s != "**")
                .count() as i32;
            let score = UriPatternScore {
                exact: i32::from(single_globs == 0 && multi_globs == 0),
                literal_components: literal_paths + 2,
                single_globs,
                multi_globs,
                pattern: parsed.value,
            };
            if best
                .as_ref()
                .is_none_or(|old: &UriPatternScore| score.local_cmp(old).is_gt())
            {
                best = Some(score);
            }
        }
    }
    Ok(best)
}

pub(crate) fn any_uri_pattern(
    patterns: &[String],
    value: &NormalizedUri,
) -> Result<bool, PolicyError> {
    Ok(best_uri_pattern(patterns, value)?.is_some())
}

fn parse_uri(input: &str, allow_glob: bool) -> Result<NormalizedUri, PolicyError> {
    if input.contains('\\') {
        return Err(uri_error("backslash is not valid in a policy URI"));
    }
    validate_percent_encoding(input)?;
    validate_glob_placement(input, allow_glob)?;

    let url = Url::parse(input).map_err(|error| uri_error(error.to_string()))?;
    if url.cannot_be_a_base() || (url.host().is_none() && url.scheme() != "file") {
        return Err(uri_error("URI must contain a valid authority"));
    }
    let userinfo = if url.username().is_empty() && url.password().is_none() {
        String::new()
    } else {
        let username = uppercase_percent_encoding(url.username());
        match url.password() {
            Some(password) => format!("{username}:{}@", uppercase_percent_encoding(password)),
            None => format!("{username}@"),
        }
    };

    let scheme = url.scheme().to_ascii_lowercase();
    let host = match url.host() {
        Some(host) => normalize_host(host),
        None => String::new(),
    };
    let port = normalize_port(&scheme, url.port());
    let authority = match port {
        Some(port) => format!("{userinfo}{host}:{port}"),
        None => format!("{userinfo}{host}"),
    };

    let raw_segments: Vec<String> = url
        .path_segments()
        .ok_or_else(|| uri_error("URI path cannot be segmented"))?
        .map(uppercase_percent_encoding)
        .collect();
    let mut path_segments = remove_dot_segments(raw_segments)?;
    if scheme == "file" {
        normalize_file_uri(&authority, &mut path_segments)?;
    }
    validate_glob_segments(&path_segments, allow_glob)?;

    let query = url.query().map(uppercase_percent_encoding);
    let fragment = url.fragment().map(uppercase_percent_encoding);

    let path = if path_segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", path_segments.join("/"))
    };
    let mut value = format!("{scheme}://{authority}{path}");
    if let Some(query) = &query {
        value.push('?');
        value.push_str(query);
    }
    if let Some(fragment) = &fragment {
        value.push('#');
        value.push_str(fragment);
    }
    Ok(NormalizedUri {
        value,
        scheme,
        authority,
        path_segments,
        query,
        fragment,
    })
}

fn uri_pattern_matches(pattern: &NormalizedUri, value: &NormalizedUri) -> bool {
    pattern.scheme == value.scheme
        && pattern.authority == value.authority
        && pattern
            .query
            .as_ref()
            .is_none_or(|query| value.query.as_ref() == Some(query))
        && pattern
            .fragment
            .as_ref()
            .is_none_or(|fragment| value.fragment.as_ref() == Some(fragment))
        && match_segments(&pattern.path_segments, &value.path_segments)
}

fn match_segments(pattern: &[String], value: &[String]) -> bool {
    let mut reachable = vec![false; value.len() + 1];
    reachable[0] = true;
    for token in pattern {
        let mut next = vec![false; value.len() + 1];
        match token.as_str() {
            "**" => {
                let mut seen = false;
                for index in 0..=value.len() {
                    seen |= reachable[index];
                    next[index] = seen;
                }
            }
            "*" => {
                for index in 0..value.len() {
                    if reachable[index] {
                        next[index + 1] = true;
                    }
                }
            }
            literal => {
                for index in 0..value.len() {
                    if reachable[index] && literal == value[index] {
                        next[index + 1] = true;
                    }
                }
            }
        }
        reachable = next;
    }
    reachable[value.len()]
}

fn normalize_host(host: Host<&str>) -> String {
    match host {
        Host::Domain(domain) => domain.to_ascii_lowercase(),
        Host::Ipv4(address) => address.to_string(),
        Host::Ipv6(address) => format!("[{address}]"),
    }
}

fn normalize_port(scheme: &str, port: Option<u16>) -> Option<u16> {
    match (scheme, port) {
        ("http", Some(80)) | ("https", Some(443)) | ("ftp", Some(21)) => None,
        (_, port) => port,
    }
}

fn normalize_file_uri(authority: &str, segments: &mut [String]) -> Result<(), PolicyError> {
    if !authority.is_empty() && authority != "localhost" {
        return Err(uri_error("file URI authority must be empty or localhost"));
    }
    if let Some(first) = segments.first_mut() {
        let bytes = first.as_bytes();
        if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            first.replace_range(0..1, &first[0..1].to_ascii_uppercase());
        }
    }
    Ok(())
}

fn remove_dot_segments(segments: Vec<String>) -> Result<Vec<String>, PolicyError> {
    let mut result = Vec::new();
    for segment in segments {
        match segment.as_str() {
            "." => {}
            ".." => {
                result.pop();
            }
            _ => result.push(segment),
        }
    }
    Ok(result)
}

fn validate_percent_encoding(input: &str) -> Result<(), PolicyError> {
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err(uri_error("invalid percent encoding"));
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    Ok(())
}

fn uppercase_percent_encoding(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            output.push('%');
            output.push((bytes[index + 1] as char).to_ascii_uppercase());
            output.push((bytes[index + 2] as char).to_ascii_uppercase());
            index += 3;
        } else {
            output.push(bytes[index] as char);
            index += 1;
        }
    }
    output
}

fn validate_glob_placement(input: &str, allow_glob: bool) -> Result<(), PolicyError> {
    if !input.contains('*') {
        return Ok(());
    }
    if !allow_glob {
        return Err(uri_error("glob tokens are not valid in a concrete URI"));
    }

    let before_fragment = input.split_once('#').map_or(input, |(head, _)| head);
    if input
        .split_once('#')
        .is_some_and(|(_, fragment)| fragment.contains('*'))
    {
        return Err(uri_error("query and fragment do not support glob tokens"));
    }
    let before_query = before_fragment
        .split_once('?')
        .map_or(before_fragment, |(head, _)| head);
    if before_fragment
        .split_once('?')
        .is_some_and(|(_, query)| query.contains('*'))
    {
        return Err(uri_error("query and fragment do not support glob tokens"));
    }

    let authority_start = before_query
        .find("://")
        .map(|index| index + 3)
        .ok_or_else(|| uri_error("URI pattern must contain a scheme and authority"))?;
    if before_query[..authority_start].contains('*') {
        return Err(uri_error("scheme and authority do not support glob tokens"));
    }
    let authority_and_path = &before_query[authority_start..];
    let (authority, path) = authority_and_path
        .split_once('/')
        .map_or((authority_and_path, ""), |(authority, path)| {
            (authority, path)
        });
    if authority.contains('*') {
        return Err(uri_error("scheme and authority do not support glob tokens"));
    }
    for segment in path.split('/') {
        if segment.contains('*') && segment != "*" && segment != "**" {
            return Err(uri_error("glob tokens must be complete path segments"));
        }
    }
    Ok(())
}

fn validate_glob_segments(segments: &[String], allow_glob: bool) -> Result<(), PolicyError> {
    for segment in segments {
        if segment.contains('*') && (!allow_glob || (segment != "*" && segment != "**")) {
            return Err(uri_error("glob tokens must be complete path segments"));
        }
        if segment.contains('[')
            || segment.contains(']')
            || segment.contains('(')
            || segment.contains(')')
        {
            return Err(uri_error("regular-expression syntax is unsupported"));
        }
    }
    Ok(())
}

fn uri_error(message: impl Into<String>) -> PolicyError {
    PolicyError::new(PolicyErrorCode::InvalidUriPattern, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_glob_and_normalization() {
        let value = normalize_uri_value("HTTPS://Example.COM:443/a/./b/../c/%2f?q=x#f").unwrap();
        assert_eq!(value.value, "https://example.com/a/c/%2F?q=x#f");
        assert!(any_uri_pattern(&["https://example.com/a/*/%2f?q=x#f".into()], &value).unwrap());
        assert!(any_uri_pattern(&["https://example.com/**".into()], &value).unwrap());
    }

    #[test]
    fn public_normalizers_cover_task_create_fixture_semantics() {
        assert_eq!(
            normalize_uri("HTTPS://Example.COM:443/inbox/./message/../request?x=%2f#Part").unwrap(),
            "https://example.com/inbox/request?x=%2F#Part"
        );
        assert_eq!(
            normalize_uri_pattern("HTTPS://Example.COM:443/a/./b/**").unwrap(),
            "https://example.com/a/b/**"
        );
        assert_eq!(
            normalize_uri_pattern("HTTPS://Example.COM:443/a/b/tmp/../cache/*").unwrap(),
            "https://example.com/a/b/cache/*"
        );

        let duplicate_patterns = [
            "HTTPS://Example.COM:443/a/./b/**",
            "https://example.com/a/b/**",
        ];
        let normalized = duplicate_patterns
            .iter()
            .map(|pattern| normalize_uri_pattern(pattern))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            normalized,
            ["https://example.com/a/b/**", "https://example.com/a/b/**"]
        );
    }

    #[test]
    fn public_pattern_normalizer_preserves_exact_query_fragment_and_file_drive() {
        assert_eq!(
            normalize_uri_pattern("HTTPS://Example.COM:443/a/**?x=%2f#Part").unwrap(),
            "https://example.com/a/**?x=%2F#Part"
        );
        assert_eq!(
            normalize_uri_pattern("file:///c:/Users/*/**").unwrap(),
            "file:///C:/Users/*/**"
        );
    }

    #[test]
    fn public_pattern_normalizer_rejects_invalid_patterns_fail_closed() {
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

    #[test]
    fn file_drive_and_invalid_forms() {
        assert_eq!(
            normalize_uri("HTTPS://User:Pass@Example.COM:443/a").unwrap(),
            "https://User:Pass@example.com/a"
        );
        assert_eq!(
            normalize_uri("file:///c:/Users/a").unwrap(),
            "file:///C:/Users/a"
        );
        assert!(normalize_uri("file:///C:\\Users\\a").is_err());
        assert!(best_uri_pattern(
            &["https://example.com/foo*".into()],
            &normalize_uri_value("https://example.com/foobar").unwrap()
        )
        .is_err());
        assert!(best_uri_pattern(
            &["https://example.com/(foo)".into()],
            &normalize_uri_value("https://example.com/foo").unwrap()
        )
        .is_err());
    }

    #[test]
    fn query_and_fragment_are_exact() {
        let value = normalize_uri_value("https://example.com/a?q=1#x").unwrap();
        assert!(!any_uri_pattern(&["https://example.com/a?q=2#x".into()], &value).unwrap());
        assert!(best_uri_pattern(&["https://example.com/a?q=*#x".into()], &value).is_err());
    }
}
