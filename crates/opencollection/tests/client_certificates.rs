use std::fs;

use probe_core::{ClientCertificate, CollectionUpdate, FieldPatch, Workspace};
use probe_opencollection::{create_bundled_workspace_from_collection, load_workspace, parse};

mod support;
use support::{copy_directory, fixture, temporary_path};

fn source() -> String {
    fs::read_to_string(fixture("client-certificates.yml")).unwrap()
}

#[test]
fn native_certificates_keep_variants_fields_defaults_and_declaration_order() {
    let parsed = parse(&source()).unwrap();
    let collection = parsed.collection();
    assert_eq!(
        collection.client_certificates,
        vec![
            ClientCertificate::Pem {
                domain: "api.example.com".into(),
                certificate_file_path: "./certs/../certs/client.pem".into(),
                private_key_file_path: "{{certDirectory}}/client-key.pem".into(),
                passphrase: None,
                disabled: false,
            },
            ClientCertificate::Pkcs12 {
                domain: "api.example.com".into(),
                pkcs12_file_path: "./certs/client archive.p12".into(),
                passphrase: Some("{{certPassphrase}}".into()),
                disabled: true,
            },
            ClientCertificate::Pem {
                domain: "another.example.com".into(),
                certificate_file_path: "/configured/client.pem".into(),
                private_key_file_path: r"C:\certs\client-key.pem".into(),
                passphrase: Some("literal-passphrase".into()),
                disabled: false,
            },
        ]
    );
    assert_eq!(
        collection.environments[0].client_certificates,
        vec![ClientCertificate::Pem {
            domain: "dev-api.example.com".into(),
            certificate_file_path: "./certs/dev.pem".into(),
            private_key_file_path: "./certs/dev-key.pem".into(),
            passphrase: None,
            disabled: false,
        }]
    );
    assert_eq!(
        collection.environments[1].client_certificates,
        vec![ClientCertificate::Pkcs12 {
            domain: "api.example.com".into(),
            pkcs12_file_path: "./certs/prod.p12".into(),
            passphrase: Some(String::new()),
            disabled: true,
        }]
    );
    assert_eq!(
        collection.environments[1].extends.as_deref(),
        Some("development")
    );
    // Inheritance still validates, but does not synthesize certificate lists.
    assert!(collection.environments[2].client_certificates.is_empty());
    assert_eq!(
        collection.environments[2].extends.as_deref(),
        Some("production")
    );
    let workspace = Workspace::from_collection(collection.clone());
    assert_eq!(
        workspace.client_certificates(),
        collection.client_certificates
    );
    assert_eq!(workspace.environments(), collection.environments);
    for text in [
        format!("{collection:?}"),
        format!("{workspace:?}"),
        format!("{parsed:?}"),
    ] {
        assert!(!text.contains("literal-passphrase"));
        assert!(!text.contains("certPassphrase"));
        assert!(text.contains("[REDACTED]"));
    }
}

#[test]
fn certificates_round_trip_through_source_and_domain_collection_creation() {
    let parsed = parse(&source()).unwrap();
    let expected = parsed.collection().clone();
    assert_eq!(
        parse(&parsed.to_yaml().unwrap()).unwrap().collection(),
        &expected
    );
    let path = temporary_path("certificate-domain.yml");
    let loaded = create_bundled_workspace_from_collection(&path, &expected).unwrap();
    assert_eq!(
        loaded.workspace().client_certificates(),
        expected.client_certificates
    );
    assert_eq!(loaded.workspace().environments(), expected.environments);
    let reloaded = parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(reloaded.collection(), &expected);

    // A certificate-only config must also be written when no environments exist.
    let mut only_certificates = expected;
    only_certificates.environments.clear();
    let path = temporary_path("certificate-only.yml");
    let loaded = create_bundled_workspace_from_collection(&path, &only_certificates).unwrap();
    assert_eq!(
        loaded.workspace().client_certificates(),
        only_certificates.client_certificates
    );
}

