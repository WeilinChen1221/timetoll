use timetoll::{
    bridge::{BrowserState, parse_report},
    config::{Config, Ratio, SiteRule, Target},
    engine::{Activity, Ledger, Policy, account_interval},
    storage::Store,
};
use url::Url;

fn matches(rule: &str, url: &str) -> bool {
    SiteRule::parse(rule)
        .unwrap()
        .matches(&Url::parse(url).unwrap())
}

#[test]
fn domain_rules_respect_dns_boundaries_and_normalize_hosts() {
    for url in [
        "https://example.com",
        "http://www.example.com/path",
        "https://EXAMPLE.COM./",
    ] {
        assert!(matches("example.com", url), "{url}");
    }
    for url in [
        "https://notexample.com",
        "https://example.com.evil.test",
        "https://example.com@evil.test",
        "file:///example.com",
    ] {
        assert!(!matches("example.com", url), "{url}");
    }
    assert!(matches("bücher.de", "https://xn--bcher-kva.de/"));
}

#[test]
fn page_and_subtree_whitelists_do_not_match_siblings() {
    assert!(matches(
        "https://example.com/learn",
        "https://example.com/learn?topic=rust#chapter"
    ));
    assert!(!matches(
        "https://example.com/learn",
        "https://example.com/learn/more"
    ));
    assert!(!matches(
        "https://example.com/learn",
        "https://sub.example.com/learn"
    ));
    assert!(!matches(
        "https://example.com/learn",
        "http://example.com/learn"
    ));
    assert!(!matches(
        "https://example.com/learn",
        "https://example.com:444/learn"
    ));
    for path in ["learn", "learn/", "learn/rust"] {
        assert!(matches(
            "https://example.com/learn/*",
            &format!("https://example.com/{path}")
        ));
    }
    for path in [
        "learning",
        "learn/../games",
        "learn/%2e%2e/games",
        "learn%2Fgames",
    ] {
        assert!(!matches(
            "https://example.com/learn/*",
            &format!("https://example.com/{path}")
        ));
    }
}

#[test]
fn query_specific_pages_and_fragments() {
    assert!(matches(
        "https://youtube.com/watch?v=lesson",
        "https://youtube.com/watch?v=lesson#part2"
    ));
    assert!(!matches(
        "https://youtube.com/watch?v=lesson",
        "https://youtube.com/watch?v=game"
    ));
    assert!(!matches(
        "https://youtube.com/watch?v=lesson",
        "https://youtube.com/watch?v=lesson&list=games"
    ));
}

#[test]
fn invalid_rules_fail_before_persisting() {
    for rule in [
        "",
        " example.com",
        "*.example.com",
        "example.com/path",
        "ftp://example.com",
        "https://user:password@example.com",
        "https://example.com/*/games",
        "https://example.com/?x=/*",
    ] {
        assert!(SiteRule::parse(rule).is_err(), "{rule}");
    }
    for app in ["", "/Applications/Chrome.app", "chrome*", "chrome.exe\n"] {
        assert!(Target::App(app.into()).validate().is_err());
    }
}

fn config() -> Config {
    Config {
        blocked: vec![
            Target::Site("example.com".into()),
            Target::App("games.exe".into()),
        ],
        earning: vec![
            Target::Site("https://example.com/learn/*".into()),
            Target::App("editor.exe".into()),
        ],
        whitelist: vec!["https://example.com/learn/*".into()],
        ..Config::default()
    }
}

#[test]
fn whitelist_overrides_site_blocks_and_can_earn() {
    let policy = Policy::new(&config()).unwrap();
    assert!(matches!(
        policy.classify("chrome.exe", Some("https://example.com/learn/rust")),
        Activity::Earning(_)
    ));
    assert!(matches!(
        policy.classify("chrome.exe", Some("https://example.com/games")),
        Activity::Spending(_)
    ));
    assert_eq!(
        policy.classify("chrome.exe", Some("chrome://newtab/")),
        Activity::Neutral
    );
}

#[test]
fn blocked_app_wins_over_whitelist_and_earning_overlap() {
    let mut config = config();
    config.blocked.push(Target::App("chrome.exe".into()));
    config.earning.push(Target::App("games.exe".into()));
    let policy = Policy::new(&config).unwrap();
    assert!(matches!(
        policy.classify("CHROME.EXE", Some("https://example.com/learn/rust")),
        Activity::Spending(_)
    ));
    assert!(matches!(
        policy.classify("games.exe", None),
        Activity::Spending(_)
    ));
    assert_eq!(policy.classify("notgames.exe", None), Activity::Neutral);
}

#[test]
fn missing_or_invalid_browser_reports_fail_closed() {
    let policy = Policy::new(&config()).unwrap();
    assert_eq!(
        policy.classify("chrome.exe", None),
        Activity::UnknownBrowser
    );
    assert_eq!(
        policy.classify("chrome.exe", Some("invalid")),
        Activity::UnknownBrowser
    );
    assert_eq!(policy.classify("notes.exe", None), Activity::Neutral);
    assert!(BrowserState::default().url("chrome.exe").is_none());
    let mut config = config();
    config.blocked.clear();
    assert_eq!(
        Policy::new(&config).unwrap().classify("chrome.exe", None),
        Activity::Neutral
    );
}

