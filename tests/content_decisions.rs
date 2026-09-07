use timetoll::{
    bridge::BrowserState,
    config::{Config, Target},
    engine::{Ledger, Policy},
};

#[test]
fn content_decisions_apply_whitelists_balance_and_app_rules() {
    let mut config = Config {
        blocked: vec![Target::Site("example.com".into())],
        whitelist: vec!["https://example.com/learn/*".into()],
        ..Config::default()
    };
    let mut state = BrowserState::default();
    state.set_policy(&Policy::new(&config).unwrap(), &Ledger::default());
    assert!(
        state
            .content_decision("chrome.exe", "https://example.com/games")
            .blocked
    );
    assert!(
        !state
            .content_decision("chrome.exe", "https://example.com/learn/rust")
            .blocked
    );
    assert!(
        !state
            .content_decision("chrome.exe", "https://another.test")
            .blocked
    );
    state.set_policy(
        &Policy::new(&config).unwrap(),
        &Ledger {
            balance_ms: 1000,
            ..Ledger::default()
        },
    );
    assert!(
        !state
            .content_decision("chrome.exe", "https://example.com/games")
            .blocked
    );
    config.blocked.push(Target::App("chrome.exe".into()));
    state.set_policy(&Policy::new(&config).unwrap(), &Ledger::default());
    assert!(
        state
            .content_decision("chrome.exe", "https://example.com/learn/rust")
            .blocked
    );
}

#[test]
fn custom_titlebar_insets_are_optional_and_validated() {
    let config = Config::default();
    let mut json = serde_json::to_value(&config).unwrap();
    json.as_object_mut().unwrap().remove("titlebar_insets");
    let mut loaded: Config = serde_json::from_value(json).unwrap();
    assert!(loaded.titlebar_insets.is_empty());
    loaded.titlebar_insets.insert("editor.exe".into(), 48);
    assert!(loaded.validate().is_ok());
    loaded.titlebar_insets.insert("editor.exe".into(), 513);
    assert!(loaded.validate().is_err());
}