#[test]
fn environment_and_collection_edits_preserve_unexposed_certificate_yaml() {
    let path = temporary_path("certificate-edits.yml");
    fs::write(&path, source()).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let expected = parse(&source()).unwrap().into_collection();
    let original: serde_yaml_ng::Value = serde_yaml_ng::from_str(&source()).unwrap();
    loaded
        .update_environment_variable("development", "baseUrl", "changed".into())
        .unwrap();
    let mut replacement = loaded.workspace().environments()[1].clone();
    replacement.extends = None;
    loaded
        .replace_environment("production", replacement)
        .unwrap();
    loaded
        .update_collection(&CollectionUpdate {
            summary: FieldPatch::Set("Updated".into()),
            ..CollectionUpdate::default()
        })
        .unwrap();
    loaded
        .create_environment("new".into(), Some("development".into()))
        .unwrap();
    loaded.delete_environment("new").unwrap();
    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(
        reloaded.workspace().client_certificates(),
        expected.client_certificates
    );
    for (index, environment) in reloaded.workspace().environments().iter().enumerate() {
        assert_eq!(
            environment.client_certificates,
            expected.environments[index].client_certificates
        );
    }
    let saved: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        saved["config"]["clientCertificates"],
        original["config"]["clientCertificates"]
    );
    for index in 0..2 {
        assert_eq!(
            saved["config"]["environments"][index]["clientCertificates"],
            original["config"]["environments"][index]["clientCertificates"]
        );
    }
}

#[test]
fn environment_certificate_replacement_and_removal_are_persisted() {
    let path = temporary_path("certificate-replacement.yml");
    fs::write(&path, source()).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let mut replacement = loaded.workspace().environments()[0].clone();
    replacement.client_certificates = loaded.workspace().client_certificates().to_vec();
    loaded
        .replace_environment("development", replacement.clone())
        .unwrap();
    assert_eq!(
        load_workspace(&path).unwrap().workspace().environments()[0],
        replacement
    );
    let saved: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let entries = saved["config"]["environments"][0]["clientCertificates"]
        .as_sequence()
        .unwrap();
    assert!(entries[0].get("disabled").is_none());
    assert_eq!(entries[1]["disabled"], serde_yaml_ng::Value::Bool(true));
    assert!(entries[2].get("disabled").is_none());
    replacement.client_certificates.clear();
    loaded
        .replace_environment("development", replacement.clone())
        .unwrap();
    assert_eq!(
        load_workspace(&path).unwrap().workspace().environments()[0],
        replacement
    );
    let saved: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(
        saved["config"]["environments"][0]
            .get("clientCertificates")
            .is_none()
    );
}

#[test]
fn unbundled_environment_rename_preserves_certificate_configuration() {
    let root = temporary_path("certificate-unbundled");
    copy_directory(&fixture("unbundled"), &root);
    fs::write(root.join("environments/development.yml"), "name: development\nclientCertificates:\n  - domain: dev.example.com\n    type: pkcs12\n    pkcs12FilePath: ../certs/dev.p12\n    passphrase: '{{passphrase}}'\n").unwrap();
    let root_path = root.join("opencollection.yml");
    let root_source = "opencollection: 1.0.0\ninfo: {name: Unbundled certificates}\nbundled: false\nconfig:\n  clientCertificates:\n    - type: pem\n      domain: api.example.com\n      certificateFilePath: cert.pem\n      privateKeyFilePath: key.pem\n";
    fs::write(&root_path, root_source).unwrap();
    let mut loaded = load_workspace(&root).unwrap();
    let expected_root = loaded.workspace().client_certificates().to_vec();
    let mut replacement = loaded
        .workspace()
        .environments()
        .iter()
        .find(|env| env.name == "development")
        .unwrap()
        .clone();
    replacement.name = "staging".into();
    loaded
        .replace_environment("development", replacement.clone())
        .unwrap();
    loaded
        .update_collection(&CollectionUpdate {
            summary: FieldPatch::Set("Updated".into()),
            ..CollectionUpdate::default()
        })
        .unwrap();
    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(reloaded.workspace().client_certificates(), expected_root);
    assert_eq!(
        reloaded
            .workspace()
            .environments()
            .iter()
            .find(|env| env.name == "staging")
            .unwrap(),
        &replacement
    );
}

