//! End-to-end trust injection uses Linux's existing SSL_CERT_FILE support only.
#![cfg(target_os = "linux")]

#[allow(dead_code, unused_imports)]
mod common;

use common::*;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn tls_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/mtls")
        .join(name)
}

fn certs(name: &str) -> Vec<CertificateDer<'static>> {
    CertificateDer::pem_slice_iter(&fs::read(tls_fixture(name)).unwrap())
        .collect::<Result<_, _>>()
        .unwrap()
}

fn mtls_server(body: &'static str) -> (u16, JoinHandle<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let worker = thread::spawn(move || {
        let mut roots = RootCertStore::empty();
        roots.add(certs("ca.pem").remove(0)).unwrap();
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            provider.clone(),
        )
        .build()
        .unwrap();
        let key = PrivateKeyDer::from_pem_slice(&fs::read(tls_fixture("server-key.pem")).unwrap())
            .unwrap();
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs("server.pem"), key)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "CLI did not connect to test server"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => panic!("test listener failed"),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut stream = StreamOwned::new(ServerConnection::new(Arc::new(config)).unwrap(), socket);
        let mut request = [0u8; 4096];
        if stream.read(&mut request).is_err() {
            return false;
        }
        let authenticated = stream
            .conn
            .peer_certificates()
            .is_some_and(|certificates| !certificates.is_empty());
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        authenticated
    });
    (port, worker)
}

#[test]
fn cli_http_and_graphql_use_collection_pem_identity_and_dry_run_does_not_load_files() {
    let directory = temporary_path("mtls");
    fs::create_dir_all(&directory).unwrap();
    let workspace = directory.join("collection.yml");
    fs::copy(tls_fixture("client.pem"), directory.join("client.pem")).unwrap();
    fs::copy(
        tls_fixture("client-encrypted-key.pem"),
        directory.join("client-key.pem"),
    )
    .unwrap();
    for protocol in ["http", "graphql"] {
        let (port, worker) = mtls_server(r#"{"mtls":true}"#);
        let body = if protocol == "graphql" {
            "      body:\n        query: 'query { viewer { id } }'\n"
        } else {
            ""
        };
        let source = format!(
            "opencollection: 1.0.0\ninfo:\n  name: CLI mTLS test\nbundled: true\nconfig:\n  clientCertificates:\n    - domain: localhost:{port}\n      type: pem\n      certificateFilePath: ./client.pem\n      privateKeyFilePath: ./client-key.pem\n      passphrase: probe-test-only-password\nitems:\n  - info:\n      name: Request\n      type: {protocol}\n    {protocol}:\n      method: POST\n      url: https://localhost:{port}/private\n{body}"
        );
        fs::write(
            &workspace,
            source.replace("./client.pem", "./absent-cert.pem"),
        )
        .unwrap();
        let dry = probe()
            .args(["request", "run"])
            .arg(&workspace)
            .args(["items/0", "--dry-run", "--json"])
            .output()
            .unwrap();
        assert!(dry.status.success());
        fs::write(&workspace, source).unwrap();
        let output = probe()
            .args(["request", "run"])
            .arg(&workspace)
            .args(["items/0", "--json"])
            .env("SSL_CERT_FILE", tls_fixture("ca.pem"))
            .env("SSL_CERT_DIR", &directory)
            .env("NO_PROXY", "localhost,127.0.0.1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "CLI mTLS request failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["response"]["status"], 200);
        assert_eq!(response["response"]["body"]["content"], r#"{"mtls":true}"#);
        assert!(worker.join().unwrap());
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cli_variables_and_secret_provider_resolve_certificate_configuration() {
    let directory = temporary_path("mtls-variables");
    fs::create_dir_all(&directory).unwrap();
    let workspace = directory.join("collection.yml");
    fs::copy(tls_fixture("client.pem"), directory.join("client.pem")).unwrap();
    fs::copy(
        tls_fixture("client-encrypted-key.pem"),
        directory.join("client-key.pem"),
    )
    .unwrap();
    let (port, worker) = mtls_server(r#"{"mtls":true,"secret":"probe-test-only-password"}"#);
    fs::write(
        &workspace,
        include_str!("../../../tests/fixtures/opencollection/mtls-variables.yml"),
    )
    .unwrap();
    let variables = [
        "--environment",
        "child",
        "--var",
        "mtlsDomain=localhost",
        "--var",
        "directory=.",
        "--var",
        "passwordAlias={{certPassword}}",
    ];
    // No provider or files are available during dry-run; certificate-only secrets
    // must remain untouched even in strict mode.
    let dry = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .args(["items/0", "--dry-run", "--strict-variables", "--json"])
        .args(variables)
        .output()
        .unwrap();
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let source = fs::read_to_string(&workspace).unwrap().replace(
        "https://api.example.com/private",
        &format!("https://localhost:{port}/private"),
    );
    fs::write(&workspace, source).unwrap();
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .args([
            "items/0",
            "--json",
            "--strict-variables",
            "--secret-provider",
            "env",
        ])
        .args(variables)
        .env("certPassword", "probe-test-only-password")
        .env("SSL_CERT_FILE", tls_fixture("ca.pem"))
        .env("SSL_CERT_DIR", &directory)
        .env("NO_PROXY", "localhost,127.0.0.1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["response"]["status"], 200);
    assert_eq!(
        response["response"]["body"]["content"],
        r#"{"mtls":true,"secret":"[REDACTED]"}"#
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("probe-test-only-password"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("probe-test-only-password"));
    assert!(worker.join().unwrap());
    fs::remove_dir_all(directory).unwrap();
}
