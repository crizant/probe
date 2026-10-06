use std::{
    cell::RefCell,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    thread::JoinHandle,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use probe_application::{
    HttpExecution, NoSecrets, RequestResolution, SECRET_DIAGNOSTIC_WITHHELD, prepare_request,
};
use probe_core::{
    Environment, EnvironmentResolutionError, EnvironmentVariable, Header, Request, SecretContext,
    SecretError, SecretProvider, SecretValue, SecretVariable, Variable, VariableValue,
    VariableValueSet,
};
use probe_http::{ExecutionOptions, HttpEngine, HttpError, ResponseCache};
use sha2::{Digest, Sha256};

const SECRET: &str = "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR";

#[tokio::test]
async fn api_key_placement_secrets_hide_url_structure_in_both_branches() {
    use probe_core::{Authentication, AuthenticationKind, AuthenticationValue, QueryParameter};

    for placement in ["header", "query"] {
        for field in ["placement", "key", "value"] {
            for transitive in [false, true] {
                let (base, server) = serve_once(http_response("", b"hello"));
                let safe_url = format!("{base}/before/../search");
                let mut request = get(&safe_url);
                request.query_parameters.push(QueryParameter {
                    name: "q".to_owned(),
                    value: "hello world".to_owned(),
                    disabled: false,
                });
                let alias = format!("{field}Alias");
                let reference = format!("{{{{{}}}}}", if transitive { &alias } else { field });
                let mut properties = std::collections::BTreeMap::from([
                    (
                        "key".to_owned(),
                        AuthenticationValue::String("X-Public-Key".to_owned()),
                    ),
                    (
                        "value".to_owned(),
                        AuthenticationValue::String("public-value".to_owned()),
                    ),
                    (
                        "placement".to_owned(),
                        AuthenticationValue::String(placement.to_owned()),
                    ),
                ]);
                properties.insert(field.to_owned(), AuthenticationValue::String(reference));
                request.authentication = Some(Authentication {
                    kind: AuthenticationKind::ApiKey,
                    properties,
                });
                let mut environments = environments();
                environments[0]
                    .variables
                    .extend([secret(field), plain(&alias, &format!("{{{{{field}}}}}"))]);
                let value = match field {
                    "placement" => placement,
                    "key" => "X-Secret-Key",
                    _ => "secret-value",
                };
                let overrides = [(field.to_owned(), value.to_owned())];
                let execution = prepare_request(
                    &request,
                    &RequestResolution {
                        overrides: &overrides,
                        ..local(&environments)
                    },
                    &NoSecrets,
                )
                .unwrap()
                .into_http()
                .unwrap();
                let response = execution
                    .execute(
                        &HttpEngine::new().unwrap(),
                        ExecutionOptions::default(),
                        None,
                        std::future::pending::<()>(),
                        |_| {},
                    )
                    .await
                    .unwrap()
                    .response;
                let head = String::from_utf8(server.join().unwrap()).unwrap();
                let key = if field == "key" {
                    value
                } else {
                    "X-Public-Key"
                };
                let auth_value = if field == "value" {
                    value
                } else {
                    "public-value"
                };
                if placement == "query" {
                    assert!(
                        head.starts_with(&format!(
                            "GET /search?q=hello+world&{key}={auth_value} HTTP/1.1\r\n"
                        )),
                        "{head}"
                    );
                } else {
                    assert!(
                        head.starts_with("GET /search?q=hello+world HTTP/1.1\r\n"),
                        "{head}"
                    );
                    assert!(
                        head.contains(&format!("{}: {auth_value}\r\n", key.to_ascii_lowercase())),
                        "{head}"
                    );
                }
                assert_eq!(
                    response.initial_url,
                    if field == "placement" || placement == "query" {
                        safe_url.clone()
                    } else {
                        format!("{base}/search?q=hello+world")
                    },
                    "placement={placement}, field={field}, transitive={transitive}"
                );
                assert_eq!(response.url, safe_url);
                assert!(!response.url_changed);
            }
        }
    }
}

#[tokio::test]
async fn secret_methods_hide_graphql_url_structure_but_preserve_http_urls() {
    use probe_core::{GraphqlBody, GraphqlOperation, QueryParameter, RequestKind};

    for graphql in [false, true] {
        for method in ["GET", "POST"] {
            for reference in ["{{method}}", "{{methodAlias}}"] {
                let (base, server) = serve_once(http_response("", b"hello"));
                let safe_url = format!("{base}/before/../search");
                let mut request = get(&safe_url);
                request.method = Some(reference.to_owned());
                request.query_parameters.push(QueryParameter {
                    name: "q".to_owned(),
                    value: "hello world".to_owned(),
                    disabled: false,
                });
                if graphql {
                    request.kind = RequestKind::Graphql {
                        body: Some(GraphqlBody::Single(GraphqlOperation {
                            query: Some("query Viewer { viewer { id } }".to_owned()),
                            variables: Some(
                                serde_json::json!({"id": "public"})
                                    .as_object()
                                    .unwrap()
                                    .clone(),
                            ),
                            ..GraphqlOperation::default()
                        })),
                    };
                }
                let mut environments = environments();
                environments[0]
                    .variables
                    .extend([secret("method"), plain("methodAlias", "{{method}}")]);
                let overrides = [("method".to_owned(), method.to_owned())];
                let prepared = prepare_request(
                    &request,
                    &RequestResolution {
                        overrides: &overrides,
                        ..local(&environments)
                    },
                    &NoSecrets,
                )
                .unwrap();
                assert_eq!(prepared.presentation().method.as_deref(), Some(reference));
                let execution = prepared.into_http().unwrap();
                assert!(execution.uses_secrets());
                let response = execution
                    .execute(
                        &HttpEngine::new().unwrap(),
                        ExecutionOptions::default(),
                        None,
                        std::future::pending::<()>(),
                        |_| {},
                    )
                    .await
                    .unwrap()
                    .response;
                let head = String::from_utf8(server.join().unwrap()).unwrap();
                assert!(
                    head.starts_with(&format!("{method} /search?q=hello+world")),
                    "{head}"
                );
                assert_eq!(
                    head.contains("&query="),
                    graphql && method == "GET",
                    "{head}"
                );
                assert_eq!(
                    head.contains("&variables="),
                    graphql && method == "GET",
                    "{head}"
                );
                assert_eq!(
                    response.initial_url,
                    if graphql {
                        safe_url.clone()
                    } else {
                        format!("{base}/search?q=hello+world")
                    },
                    "graphql={graphql}, method={method}, reference={reference}"
                );
                assert_eq!(response.url, safe_url);
                assert!(!response.url_changed);
            }
        }
    }
}

#[tokio::test]
async fn initial_url_disclosure_tracks_secret_provenance_and_http_placement() {
    use probe_core::{
        Authentication, AuthenticationKind, AuthenticationValue, Body, GraphqlBody,
        GraphqlOperation, QueryParameter, RawBody, RawBodyKind, RequestBody, RequestKind,
    };
    for case in [
        "header",
        "body",
        "bearer",
        "api-header",
        "api-query",
        "url",
        "transitive",
        "path",
        "query",
        "query-name",
        "disabled-query",
        "graphql-post",
        "graphql-get",
        "graphql-variables",
        "graphql-operation",
        "graphql-extensions",
        "graphql-header",
    ] {
        let (target, target_server) = serve_once(http_response("", b"hello"));
        let (base, server) = serve_once(format!("HTTP/1.1 302 Found\r\nLocation: {target}/{SECRET}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes());
        let mut request = get(&format!("{base}/before/../search"));
        request.headers.push(Header {
            name: "X-Secret".to_owned(),
            value: "{{token}}".to_owned(),
            disabled: false,
        });
        let parameter = |name: &str, value: &str, disabled| QueryParameter {
            name: name.to_owned(),
            value: value.to_owned(),
            disabled,
        };
        request
            .query_parameters
            .push(parameter("q", "hello world", false));
        match case {
            "header" => {}
            "body" => {
                request.headers.clear();
                request.kind = RequestKind::Http {
                    body: Some(RequestBody::Single(Body::Raw(RawBody {
                        kind: RawBodyKind::Text,
                        data: "{{token}}".to_owned(),
                    }))),
                };
            }
            "bearer" | "api-header" | "api-query" => {
                request.headers.clear();
                let mut properties = std::collections::BTreeMap::new();
                if case == "bearer" {
                    properties.insert(
                        "token".to_owned(),
                        AuthenticationValue::String("{{token}}".to_owned()),
                    );
                } else {
                    for (name, value) in [
                        ("key", "api-key"),
                        ("value", "{{token}}"),
                        (
                            "placement",
                            if case == "api-query" {
                                "query"
                            } else {
                                "header"
                            },
                        ),
                    ] {
                        properties.insert(
                            name.to_owned(),
                            AuthenticationValue::String(value.to_owned()),
                        );
                    }
                }
                request.authentication = Some(Authentication {
                    kind: if case == "bearer" {
                        AuthenticationKind::Bearer
                    } else {
                        AuthenticationKind::ApiKey
                    },
                    properties,
                });
            }
            "url" => request.url = Some(format!("{base}/{{{{token}}}}")),
            "transitive" => request.url = Some(format!("{base}/{{{{authorization}}}}")),
            "path" => {
                request.url = Some(format!("{base}/:id"));
                request
                    .path_parameters
                    .push(parameter("id", "{{authorization}}", false));
            }
            "query" => {
                request
                    .query_parameters
                    .push(parameter("token", "{{authorization}}", false))
            }
            "query-name" => request
                .query_parameters
                .push(parameter("{{token}}", "plain", false)),
            "disabled-query" => {
                request
                    .query_parameters
                    .push(parameter("token", "{{token}}", true))
            }
            _ => {
                let mut operation = GraphqlOperation {
                    query: Some("query Viewer { viewer { id } }".to_owned()),
                    ..GraphqlOperation::default()
                };
                match case {
                    "graphql-variables" => {
                        operation.variables = Some(
                            serde_json::json!({"token": "{{authorization}}"})
                                .as_object()
                                .unwrap()
                                .clone(),
                        )
                    }
                    "graphql-operation" => operation.operation_name = Some("{{token}}".to_owned()),
                    "graphql-extensions" => {
                        operation.extensions = Some(
                            serde_json::json!({"token": "{{token}}"})
                                .as_object()
                                .unwrap()
                                .clone(),
                        )
                    }
                    "graphql-header" => {}
                    _ => {
                        operation.query =
                            Some("query Viewer { viewer(token: \"{{token}}\") { id } }".to_owned())
                    }
                }
                if case == "graphql-post" {
                    request.method = Some("POST".to_owned());
                }
                request.kind = RequestKind::Graphql {
                    body: Some(GraphqlBody::Single(operation)),
                };
            }
        }
        let secret_url = matches!(
            case,
            "api-query"
                | "url"
                | "transitive"
                | "path"
                | "query"
                | "query-name"
                | "graphql-get"
                | "graphql-variables"
                | "graphql-operation"
                | "graphql-extensions"
        );
        let environments = environments();
        let prepared = prepare_request(
            &request,
            &local(&environments),
            &RecordingProvider::default(),
        )
        .unwrap();
        let safe_url = prepared.presentation().url.clone().unwrap();
        let execution = prepared.into_http().unwrap();
        assert!(execution.uses_secrets(), "{case}");
        let response = execution
            .execute(
                &HttpEngine::new().unwrap(),
                ExecutionOptions::default(),
                None,
                std::future::pending::<()>(),
                |_| {},
            )
            .await
            .unwrap()
            .response;
        let head = server.join().unwrap();
        target_server.join().unwrap();
        if secret_url {
            assert_eq!(response.initial_url, safe_url, "{case}");
        } else {
            assert!(
                response
                    .initial_url
                    .starts_with(&format!("{base}/search?q=hello+world")),
                "{case}: {}",
                response.initial_url
            );
        }
        assert!(!response.initial_url.contains(SECRET), "{case}");
        assert_eq!(response.url, safe_url, "{case}");
        assert!(!response.url_changed, "{case}");
        if case == "path" {
            assert!(contains(&head, &format!("/Bearer%20{SECRET}")));
        }
        if case == "graphql-get" {
            assert!(contains(&head, SECRET));
        }
    }
}

/// Returns `SECRET` for `token`, fails for `broken`, and records every lookup.
#[derive(Default)]
struct RecordingProvider {
    lookups: RefCell<Vec<String>>,
}

impl SecretProvider for RecordingProvider {
    fn resolve_secret(
        &self,
        context: &SecretContext<'_>,
    ) -> Result<Option<SecretValue>, SecretError> {
        self.lookups
            .borrow_mut()
            .push(context.variable_name.to_owned());
        match context.variable_name {
            "token" | "unused" => Ok(Some(SecretValue::new(SECRET.to_owned()))),
            "broken" => Err(SecretError),
            _ => Ok(None),
        }
    }
}

fn secret(name: &str) -> EnvironmentVariable {
    EnvironmentVariable::Secret(SecretVariable {
        name: Some(name.to_owned()),
        value_type: None,
        disabled: false,
    })
}

fn plain(name: &str, value: &str) -> EnvironmentVariable {
    EnvironmentVariable::Plain(Variable {
        name: Some(name.to_owned()),
        value: Some(VariableValueSet::Single(VariableValue::String(
            value.to_owned(),
        ))),
        disabled: false,
    })
}

fn environments() -> Vec<Environment> {
    vec![Environment {
        name: "local".to_owned(),
        color: None,
        extends: None,
        description: None,
        dot_env_file_path: None,
        variables: vec![
            secret("token"),
            secret("unused"),
            secret("broken"),
            secret("absent"),
            plain("authorization", "Bearer {{token}}"),
        ],
    }]
}

fn local<'a>(environments: &'a [Environment]) -> RequestResolution<'a> {
    RequestResolution {
        environments,
        environment: Some("local"),
        ..RequestResolution::default()
    }
}

fn get(url: &str) -> Request {
    Request {
        method: Some("GET".to_owned()),
        url: Some(url.to_owned()),
        ..Request::default()
    }
}

fn secret_request(base: &str) -> Request {
    Request {
        headers: vec![Header {
            name: "Authorization".to_owned(),
            value: "{{authorization}}".to_owned(),
            disabled: false,
        }],
        ..get(&format!("{base}/{{{{token}}}}"))
    }
}

fn prepare_secret_execution(base: &str) -> HttpExecution {
    let environments = environments();
    prepare_request(
        &secret_request(base),
        &local(&environments),
        &RecordingProvider::default(),
    )
    .unwrap()
    .into_http()
    .unwrap()
}

fn serve_once(response: Vec<u8>) -> (String, JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut head = Vec::new();
        let mut buffer = [0; 4096];
        while !head.windows(4).any(|part| part == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0);
            head.extend_from_slice(&buffer[..count]);
        }
        // The client may close early after an output failure.
        let _ = stream.write_all(&response);
        head
    });
    (base, server)
}