#[test]
fn invalid_native_certificate_data_is_rejected_without_disclosing_passphrases() {
    for scope in ["collection", "environment"] {
        for invalid in [
            "type: pfx\n    domain: api.example.com\n    pkcs12FilePath: client.p12",
            "type: PEM\n    domain: api.example.com\n    certificateFilePath: client.pem\n    privateKeyFilePath: key.pem",
            "type: pem\n    domain: api.example.com\n    certificateFilePath: client.pem",
            "type: pkcs12\n    pkcs12FilePath: client.p12",
            "domain: api.example.com\n    pkcs12FilePath: client.p12",
            "type: pkcs12\n    domain: api.example.com\n    pkcs12FilePath: [invalid]",
            "type: pkcs12\n    domain: api.example.com\n    pkcs12FilePath: client.p12\n    passphrase: [SENSITIVE_VALUE]",
            "type: pkcs12\n    domain: api.example.com\n    pkcs12FilePath: client.p12\n    disabled: SENSITIVE_VALUE",
            "type: pem\n    domain: api.example.com\n    certificateFilePath: client.pem\n    privateKeyFilePath: key.pem\n    unknownKey: SENSITIVE_VALUE",
            "type: pkcs12\n    domain: api.example.com\n    pkcs12FilePath: client.p12\n    unknownKey: SENSITIVE_VALUE",
            "type: pkcs12\n    domain: 123\n    pkcs12FilePath: client.p12",
            "type: pem\n    domain: false\n    certificateFilePath: client.pem\n    privateKeyFilePath: key.pem",
            "type: pkcs12\n    domain: [api.example.com]\n    pkcs12FilePath: client.p12",
            "type: pem\n    domain: api.example.com\n    certificateFilePath: client.pem\n    privateKeyFilePath: key.pem\n    pkcs12FilePath: client.p12\n    passphrase: SENSITIVE_VALUE",
            "type: pkcs12\n    domain: api.example.com\n    pkcs12FilePath: client.p12\n    certificateFilePath: client.pem\n    passphrase: SENSITIVE_VALUE",
            "type: pem\n    domain: api.example.com\n    certificateFilePath: client.pem\n    privateKeyFilePath: key.pem\n    disabledd: true\n    passphrase: SENSITIVE_VALUE",
            "type: pkcs12\n    domain: api.example.com\n    pkcs12FilePath: client.p12\n    disabledd: true\n    passphrase: SENSITIVE_VALUE",
        ] {
            let certificates = format!("clientCertificates:\n  - {invalid}\n");
            let config = if scope == "collection" {
                certificates
                    .lines()
                    .map(|line| format!("  {line}\n"))
                    .collect::<String>()
            } else {
                format!(
                    "  environments:\n    - name: development\n{}",
                    certificates
                        .lines()
                        .map(|line| format!("      {line}\n"))
                        .collect::<String>()
                )
            };
            let yaml = format!(
                "opencollection: 1.0.0\ninfo: {{name: Invalid}}\nbundled: true\nconfig:\n{config}items: []\n"
            );
            let error = parse(&yaml).unwrap_err();
            assert!(error.to_string().contains("invalid clientCertificates[0]"));
            assert!(!format!("{error:?} {error}").contains("SENSITIVE_VALUE"));
        }
    }
}

