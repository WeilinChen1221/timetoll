use timetoll::{
    config::{Config, Target},
    engine::{Activity, Policy},
};

#[test]
fn thorium_app_block_uses_the_macos_bundle_identifier() {
    let config = Config {
        blocked: vec![Target::App("org.chromium.Thorium".into())],
        ..Config::default()
    };
    let policy = Policy::new(&config).unwrap();
    assert!(matches!(
        policy.classify("org.chromium.Thorium", Some("https://example.com")),
        Activity::Spending(_)
    ));
}

#[test]
fn thorium_website_block_applies_with_default_browser_registration() {
    let config = Config {
        blocked: vec![Target::Site("example.com".into())],
        ..Config::default()
    };
    let policy = Policy::new(&config).unwrap();
    assert!(
        matches!(
            policy.classify("org.chromium.Thorium", Some("https://example.com")),
            Activity::Spending(_)
        ),
        "a blocked website in Thorium must not be classified as neutral"
    );
}
