use std::process::Command;

/// Proves the library, the binary and the test harness are wired together.
#[test]
fn binary_reports_the_library_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_dbctx"))
        .output()
        .expect("dbctx binary runs");

    assert!(
        output.status.success(),
        "dbctx exited with {}",
        output.status
    );

    let stdout = String::from_utf8(output.stdout).expect("dbctx writes UTF-8");
    assert_eq!(stdout.trim(), format!("dbctx {}", dbctx::VERSION));
}
