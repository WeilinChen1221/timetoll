use crate::config::{Config, Ratio, SiteRule, Target};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activity {
    Neutral,
    Earning(String),
    Spending(String),
    UnknownBrowser,
}

pub struct Policy {
    blocked_apps: Vec<String>,
    earning_apps: Vec<String>,
    blocked_sites: Vec<SiteRule>,
    earning_sites: Vec<SiteRule>,
    whitelist: Vec<SiteRule>,
    browsers: Vec<String>,
}

impl Policy {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        config.validate()?;
        fn split(targets: &[Target]) -> anyhow::Result<(Vec<String>, Vec<SiteRule>)> {
            let mut apps = vec![];
            let mut sites = vec![];
            for target in targets {
                match target {
                    Target::App(s) => apps.push(s.to_lowercase()),
                    Target::Site(s) => sites.push(SiteRule::parse(s)?),
                }
            }
            Ok((apps, sites))
        }
        let (blocked_apps, blocked_sites) = split(&config.blocked)?;
        let (earning_apps, earning_sites) = split(&config.earning)?;
        Ok(Self {
            blocked_apps,
            earning_apps,
            blocked_sites,
            earning_sites,
            whitelist: config
                .whitelist
                .iter()
                .map(|s| SiteRule::parse(s))
                .collect::<anyhow::Result<_>>()?,
            browsers: config.browsers.iter().map(|s| s.to_lowercase()).collect(),
        })
    }

    pub fn classify(&self, app: &str, url: Option<&str>) -> Activity {
        let app = app.to_lowercase();
        if self.blocked_apps.contains(&app) {
            return Activity::Spending(app);
        }
        if self.browsers.contains(&app) {
            let parsed = url.and_then(|s| Url::parse(s).ok());
            if parsed.is_none() && !self.blocked_sites.is_empty() {
                return Activity::UnknownBrowser;
            }
            if let Some(url) = parsed {
                let whitelisted = self.whitelist.iter().any(|rule| rule.matches(&url));
                if !whitelisted && self.blocked_sites.iter().any(|rule| rule.matches(&url)) {
                    return Activity::Spending(format!(
                        "{}{}",
                        url.host_str().unwrap_or("website"),
                        url.path()
                    ));
                }
                if self.earning_sites.iter().any(|rule| rule.matches(&url)) {
                    return Activity::Earning(url.to_string());
                }
            }
        }
        if self.earning_apps.contains(&app) {
            Activity::Earning(app)
        } else {
            Activity::Neutral
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub balance_ms: u64,
    pub progress_ms: u64,
    pub ratio: Ratio,
    pub total_earned_ms: u64,
    pub total_spent_ms: u64,
}

impl Ledger {
    pub fn set_ratio(&mut self, ratio: Ratio) {
        if self.ratio != ratio {
            self.progress_ms = 0;
            self.ratio = ratio;
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        self.ratio.validate()?;
        anyhow::ensure!(
            self.progress_ms < self.ratio.earn_seconds * 1000,
            "invalid earning progress in state file"
        );
        Ok(())
    }

    pub fn advance(&mut self, activity: &Activity, elapsed_ms: u64) {
        match activity {
            Activity::Earning(_) => {
                let interval = u128::from(self.ratio.earn_seconds) * 1000;
                let progress = u128::from(self.progress_ms) + u128::from(elapsed_ms);
                let earned = (progress / interval * u128::from(self.ratio.unlock_seconds) * 1000)
                    .min(u128::from(u64::MAX)) as u64;
                self.progress_ms = (progress % interval) as u64;
                self.balance_ms = self.balance_ms.saturating_add(earned);
                self.total_earned_ms = self.total_earned_ms.saturating_add(earned);
            }
            Activity::Spending(_) => {
                let spent = self.balance_ms.min(elapsed_ms);
                self.balance_ms -= spent;
                self.total_spent_ms = self.total_spent_ms.saturating_add(spent);
            }
            _ => {}
        }
    }

    pub fn blocked(&self, activity: &Activity) -> bool {
        matches!(activity, Activity::UnknownBrowser)
            || matches!(activity, Activity::Spending(_)) && self.balance_ms == 0
    }
}

/// Attribute only short, continuously observed intervals. Suspend, focus changes,
/// and idle observations never turn into retroactive rewards or charges.
pub fn account_interval(
    ledger: &mut Ledger,
    previous: &Activity,
    current: &Activity,
    elapsed_ms: u64,
    active: bool,
) {
    if active && previous == current && elapsed_ms <= 2000 {
        ledger.advance(current, elapsed_ms);
    }
}
