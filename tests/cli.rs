use std::process::Command;

fn run(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_timetoll"))
        .arg("--data-dir")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn configure_and_inspect_from_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["init"],
        vec!["block", "add", "site", "example.com"],
        vec!["earn", "add", "app", "editor.exe"],
        vec!["whitelist", "add", "https://example.com/learn/*"],
        vec!["ratio", "--earn", "20", "--unlock", "5"],
    ] {
        let output = run(dir.path(), &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let config = run(dir.path(), &["config"]);
    let text = String::from_utf8(config.stdout).unwrap();
    assert!(text.contains("example.com"));
    assert!(text.contains("earn_seconds = 1200"));
    assert!(text.contains("<redacted; use timetoll pair>"));
    let output = run(dir.path(), &["status", "--json"]);
    let state: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(state["balance_ms"], 0);
    assert_eq!(state["ratio"]["unlock_seconds"], 300);
    assert!(
        !run(dir.path(), &["ratio", "--earn", "0", "--unlock", "5"])
            .status
            .success()
    );
    assert!(
        !run(dir.path(), &["block", "add", "site", "*.example.com"])
            .status
            .success()
    );
    assert!(
        run(dir.path(), &["block", "remove", "site", "example.com"])
            .status
            .success()
    );
    assert!(
        !run(dir.path(), &["block", "remove", "site", "example.com"])
            .status
            .success()
    );
}