fn http_response(extra_headers: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|part| part == needle.as_bytes())
}

fn temporary_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "probe-application-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn invocation_without_resolution_inputs_keeps_templates_and_never_consults_the_provider() {
    let environments = environments();
    let provider = RecordingProvider::default();
    let source = secret_request("http://example.test");
    let prepared = prepare_request(
        &source,
        &RequestResolution {
            environments: &environments,
            ..RequestResolution::default()
        },
        &provider,
    )
    .unwrap();
    assert_eq!(prepared.presentation(), &source);
    assert!(!prepared.uses_secrets());
    assert!(provider.lookups.borrow().is_empty());

    let unfinished = get("http://example.test/{{unfinished");
    let prepared = prepare_request(&unfinished, &RequestResolution::default(), &provider).unwrap();
    assert_eq!(prepared.presentation(), &unfinished);
}

#[test]
fn presentation_and_debug_output_keep_secret_references_reached_through_plain_variables() {
    let environments = environments();
    let provider = RecordingProvider::default();
    let prepared = prepare_request(
        &secret_request("http://example.test"),
        &local(&environments),
        &provider,
    )
    .unwrap();
    assert!(prepared.uses_secrets());
    assert_eq!(*provider.lookups.borrow(), ["token"]);
    assert_eq!(
        prepared.presentation().url.as_deref(),
        Some("http://example.test/{{token}}")
    );
    assert_eq!(
        prepared.presentation().headers[0].value,
        "{{authorization}}"
    );
    let debug = format!("{prepared:?}");
    assert!(!debug.contains(SECRET));
    assert!(debug.contains("{{authorization}}") && debug.contains("uses_secrets: true"));
    let execution = prepared.into_http().unwrap();
    assert!(execution.uses_secrets());
    let debug = format!("{execution:?}");
    assert!(!debug.contains(SECRET));
    assert!(debug.contains("uses_secrets: true"));
}

