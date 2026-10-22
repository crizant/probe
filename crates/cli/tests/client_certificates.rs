//! End-to-end trust injection uses Linux's existing SSL_CERT_FILE support only.
#![cfg(target_os = "linux")]

#[allow(dead_code, unused_imports)]
mod common;

use common::*;

#[path = "../../../tests/support/mtls.rs"]
mod mtls_support;
use mtls_support::tls_fixture;

fn mtls_server(body: &'static str) -> (u16, JoinHandle<bool>) {
    mtls_support::mtls_server(body, false)
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
    for (protocol, pkcs12) in [
        ("http", false),
        ("graphql", false),
        ("http", true),
        ("graphql", true),
    ] {
        let (port, worker) = mtls_server(r#"{"mtls":true}"#);
        let body = if protocol == "graphql" {
            "      body:\n        query: 'query { viewer { id } }'\n"
        } else {
            ""
        };
        let source = format!(
            "opencollection: 1.0.0\ninfo:\n  name: CLI mTLS test\nbundled: true\nconfig:\n  clientCertificates:\n    - domain: localhost:{port}\n      type: pem\n      certificateFilePath: ./client.pem\n      privateKeyFilePath: ./client-key.pem\n      passphrase: probe-test-only-password\nitems:\n  - info:\n      name: Request\n      type: {protocol}\n    {protocol}:\n      method: POST\n      url: https://localhost:{port}/private\n{body}"
        );
        let source = if pkcs12 {
            fs::copy(
                tls_fixture("client-protected.p12"),
                directory.join("client.pfx"),
            )
            .unwrap();
            source.replace("type: pem\n      certificateFilePath: ./client.pem\n      privateKeyFilePath: ./client-key.pem", "type: pkcs12\n      pkcs12FilePath: ./client.pfx")
        } else {
            source
        };
        fs::write(
            &workspace,
            source
                .replace("./client.pem", "./absent-cert.pem")
                .replace("./client.pfx", "./absent-container.pfx"),
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
    for pkcs12 in [false, true] {
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
        let source = fs::read_to_string(&workspace).unwrap();
        let source = if pkcs12 {
            fs::copy(
                tls_fixture("client-protected.p12"),
                directory.join("client.pfx"),
            )
            .unwrap();
            source.replace("type: pem\n      certificateFilePath: '{{certPath}}'\n      privateKeyFilePath: '{{keyPath}}'", "type: pkcs12\n      pkcs12FilePath: '{{directory}}/client.pfx'")
        } else {
            source
        };
        let source = source.replace(
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
    }
    fs::remove_dir_all(directory).unwrap();
}
