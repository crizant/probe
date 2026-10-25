//! Pure, conservative import of one POSIX cURL command.

use std::{collections::BTreeMap, time::Duration};

use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Body, Header, RawBody, RawBodyKind,
    Request, RequestBody, RequestKind, RequestSettings,
};

/// A cURL import failure. Diagnostics never retain any input or secret values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurlParseError {
    /// Invalid quoting, shell syntax, or an unsupported shell expansion.
    ShellSyntax,
    /// The command must start with `curl` and describe exactly one URL.
    InvalidCommand,
    /// An option is unknown or outside the supported request subset.
    UnsupportedOption,
    /// A supported option has a missing or invalid value.
    InvalidValue,
    /// The request needs behavior the native model cannot faithfully represent.
    UnsupportedBehavior,
}

impl std::fmt::Display for CurlParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ShellSyntax => "invalid or unsupported POSIX shell syntax",
            Self::InvalidCommand => "expected one curl command with one HTTP URL",
            Self::UnsupportedOption => "unsupported curl option",
            Self::InvalidValue => "missing or invalid curl option value",
            Self::UnsupportedBehavior => "curl behavior cannot be represented faithfully",
        })
    }
}
impl std::error::Error for CurlParseError {}

/// Parses one pasted cURL command without executing code or doing I/O.
///
/// Supports POSIX quoting, backslash continuations, URL, method, ordered headers,
/// inline data/cookies, Basic credentials, and exporter-compatible redirects/timeouts.
/// Unknown options and shell expansions are rejected. Bodies remain raw text with an
/// explicit content type, preserving JSON and encoded form bytes without normalization.
/// Only GET, POST, PUT, PATCH, and DELETE are accepted, matching Probe's engine.
/// `--location`/`-L`, `--max-redirs` (nonnegative), and `--max-time` (seconds) map to
/// native settings. Redirects default to off; the redirect limit defaults to 50.
/// `--globoff`/`-g` disables cURL URL globbing. Long `--option=value` and attached
/// short values (`-XPOST`, `-H...`, `-d...`, `-u...`, `-b...`) are accepted.
///
/// File inputs, multipart, compression/TLS/proxy options, header suppression,
/// non-ASCII Basic credentials, URL credentials, shell expansions, and other shells
/// are deliberately rejected. Quoted shell metacharacters remain literal data.
/// Bodies are never reclassified as JSON because Probe strips comments from JSON
/// bodies during execution. Explicit content types retain their original meaning.
/// Request metadata is left for the caller. Persistence and UI are separate operations.
pub fn parse_curl(command: &str) -> Result<Request, CurlParseError> {
    let words = tokenize(command)?;
    if words.first().map(String::as_str) != Some("curl") {
        return Err(CurlParseError::InvalidCommand);
    }
    let mut request = Request {
        settings: RequestSettings {
            follow_redirects: Some(false),
            max_redirects: Some(50),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut body: Option<String> = None;
    let mut globoff = false;
    let mut basic = false;
    let mut cookie = None;
    let mut args = words.iter().skip(1);
    let mut positional = false;
    while let Some(arg) = args.next() {
        if arg == "--" && !positional {
            positional = true;
            continue;
        }
        if positional || !arg.starts_with('-') {
            set_url(&mut request, arg)?;
            continue;
        }
        let (option, attached) = if arg.starts_with("--") {
            arg.split_once('=')
                .map_or((arg.as_str(), None), |(a, b)| (a, Some(b)))
        } else if arg.len() > 2
            && arg.is_char_boundary(2)
            && matches!(&arg[..2], "-X" | "-H" | "-d" | "-b" | "-u")
        {
            (&arg[..2], Some(&arg[2..]))
        } else {
            (arg.as_str(), None)
        };
        if matches!(option, "--globoff" | "-g" | "--location" | "-L" | "--basic") {
            if attached.is_some() {
                return Err(CurlParseError::InvalidValue);
            }
            match option {
                "--globoff" | "-g" => globoff = true,
                "--basic" => basic = true,
                _ => request.settings.follow_redirects = Some(true),
            }
            continue;
        }
        if !matches!(
            option,
            "--url"
                | "-X"
                | "--request"
                | "-H"
                | "--header"
                | "-d"
                | "--data"
                | "--data-raw"
                | "--data-binary"
                | "-b"
                | "--cookie"
                | "-u"
                | "--user"
                | "--max-redirs"
                | "--max-time"
        ) {
            return Err(CurlParseError::UnsupportedOption);
        }
        let value = attached
            .or_else(|| args.next().map(String::as_str))
            .ok_or(CurlParseError::InvalidValue)?;
        match option {
            "--url" => set_url(&mut request, value)?,
            "-X" | "--request" => {
                // Probe's HTTP engine currently supports these five methods only.
                if !matches!(value, "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
                    return Err(CurlParseError::UnsupportedBehavior);
                }
                request.method = Some(value.into());
            }
            "-H" | "--header" => request.headers.push(parse_header(value)?),
            "-d" | "--data" | "--data-raw" | "--data-binary" => {
                if option != "--data-raw" && value.starts_with('@') {
                    return Err(CurlParseError::UnsupportedBehavior);
                }
                if let Some(body) = &mut body {
                    body.push('&');
                    body.push_str(value);
                } else {
                    body = Some(value.into());
                }
            }
            "-b" | "--cookie" => {
                if !value.contains('=') || value.contains(['\r', '\n']) {
                    return Err(CurlParseError::UnsupportedBehavior);
                }
                cookie = Some(value);
            }
            "-u" | "--user" => {
                let (username, password) = value
                    .split_once(':')
                    .ok_or(CurlParseError::UnsupportedBehavior)?;
                if !username.is_ascii() || !password.is_ascii() || value.contains(['\r', '\n']) {
                    return Err(CurlParseError::UnsupportedBehavior);
                }
                request.authentication = Some(Authentication {
                    kind: AuthenticationKind::Basic,
                    properties: BTreeMap::from([
                        (
                            "username".into(),
                            AuthenticationValue::String(username.into()),
                        ),
                        (
                            "password".into(),
                            AuthenticationValue::String(password.into()),
                        ),
                    ]),
                });
            }
            "--max-redirs" => {
                request.settings.max_redirects =
                    Some(value.parse().map_err(|_| CurlParseError::InvalidValue)?)
            }
            "--max-time" => {
                let seconds: f64 = value.parse().map_err(|_| CurlParseError::InvalidValue)?;
                request.settings.timeout = Some(
                    Duration::try_from_secs_f64(seconds)
                        .map_err(|_| CurlParseError::InvalidValue)?,
                );
            }
            _ => unreachable!(),
        }
    }
    let url = request
        .url
        .as_deref()
        .ok_or(CurlParseError::InvalidCommand)?;
    if !globoff && url.contains(['{', '}', '[', ']']) {
        return Err(CurlParseError::UnsupportedBehavior);
    }
    if basic && request.authentication.is_none() {
        return Err(CurlParseError::UnsupportedBehavior);
    }
    if request.authentication.is_some()
        && request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("authorization"))
    {
        return Err(CurlParseError::UnsupportedBehavior);
    }
    if let Some(cookie) = cookie {
        if request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("cookie"))
        {
            return Err(CurlParseError::UnsupportedBehavior);
        }
        request.headers.push(Header {
            name: "Cookie".into(),
            value: cookie.into(),
            disabled: false,
        });
    }
    request
        .method
        .get_or_insert_with(|| if body.is_some() { "POST" } else { "GET" }.into());
    if let Some(data) = body {
        if !request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type"))
        {
            request.headers.push(Header {
                name: "Content-Type".into(),
                value: "application/x-www-form-urlencoded".into(),
                disabled: false,
            });
        }
        request.kind = RequestKind::Http {
            body: Some(RequestBody::Single(Body::Raw(RawBody {
                kind: RawBodyKind::Text,
                data,
            }))),
        };
    }
    Ok(request)
}

