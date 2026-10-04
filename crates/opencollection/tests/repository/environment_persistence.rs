use super::*;
use probe_core::Documentation;

fn environment_description(root: &std::path::Path, name: &str) -> Option<Documentation> {
    load_workspace(root)
        .unwrap()
        .workspace()
        .environments()
        .iter()
        .find(|environment| environment.name == name)
        .and_then(|environment| environment.description.clone())
}

fn yaml(path: &std::path::Path) -> serde_yaml_ng::Value {
    serde_yaml_ng::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

pub(super) fn resolved_variable(
    loaded: &probe_opencollection::LoadedWorkspace,
    environment: &str,
    name: &str,
) -> Option<String> {
    resolve_environment(loaded.workspace().environments(), environment)
        .ok()
        .and_then(|resolved| resolved.variable(name).map(str::to_owned))
}

#[test]
fn environment_description_set_and_unset_round_trip() {
    let path = temporary_path("env-description.yml");
    fs::write(
        &path,
        concat!(
            "opencollection: 1.0.0\n",
            "info:\n  name: Env description\n",
            "bundled: true\n",
            "config:\n",
            "  environments:\n",
            "    - name: base\n",
            "      variables:\n",
            "        - name: host\n",
            "          value: api.example.com\n",
            "    - name: development\n",
            "      extends: base\n",
            "      vendor.example: retained\n",
            "      variables:\n",
            "        - name: host\n",
            "          value: dev.example.com\n",
            "          description: Variable note\n",
        ),
    )
    .unwrap();

    let mut loaded = load_workspace(&path).unwrap();
    assert!(loaded.workspace().environments()[1].description.is_none());
    assert!(matches!(
        loaded
            .update_environment_description("development", &FieldPatch::Unchanged)
            .unwrap_err(),
        SaveError::EmptyUpdate
    ));

    loaded
        .update_environment_description(
            "development",
            &FieldPatch::Set(Documentation::Text("Local development".to_owned())),
        )
        .unwrap();
    assert_eq!(
        loaded.workspace().environments()[1].description,
        Some(Documentation::Text("Local development".to_owned()))
    );
    loaded
        .update_environment_variable("development", "host", "local.example.com".to_owned())
        .unwrap();

    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(
        reloaded.workspace().environments()[1].description,
        Some(Documentation::Text("Local development".to_owned()))
    );
    assert_eq!(
        resolved_variable(&reloaded, "development", "host").as_deref(),
        Some("local.example.com")
    );
    let document = yaml(&path);
    let environment = &document["config"]["environments"][1];
    assert_eq!(
        environment["description"].as_str(),
        Some("Local development")
    );
    assert_eq!(environment["vendor.example"].as_str(), Some("retained"));
    assert_eq!(
        environment["variables"][0]["description"].as_str(),
        Some("Variable note")
    );

    let mut loaded = load_workspace(&path).unwrap();
    loaded
        .update_environment_description(
            "development",
            &FieldPatch::Set(Documentation::Content {
                content: "Local development".to_owned(),
                media_type: "text/markdown".to_owned(),
            }),
        )
        .unwrap();
    let environment = &yaml(&path)["config"]["environments"][1];
    assert_eq!(
        environment["description"]["content"].as_str(),
        Some("Local development")
    );
    assert_eq!(
        environment["description"]["type"].as_str(),
        Some("text/markdown")
    );
    assert_eq!(
        load_workspace(&path).unwrap().workspace().environments()[1].description,
        Some(Documentation::Content {
            content: "Local development".to_owned(),
            media_type: "text/markdown".to_owned(),
        })
    );

    let mut loaded = load_workspace(&path).unwrap();
    loaded
        .update_environment_description("development", &FieldPatch::Set(Documentation::Null))
        .unwrap();
    assert!(yaml(&path)["config"]["environments"][1]["description"].is_null());
    assert_eq!(
        load_workspace(&path).unwrap().workspace().environments()[1].description,
        Some(Documentation::Null)
    );

    let mut loaded = load_workspace(&path).unwrap();
    loaded
        .update_environment_description("development", &FieldPatch::Clear)
        .unwrap();
    let environment = &yaml(&path)["config"]["environments"][1];
    assert!(environment.get("description").is_none());
    assert_eq!(environment["vendor.example"].as_str(), Some("retained"));
    assert_eq!(
        environment["variables"][0]["description"].as_str(),
        Some("Variable note")
    );
    assert_eq!(
        environment["variables"][0]["value"].as_str(),
        Some("local.example.com")
    );
    assert!(
        load_workspace(&path).unwrap().workspace().environments()[1]
            .description
            .is_none()
    );
    assert!(matches!(
        loaded
            .update_environment_description("missing", &FieldPatch::Clear)
            .unwrap_err(),
        SaveError::Environment(EnvironmentResolutionError::EnvironmentNotFound(_))
    ));
    fs::remove_file(path).unwrap();
}

#[test]
fn prepared_environment_description_updates_memory_only_after_completion() {
    let path = temporary_path("env-description-prepared.yml");
    fs::write(
        &path,
        concat!(
            "opencollection: 1.0.0\n",
            "info:\n  name: Prepared description\n",
            "bundled: true\n",
            "config:\n",
            "  environments:\n",
            "    - name: development\n",
            "      vendor.example: retained\n",
            "      variables:\n",
            "        - name: host\n",
            "          value: dev.example.com\n",
            "          description: Variable note\n",
        ),
    )
    .unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let unchanged = loaded
        .prepare_environment_description("development", &FieldPatch::Unchanged)
        .unwrap_err();
    assert!(matches!(unchanged, SaveError::EmptyUpdate));
    let missing = loaded
        .prepare_environment_description(
            "missing",
            &FieldPatch::Set(Documentation::Text("Local development".to_owned())),
        )
        .unwrap_err();
    assert!(matches!(
        missing,
        SaveError::Environment(EnvironmentResolutionError::EnvironmentNotFound(_))
    ));

    let text = FieldPatch::Set(Documentation::Text("Local development".to_owned()));
    let prepared = loaded
        .prepare_environment_description("development", &text)
        .unwrap();
    assert!(loaded.workspace().environments()[0].description.is_none());
    let saved = prepared.execute().unwrap();
    loaded
        .complete_environment_description("development", &text, saved)
        .unwrap();
    assert_eq!(
        loaded.workspace().environments()[0].description,
        Some(Documentation::Text("Local development".to_owned()))
    );
    assert_eq!(
        environment_description(&path, "development"),
        Some(Documentation::Text("Local development".to_owned()))
    );
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("vendor.example: retained"));
    assert!(source.contains("description: Variable note"));

    let cleared = FieldPatch::Set(Documentation::Content {
        content: String::new(),
        media_type: "text/markdown".to_owned(),
    });
    let prepared = loaded
        .prepare_environment_description("development", &cleared)
        .unwrap();
    let saved = prepared.execute().unwrap();
    loaded
        .complete_environment_description("development", &cleared, saved)
        .unwrap();
    assert_eq!(
        environment_description(&path, "development"),
        Some(Documentation::Content {
            content: String::new(),
            media_type: "text/markdown".to_owned(),
        })
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn unbundled_environment_description_set_and_unset_round_trip() {
    let root = temporary_path("unbundled-env-description");
    copy_directory(&fixture("unbundled"), &root);
    let mut loaded = load_workspace(&root).unwrap();
    loaded
        .update_environment_description(
            "development",
            &FieldPatch::Set(Documentation::Text("Child environment".to_owned())),
        )
        .unwrap();
    let path = root.join("environments/development.yml");
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("description: Child environment"));
    assert!(saved.contains("color: green"));
    assert_eq!(
        environment_description(&root, "development"),
        Some(Documentation::Text("Child environment".to_owned()))
    );

    loaded
        .update_environment_description("development", &FieldPatch::Clear)
        .unwrap();
    let document = yaml(&path);
    assert!(document.get("description").is_none());
    assert_eq!(document["color"].as_str(), Some("green"));
    assert_eq!(document["name"].as_str(), Some("development"));
    assert_eq!(environment_description(&root, "development"), None);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_environment_description_fails_to_load() {
    let error = load_workspace_from_str(concat!(
        "opencollection: 1.0.0\n",
        "info:\n  name: Bad description\n",
        "bundled: true\n",
        "config:\n",
        "  environments:\n",
        "    - name: development\n",
        "      description: 1\n",
    ))
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("documentation must be a string"),
        "{message}"
    );
}

#[test]
fn bundled_environment_set_unset_save_reload_preserves_unknown_fields() {
    let path = temporary_path("bundled-env.yml");
    fs::write(
        &path,
        concat!(
            "opencollection: 1.0.0\n",
            "info:\n  name: Env persist\n",
            "bundled: true\n",
            "config:\n",
            "  environments:\n",
            "    - name: base\n",
            "      vendor.example: retained-base\n",
            "      variables:\n",
            "        - name: host\n",
            "          value: api.example.com\n",
            "          description: Canonical host\n",
            "        - name: region\n",
            "          value:\n",
            "            - title: AU\n",
            "              selected: true\n",
            "              value: au\n",
            "              note: default\n",
            "            - title: US\n",
            "              value: us\n",
            "    - name: development\n",
            "      extends: base\n",
            "      variables:\n",
            "        - name: host\n",
            "          value: dev.example.com\n",
            "        - name: token\n",
            "          value: development-token\n",
            "items:\n",
            "  - info:\n",
            "      name: Health\n",
            "      type: http\n",
            "    http:\n",
            "      method: GET\n",
            "      url: https://example.com/health\n",
        ),
    )
    .unwrap();
    let mut loaded = load_workspace(&path).unwrap();

    loaded
        .update_environment_variable("development", "host", "local.example.com".to_owned())
        .unwrap();
    loaded
        .update_environment_variable("development", "baseUrl", "https://local.example".to_owned())
        .unwrap();
    loaded
        .update_environment_variable("base", "region", "nz".to_owned())
        .unwrap();
    loaded
        .unset_environment_variable("development", "host")
        .unwrap();

    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(
        resolved_variable(&reloaded, "development", "host").as_deref(),
        Some("api.example.com")
    );
    assert_eq!(
        resolved_variable(&reloaded, "development", "baseUrl").as_deref(),
        Some("https://local.example")
    );
    assert_eq!(
        resolved_variable(&reloaded, "base", "region").as_deref(),
        Some("nz")
    );
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("vendor.example: retained-base"));
    assert!(saved.contains("description: Canonical host"));
    assert!(saved.contains("note: default"));
    assert!(saved.contains("title: US"));
    fs::remove_file(path).unwrap();
}