#[test]
fn inheritance_errors_and_certificate_free_defaults_are_unchanged() {
    let original = source();
    for invalid in [
        original.replace("extends: development", "extends: missing"),
        original.replace("extends: development", "extends: inherited-variables"),
    ] {
        assert!(parse(&invalid).is_err());
    }
    let parsed = parse(&fs::read_to_string(fixture("phase4-environments.yml")).unwrap()).unwrap();
    assert!(parsed.collection().client_certificates.is_empty());
    assert!(
        parsed
            .collection()
            .environments
            .iter()
            .all(|env| env.client_certificates.is_empty())
    );
}

fn assert_redacted(value: &impl std::fmt::Debug) {
    let debug = format!("{value:?}");
    assert!(
        !debug.is_empty(),
        "debug output should identify the operation"
    );
    assert!(!debug.contains("literal-passphrase"));
    assert!(!debug.contains("certPassphrase"));
    assert!(debug.contains("[REDACTED]"));
    assert!(debug.contains("api.example.com"));
    assert!(debug.contains("clientCertificates"));
}

#[test]
fn repository_debug_output_never_dumps_passphrase_bearing_source() {
    let path = temporary_path("certificate-debug.yml");
    let source = source().replace("items: []", "items:\n  - info: {name: Example, type: http}\n    http: {method: GET, url: https://api.example.com}");
    fs::write(&path, source).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    assert_redacted(&loaded);
    assert!(format!("{loaded:?}").contains("SourceDocument"));
    let prepared = loaded
        .prepare_collection_save(CollectionUpdate {
            summary: FieldPatch::Set("Updated".into()),
            ..CollectionUpdate::default()
        })
        .unwrap();
    assert_redacted(&prepared);
    let completed = prepared.execute().unwrap();
    assert_redacted(&completed);
    loaded.complete_documentation_save(completed).unwrap();

    let prepared = loaded
        .prepare_request_save(
            "items/0",
            probe_core::RequestUpdate {
                url: FieldPatch::Set("https://changed.example.com".into()),
                ..probe_core::RequestUpdate::default()
            },
        )
        .unwrap();
    assert_redacted(&prepared);
    let completed = prepared.execute().unwrap();
    assert_redacted(&completed);
    loaded.complete_request_save(completed).unwrap();

    loaded
        .set_environment_variable("development", "baseUrl", "updated".into())
        .unwrap();
    let prepared = loaded
        .prepare_environment_variable_save("development", "baseUrl")
        .unwrap();
    assert_redacted(&prepared);
    let completed = prepared.execute().unwrap();
    assert_redacted(&completed);
    loaded.complete_environment_save(completed).unwrap();

    let replacement = loaded.workspace().environments()[0].clone();
    let prepared = loaded
        .prepare_environment_replace("development", replacement)
        .unwrap();
    assert_redacted(&prepared);
    let completed = prepared.execute().unwrap();
    assert_redacted(&completed);
    loaded.complete_environment_replace(completed).unwrap();

    let prepared = loaded
        .prepare_environment_create("new".into(), None)
        .unwrap();
    assert_redacted(&prepared);
    assert!(format!("{prepared:?}").contains("Bundled"));
    let completed = prepared.execute().unwrap();
    assert_redacted(&completed);
    loaded.complete_environment_create(completed).unwrap();

    let prepared = loaded.prepare_environment_delete("new").unwrap();
    assert_redacted(&prepared);
    let completed = prepared.execute().unwrap();
    assert_redacted(&completed);
    loaded.complete_environment_delete(completed).unwrap();

    let prepared = loaded
        .prepare_structure(probe_opencollection::StructureOperation::CreateFolder {
            parent: None,
            index: None,
            name: "New folder".into(),
        })
        .unwrap();
    assert_redacted(&prepared);
    let (_, reloaded) = prepared.execute().unwrap();
    assert_eq!(
        reloaded.workspace().client_certificates(),
        loaded.workspace().client_certificates()
    );
    assert_eq!(
        reloaded.workspace().environments(),
        loaded.workspace().environments()
    );
}

