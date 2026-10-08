#[allow(dead_code, unused_imports)]
mod common;

use common::*;

#[test]
fn agent_help_and_unsupported_arguments() {
    let output = probe().args(["agent", "--help"]).output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).contains("install [--force] [--json]"));
    for args in [
        vec!["agent"],
        vec!["agent", "unknown"],
        vec!["agent", "install"],
        vec!["agent", "skill"],
        vec!["agent", "skill", "unknown"],
        vec!["agent", "skill", "install", "--unknown"],
        vec!["agent", "skill", "install", "extra"],
    ] {
        let output = probe().args(args).arg("--json").output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        let json: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["error"]["category"], "invalid_arguments");
    }
}

// BaseDirs uses HOME on Unix. Change only the child process environment; Windows
// uses Known Folders and exercises the injectable home through the unit tests.
#[cfg(unix)]
#[test]
fn moved_binary_installs_offline_without_a_repository_or_working_directory_dependency() {
    let home = temporary_path("agent-home");
    fs::create_dir(&home).unwrap();
    let binary = home.join("probe");
    fs::copy(env!("CARGO_BIN_EXE_probe"), &binary).unwrap();
    let invoke = |force: bool| {
        let mut command = Command::new(&binary);
        command
            .current_dir(&home)
            .env("HOME", &home)
            .args(["agent", "skill", "install", "--json"]);
        if force {
            command.arg("--force");
        }
        command.output().unwrap()
    };
    let skill = home.join(".agents/skills/probe");
    for installed in [true, false] {
        let output = invoke(false);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(output.stderr.is_empty());
        let json: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "schemaVersion": 1,
                "installed": installed,
                "path": skill,
                "version": env!("CARGO_PKG_VERSION"),
            })
        );
    }
    assert_eq!(
        fs::read(skill.join("SKILL.md")).unwrap(),
        include_bytes!("../../../.agents/skills/probe/SKILL.md")
    );
    assert_eq!(
        fs::read(skill.join("references/cli.md")).unwrap(),
        include_bytes!("../../../docs/CLI.md")
    );
    fs::write(skill.join("SKILL.md"), "modified").unwrap();
    let output = invoke(false);
    assert_eq!(output.status.code(), Some(7));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error"]["category"], "agent_skill_exists");
    assert!(invoke(true).status.success());
    assert_eq!(
        fs::read(skill.join("SKILL.md")).unwrap(),
        include_bytes!("../../../.agents/skills/probe/SKILL.md")
    );
    fs::remove_dir_all(home).unwrap();
}
