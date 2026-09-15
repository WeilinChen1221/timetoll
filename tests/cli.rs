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

#[test]
fn list_target_groups_without_changing_config() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["init"],
        vec!["block", "add", "site", "youtube.com"],
        vec!["block", "add", "app", "steam.exe"],
        vec!["earn", "add", "app", "Code.exe"],
        vec!["earn", "add", "site", "https://example.com/learn/*"],
        vec!["whitelist", "add", "example.org"],
    ] {
        let output = run(dir.path(), &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let config_path = dir.path().join("config.toml");
    let before = std::fs::read(&config_path).unwrap();
    let store = timetoll::storage::Store::new(Some(dir.path().to_path_buf())).unwrap();
    let _lock = store.lock("config.lock").unwrap();
    for (group, expected) in [
        ("block", "site youtube.com\napp steam.exe\n"),
        ("earn", "app Code.exe\nsite https://example.com/learn/*\n"),
    ] {
        let output = run(dir.path(), &[group, "ls"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
        assert!(output.stderr.is_empty());
        assert_eq!(std::fs::read(&config_path).unwrap(), before);
    }
}

#[test]
fn list_empty_target_groups() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run(dir.path(), &["init"]).status.success());
    for group in ["block", "earn"] {
        let output = run(dir.path(), &[group, "ls"]);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn list_target_groups_requires_configuration() {
    let dir = tempfile::tempdir().unwrap();
    for group in ["block", "earn"] {
        let output = run(dir.path(), &[group, "ls"]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("run timetoll init first"));
        assert!(!dir.path().join("config.toml").exists());
        assert!(!dir.path().join("config.lock").exists());
    }
}
