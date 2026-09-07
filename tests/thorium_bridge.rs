use timetoll::{
    bridge::parse_report,
    config::{Config, Target},
    engine::{Activity, Ledger, Policy},
};

#[test]
fn thorium_reports_reach_website_policy() {
    let config = Config {
        blocked: vec![Target::Site("example.com".into())],
        whitelist: vec!["https://example.com/learn".into()],
        ..Config::default()
    };
    let policy = Policy::new(&config).unwrap();
    for (url, blocked) in [
        ("https://example.com", true),
        ("https://example.com/learn", false),
        ("chrome://newtab/", false),
    ] {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "app": "org.chromium.Thorium", "url": url
        }))
        .unwrap();
        let report = parse_report(&bytes, &config.browsers)
            .expect("the bridge must accept the macOS Thorium identifier");
        assert_eq!(
            Ledger::default().blocked(&policy.classify(&report.app, Some(&report.url))),
            blocked
        );
    }
}

#[test]
fn thorium_without_a_fresh_report_requires_connection() {
    let config = Config {
        blocked: vec![Target::Site("example.com".into())],
        ..Config::default()
    };
    assert_eq!(
        Policy::new(&config)
            .unwrap()
            .classify("org.chromium.Thorium", None),
        Activity::UnknownBrowser
    );
}
