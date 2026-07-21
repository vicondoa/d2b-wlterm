use std::process::Command;

#[test]
fn binary_reports_version_with_success() {
    let output = Command::new(env!("CARGO_BIN_EXE_d2b-wlterm"))
        .arg("--version")
        .output()
        .expect("run d2b-wlterm");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"2.0.0\n");
}

#[test]
fn binary_rejects_unknown_command_with_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_d2b-wlterm"))
        .arg("unknown")
        .output()
        .expect("run d2b-wlterm");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command"));
}
