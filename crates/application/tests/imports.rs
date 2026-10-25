use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use probe_application::imports::{
    ImportConversion, ImportError, ImportMetadata, ImportSource, inspect_import,
};
use probe_opencollection::{CreateError, create_bundled_workspace_from_collection, load_workspace};
use probe_postman::inspect_postman_source;
use probe_yaak::{YaakImportError, inspect_yaak_source};

fn fixture(provider: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(provider)
        .join(name)
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "probe-import-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn staged_imports_preserve_provider_metadata_diagnostics_and_persistence() {
    let scratch = Scratch::new();
    for (source, provider, name) in [
        (ImportSource::Postman, "postman", "collection-v2.json"),
        (ImportSource::Postman, "postman", "collection-v2.1.json"),
        (ImportSource::Postman, "postman", "collection-lossy.json"),
        (ImportSource::Yaak, "yaak", "export-v4.json"),
        (ImportSource::Yaak, "yaak", "sync"),
    ] {
        let path = fixture(provider, name);
        let conversion = inspect_import(source, &path).unwrap();
        let imported = conversion.convert(true).unwrap();
        let direct_collection = match &imported.metadata {
            ImportMetadata::Postman { format, collection } => {
                let preview = inspect_postman_source(&path).unwrap();
                let direct = preview.convert(true).unwrap();
                assert_eq!(*format, preview.format());
                assert_eq!(*collection, direct.source);
                assert_eq!(imported.diagnostics, direct.diagnostics);
                assert_eq!(imported.partial, direct.partial);
                assert_eq!(
                    imported.selected_environment,
                    direct.collection_variables_environment
                );
                direct.collection
            }
            ImportMetadata::Yaak { format, workspace } => {
                let preview = inspect_yaak_source(&path).unwrap();
                let direct = preview.convert(None, true).unwrap();
                assert_eq!(*format, preview.format());
                assert_eq!(*workspace, direct.workspace);
                assert_eq!(imported.diagnostics, direct.diagnostics);
                assert_eq!(imported.partial, direct.partial);
                assert_eq!(imported.selected_environment, direct.default_environment);
                direct.collection
            }
        };
        assert_eq!(imported.collection, direct_collection);
        let destination = scratch.0.join("imported");
        let loaded = imported.create_workspace(&destination).unwrap();
        let written = loaded.source_path().unwrap();
        let direct_path = scratch.0.join("direct.yml");
        create_bundled_workspace_from_collection(&direct_path, &direct_collection).unwrap();
        let expected = fs::read(&direct_path).unwrap();
        assert_eq!(fs::read(written).unwrap(), expected);
        let reloaded = load_workspace(written).unwrap();
        assert_eq!(
            reloaded.workspace().request_count(),
            loaded.workspace().request_count()
        );
        assert_eq!(
            reloaded.workspace().environments(),
            imported.collection.environments
        );
        assert!(matches!(
            imported.create_workspace(&destination),
            Err(CreateError::AlreadyExists(_))
        ));
        assert_eq!(fs::read(written).unwrap(), expected);
        fs::remove_file(written).unwrap();
        fs::remove_file(direct_path).unwrap();
    }
}

#[test]
fn strict_conversion_keeps_diagnostics_and_can_be_retried_after_consent() {
    let scratch = Scratch::new();
    let yaak = scratch.0.join("lossy.json");
    fs::write(&yaak, r#"{"yaakSchema":4,"resources":{"workspaces":[{"model":"workspace","id":"wk_1","name":"Mixed"}],"grpcRequests":[{"model":"grpc_request","id":"gr_1","workspaceId":"wk_1"}]}}"#).unwrap();
    for (source, path) in [
        (
            ImportSource::Postman,
            fixture("postman", "collection-lossy.json"),
        ),
        (ImportSource::Yaak, yaak),
    ] {
        let conversion = inspect_import(source, &path).unwrap();
        let direct_error = match source {
            ImportSource::Postman => ImportError::Postman(
                inspect_postman_source(&path)
                    .unwrap()
                    .convert(false)
                    .unwrap_err(),
            ),
            ImportSource::Yaak => ImportError::Yaak(
                inspect_yaak_source(&path)
                    .unwrap()
                    .convert(None, false)
                    .unwrap_err(),
            ),
        };
        let error = conversion.convert(false).unwrap_err();
        assert_eq!(error.diagnostics(), direct_error.diagnostics());
        assert_eq!(error.to_string(), direct_error.to_string());
        let imported = conversion.convert(true).unwrap();
        assert!(imported.partial);
        assert_eq!(Some(imported.diagnostics.as_slice()), error.diagnostics());
    }
}

#[test]
fn yaak_selection_errors_and_selected_identity_are_preserved() {
    let scratch = Scratch::new();
    let path = scratch.0.join("source.json");
    fs::write(&path, r#"{"yaakSchema":4,"resources":{"workspaces":[{"model":"workspace","id":"wk_a","name":"A"},{"model":"workspace","id":"wk_b","name":"B"}]}}"#).unwrap();
    let mut conversion = inspect_import(ImportSource::Yaak, &path).unwrap();
    let error = conversion.convert(false).unwrap_err();
    let ImportError::Yaak(YaakImportError::WorkspaceSelectionRequired(workspaces)) = error else {
        panic!("expected workspace selection");
    };
    assert_eq!(
        workspaces
            .iter()
            .map(|workspace| workspace.id.as_str())
            .collect::<Vec<_>>(),
        ["wk_a", "wk_b"]
    );
    let ImportConversion::Yaak { workspace_id, .. } = &mut conversion else {
        panic!("expected Yaak preview");
    };
    *workspace_id = Some("missing".into());
    assert!(
        matches!(conversion.convert(false), Err(ImportError::Yaak(YaakImportError::WorkspaceNotFound(id))) if id == "missing")
    );
    let ImportConversion::Yaak { workspace_id, .. } = &mut conversion else {
        unreachable!()
    };
    *workspace_id = Some("wk_b".into());
    let imported = conversion.convert(false).unwrap();
    let ImportMetadata::Yaak { workspace, .. } = imported.metadata else {
        panic!("expected Yaak metadata");
    };
    assert_eq!(workspace.id, "wk_b");
    assert_eq!(workspace.name, "B");
}