#[test]
fn unreferenced_secret_declarations_are_not_read_and_do_not_mark_secret_use() {
    let environments = environments();
    let provider = RecordingProvider::default();
    let prepared = prepare_request(
        &get("http://example.test/"),
        &local(&environments),
        &provider,
    )
    .unwrap();
    assert!(!prepared.uses_secrets());
    assert!(provider.lookups.borrow().is_empty());
}

#[test]
fn referenced_unavailable_or_failed_secrets_fail_closed() {
    let environments = environments();
    assert_eq!(
        prepare_request(
            &get("http://example.test/{{token}}"),
            &local(&environments),
            &NoSecrets,
        )
        .unwrap_err(),
        EnvironmentResolutionError::SecretVariableUnavailable("token".to_owned())
    );
    assert_eq!(
        prepare_request(
            &get("http://example.test/{{broken}}"),
            &local(&environments),
            &RecordingProvider::default(),
        )
        .unwrap_err(),
        EnvironmentResolutionError::SecretProviderFailure("broken".to_owned())
    );
}

#[test]
fn strict_resolution_rejects_missing_values_that_lenient_resolution_keeps_literal() {
    let environments = environments();
    let request = get("http://example.test/{{missing}}");
    let lenient = prepare_request(&request, &local(&environments), &NoSecrets).unwrap();
    assert_eq!(
        lenient.presentation().url.as_deref(),
        Some("http://example.test/{{missing}}")
    );
    let strict = RequestResolution {
        strict_variables: true,
        ..local(&environments)
    };
    assert_eq!(
        prepare_request(&request, &strict, &NoSecrets).unwrap_err(),
        EnvironmentResolutionError::MissingVariable("missing".to_owned())
    );
    let strict_without_environment = RequestResolution {
        strict_variables: true,
        ..RequestResolution::default()
    };
    assert!(prepare_request(&request, &strict_without_environment, &NoSecrets).is_err());
}