#[test]
fn unbundled_environment_set_unset_save_reload_preserves_unknown_fields() {
    let root = temporary_path("unbundled-env");
    copy_directory(&fixture("unbundled"), &root);
    fs::write(
        root.join("environments/base.yml"),
        concat!(
            "name: base\n",
            "variables:\n",
            "  - name: host\n",
            "    value: api.example.com\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join("environments/development.yml"),
        concat!(
            "name: development\n",
            "extends: base\n",
            "color: green\n",
            "vendor.example: retained-env\n",
            "variables:\n",
            "  - name: host\n",
            "    value: dev.example.com\n",
            "    description: Child host\n",
            "  - name: baseUrl\n",
            "    value: https://{{host}}\n",
        ),
    )
    .unwrap();

    let mut loaded = load_workspace(&root).unwrap();
    loaded
        .update_environment_variable("development", "host", "local.example.com".to_owned())
        .unwrap();
    loaded
        .unset_environment_variable("development", "host")
        .unwrap();
    loaded
        .update_environment_variable("development", "token", "dev-token".to_owned())
        .unwrap();

    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(
        resolved_variable(&reloaded, "development", "host").as_deref(),
        Some("api.example.com")
    );
    assert_eq!(
        resolved_variable(&reloaded, "development", "token").as_deref(),
        Some("dev-token")
    );
    let saved = fs::read_to_string(root.join("environments/development.yml")).unwrap();
    assert!(saved.contains("vendor.example: retained-env"));
    assert!(saved.contains("color: green"));
    let base = fs::read_to_string(root.join("environments/base.yml")).unwrap();
    assert!(base.contains("value: api.example.com"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn environment_update_refuses_externally_modified_document() {
    let path = temporary_path("env-conflict.yml");
    fs::copy(fixture("phase4-environments.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let mut external = fs::read_to_string(&path).unwrap();
    external.push_str("external: true\n");
    fs::write(&path, &external).unwrap();

    let error = loaded
        .update_environment_variable("development", "token", "rotated".to_owned())
        .expect_err("external modification should be rejected");
    assert!(matches!(error, SaveError::ConcurrentModification(_)));
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
    assert_eq!(
        resolved_variable(&loaded, "development", "token").as_deref(),
        Some("rotated")
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn environment_replace_preserves_unknown_fields_and_edits_secret_declarations() {
    let path = temporary_path("env-replace.yml");
    fs::copy(fixture("phase4-environments.yml"), &path).unwrap();
    let source = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
    let with_description = source.replacen(
        "    - name: development\n      extends: base\n",
        "    - name: development\n      extends: base\n      description:\n        content: Staging notes\n        type: text/markdown\n",
        1,
    );
    assert_ne!(with_description, source);
    fs::write(&path, with_description).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let mut replacement = loaded.workspace().environments()[1].clone();
    replacement.description = None;
    replacement.extends = None;
    replacement.variables.retain(|variable| match variable {
        probe_core::EnvironmentVariable::Plain(variable) => {
            variable.name.as_deref() != Some("host")
        }
        probe_core::EnvironmentVariable::Secret(_) => true,
    });
    replacement
        .variables
        .push(probe_core::EnvironmentVariable::Plain(
            probe_core::Variable {
                name: Some("region".to_owned()),
                value: Some(probe_core::VariableValueSet::Single(
                    probe_core::VariableValue::String("ap-southeast-2".to_owned()),
                )),
                disabled: true,
            },
        ));

    let prepared = loaded
        .prepare_environment_replace("development", replacement)
        .unwrap();
    let saved = prepared.execute().unwrap();
    loaded.complete_environment_replace(saved).unwrap();

    let source = fs::read_to_string(&path).unwrap();
    assert!(!source.contains("extends: base"));
    assert!(source.contains("content: Staging notes"));
    assert!(source.contains("type: text/markdown"));
    assert!(source.contains("name: region"));
    assert!(source.contains("disabled: true"));
    assert!(source.contains("secret: true"));
    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(
        loaded.workspace().environments()[1],
        reloaded.workspace().environments()[1]
    );
    assert_eq!(reloaded.workspace().environments()[1].extends, None);
    assert_eq!(
        reloaded.workspace().environments()[1].description,
        Some(Documentation::Content {
            content: "Staging notes".to_owned(),
            media_type: "text/markdown".to_owned(),
        })
    );
    assert!(reloaded.workspace().environments()[1].variables.iter().all(
        |variable| match variable {
            probe_core::EnvironmentVariable::Plain(variable) => {
                variable.name.as_deref() != Some("host")
            }
            probe_core::EnvironmentVariable::Secret(_) => true,
        }
    ));

    let mut base = loaded.workspace().environments()[0].clone();
    base.variables
        .retain(|variable| matches!(variable, probe_core::EnvironmentVariable::Plain(_)));
    let prepared = loaded.prepare_environment_replace("base", base).unwrap();
    let saved = prepared.execute().unwrap();
    loaded.complete_environment_replace(saved).unwrap();
    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(
        loaded.workspace().environments()[0],
        reloaded.workspace().environments()[0]
    );
    assert!(
        !reloaded.workspace().environments()[0]
            .variables
            .iter()
            .any(|variable| matches!(variable,
            probe_core::EnvironmentVariable::Secret(secret)
                if secret.name.as_deref() == Some("secretToken")))
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn environment_replace_adds_and_renames_secret_declarations_without_values() {
    let path = temporary_path("env-secret-edit.yml");
    fs::copy(fixture("phase4-environments.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let mut development = loaded.workspace().environments()[1].clone();
    development
        .variables
        .push(probe_core::EnvironmentVariable::Secret(
            probe_core::SecretVariable {
                name: Some("signingKey".to_owned()),
                value_type: None,
                disabled: false,
            },
        ));
    let saved = loaded
        .prepare_environment_replace("development", development)
        .unwrap()
        .execute()
        .unwrap();
    loaded.complete_environment_replace(saved).unwrap();
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("name: signingKey"));
    assert!(source.contains("secret: true"));
    assert!(!source.contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
    let mut reloaded = load_workspace(&path).unwrap();
    let mut development = reloaded.workspace().environments()[1].clone();
    let secret = development
        .variables
        .iter_mut()
        .find_map(|variable| match variable {
            probe_core::EnvironmentVariable::Secret(secret)
                if secret.name.as_deref() == Some("signingKey") =>
            {
                Some(secret)
            }
            _ => None,
        })
        .unwrap();
    secret.name = Some("newSigningKey".to_owned());
    secret.disabled = true;
    let saved = reloaded
        .prepare_environment_replace("development", development)
        .unwrap()
        .execute()
        .unwrap();
    reloaded.complete_environment_replace(saved).unwrap();
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("name: newSigningKey"));
    assert!(!source.contains("name: signingKey"));
    assert!(!source.contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
    let again = load_workspace(&path).unwrap();
    assert_eq!(
        again.workspace().environments()[1],
        reloaded.workspace().environments()[1]
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn environment_replace_rejects_variable_name_collisions_without_writing() {
    let path = temporary_path("env-replace-collisions.yml");
    fs::copy(fixture("phase4-environments.yml"), &path).unwrap();
    let original = fs::read_to_string(&path).unwrap();
    let loaded = load_workspace(&path).unwrap();
    let base = loaded.workspace().environments()[0].clone();
    let mut replacement = base.clone();
    replacement.variables.retain(|variable| {
        !matches!(
            variable,
            probe_core::EnvironmentVariable::Secret(secret)
                if secret.name.as_deref() == Some("secretToken")
        )
    });
    replacement
        .variables
        .push(probe_core::EnvironmentVariable::Plain(
            probe_core::Variable {
                name: Some("secretToken".to_owned()),
                value: Some(probe_core::VariableValueSet::Single(
                    probe_core::VariableValue::String("plain".to_owned()),
                )),
                disabled: false,
            },
        ));

    let error = loaded
        .prepare_environment_replace("base", replacement)
        .unwrap_err();
    assert!(
        matches!(
            error,
            SaveError::Environment(EnvironmentResolutionError::DuplicateVariable {
                ref environment,
                ref variable,
            }) if environment == "base" && variable == "secretToken"
        ),
        "{error:?}"
    );
    assert_eq!(loaded.workspace().environments()[0], base);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    fs::remove_file(path).unwrap();
}

#[test]
fn bundled_environment_delete_updates_following_indices() {
    let path = temporary_path("env-delete.yml");
    fs::copy(fixture("phase4-environments.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let prepared = loaded.prepare_environment_delete("development").unwrap();
    let saved = prepared.execute().unwrap();
    loaded.complete_environment_delete(saved).unwrap();
    assert!(
        loaded
            .workspace()
            .environments()
            .iter()
            .all(|environment| environment.name != "development")
    );
    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(reloaded.workspace().environments().len(), 1);
    fs::remove_file(path).unwrap();
}

#[test]
fn unbundled_environment_delete_removes_only_the_environment_document() {
    let root = temporary_path("unbundled-env-delete");
    copy_directory(&fixture("unbundled"), &root);
    fs::write(
        root.join("environments/development.yml"),
        "name: development\nvariables:\n  - name: token\n    value: dev\n",
    )
    .unwrap();
    let mut loaded = load_workspace(&root).unwrap();
    let prepared = loaded.prepare_environment_delete("development").unwrap();
    let saved = prepared.execute().unwrap();
    loaded.complete_environment_delete(saved).unwrap();
    assert!(!root.join("environments/development.yml").exists());
    assert!(root.join("opencollection.yml").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unbundled_environment_rename_moves_the_document() {
    let root = temporary_path("unbundled-env-rename");
    copy_directory(&fixture("unbundled"), &root);
    let mut loaded = load_workspace(&root).unwrap();
    let mut replacement = loaded
        .workspace()
        .environments()
        .iter()
        .find(|environment| environment.name == "development")
        .cloned()
        .unwrap();
    replacement.name = "staging".to_owned();
    let prepared = loaded
        .prepare_environment_replace("development", replacement)
        .unwrap();
    let saved = prepared.execute().unwrap();
    loaded.complete_environment_replace(saved).unwrap();

    assert!(!root.join("environments/development.yml").exists());
    assert!(root.join("environments/staging.yml").exists());
    let saved = fs::read_to_string(root.join("environments/staging.yml")).unwrap();
    assert!(saved.contains("name: staging"));
    assert!(saved.contains("color: green"));
    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(
        loaded
            .workspace()
            .environments()
            .iter()
            .map(|environment| environment.name.as_str())
            .collect::<Vec<_>>(),
        reloaded
            .workspace()
            .environments()
            .iter()
            .map(|environment| environment.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        reloaded
            .workspace()
            .environments()
            .iter()
            .any(|environment| environment.name == "staging")
    );
    loaded
        .create_environment("development".to_owned(), None)
        .unwrap();
    assert!(root.join("environments/development.yml").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unbundled_environment_rename_refuses_an_existing_destination() {
    let root = temporary_path("unbundled-env-rename-conflict");
    copy_directory(&fixture("unbundled"), &root);
    let loaded = load_workspace(&root).unwrap();
    let original = fs::read_to_string(root.join("environments/development.yml")).unwrap();
    fs::write(
        root.join("environments/staging.yml"),
        "name: staging\nvariables:\n  - name: token\n    value: staging\n",
    )
    .unwrap();
    let mut replacement = loaded
        .workspace()
        .environments()
        .iter()
        .find(|environment| environment.name == "development")
        .cloned()
        .unwrap();
    replacement.name = "staging".to_owned();
    let error = loaded
        .prepare_environment_replace("development", replacement)
        .unwrap()
        .execute()
        .unwrap_err();
    assert!(matches!(error, SaveError::ConcurrentModification(_)));
    assert_eq!(
        fs::read_to_string(root.join("environments/development.yml")).unwrap(),
        original
    );
    assert_eq!(
        loaded
            .workspace()
            .environments()
            .iter()
            .find(|environment| environment.name == "development")
            .map(|environment| environment.name.as_str()),
        Some("development")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn environment_update_rejects_secrets_and_missing_environments() {
    let path = temporary_path("env-secrets.yml");
    fs::copy(fixture("phase4-environments.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();

    let secret = loaded
        .update_environment_variable("development", "secretToken", "nope".to_owned())
        .unwrap_err();
    assert!(matches!(
        secret,
        SaveError::Environment(EnvironmentResolutionError::SecretVariableUnavailable(_))
    ));
    let missing = loaded
        .update_environment_variable("production", "host", "nope".to_owned())
        .unwrap_err();
    assert!(matches!(
        missing,
        SaveError::Environment(EnvironmentResolutionError::EnvironmentNotFound(_))
    ));
    let unset_missing = loaded
        .unset_environment_variable("development", "baseUrl")
        .unwrap_err();
    assert!(matches!(
        unset_missing,
        SaveError::Environment(EnvironmentResolutionError::VariableNotFound { .. })
    ));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        fs::read_to_string(fixture("phase4-environments.yml")).unwrap()
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn environment_update_rejects_stdin_workspaces() {
    let source = fs::read_to_string(fixture("phase4-environments.yml")).unwrap();
    let mut loaded = load_workspace_from_str(&source).unwrap();
    let error = loaded
        .update_environment_variable("development", "token", "rotated".to_owned())
        .unwrap_err();
    assert!(matches!(error, SaveError::ReadOnlySource));
    assert_eq!(
        resolved_variable(&loaded, "development", "token").as_deref(),
        Some("rotated")
    );
}

#[test]
fn environment_replace_updates_plain_and_existing_secret_without_leaking_value() {
    let path = temporary_path("env-secret-update.yml");
    let fixture_text = fs::read_to_string(fixture("phase4-environments.yml"))
        .unwrap()
        .replace("\r\n", "\n");
    let declaration = "name: secretToken\n          type: string";
    assert!(
        fixture_text.contains(declaration),
        "the fixture secret declaration should be present before adding an unknown field"
    );
    fs::write(
        &path,
        fixture_text.replace(
            declaration,
            "name: secretToken\n          type: string\n          x-extra: keep",
        ),
    )
    .unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let mut base = loaded.workspace().environments()[0].clone();
    for variable in &mut base.variables {
        match variable {
            probe_core::EnvironmentVariable::Plain(plain)
                if plain.name.as_deref() == Some("host") =>
            {
                plain.value = Some(probe_core::VariableValueSet::Single(
                    probe_core::VariableValue::String("changed.example".into()),
                ));
            }
            probe_core::EnvironmentVariable::Secret(secret)
                if secret.name.as_deref() == Some("secretToken") =>
            {
                secret.disabled = true;
            }
            _ => {}
        }
    }
    let saved = loaded
        .prepare_environment_replace("base", base)
        .unwrap()
        .execute()
        .unwrap();
    loaded.complete_environment_replace(saved).unwrap();
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("changed.example"));
    assert!(source.contains("x-extra: keep"));
    assert!(!source.contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(
        loaded.workspace().environments()[0],
        reloaded.workspace().environments()[0]
    );
    assert!(reloaded.workspace().environments()[0].variables.iter().any(
        |variable| matches!(variable,
        probe_core::EnvironmentVariable::Secret(secret)
            if secret.name.as_deref() == Some("secretToken") && secret.disabled)
    ));
    fs::remove_file(path).unwrap();
}

#[test]
fn environment_replace_plain_with_secret_removes_plaintext_value() {
    let path = temporary_path("env-plain-to-secret.yml");
    fs::copy(fixture("phase4-environments.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let mut development = loaded.workspace().environments()[1].clone();
    let token = development
        .variables
        .iter_mut()
        .find(|variable| {
            matches!(variable,
        probe_core::EnvironmentVariable::Plain(plain) if plain.name.as_deref() == Some("token"))
        })
        .unwrap();
    *token = probe_core::EnvironmentVariable::Secret(probe_core::SecretVariable {
        name: Some("token".to_owned()),
        value_type: None,
        disabled: false,
    });
    let saved = loaded
        .prepare_environment_replace("development", development)
        .unwrap()
        .execute()
        .unwrap();
    loaded.complete_environment_replace(saved).unwrap();
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("secret: true"));
    assert!(!source.contains("development-token"));
    assert!(!source.contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(
        reloaded.workspace().environments()[1],
        loaded.workspace().environments()[1]
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn environment_replace_and_delete_reject_wrong_loaded_generation() {
    for delete in [false, true] {
        let path = temporary_path(if delete {
            "env-wrong-delete.yml"
        } else {
            "env-wrong-replace.yml"
        });
        fs::copy(fixture("phase4-environments.yml"), &path).unwrap();
        let original = load_workspace(&path).unwrap();
        let mut reloaded;
        if delete {
            let saved = original
                .prepare_environment_delete("development")
                .unwrap()
                .execute()
                .unwrap();
            reloaded = load_workspace(&path).unwrap();
            let before = reloaded.workspace().environments().to_vec();
            assert!(matches!(
                reloaded.complete_environment_delete(saved),
                Err(SaveError::CommittedButNotIntegrated)
            ));
            assert_eq!(reloaded.workspace().environments(), before);
        } else {
            let mut replacement = original.workspace().environments()[1].clone();
            replacement.extends = None;
            let saved = original
                .prepare_environment_replace("development", replacement)
                .unwrap()
                .execute()
                .unwrap();
            reloaded = load_workspace(&path).unwrap();
            let before = reloaded.workspace().environments().to_vec();
            assert!(matches!(
                reloaded.complete_environment_replace(saved),
                Err(SaveError::CommittedButNotIntegrated)
            ));
            assert_eq!(reloaded.workspace().environments(), before);
        }
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn environment_variable_save_rejects_advanced_baseline_before_writing() {
    let path = temporary_path("env-save-advanced-baseline.yml");
    fs::copy(fixture("phase4-environments.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    loaded
        .set_environment_variable("development", "token", "rotated".to_owned())
        .unwrap();
    let stale = loaded
        .prepare_environment_variable_save("development", "token")
        .unwrap();
    let create = loaded
        .prepare_environment_create("staging".to_owned(), None)
        .unwrap();
    loaded
        .complete_environment_create(create.execute().unwrap())
        .unwrap();
    let before = fs::read(&path).unwrap();
    assert!(matches!(stale.execute(), Err(SaveError::StaleCompletion)));
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_file(path).unwrap();
}

#[test]
fn synchronous_environment_replace_and_delete_refresh_memory_and_disk() {
    let path = temporary_path("sync-environment-completion.yml");
    fs::copy(fixture("phase4-environments.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let mut replacement = loaded.workspace().environments()[1].clone();
    replacement.extends = None;
    loaded
        .replace_environment("development", replacement.clone())
        .unwrap();
    assert_eq!(loaded.workspace().environments()[1], replacement);
    assert_eq!(
        load_workspace(&path).unwrap().workspace().environments()[1],
        replacement
    );

    loaded.delete_environment("development").unwrap();
    assert_eq!(loaded.workspace().environments().len(), 1);
    assert_eq!(
        load_workspace(&path)
            .unwrap()
            .workspace()
            .environments()
            .len(),
        1
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn stale_completion_error_has_a_diagnostic_and_io_errors_keep_their_source() {
    assert!(
        SaveError::StaleCompletion
            .to_string()
            .contains("repository changed")
    );
    let error = SaveError::Io {
        path: temporary_path("io-error.yml").to_path_buf(),
        source: std::io::Error::other("write failed"),
    };
    assert!(std::error::Error::source(&error).is_some());
}
