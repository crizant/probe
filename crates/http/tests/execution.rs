use std::{collections::BTreeMap, io, time::Duration};

use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Body, FileReference, FormField,
    Header, MultipartPart, MultipartPartKind, MultipartValue, QueryParameter, RawBody, RawBodyKind,
    RequestBody,
};
use probe_http::{ExecutionOptions, HttpEngine};
use tokio::net::TcpListener;

#[path = "execution/support.rs"]
mod support;

use support::{http_body, read_request, request, serve_once, temporary_path, write_response};
#[tokio::test]
async fn ignores_enabled_parameters_without_names() {
    let (base_url, captured) = serve_once("200 OK", &[], b"ok").await.unwrap();
    let mut request = request("POST", format!("{base_url}/unnamed"));
    request.headers = vec![
        Header {
            name: String::new(),
            value: "ignored".to_owned(),
            disabled: false,
        },
        Header {
            name: "   ".to_owned(),
            value: "ignored".to_owned(),
            disabled: false,
        },
        Header {
            name: "X-Valid".to_owned(),
            value: "sent".to_owned(),
            disabled: false,
        },
    ];
    request.query_parameters = vec![
        QueryParameter {
            name: String::new(),
            value: "ignored".to_owned(),
            disabled: false,
        },
        QueryParameter {
            name: "   ".to_owned(),
            value: "ignored".to_owned(),
            disabled: false,
        },
        QueryParameter {
            name: "limit".to_owned(),
            value: "10".to_owned(),
            disabled: false,
        },
        QueryParameter {
            name: "flag".to_owned(),
            value: String::new(),
            disabled: false,
        },
    ];
    request.kind = http_body(RequestBody::Single(Body::FormUrlEncoded(vec![
        FormField {
            name: String::new(),
            value: "ignored".to_owned(),
            disabled: false,
        },
        FormField {
            name: "name".to_owned(),
            value: "Probe".to_owned(),
            disabled: false,
        },
    ])));

    HttpEngine::new()
        .unwrap()
        .execute(&request.into_http().unwrap(), &ExecutionOptions::default())
        .await
        .unwrap();
    let captured = captured.await.unwrap().unwrap();

    assert_eq!(
        captured.request_line,
        "POST /unnamed?limit=10&flag= HTTP/1.1"
    );
    assert_eq!(captured.header("x-valid"), Some("sent"));
    assert!(
        captured
            .headers
            .iter()
            .all(|(name, _)| !name.trim().is_empty())
    );
    assert_eq!(captured.body, b"name=Probe");
}

#[tokio::test]
async fn ignores_multipart_parts_without_names() {
    let (base_url, captured) = serve_once("200 OK", &[], b"ok").await.unwrap();
    let mut request = request("POST", format!("{base_url}/upload"));
    request.kind = http_body(RequestBody::Single(Body::Multipart(vec![
        MultipartPart {
            name: String::new(),
            kind: MultipartPartKind::Text,
            value: MultipartValue::Single("ignored".to_owned()),
            content_type: None,
            disabled: false,
        },
        MultipartPart {
            name: "caption".to_owned(),
            kind: MultipartPartKind::Text,
            value: MultipartValue::Single("hello".to_owned()),
            content_type: None,
            disabled: false,
        },
    ])));

    HttpEngine::new()
        .unwrap()
        .execute(&request.into_http().unwrap(), &ExecutionOptions::default())
        .await
        .unwrap();
    let captured = captured.await.unwrap().unwrap();
    let body = String::from_utf8_lossy(&captured.body);
    assert!(body.contains("name=\"caption\""));
    assert!(body.contains("hello"));
    assert!(!body.contains("name=\"\""));
    assert!(!body.contains("ignored"));
}

