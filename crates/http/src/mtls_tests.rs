//! Public test-only CA fixtures; no production trust override is exposed.

use super::*;
use crate::PemClientIdentity;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/mtls")
        .join(name)
}

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

fn certs(name: &str) -> Vec<rustls::pki_types::CertificateDer<'static>> {
    CertificateDer::pem_slice_iter(&bytes(name))
        .collect::<Result<_, _>>()
        .unwrap()
}

fn server(response: String) -> (u16, std::thread::JoinHandle<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let worker = std::thread::spawn(move || {
        let mut roots = RootCertStore::empty();
        roots.add(certs("ca.pem").remove(0)).unwrap();
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            provider.clone(),
        )
        .build()
        .unwrap();
        let key = PrivateKeyDer::from_pem_slice(&bytes("server-key.pem")).unwrap();
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs("server.pem"), key)
            .unwrap();
        let (socket, _) = listener.accept().unwrap();
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
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        authenticated
    });
    (port, worker)
}

async fn client(
    identity: Option<(&str, &str)>,
    domain: &str,
    follow: bool,
    maximum: usize,
) -> Client {
    let identity = match identity {
        Some((cert, key)) => {
            let configuration = PemClientIdentity::new(
                domain.into(),
                fixture(cert).to_str().unwrap().into(),
                fixture(key).to_str().unwrap().into(),
                None,
            );
            Some((
                configuration
                    .load(&ExecutionOptions::default())
                    .await
                    .unwrap(),
                configuration,
            ))
        }
        None => None,
    };
    let ca = reqwest::Certificate::from_pem(&bytes("ca.pem")).unwrap();
    configure_client(
        Client::builder()
            .tls_certs_only([ca])
            .no_proxy()
            .timeout(Duration::from_secs(5)),
        follow,
        maximum,
        identity,
    )
    .unwrap()
}

fn ok() -> String {
    "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".into()
}

#[tokio::test]
async fn mtls_requires_a_trusted_client_and_valid_identity_succeeds() {
    for (identity, succeeds) in [
        (None, false),
        (Some(("client.pem", "client-pkcs8-key.pem")), true),
        (Some(("wrong-client.pem", "wrong-client-key.pem")), false),
    ] {
        let (port, worker) = server(ok());
        let result = client(identity, "localhost", true, 10)
            .await
            .get(format!("https://localhost:{port}/private"))
            .send()
            .await;
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(worker.join().unwrap(), succeeds);
        if let Ok(response) = result {
            assert_eq!(response.text().await.unwrap(), "ok");
        }
    }
}

#[tokio::test]
async fn redirect_outside_identity_domain_never_connects_to_destination() {
    let unrelated = TcpListener::bind("127.0.0.1:0").unwrap();
    unrelated.set_nonblocking(true).unwrap();
    let target = unrelated.local_addr().unwrap();
    let (port, worker) = server(format!(
        "HTTP/1.1 302 Found\r\nLocation: https://{target}/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    ));
    let error = client(
        Some(("client.pem", "client-key.pem")),
        "localhost",
        true,
        10,
    )
    .await
    .get(format!("https://localhost:{port}/private"))
    .send()
    .await
    .map_err(map_reqwest_error)
    .unwrap_err();
    assert_eq!(error, HttpError::ClientCertificateRedirectBlocked);
    assert!(error.to_string().contains("redirect blocked"));
    assert!(worker.join().unwrap());
    assert_eq!(
        unrelated.accept().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[tokio::test]
async fn matching_redirects_continue_and_redirect_settings_remain_effective() {
    let (target_port, target) = server(ok());
    let (port, origin) = server(format!(
        "HTTP/1.1 302 Found\r\nLocation: https://localhost:{target_port}/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    ));
    let response = client(
        Some(("client.pem", "client-key.pem")),
        "localhost",
        true,
        10,
    )
    .await
    .get(format!("https://localhost:{port}/private"))
    .send()
    .await
    .unwrap();
    assert_eq!(response.status(), 200);
    assert!(origin.join().unwrap());
    assert!(target.join().unwrap());

    for (follow, maximum, fails) in [(false, 10, false), (true, 0, true)] {
        let (port, worker) = server("HTTP/1.1 302 Found\r\nLocation: /again\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        let result = client(
            Some(("client.pem", "client-key.pem")),
            "localhost",
            follow,
            maximum,
        )
        .await
        .get(format!("https://localhost:{port}/private"))
        .send()
        .await;
        assert_eq!(result.is_err(), fails);
        if let Ok(response) = result {
            assert_eq!(response.status(), 302);
        }
        assert!(worker.join().unwrap());
    }
}

#[tokio::test]
async fn https_to_http_redirect_never_connects_even_when_hostname_matches() {
    let plaintext = TcpListener::bind("127.0.0.1:0").unwrap();
    plaintext.set_nonblocking(true).unwrap();
    let target_port = plaintext.local_addr().unwrap().port();
    let (port, worker) = server(format!(
        "HTTP/1.1 302 Found\r\nLocation: http://localhost:{target_port}/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    ));
    let error = client(Some(("client.pem", "client-key.pem")), "localhost", true, 10)
        .await
        .get(format!("https://localhost:{port}/private"))
        .send()
        .await
        .map_err(map_reqwest_error)
        .unwrap_err();
    assert_eq!(error, HttpError::ClientCertificateRedirectBlocked);
    assert!(worker.join().unwrap());
    assert_eq!(plaintext.accept().err().unwrap().kind(), std::io::ErrorKind::WouldBlock);
}