fn set_url(request: &mut Request, value: &str) -> Result<(), CurlParseError> {
    if request.url.is_some() {
        return Err(CurlParseError::InvalidCommand);
    }
    let url = url::Url::parse(value).map_err(|_| CurlParseError::InvalidValue)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(CurlParseError::InvalidValue);
    }
    // URL credentials have different transport behavior across clients; require --user.
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CurlParseError::UnsupportedBehavior);
    }
    request.url = Some(value.into());
    Ok(())
}

fn parse_header(value: &str) -> Result<Header, CurlParseError> {
    let (name, value) = value
        .split_once(':')
        .ok_or(CurlParseError::UnsupportedBehavior)?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        || value.bytes().any(|b| b.is_ascii_control() && b != b'\t')
    {
        return Err(CurlParseError::InvalidValue);
    }
    // cURL's empty-value colon syntax suppresses a header; Probe cannot express that.
    let value = value.trim_start_matches([' ', '\t']);
    if value.is_empty() {
        return Err(CurlParseError::UnsupportedBehavior);
    }
    Ok(Header {
        name: name.into(),
        value: value.into(),
        disabled: false,
    })
}

fn tokenize(command: &str) -> Result<Vec<String>, CurlParseError> {
    if command.contains('\0') {
        return Err(CurlParseError::ShellSyntax);
    }
    let mut chars = command.chars().peekable();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    while let Some(c) = chars.next() {
        if c == '\r' && quote.is_some() {
            return Err(CurlParseError::ShellSyntax);
        }
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '\\' => {
                let next = chars.next().ok_or(CurlParseError::ShellSyntax)?;
                if next == '\n' {
                    continue;
                }
                if next == '\r' {
                    if chars.next() != Some('\n') {
                        return Err(CurlParseError::ShellSyntax);
                    }
                    continue;
                }
                if quote == Some('"') && !matches!(next, '$' | '`' | '"' | '\\') {
                    word.push('\\');
                }
                word.push(next);
                started = true;
            }
            '$' | '`' => return Err(CurlParseError::ShellSyntax),
            '"' | '\'' if quote.is_none() => {
                quote = Some(c);
                started = true;
            }
            '"' if quote == Some('"') => quote = None,
            c if quote.is_some() => word.push(c),
            '\n' | '\r' => {
                if c == '\r' && chars.next() != Some('\n') {
                    return Err(CurlParseError::ShellSyntax);
                }
                if words.is_empty() && !started {
                    continue;
                }
                if chars.all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) {
                    break;
                }
                return Err(CurlParseError::ShellSyntax);
            }
            ';' | '|' | '&' | '<' | '>' | '(' | ')' | '#' | '~' | '*' | '?' | '[' | ']' | '{'
            | '}' | '^' => return Err(CurlParseError::ShellSyntax),
            ' ' | '\t' => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(CurlParseError::ShellSyntax);
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;
    use probe_http::{ExecutionOptions, HttpEngine};

    fn data(request: &Request) -> &str {
        let Some(RequestBody::Single(Body::Raw(body))) = request.http_body() else {
            panic!("expected raw body")
        };
        &body.data
    }

    #[test]
    fn chrome_multiline_export_preserves_headers_query_and_json() {
        let request = parse_curl(
            r#"curl 'https://example.com/api?q=a%2Bb&q=two' \
  -H 'accept: application/json' \
  -H 'x-tag: first' \
  -H 'x-tag: second' \
  -H 'content-type: application/json' \
  -b 'session=private; theme=dark' \
  --data-raw '{ "name": "O'\''Brien", "price": "$5", "path": "C:\\tmp" }'"#,
        )
        .unwrap();
        assert_eq!(
            request.url.as_deref(),
            Some("https://example.com/api?q=a%2Bb&q=two")
        );
        assert!(request.query_parameters.is_empty());
        assert_eq!(request.method.as_deref(), Some("POST"));
        assert_eq!(
            request
                .headers
                .iter()
                .map(|h| (h.name.as_str(), h.value.as_str()))
                .collect::<Vec<_>>(),
            [
                ("accept", "application/json"),
                ("x-tag", "first"),
                ("x-tag", "second"),
                ("content-type", "application/json"),
                ("Cookie", "session=private; theme=dark")
            ]
        );
        assert_eq!(
            data(&request),
            r#"{ "name": "O'Brien", "price": "$5", "path": "C:\\tmp" }"#
        );
        assert_eq!(request.settings.follow_redirects, Some(false));
    }

    #[test]
    fn posix_escapes_and_continuations() {
        let request = parse_curl("curl --url=\"https://example.com\" -H \"x-test: \\\"quoted\\\" \\$5 \\`literal\\` \\\\ \\q\" --data-raw a\\ b\\\nc").unwrap();
        assert_eq!(data(&request), "a bc");
        assert_eq!(request.headers[0].value, "\"quoted\" $5 `literal` \\ \\q");
        assert!(parse_curl("curl 'https://example.com' \\\r\n --data 'one\ntwo'").is_ok());
        assert_eq!(
            data(&parse_curl("curl https://example.com -d ''").unwrap()),
            ""
        );
    }

    #[test]
    fn adjacent_quotes_empty_values_and_trailing_escapes() {
        let request =
            parse_curl(r#"curl https://example.com --data-raw 'one'"two"three\ "#).unwrap();
        assert_eq!(data(&request), "onetwothree ");
        assert_eq!(
            data(&parse_curl("curl https://example.com --data-raw a\u{a0}b").unwrap()),
            "a\u{a0}b"
        );
        let request =
            parse_curl("\n curl https://example.com --data-raw 'a;b|c&d>e<$HOME`literal`'\r\n")
                .unwrap();
        assert_eq!(data(&request), "a;b|c&d>e<$HOME`literal`");
        let request =
            parse_curl("curl https://example.com -XPOST -H'X-Test: yes' -d'a=b'").unwrap();
        assert_eq!(request.method.as_deref(), Some("POST"));
        assert_eq!(data(&request), "a=b");
        assert!(parse_curl("curl https://example.com --data-raw 'bad\0data'").is_err());
        assert!(parse_curl("curl https://example.com --header 'X-Test: bad\u{1}data'").is_err());
        assert!(
            parse_curl("curl https://example.com --data-raw 'a' && curl https://other.com")
                .is_err()
        );
    }

    #[test]
    fn method_inference_and_verbatim_data() {
        assert_eq!(
            parse_curl("curl https://example.com")
                .unwrap()
                .method
                .as_deref(),
            Some("GET")
        );
        for option in ["-d", "--data", "--data-raw", "--data-binary"] {
            let request = parse_curl(&format!(
                "curl https://example.com {option} 'a=%2f+%20&b=&a=2' -d 'z=3' -X PUT"
            ))
            .unwrap();
            assert_eq!(request.method.as_deref(), Some("PUT"));
            assert_eq!(data(&request), "a=%2f+%20&b=&a=2&z=3");
            assert_eq!(
                request.headers[0].value,
                "application/x-www-form-urlencoded"
            );
        }
        let request = parse_curl(
            "curl https://example.com --data-binary 'hello\n世界' -H 'Content-Type: text/plain'",
        )
        .unwrap();
        assert_eq!(data(&request), "hello\n世界");
        assert_eq!(
            data(&parse_curl("curl https://example.com --data-raw '@literal'").unwrap()),
            "@literal"
        );
    }

    #[test]
    fn basic_auth_and_exporter_settings() {
        let request = parse_curl("curl -g -L --max-redirs 4 --max-time 1.25 --basic -u 'alice:p:a:ss' 'https://example.com/[id]'").unwrap();
        assert_eq!(
            request.authentication.as_ref().unwrap().properties["password"],
            AuthenticationValue::String("p:a:ss".into())
        );
        assert_eq!(request.settings.follow_redirects, Some(true));
        assert_eq!(request.settings.max_redirects, Some(4));
        assert_eq!(request.settings.timeout, Some(Duration::from_millis(1250)));
    }

    #[test]
    fn rejects_shell_syntax_and_malformed_commands() {
        for command in [
            "",
            "wget https://example.com",
            "curl",
            "curl 'unfinished",
            "curl \"unfinished",
            "curl https://example.com \\",
            "curl https://example.com; id",
            "curl https://example.com && id",
            "curl https://example.com | cat",
            "curl https://example.com > file",
            "curl https://example.com < file",
            "curl $(id)",
            "curl `id`",
            "curl \"$URL\"",
            "curl $'https://example.com'",
            "curl %URL%",
            "curl https://example.com ^",
            "curl https://example.com\ncurl https://other.com",
            "curl https://example.com # comment",
            "curl https://example.com https://other.com",
            "curl https://example.com -H",
            "curl https://example.com -é",
            "curl https://example.com --data a\\\rb",
            "curl https://example.com -d \"${TOKEN}\"",
            "curl https://example.com -d <(cat file)",
        ] {
            assert!(parse_curl(command).is_err(), "accepted: {command}");
        }
    }

    #[test]
    fn rejects_behavior_outside_native_subset() {
        for suffix in [
            "--compressed",
            "--insecure",
            "--http2",
            "--get",
            "--head",
            "--request HEAD",
            "--request post",
            "--config file",
            "--next",
            "--form a=b",
            "--data @file",
            "--data-binary @-",
            "--header @file",
            "--header 'Accept:'",
            "--header 'Empty;'",
            "--cookie file",
            "--user alice",
            "--digest",
            "--max-time NaN",
            "--max-time -1",
            "--max-redirs -1",
            "--globoff=yes",
            "--basic",
            "--user alice:pass --header 'Authorization: Bearer other'",
            "--cookie a=b --header 'Cookie: c=d'",
        ] {
            assert!(
                parse_curl(&format!("curl https://example.com {suffix}")).is_err(),
                "accepted: {suffix}"
            );
        }
        assert!(parse_curl("curl 'https://example.com/{a,b}'").is_err());
        assert!(parse_curl("curl 'https://alice:pass@example.com'").is_err());
        assert!(parse_curl("curl file:///tmp/input").is_err());
    }

    #[test]
    fn errors_do_not_retain_secrets() {
        for command in [
            "curl https://example.com -H 'Authorization: secret-token' --unknown=secret-token",
            "curl https://example.com -u 'alice:secret-token' --digest",
            "curl https://example.com --cookie 'session=secret-token' --insecure",
            "curl https://example.com --data-raw 'secret-token' --form 'secret-token'",
            "curl 'https://secret-token:pass@example.com'",
            "curl https://example.com -H 'secret-token'",
            "curl https://example.com --data-raw 'secret-token",
        ] {
            let error = parse_curl(command).unwrap_err();
            assert!(!format!("{error} {error:?}").contains("secret-token"));
            assert!(std::error::Error::source(&error).is_none());
        }
    }

    #[test]
    fn imports_native_json_and_form_exports_without_changing_wire_data() {
        let engine = HttpEngine::new().unwrap();
        for body in [
            Body::Raw(RawBody {
                kind: RawBodyKind::Json,
                data: r#"{ /* comment */ "text": "O'Brien", "literal": "$HOME" }"#.into(),
            }),
            Body::FormUrlEncoded(vec![
                probe_core::FormField {
                    name: "key".into(),
                    value: "a+b 世界".into(),
                    disabled: false,
                },
                probe_core::FormField {
                    name: "key".into(),
                    value: "second".into(),
                    disabled: false,
                },
            ]),
        ] {
            let mut request = parse_curl("curl https://example.com -X POST").unwrap();
            request.kind = RequestKind::Http {
                body: Some(RequestBody::Single(body)),
            };
            request.query_parameters.push(probe_core::QueryParameter {
                name: "q".into(),
                value: "a+b".into(),
                disabled: false,
            });
            let exported = engine
                .curl_command(&request.into_http().unwrap(), &ExecutionOptions::default())
                .unwrap();
            let imported = parse_curl(&exported).unwrap();
            let again = engine
                .curl_command(&imported.into_http().unwrap(), &ExecutionOptions::default())
                .unwrap();
            assert_eq!(exported, again);
        }
    }

    #[test]
    fn copy_as_curl_supported_subset_is_semantically_stable() {
        let engine = HttpEngine::new().unwrap();
        for command in [
            "curl 'https://example.com/api?a=%2F&b=1' -H 'x-tag: a' -H 'x-tag: b'",
            "curl https://example.com -X PATCH -H 'Content-Type: application/json' --data-raw '{ \"text\": \"O'\\''Brien\" }'",
            "curl https://example.com -d 'a=%2f+%20&empty=&a=2' -u 'alice:p:ass' -L --max-redirs 3 --max-time 0.5",
            "curl https://example.com -H 'Content-Type: text/plain' --data-binary 'a\nb' -b 'session=abc'",
        ] {
            let original = parse_curl(command).unwrap();
            let exported = engine
                .curl_command(
                    &original.clone().into_http().unwrap(),
                    &ExecutionOptions::default(),
                )
                .unwrap();
            let imported = parse_curl(&exported).unwrap();
            let again = engine
                .curl_command(
                    &imported.clone().into_http().unwrap(),
                    &ExecutionOptions::default(),
                )
                .unwrap();
            assert_eq!(exported, again);
            assert_eq!(original.method, imported.method);
            assert_eq!(
                url::Url::parse(original.url.as_deref().unwrap()).unwrap(),
                url::Url::parse(imported.url.as_deref().unwrap()).unwrap()
            );
            assert_eq!(original.authentication, imported.authentication);
            assert_eq!(original.http_body(), imported.http_body());
            for header in &original.headers {
                assert_eq!(
                    original
                        .headers
                        .iter()
                        .filter(|h| h.name.eq_ignore_ascii_case(&header.name))
                        .map(|h| &h.value)
                        .collect::<Vec<_>>(),
                    imported
                        .headers
                        .iter()
                        .filter(|h| h.name.eq_ignore_ascii_case(&header.name))
                        .map(|h| &h.value)
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}