#[tokio::test]
async fn executes_json_with_headers_query_bearer_auth_and_response_metadata() {
    let (base_url, captured) = serve_once(
        "201 Created",
        &[("X-Zeta", "last"), ("X-Alpha", "first")],
        b"created",
    )
    .await
    .unwrap();
    let mut request = request("POST", format!("{base_url}/users/:userId?existing=yes"));
    request.headers = vec![
        Header {
            name: "X-Probe".to_owned(),
            value: "enabled".to_owned(),
            disabled: false,
        },
        Header {
            name: "X-Skipped".to_owned(),
            value: "disabled".to_owned(),
            disabled: true,
        },
    ];
    request.query_parameters = vec![QueryParameter {
        name: "search".to_owned(),
        value: "hello world".to_owned(),
        disabled: false,
    }];
    request.path_parameters = vec![QueryParameter {
        name: "userId".to_owned(),
        value: "probe/user".to_owned(),
        disabled: false,
    }];
    request.kind = http_body(RequestBody::Single(Body::Raw(RawBody {
        kind: RawBodyKind::Json,
        data: "{\"name\":\"Milo\"}".to_owned(),
    })));
    request.authentication = Some(Authentication {
        kind: AuthenticationKind::Bearer,
        properties: BTreeMap::from([(
            "token".to_owned(),
            AuthenticationValue::String("token-value".to_owned()),
        )]),
    });

    let response = HttpEngine::new()
        .unwrap()
        .execute(&request.into_http().unwrap(), &ExecutionOptions::default())
        .await
        .unwrap();
    let captured = captured.await.unwrap().unwrap();

    assert_eq!(
        captured.request_line,
        "POST /users/probe%2Fuser?existing=yes&search=hello+world HTTP/1.1"
    );
    assert_eq!(captured.header("x-probe"), Some("enabled"));
    assert_eq!(captured.header("x-skipped"), None);
    assert_eq!(captured.header("authorization"), Some("Bearer token-value"));
    assert_eq!(captured.header("content-type"), Some("application/json"));
    assert_eq!(captured.body, br#"{"name":"Milo"}"#);
    assert_eq!(response.status, 201);
    assert_eq!(response.reason, "Created");
    assert_eq!(response.body, b"created");
    assert_eq!(response.size, 7);
    assert_eq!(response.headers[0].name, "connection");
    assert!(response.duration > Duration::ZERO);
}

#[tokio::test]
async fn strips_json_body_comments_before_sending() {
    let (base_url, captured) = serve_once("200 OK", &[], b"ok").await.unwrap();
    let mut request = request("POST", format!("{base_url}/comments"));
    request.kind = http_body(RequestBody::Single(Body::Raw(RawBody {
        kind: RawBodyKind::Json,
        data: r#"{
  // editor-only line comment
  "url": "https://example.com/a//b",
  "literal": "/* not a comment */",
  /* editor-only block
     comment */
  "name": "Café"
}"#
        .to_owned(),
    })));

    HttpEngine::new()
        .unwrap()
        .execute(&request.into_http().unwrap(), &ExecutionOptions::default())
        .await
        .unwrap();
    let captured = captured.await.unwrap().unwrap();
    let body = String::from_utf8(captured.body).unwrap();

    assert!(!body.contains("editor-only line comment"));
    assert!(!body.contains("editor-only block"));
    assert!(body.contains(r#""url": "https://example.com/a//b""#));
    assert!(body.contains(r#""literal": "/* not a comment */""#));
    assert!(body.contains(r#""name": "Café""#));
}

#[tokio::test]
async fn supports_all_phase_five_methods() {
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
        let (base_url, captured) = serve_once("204 No Content", &[], b"").await.unwrap();
        let response = HttpEngine::new()
            .unwrap()
            .execute(
                &request(method, format!("{base_url}/method"))
                    .into_http()
                    .unwrap(),
                &ExecutionOptions::default(),
            )
            .await
            .unwrap();
        assert_eq!(response.status, 204);
        assert_eq!(
            captured.await.unwrap().unwrap().request_line,
            format!("{method} /method HTTP/1.1")
        );
    }
}

#[tokio::test]
async fn sends_urlencoded_forms_and_basic_auth() {
    let (base_url, captured) = serve_once("200 OK", &[], b"ok").await.unwrap();
    let mut request = request("POST", format!("{base_url}/form"));
    request.kind = http_body(RequestBody::Single(Body::FormUrlEncoded(vec![
        FormField {
            name: "name".to_owned(),
            value: "Probe Client".to_owned(),
            disabled: false,
        },
        FormField {
            name: "skip".to_owned(),
            value: "true".to_owned(),
            disabled: true,
        },
    ])));
    request.authentication = Some(Authentication {
        kind: AuthenticationKind::Basic,
        properties: BTreeMap::from([
            (
                "username".to_owned(),
                AuthenticationValue::String("demo".to_owned()),
            ),
            (
                "password".to_owned(),
                AuthenticationValue::String("secret".to_owned()),
            ),
        ]),
    });

    HttpEngine::new()
        .unwrap()
        .execute(&request.into_http().unwrap(), &ExecutionOptions::default())
        .await
        .unwrap();
    let captured = captured.await.unwrap().unwrap();
    assert_eq!(
        captured.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(
        captured.header("authorization"),
        Some("Basic ZGVtbzpzZWNyZXQ=")
    );
    assert_eq!(captured.body, b"name=Probe+Client");
}

#[tokio::test]
async fn sends_text_body_with_default_content_type() {
    let (base_url, captured) = serve_once("200 OK", &[], b"ok").await.unwrap();
    let mut request = request("PATCH", format!("{base_url}/text"));
    request.kind = http_body(RequestBody::Single(Body::Raw(RawBody {
        kind: RawBodyKind::Text,
        data: "plain text // keep this".to_owned(),
    })));

    HttpEngine::new()
        .unwrap()
        .execute(&request.into_http().unwrap(), &ExecutionOptions::default())
        .await
        .unwrap();
    let captured = captured.await.unwrap().unwrap();
    assert_eq!(
        captured.header("content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(captured.body, b"plain text // keep this");
}

#[tokio::test]
async fn sends_multipart_text_and_file_parts_from_the_execution_base_directory() {
    let path = temporary_path("multipart.txt");
    std::fs::write(&path, b"file contents").unwrap();
    let directory = path.parent().unwrap().to_owned();
    let filename = path.file_name().unwrap().to_string_lossy().into_owned();
    let (base_url, captured) = serve_once("200 OK", &[], b"uploaded").await.unwrap();
    let mut request = request("POST", format!("{base_url}/upload"));
    request.kind = http_body(RequestBody::Single(Body::Multipart(vec![
        MultipartPart {
            name: "caption".to_owned(),
            kind: MultipartPartKind::Text,
            value: MultipartValue::Single("hello".to_owned()),
            content_type: None,
            disabled: false,
        },
        MultipartPart {
            name: "asset".to_owned(),
            kind: MultipartPartKind::File,
            value: MultipartValue::Single(filename),
            content_type: Some("text/plain".to_owned()),
            disabled: false,
        },
    ])));

    HttpEngine::new()
        .unwrap()
        .execute(
            &request.into_http().unwrap(),
            &ExecutionOptions {
                base_directory: Some(directory),
                ..ExecutionOptions::default()
            },
        )
        .await
        .unwrap();
    let captured = captured.await.unwrap().unwrap();
    let body = String::from_utf8_lossy(&captured.body);
    assert!(
        captured
            .header("content-type")
            .unwrap()
            .starts_with("multipart/form-data; boundary=")
    );
    assert!(body.contains("name=\"caption\""));
    assert!(body.contains("hello"));
    assert!(body.contains("name=\"asset\""));
    assert!(body.contains("file contents"));
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn streams_selected_file_body() {
    let path = temporary_path("body.txt");
    std::fs::write(&path, b"streamed body").unwrap();
    let directory = path.parent().unwrap().to_owned();
    let filename = path.file_name().unwrap().to_string_lossy().into_owned();
    let (base_url, captured) = serve_once("200 OK", &[], b"ok").await.unwrap();
    let mut request = request("PUT", format!("{base_url}/file"));
    request.kind = http_body(RequestBody::Single(Body::File(vec![FileReference {
        file_path: filename,
        content_type: "text/plain".to_owned(),
        selected: true,
    }])));

    HttpEngine::new()
        .unwrap()
        .execute(
            &request.into_http().unwrap(),
            &ExecutionOptions {
                base_directory: Some(directory),
                ..ExecutionOptions::default()
            },
        )
        .await
        .unwrap();
    let captured = captured.await.unwrap().unwrap();
    assert_eq!(captured.header("content-type"), Some("text/plain"));
    assert_eq!(captured.body, b"streamed body");
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn follows_or_returns_redirects_according_to_request_settings() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let redirect_server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await?;
        read_request(&mut first).await?;
        write_response(
            &mut first,
            "302 Found",
            &[("Location".to_owned(), format!("http://{address}/final"))],
            b"",
        )
        .await?;
        let (mut second, _) = listener.accept().await?;
        let captured = read_request(&mut second).await?;
        write_response(&mut second, "200 OK", &[], b"final").await?;
        Ok::<_, io::Error>(captured)
    });
    let mut following = request("GET", format!("http://{address}/redirect"));
    following.settings.max_redirects = Some(3);
    let response = HttpEngine::new()
        .unwrap()
        .execute(
            &following.into_http().unwrap(),
            &ExecutionOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"final");
    assert_eq!(
        redirect_server.await.unwrap().unwrap().request_line,
        "GET /final HTTP/1.1"
    );

    let (base_url, captured) = serve_once(
        "302 Found",
        &[("Location", "http://127.0.0.1:1/unreachable")],
        b"redirect",
    )
    .await
    .unwrap();
    let mut not_following = request("GET", format!("{base_url}/redirect"));
    not_following.settings.follow_redirects = Some(false);
    let response = HttpEngine::new()
        .unwrap()
        .execute(
            &not_following.into_http().unwrap(),
            &ExecutionOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(response.status, 302);
    captured.await.unwrap().unwrap();
}

#[path = "execution/response.rs"]
mod response;

#[tokio::test]
async fn curl_export_preserves_enabled_fields_auth_body_and_shell_literals() {
    let engine = HttpEngine::new().unwrap();
    let mut request = request("POST", "https://example.com/:id?existing=yes".into());
    request.path_parameters = vec![QueryParameter {
        name: "id".into(),
        value: "42".into(),
        disabled: false,
    }];
    request.query_parameters = vec![
        QueryParameter {
            name: "q".into(),
            value: "a & b".into(),
            disabled: false,
        },
        QueryParameter {
            name: "ignored".into(),
            value: "ignored".into(),
            disabled: true,
        },
    ];
    request.headers = vec![
        Header {
            name: "X-Test".into(),
            value: "it's $(literal)".into(),
            disabled: false,
        },
        Header {
            name: "X-Ignored".into(),
            value: "ignored".into(),
            disabled: true,
        },
    ];
    request.authentication = Some(Authentication {
        kind: AuthenticationKind::Bearer,
        properties: BTreeMap::from([(
            "token".into(),
            AuthenticationValue::String("token-value".into()),
        )]),
    });
    request.settings.timeout = Some(Duration::from_millis(1500));
    request.settings.max_redirects = Some(3);
    request.kind = http_body(RequestBody::Single(Body::Raw(RawBody {
        kind: RawBodyKind::Text,
        data: "@it's\n$(literal)".into(),
    })));
    let command = engine
        .curl_command(&request.into_http().unwrap(), &ExecutionOptions::default())
        .await
        .unwrap();
    assert!(command.contains("--request 'POST'"));
    assert!(command.contains("--url 'https://example.com/42?existing=yes&q=a+%26+b'"));
    assert!(command.contains("--max-time '1.5'"));
    assert!(command.contains("--max-redirs '3'"));
    assert!(command.contains("--location"));
    assert!(command.contains("authorization: Bearer token-value"));
    assert!(command.contains("content-type: text/plain; charset=utf-8"));
    assert!(!command.contains("ignored"));
    #[cfg(unix)]
    {
        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("set -- {command}; printf '%s\\0' \"$@\""))
            .output()
            .unwrap();
        assert!(output.status.success());
        let args: Vec<_> = output
            .stdout
            .split(|byte| *byte == 0)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .collect();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--data-raw", "@it's\n$(literal)"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--header", "x-test: it's $(literal)"])
        );
    }
}

#[tokio::test]
async fn curl_export_encodes_forms_and_references_files_without_reading_them() {
    let engine = HttpEngine::new().unwrap();
    let mut request = request("PUT", "https://example.com".into());
    request.settings.follow_redirects = Some(false);
    request.settings.timeout = Some(Duration::ZERO);
    request.kind = http_body(RequestBody::Single(Body::FormUrlEncoded(vec![FormField {
        name: "value".into(),
        value: "a & b".into(),
        disabled: false,
    }])));
    let command = engine
        .curl_command(
            &request.clone().into_http().unwrap(),
            &ExecutionOptions::default(),
        )
        .await
        .unwrap();
    assert!(command.contains("--data-raw 'value=a+%26+b'"));
    assert!(!command.contains("--location"));
    assert!(!command.contains("--max-time"));
    let options = ExecutionOptions {
        base_directory: Some(std::env::temp_dir()),
        ..ExecutionOptions::default()
    };
    request.kind = http_body(RequestBody::Single(Body::File(vec![FileReference {
        file_path: "nonexistent-export-body.bin".into(),
        content_type: "application/octet-stream".into(),
        selected: true,
    }])));
    let command = engine
        .curl_command(&request.clone().into_http().unwrap(), &options)
        .await
        .unwrap();
    assert!(command.contains(&format!(
            "--data-binary '@{}'",
            std::env::temp_dir()
                .join("nonexistent-export-body.bin")
                .display()
        )));
    assert!(command.contains("content-type: application/octet-stream"));
    request.kind = http_body(RequestBody::Single(Body::Multipart(vec![
        MultipartPart {
            name: "text".into(),
            kind: MultipartPartKind::Text,
            value: MultipartValue::Single("@literal;value".into()),
            content_type: None,
            disabled: false,
        },
        MultipartPart {
            name: "upload".into(),
            kind: MultipartPartKind::File,
            value: MultipartValue::Multiple(vec![
                "missing-one.txt".into(),
                "missing-two.txt".into(),
            ]),
            content_type: Some("text/plain".into()),
            disabled: false,
        },
        MultipartPart {
            name: "ignored".into(),
            kind: MultipartPartKind::Text,
            value: MultipartValue::Single("ignored".into()),
            content_type: None,
            disabled: true,
        },
    ])));
    let command = engine
        .curl_command(&request.into_http().unwrap(), &options)
        .await
        .unwrap();
    assert!(command.contains("text=\"@literal;value\""));
    assert!(command.contains("missing-one.txt"));
    assert!(command.contains("missing-two.txt"));
    assert!(command.contains(";type=text/plain"));
    assert!(!command.contains("ignored"));
}

#[tokio::test]
async fn curl_export_basic_auth_custom_content_type_and_selection_failures() {
    let engine = HttpEngine::new().unwrap();
    let options = ExecutionOptions::default();
    let mut request = request("PATCH", "https://example.com".into());
    request.authentication = Some(Authentication {
        kind: AuthenticationKind::Basic,
        properties: BTreeMap::from([
            (
                "username".into(),
                AuthenticationValue::String("{{username}}".into()),
            ),
            (
                "password".into(),
                AuthenticationValue::String("{{password}}".into()),
            ),
        ]),
    });
    request.headers.push(Header {
        name: "Content-Type".into(),
        value: "application/custom".into(),
        disabled: false,
    });
    request.kind = http_body(RequestBody::Single(Body::Raw(RawBody {
        kind: RawBodyKind::Json,
        data: "{\"name\":\"value\" /*comment*/}".into(),
    })));
    let command = engine
        .curl_command(&request.clone().into_http().unwrap(), &options)
        .await
        .unwrap();
    assert!(command.contains("--user '{{username}}:{{password}}'"));
    assert!(!command.contains("authorization:"));
    assert!(command.contains("content-type: application/custom"));
    assert!(!command.contains("application/json"));
    assert!(!command.contains("/*comment*/"));
    let file = FileReference {
        file_path: "missing".into(),
        content_type: "application/octet-stream".into(),
        selected: true,
    };
    request.kind = http_body(RequestBody::Single(Body::File(vec![file.clone()])));
    let command = engine
        .curl_command(&request.clone().into_http().unwrap(), &options)
        .await
        .unwrap();
    assert!(command.contains("content-type: application/custom"));
    assert!(!command.contains("application/octet-stream"));
    request.kind = http_body(RequestBody::Single(Body::File(vec![])));
    assert!(matches!(
        engine
            .curl_command(&request.clone().into_http().unwrap(), &options)
            .await,
        Err(probe_http::HttpError::InvalidBodySelection(_))
    ));
    request.kind = http_body(RequestBody::Single(Body::File(vec![file.clone(), file])));
    assert!(matches!(
        engine
            .curl_command(&request.clone().into_http().unwrap(), &options)
            .await,
        Err(probe_http::HttpError::InvalidBodySelection(_))
    ));
    request.kind = http_body(RequestBody::Single(Body::Multipart(vec![MultipartPart {
        name: "text".into(),
        kind: MultipartPartKind::Text,
        value: MultipartValue::Multiple(vec!["one".into(), "two".into()]),
        content_type: None,
        disabled: false,
    }])));
    assert!(matches!(
        engine
            .curl_command(&request.into_http().unwrap(), &options)
            .await,
        Err(probe_http::HttpError::InvalidBody(_))
    ));
}