#[test]
fn runtime_overrides_resolve_without_an_environment_and_stay_secret_for_secret_declarations() {
    let overrides = [("host".to_owned(), "example.test".to_owned())];
    let prepared = prepare_request(
        &get("http://{{host}}/"),
        &RequestResolution {
            overrides: &overrides,
            ..RequestResolution::default()
        },
        &NoSecrets,
    )
    .unwrap();
    assert_eq!(
        prepared.presentation().url.as_deref(),
        Some("http://example.test/")
    );
    assert!(!prepared.uses_secrets());

    let environments = environments();
    let overrides = [("token".to_owned(), SECRET.to_owned())];
    let prepared = prepare_request(
        &get("http://example.test/{{token}}"),
        &RequestResolution {
            overrides: &overrides,
            ..local(&environments)
        },
        &NoSecrets,
    )
    .unwrap();
    assert!(prepared.uses_secrets());
    assert_eq!(
        prepared.presentation().url.as_deref(),
        Some("http://example.test/{{token}}")
    );
    assert!(!format!("{prepared:?}").contains(SECRET));
}

#[tokio::test]
async fn secret_bearing_response_is_redacted_and_reports_the_presentation_url() {
    let mut body = SECRET.as_bytes().to_vec();
    body.extend_from_slice(b"\xff-tail");
    let (target, target_server) =
        serve_once(http_response(&format!("X-Echo: {SECRET}\r\n"), &body));
    let (base, server) = serve_once(
        format!("HTTP/1.1 302 Found\r\nLocation: {target}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes(),
    );
    let executed = prepare_secret_execution(&base)
        .execute(
            &HttpEngine::new().unwrap(),
            ExecutionOptions::default(),
            None,
            std::future::pending::<()>(),
            |_| {},
        )
        .await
        .unwrap();
    let head = server.join().unwrap();
    target_server.join().unwrap();
    assert!(contains(&head, &format!("/{SECRET}")));
    assert!(contains(&head, &format!("Bearer {SECRET}")));
    let response = executed.response;
    assert_eq!(response.url, format!("{base}/{{{{token}}}}"));
    assert_eq!(response.initial_url, format!("{base}/{{{{token}}}}"));
    assert!(!response.url_changed);
    assert_eq!(response.body, b"[REDACTED]\xff-tail");
    let echo = response
        .headers
        .iter()
        .find(|header| header.name == "x-echo")
        .unwrap();
    assert_eq!(echo.value, "[REDACTED]");
    assert_eq!(executed.body_sha256, None);
}

#[tokio::test]
async fn plain_response_is_returned_unchanged() {
    let (base, server) = serve_once(http_response("", SECRET.as_bytes()));
    let environments = environments();
    let executed = prepare_request(
        &get(&format!("{base}/plain")),
        &local(&environments),
        &RecordingProvider::default(),
    )
    .unwrap()
    .into_http()
    .unwrap()
    .execute(
        &HttpEngine::new().unwrap(),
        ExecutionOptions::default(),
        None,
        std::future::pending::<()>(),
        |_| {},
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(executed.response.url, format!("{base}/plain"));
    assert_eq!(executed.response.body, SECRET.as_bytes());
}

#[tokio::test]
async fn only_secret_bearing_large_responses_bypass_the_spool_cache() {
    let body = vec![b'x'; probe_http::MAX_IN_MEMORY_RESPONSE_BYTES + 1024];
    let engine = HttpEngine::new().unwrap();

    let plain_cache = temporary_path("plain-cache");
    let (base, server) = serve_once(http_response("", &body));
    let plain = prepare_request(
        &get(&format!("{base}/")),
        &RequestResolution::default(),
        &NoSecrets,
    )
    .unwrap()
    .into_http()
    .unwrap()
    .execute(
        &engine,
        ExecutionOptions {
            response_cache: Some(ResponseCache::new(plain_cache.clone(), 64 * 1024 * 1024)),
            ..ExecutionOptions::default()
        },
        None,
        std::future::pending::<()>(),
        |_| {},
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert!(plain.response.body_file.is_some());
    drop(plain);
    let _ = std::fs::remove_dir_all(&plain_cache);

    let secret_cache = temporary_path("secret-cache");
    let (base, server) = serve_once(http_response("", &body));
    let secret = prepare_secret_execution(&base)
        .execute(
            &engine,
            ExecutionOptions {
                response_cache: Some(ResponseCache::new(secret_cache.clone(), 64 * 1024 * 1024)),
                ..ExecutionOptions::default()
            },
            None,
            std::future::pending::<()>(),
            |_| {},
        )
        .await
        .unwrap();
    server.join().unwrap();
    assert!(!secret.response.body_complete);
    assert!(secret.response.body_file.is_none());
    assert!(!secret_cache.exists());
}

#[tokio::test]
async fn file_output_keeps_original_bytes_and_returns_their_digest() {
    let (base, server) = serve_once(http_response("", SECRET.as_bytes()));
    let output = temporary_path("output");
    let executed = prepare_secret_execution(&base)
        .execute(
            &HttpEngine::new().unwrap(),
            ExecutionOptions::default(),
            Some(&output),
            std::future::pending::<()>(),
            |_| {},
        )
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), SECRET.as_bytes());
    assert_eq!(
        executed.body_sha256,
        Some(Sha256::digest(SECRET.as_bytes()).into())
    );
    assert!(!contains(&executed.response.body, SECRET));
    std::fs::remove_file(output).unwrap();
}

#[tokio::test]
async fn secret_bearing_failures_withhold_diagnostics_but_keep_their_kind() {
    let engine = HttpEngine::new().unwrap();
    let withheld = SECRET_DIAGNOSTIC_WITHHELD.to_owned();

    let cancelled = prepare_secret_execution("http://127.0.0.1:1")
        .execute(
            &engine,
            ExecutionOptions::default(),
            None,
            std::future::ready(()),
            |_| {},
        )
        .await
        .unwrap_err();
    assert_eq!(cancelled, HttpError::Cancelled);

    let transport = prepare_secret_execution("http://127.0.0.1:1")
        .execute(
            &engine,
            ExecutionOptions::default(),
            None,
            std::future::pending::<()>(),
            |_| {},
        )
        .await
        .unwrap_err();
    assert_eq!(transport, HttpError::Transport(withheld.clone()));

    let environments = environments();
    let overrides = [("token".to_owned(), "has space\n".to_owned())];
    let configuration = prepare_request(
        &Request {
            headers: vec![Header {
                name: "X-{{token}}".to_owned(),
                value: String::new(),
                disabled: false,
            }],
            ..get("http://127.0.0.1:1/")
        },
        &RequestResolution {
            overrides: &overrides,
            ..local(&environments)
        },
        &NoSecrets,
    )
    .unwrap()
    .into_http()
    .unwrap()
    .execute(
        &engine,
        ExecutionOptions::default(),
        None,
        std::future::pending::<()>(),
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(configuration, HttpError::InvalidRequest(withheld.clone()));
    assert!(configuration.is_configuration());

    let (base, server) = serve_once(http_response("", b"body"));
    let output = temporary_path("missing-directory").join("response.bin");
    let output_failure = prepare_secret_execution(&base)
        .execute(
            &engine,
            ExecutionOptions::default(),
            Some(&output),
            std::future::pending::<()>(),
            |_| {},
        )
        .await
        .unwrap_err();
    server.join().unwrap();
    assert_eq!(
        output_failure,
        HttpError::ResponseOutput {
            path: output,
            message: withheld,
        }
    );
}

#[tokio::test]
async fn failures_without_secrets_keep_their_diagnostics() {
    let error = prepare_request(
        &get("http://127.0.0.1:1/"),
        &RequestResolution::default(),
        &NoSecrets,
    )
    .unwrap()
    .into_http()
    .unwrap()
    .execute(
        &HttpEngine::new().unwrap(),
        ExecutionOptions::default(),
        None,
        std::future::pending::<()>(),
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, HttpError::Transport(message) if message != SECRET_DIAGNOSTIC_WITHHELD)
    );
}

#[test]
fn curl_export_resolves_plain_variables_but_keeps_secret_references() {
    let mut environments = environments();
    environments[0]
        .variables
        .push(plain("host", "https://example.com"));
    let mut request = secret_request("{{host}}");
    request.url = Some("{{host}}/current-draft".into());
    let command = probe_application::copy_as_curl(
        &request,
        &local(&environments),
        &HttpEngine::new().unwrap(),
        &ExecutionOptions::default(),
    )
    .unwrap();
    assert!(command.contains("https://example.com/current-draft"));
    assert!(command.contains("authorization: {{authorization}}"));
    assert!(!command.contains(SECRET));
    assert!(!command.contains("{{host}}"));
}

#[test]
fn curl_export_reports_resolution_errors() {
    let engine = HttpEngine::new().unwrap();
    let request = get("https://example.com");
    let bad_environment = RequestResolution {
        environment: Some("missing"),
        ..RequestResolution::default()
    };
    assert!(
        probe_application::copy_as_curl(
            &request,
            &bad_environment,
            &engine,
            &ExecutionOptions::default()
        )
        .is_err()
    );
}

#[test]
fn curl_export_keeps_secret_placeholders_in_url_parameters_and_all_bodies() {
    use probe_core::{
        Body, FormField, MultipartPart, MultipartPartKind, MultipartValue, QueryParameter, RawBody,
        RawBodyKind, RequestBody, RequestKind,
    };
    let mut environments = environments();
    environments[0]
        .variables
        .extend([secret("secret"), secret("SecretHost")]);
    let mut request =
        get("https://{{SecretHost}}/{{secret}}/:id?original={{secret}}&encoded=%7B%7Bsecret%7D%7D");
    request.method = Some("POST".into());
    request.path_parameters = vec![QueryParameter {
        name: "id".into(),
        value: "{{secret}}".into(),
        disabled: false,
    }];
    request.query_parameters = vec![QueryParameter {
        name: "{{secret}}".into(),
        value: "prefix {{secret}}+tail".into(),
        disabled: false,
    }];
    let engine = HttpEngine::new().unwrap();
    let resolution = RequestResolution {
        overrides: &[
            ("secret".into(), SECRET.into()),
            ("SecretHost".into(), "secret-host-value".into()),
        ],
        ..local(&environments)
    };
    for body in [
        Body::Raw(RawBody {
            kind: RawBodyKind::Json,
            data: r#"{"secret":"{{secret}}"}"#.into(),
        }),
        Body::FormUrlEncoded(vec![FormField {
            name: "{{secret}}".into(),
            value: "a {{secret}}+b".into(),
            disabled: false,
        }]),
        Body::Multipart(vec![MultipartPart {
            name: "field".into(),
            kind: MultipartPartKind::Text,
            value: MultipartValue::Single("{{secret}}".into()),
            content_type: None,
            disabled: false,
        }]),
    ] {
        let expected_body = match &body {
            Body::Raw(_) => r#"{"secret":"{{secret}}"}"#,
            Body::FormUrlEncoded(_) => "{{secret}}=a+{{secret}}%2Bb",
            Body::Multipart(_) => "{{secret}}",
            Body::File(_) => unreachable!(),
        };
        request.kind = RequestKind::Http {
            body: Some(RequestBody::Single(body.clone())),
        };
        let command = probe_application::copy_as_curl(
            &request,
            &resolution,
            &engine,
            &ExecutionOptions::default(),
        )
        .unwrap();
        assert!(command.contains("https://{{SecretHost}}/{{secret}}/{{secret}}?original={{secret}}&encoded=%7B%7Bsecret%7D%7D&{{secret}}=prefix+{{secret}}%2Btail"), "{command}");
        // This body must remain a template even when invocation overrides contain real secrets.
        assert!(command.contains(expected_body), "{command}");
        assert!(!command.contains(SECRET));
        assert!(!command.contains("secret-host-value"));
        assert!(!command.contains("%7B%7BSecretHost%7D%7D"));
        assert!(!command.contains("%7B%7Bsecret%7D%7D%2B"));
    }
}

#[test]
fn graphql_curl_export_preserves_selected_operation_resolution_and_http_semantics() {
    use probe_core::{
        Authentication, AuthenticationKind, AuthenticationValue, GraphqlBody, GraphqlBodyVariant,
        GraphqlOperation, RequestKind,
    };

    let loaded = probe_opencollection::load_workspace_from_str(include_str!(
        "../../../tests/fixtures/opencollection/graphql-http.yml"
    ))
    .unwrap();
    let key = loaded.requests()[0].key();
    let mut request = loaded.workspace().request(key).unwrap().clone();
    let mut operation = request.selected_graphql().unwrap().unwrap().clone();
    operation
        .variables
        .as_mut()
        .unwrap()
        .insert("token".into(), "{{token}}".into());
    operation
        .extensions
        .as_mut()
        .unwrap()
        .insert("credential".into(), "{{authorization}}".into());
    request.kind = RequestKind::Graphql {
        body: Some(GraphqlBody::Variants(vec![
            GraphqlBodyVariant {
                title: "Inactive".into(),
                selected: false,
                body: GraphqlOperation {
                    query: Some("INACTIVE_QUERY".into()),
                    ..GraphqlOperation::default()
                },
            },
            GraphqlBodyVariant {
                title: "Viewer".into(),
                selected: true,
                body: operation,
            },
        ])),
    };
    request.headers.push(Header {
        name: "X-Login".into(),
        value: "{{login}}".into(),
        disabled: false,
    });
    request.authentication = Some(Authentication {
        kind: AuthenticationKind::Bearer,
        properties: [(
            "token".into(),
            AuthenticationValue::String("{{token}}".into()),
        )]
        .into(),
    });
    let mut environments = environments();
    environments[0].variables.extend([
        plain("serverUrl", "https://example.com"),
        plain("login", "octocat"),
    ]);
    let resolution = RequestResolution {
        strict_variables: true,
        ..local(&environments)
    };
    let engine = HttpEngine::new().unwrap();
    let options = ExecutionOptions::default();
    for method in ["POST", "GET"] {
        request.method = Some(method.into());
        let command =
            probe_application::copy_as_curl(&request, &resolution, &engine, &options).unwrap();
        assert!(command.contains(&format!("--request '{method}'")));
        assert!(command.contains("https://example.com/graphql"));
        assert!(command.contains("--header 'x-login: octocat'"));
        assert!(command.contains("--header 'authorization: Bearer {{token}}'"));
        assert!(!command.contains("INACTIVE_QUERY"));
        assert!(!command.contains("{{login}}"));
        assert!(!command.contains(SECRET));
        if method == "POST" {
            assert!(command.contains("--header 'content-type: application/json'"));
            let payload = command
                .split_once("--data-raw '")
                .unwrap()
                .1
                .strip_suffix("'")
                .unwrap();
            let payload: serde_json::Value = serde_json::from_str(payload).unwrap();
            assert_eq!(
                payload,
                serde_json::json!({
                    "query": "query Viewer($login: String!) { viewer(login: $login) { login } }",
                    "variables": {"login": "octocat", "token": "{{token}}"},
                    "operationName": "Viewer",
                    "extensions": {"credential": "{{authorization}}", "trace": {"enabled": true}},
                })
            );
        } else {
            assert!(!command.contains("--data-raw"));
            assert!(command.contains("query=query+Viewer%28%24login%3A+String%21%29+%7B+viewer%28login%3A+%24login%29+%7B+login+%7D+%7D"));
            assert!(command.contains("variables=%7B"));
            assert!(command.contains("%22login%22%3A%22octocat%22"));
            assert!(command.contains("%22token%22%3A%22{{token}}%22"));
            assert!(command.contains("operationName=Viewer"));
            assert!(command.contains("extensions=%7B"));
            assert!(command.contains("%22credential%22%3A%22{{authorization}}%22"));
            assert!(command.contains("%22trace%22%3A%7B%22enabled%22%3Atrue%7D"));
        }
    }
    let RequestKind::Graphql {
        body: Some(GraphqlBody::Variants(variants)),
    } = &mut request.kind
    else {
        unreachable!()
    };
    variants[1].selected = false;
    assert!(
        probe_application::copy_as_curl(&request, &RequestResolution::default(), &engine, &options)
            .is_err()
    );
    let RequestKind::Graphql {
        body: Some(GraphqlBody::Variants(variants)),
    } = &mut request.kind
    else {
        unreachable!()
    };
    for variant in variants {
        variant.selected = true;
    }
    assert!(
        probe_application::copy_as_curl(&request, &RequestResolution::default(), &engine, &options)
            .is_err()
    );
}
