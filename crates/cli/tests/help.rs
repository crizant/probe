use std::process::Command;

#[test]
fn help_starts_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_probe"))
        .arg("--help")
        .output()
        .expect("probe binary should start");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: probe"));
    assert!(output.stderr.is_empty());
}