#[test]
fn both_variants_preserve_optional_fields_independently() {
    for (kind, paths) in [
        (
            "pem",
            "certificateFilePath: client.pem\n      privateKeyFilePath: key.pem",
        ),
        ("pkcs12", "pkcs12FilePath: client.p12"),
    ] {
        for (options, expected_passphrase, expected_disabled) in [
            ("", None, false),
            ("      passphrase: null\n", None, false),
            (
                "      passphrase: ''\n      disabled: false\n",
                Some(""),
                false,
            ),
            (
                "      passphrase: '{{passphrase}}'\n      disabled: true\n",
                Some("{{passphrase}}"),
                true,
            ),
        ] {
            let yaml = format!(
                "opencollection: 1.0.0\ninfo: {{name: Optional fields}}\nbundled: true\nconfig:\n  clientCertificates:\n    - domain: api.example.com\n      type: {kind}\n      {paths}\n{options}items: []\n"
            );
            let collection = parse(&yaml).unwrap().into_collection();
            let (passphrase, disabled) = match &collection.client_certificates[0] {
                ClientCertificate::Pem {
                    passphrase,
                    disabled,
                    ..
                }
                | ClientCertificate::Pkcs12 {
                    passphrase,
                    disabled,
                    ..
                } => (passphrase, disabled),
            };
            assert_eq!(passphrase.as_deref(), expected_passphrase);
            assert_eq!(*disabled, expected_disabled);
            let path = temporary_path("certificate-options.yml");
            let loaded = create_bundled_workspace_from_collection(&path, &collection).unwrap();
            assert_eq!(
                loaded.workspace().client_certificates(),
                collection.client_certificates
            );
            let saved: serde_yaml_ng::Value =
                serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            let disabled = saved["config"]["clientCertificates"][0].get("disabled");
            assert_eq!(
                disabled,
                expected_disabled.then_some(&serde_yaml_ng::Value::Bool(true))
            );
        }
    }
}

#[test]
fn certificate_errors_identify_the_entry_index_in_both_scopes() {
    for scope in ["collection", "environment"] {
        for invalid_index in [0, 1, 2] {
            let mut document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&source()).unwrap();
            let entries = if scope == "collection" {
                document["config"]["clientCertificates"]
                    .as_sequence_mut()
                    .unwrap()
            } else {
                let entries = document["config"]["environments"][0]["clientCertificates"]
                    .as_sequence_mut()
                    .unwrap();
                entries.resize(3, entries[0].clone());
                entries
            };
            entries[invalid_index].as_mapping_mut().unwrap().insert(
                serde_yaml_ng::Value::String("unknownKey".into()),
                serde_yaml_ng::Value::String("SENSITIVE_VALUE".into()),
            );
            let error = parse(&serde_yaml_ng::to_string(&document).unwrap()).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(&format!("clientCertificates[{invalid_index}]"))
            );
            assert!(!format!("{error:?} {error}").contains("SENSITIVE_VALUE"));
        }
    }
}

#[test]
fn unrelated_environment_edit_retains_explicit_optional_certificate_fields() {
    let source = source().replace("\r\n", "\n").replace(
        "          privateKeyFilePath: ./certs/dev-key.pem",
        "          privateKeyFilePath: ./certs/dev-key.pem\n          passphrase: null\n          disabled: false",
    );
    let original: serde_yaml_ng::Value = serde_yaml_ng::from_str(&source).unwrap();
    let path = temporary_path("certificate-optional-fields.yml");
    fs::write(&path, source).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let mut environment = loaded.workspace().environments()[0].clone();
    assert!(matches!(
        &environment.client_certificates[0],
        ClientCertificate::Pem {
            passphrase: None,
            disabled: false,
            ..
        }
    ));
    environment.variables.clear();
    loaded
        .replace_environment("development", environment)
        .unwrap();
    let saved: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        saved["config"]["environments"][0]["clientCertificates"],
        original["config"]["environments"][0]["clientCertificates"]
    );
}
