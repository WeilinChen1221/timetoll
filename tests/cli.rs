use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

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

#[test]
fn status_text_and_json_keep_their_existing_format() {
    let dir = tempfile::tempdir().unwrap();
    let store = timetoll::storage::Store::new(Some(dir.path().to_path_buf())).unwrap();
    store.init().unwrap();
    let ledger = timetoll::engine::Ledger {
        balance_ms: 125_000,
        progress_ms: 45_000,
        ..Default::default()
    };
    store.save_ledger(&ledger).unwrap();
    let output = run(dir.path(), &["status"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Banked access: 2m 5s\nEarning progress: 0m 45s / 15m 0s\nRatio: 15m 0s earns 10m 0s\n"
    );
    assert!(output.stderr.is_empty());
    let output = run(dir.path(), &["status", "--json"]);
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<timetoll::engine::Ledger>(&output.stdout).unwrap(),
        ledger
    );
}

#[test]
fn watch_rejects_json_and_requires_valid_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), &["status", "--watch", "--json"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
    let output = run(dir.path(), &["status", "--watch"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("run timetoll init first"));
}

// Kill the watcher even if an assertion fails, so no background child is left running.
struct Watcher(Child);

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn watch(dir: &std::path::Path) -> (Watcher, Receiver<String>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_timetoll"))
        .arg("--data-dir")
        .arg(dir)
        .args(["status", "--watch"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut snapshot = String::new();
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if line.is_empty() {
                if sender.send(std::mem::take(&mut snapshot)).is_err() {
                    break;
                }
            } else {
                snapshot.push_str(&line);
                snapshot.push('\n');
            }
        }
    });
    (Watcher(child), receiver)
}

fn expect_snapshot(receiver: &Receiver<String>, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("watcher did not flush the expected snapshot within five seconds");
        assert!(
            !snapshot.contains('\x1b'),
            "piped output contains ANSI codes"
        );
        if snapshot == expected {
            return;
        }
    }
}

#[test]
fn watch_reloads_saved_state_and_ratio_without_writing_or_locking() {
    let dir = tempfile::tempdir().unwrap();
    let store = timetoll::storage::Store::new(Some(dir.path().to_path_buf())).unwrap();
    store.init().unwrap();
    let _monitor_lock = store.lock("monitor.lock").unwrap();
    let _config_lock = store.lock("config.lock").unwrap();
    let mut ledger = timetoll::engine::Ledger {
        balance_ms: 125_000,
        progress_ms: 45_000,
        ..Default::default()
    };
    store.save_ledger(&ledger).unwrap();
    let (mut watcher, receiver) = watch(dir.path());
    expect_snapshot(
        &receiver,
        "Banked access: 2m 5s\nEarning progress: 0m 45s / 15m 0s\nRatio: 15m 0s earns 10m 0s\n",
    );
    ledger.balance_ms = 61_000;
    ledger.progress_ms = 46_000;
    store.save_ledger(&ledger).unwrap();
    expect_snapshot(
        &receiver,
        "Banked access: 1m 1s\nEarning progress: 0m 46s / 15m 0s\nRatio: 15m 0s earns 10m 0s\n",
    );
    let mut config = store.config().unwrap();
    config.ratio = timetoll::config::Ratio {
        earn_seconds: 1200,
        unlock_seconds: 300,
    };
    store.save_config(&config).unwrap();
    let config_before = std::fs::read(dir.path().join("config.toml")).unwrap();
    let state_before = std::fs::read(dir.path().join("state.json")).unwrap();
    expect_snapshot(
        &receiver,
        "Banked access: 1m 1s\nEarning progress: 0m 0s / 20m 0s\nRatio: 20m 0s earns 5m 0s\n",
    );
    assert_eq!(
        std::fs::read(dir.path().join("config.toml")).unwrap(),
        config_before
    );
    assert_eq!(
        std::fs::read(dir.path().join("state.json")).unwrap(),
        state_before
    );

    std::fs::write(dir.path().join("state.json"), "corrupt").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let exit = loop {
        if let Some(exit) = watcher.0.try_wait().unwrap() {
            break exit;
        }
        assert!(
            Instant::now() < deadline,
            "watcher did not exit on corrupt state"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(!exit.success());
    let mut error = String::new();
    watcher
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut error)
        .unwrap();
    assert!(error.contains("state is corrupt"), "{error}");
}
