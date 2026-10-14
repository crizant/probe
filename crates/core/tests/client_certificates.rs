use probe_core::{ClientCertificate, Collection, Workspace};

#[test]
fn certificate_configuration_stays_scoped_and_redacts_passphrases_in_debug() {
    let certificates = vec![
        ClientCertificate::Pem {
            domain: "api.example.com".into(),
            certificate_file_path: "./client.pem".into(),
            private_key_file_path: "./key.pem".into(),
            passphrase: Some("sensitive-pem-passphrase".into()),
            disabled: false,
        },
        ClientCertificate::Pkcs12 {
            domain: "api.example.com".into(),
            pkcs12_file_path: "./client.p12".into(),
            passphrase: Some("sensitive-pkcs12-passphrase".into()),
            disabled: true,
        },
    ];
    let collection = Collection {
        client_certificates: certificates.clone(),
        ..Collection::default()
    };
    let workspace = Workspace::from_collection(collection.clone());
    assert_eq!(workspace.client_certificates(), certificates);
    assert!(workspace.environments().is_empty());
    for certificate in certificates {
        let debug = format!("{certificate:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(debug.contains("api.example.com"));
        assert!(!debug.contains("sensitive-"));
    }
    for debug in [format!("{collection:?}"), format!("{workspace:?}")] {
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("sensitive-"));
    }
    let empty = Workspace::from_collection(Collection::default());
    assert!(empty.client_certificates().is_empty());
}
