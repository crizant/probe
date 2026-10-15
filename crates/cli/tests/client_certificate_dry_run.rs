#[allow(dead_code, unused_imports)]
mod common;

use common::*;

#[test]
fn pkcs12_dry_run_never_requires_reading_parsing_or_decrypting_the_container() {
    let directory = temporary_path("pkcs12-dry-run");
    fs::create_dir_all(&directory).unwrap();
    let workspace = directory.join("collection.yml");
    fs::write(directory.join("malformed.p12"), b"not a PKCS12 container").unwrap();
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/mtls/client-protected.p12"),
        directory.join("protected.pfx"),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!(
        "https://localhost:{}/private",
        listener.local_addr().unwrap().port()
    );
    for protocol in ["http", "graphql"] {
        for (path, diagnostic) in [
            ("absent.p12", "cannot read PKCS#12 client certificate file"),
            (
                "malformed.p12",
                "malformed or unsupported PKCS#12 container",
            ),
            ("protected.pfx", "PKCS#12 integrity/MAC verification failed"),
        ] {
            let body = if protocol == "graphql" {
                "      body:\n        query: 'query { viewer { id } }'\n"
            } else {
                ""
            };
            fs::write(&workspace, format!(
                "opencollection: 1.0.0\ninfo:\n  name: PKCS12 dry-run\nbundled: true\nconfig:\n  clientCertificates:\n    - domain: localhost\n      type: pkcs12\n      pkcs12FilePath: ./{path}\n      passphrase: wrong-password-marker\nitems:\n  - info:\n      name: Request\n      type: {protocol}\n    {protocol}:\n      method: POST\n      url: {url}\n{body}"
            )).unwrap();
            let dry = probe()
                .args(["request", "run"])
                .arg(&workspace)
                .args(["items/0", "--dry-run", "--json"])
                .output()
                .unwrap();
            assert!(
                dry.status.success(),
                "{protocol}, {path}: {}",
                String::from_utf8_lossy(&dry.stderr)
            );
            assert!(dry.stderr.is_empty());
            let document: Value = serde_json::from_slice(&dry.stdout).unwrap();
            assert_eq!(document["dryRun"], true);
            assert_eq!(document["request"]["url"], url);
            assert!(document.get("response").is_none());
            assert!(!String::from_utf8_lossy(&dry.stdout).contains("wrong-password-marker"));
            // The same configuration fails in execution: the dry-run success did
            // not rely on a usable certificate, readable file or correct password.
            let execution = probe()
                .args(["request", "run"])
                .arg(&workspace)
                .args(["items/0", "--json"])
                .output()
                .unwrap();
            assert!(!execution.status.success());
            let failure = format!(
                "{}{}",
                String::from_utf8_lossy(&execution.stdout),
                String::from_utf8_lossy(&execution.stderr)
            );
            assert!(
                failure.contains(diagnostic),
                "{protocol}, {path}: {failure}"
            );
            assert!(!failure.contains("wrong-password-marker"));
        }
    }
    assert_eq!(
        listener.accept().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
    fs::remove_dir_all(directory).unwrap();
}