#[test]
fn fifteen_minutes_earns_ten_in_complete_blocks_and_accumulates() {
    let mut ledger = Ledger::default();
    let activity = Activity::Earning("editor".into());
    ledger.advance(&activity, 899_999);
    assert_eq!(ledger.balance_ms, 0);
    ledger.advance(&activity, 1);
    assert_eq!(ledger.balance_ms, 600_000);
    assert_eq!(ledger.progress_ms, 0);
    ledger.advance(&activity, 1_850_000);
    assert_eq!(ledger.balance_ms, 1_800_000);
    assert_eq!(ledger.progress_ms, 50_000);
}

#[test]
fn only_active_blocked_usage_spends_credit() {
    let mut ledger = Ledger {
        balance_ms: 10_000,
        ..Ledger::default()
    };
    ledger.advance(&Activity::Neutral, 9000);
    ledger.advance(&Activity::UnknownBrowser, 9000);
    assert_eq!(ledger.balance_ms, 10_000);
    ledger.advance(&Activity::Spending("game".into()), 4000);
    assert_eq!(ledger.balance_ms, 6000);
    ledger.advance(&Activity::Spending("another game".into()), 9000);
    assert_eq!(ledger.balance_ms, 0);
    assert_eq!(ledger.total_spent_ms, 10_000);
    assert!(ledger.blocked(&Activity::Spending("game".into())));
    assert!(!ledger.blocked(&Activity::Neutral));
}

#[test]
fn changing_ratio_preserves_balance_but_resets_partial_progress() {
    let mut ledger = Ledger {
        balance_ms: 500,
        progress_ms: 800_000,
        ..Ledger::default()
    };
    ledger.set_ratio(Ratio {
        earn_seconds: 60,
        unlock_seconds: 120,
    });
    assert_eq!(ledger.balance_ms, 500);
    assert_eq!(ledger.progress_ms, 0);
    ledger.advance(&Activity::Earning("editor".into()), 60_000);
    assert_eq!(ledger.balance_ms, 120_500);
}

#[test]
fn suspension_idle_and_focus_transitions_are_not_accounted() {
    let mut ledger = Ledger::default();
    let earning = Activity::Earning("editor".into());
    account_interval(&mut ledger, &earning, &earning, 900_000, true);
    account_interval(&mut ledger, &earning, &earning, 250, false);
    account_interval(&mut ledger, &Activity::Neutral, &earning, 250, true);
    assert_eq!(ledger.progress_ms, 0);
    account_interval(&mut ledger, &earning, &earning, 250, true);
    assert_eq!(ledger.progress_ms, 250);
}

#[test]
fn credit_arithmetic_saturates_without_wrapping() {
    let mut ledger = Ledger {
        balance_ms: u64::MAX - 1,
        ..Ledger::default()
    };
    ledger.advance(&Activity::Earning("editor".into()), u64::MAX);
    assert_eq!(ledger.balance_ms, u64::MAX);
    assert!(ledger.progress_ms < ledger.ratio.earn_seconds * 1000);
}

#[test]
fn bridge_accepts_only_known_browsers_and_valid_urls() {
    let browsers = vec!["chrome.exe".into()];
    assert!(
        parse_report(
            br#"{"app":"CHROME.EXE","url":"https://example.com"}"#,
            &browsers
        )
        .is_ok()
    );
    for data in [
        br#"{"app":"editor.exe","url":"https://example.com"}"#.as_slice(),
        br#"{"app":"chrome.exe","url":"javascript:alert(1)"}"#,
        br#"{"app":"chrome.exe","url":"invalid"}"#,
        br#"{"app":"chrome.exe","url":"https://example.com","earned":100}"#,
    ] {
        assert!(parse_report(data, &browsers).is_err());
    }
}

#[test]
fn state_survives_restart_and_duplicate_monitors_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(Some(temp.path().into())).unwrap();
    store.init().unwrap();
    assert!(store.init().is_err());
    let lock = store.lock("monitor.lock").unwrap();
    assert!(store.lock("monitor.lock").is_err());
    drop(lock);
    assert!(store.lock("monitor.lock").is_ok());
    let ledger = Ledger {
        balance_ms: 12345,
        progress_ms: 6789,
        ..Ledger::default()
    };
    store.save_ledger(&ledger).unwrap();
    assert_eq!(store.ledger().unwrap(), ledger);
    store.save_ledger(&Ledger::default()).unwrap();
    assert_eq!(store.ledger().unwrap(), Ledger::default());
    std::fs::write(temp.path().join("state.json"), "broken").unwrap();
    assert!(store.ledger().is_err());
}

#[test]
fn config_and_state_reject_invalid_ratios_and_missing_tokens() {
    let mut config = config();
    config.ratio.earn_seconds = 0;
    assert!(config.validate().is_err());
    config.ratio.earn_seconds = u64::MAX;
    assert!(config.validate().is_err());
    let ledger = Ledger {
        progress_ms: 900_000,
        ..Ledger::default()
    };
    assert!(ledger.validate().is_err());
    let text = toml::to_string(&Config::default()).unwrap();
    let text = text
        .lines()
        .filter(|line| !line.starts_with("bridge_token"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(toml::from_str::<Config>(&text).is_err());
}
