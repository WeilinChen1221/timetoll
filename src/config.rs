use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub enum Target {
    App(String),
    Site(String),
}

impl Target {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::App(value) => {
                ensure!(
                    !value.trim().is_empty() && value.len() <= 255,
                    "app identifier must contain 1 to 255 characters"
                );
                ensure!(
                    !value.chars().any(|c| c.is_control() || "/\\*".contains(c)),
                    "use an app bundle ID on macOS or executable name on Windows"
                );
                ensure!(
                    value.trim() == value,
                    "app identifier cannot start or end with whitespace"
                );
                Ok(())
            }
            Self::Site(value) => SiteRule::parse(value).map(|_| ()),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ratio {
    pub earn_seconds: u64,
    pub unlock_seconds: u64,
}

impl Default for Ratio {
    fn default() -> Self {
        Self {
            earn_seconds: 900,
            unlock_seconds: 600,
        }
    }
}

impl Ratio {
    pub fn validate(self) -> Result<()> {
        ensure!(
            (1..=31_536_000).contains(&self.earn_seconds),
            "earning interval must be 1 second to 365 days"
        );
        ensure!(
            (1..=31_536_000).contains(&self.unlock_seconds),
            "unlock interval must be 1 second to 365 days"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub ratio: Ratio,
    pub idle_seconds: u64,
    pub bridge_port: u16,
    pub bridge_token: String,
    pub browsers: Vec<String>,
    pub blocked: Vec<Target>,
    pub earning: Vec<Target>,
    pub whitelist: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            ratio: Ratio::default(),
            idle_seconds: 60,
            bridge_port: 47821,
            bridge_token: uuid::Uuid::new_v4().simple().to_string(),
            browsers: [
                "com.google.Chrome",
                "org.chromium.Thorium",
                "com.microsoft.edgemac",
                "org.mozilla.firefox",
                "com.brave.Browser",
                "company.thebrowser.Browser",
                "com.apple.Safari",
                "com.operasoftware.Opera",
                "com.vivaldi.Vivaldi",
                "chrome.exe",
                "msedge.exe",
                "firefox.exe",
                "brave.exe",
                "arc.exe",
                "opera.exe",
                "vivaldi.exe",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            blocked: vec![],
            earning: vec![],
            whitelist: vec![],
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported config version {}",
            self.version
        );
        self.ratio.validate()?;
        ensure!(
            (1..=3600).contains(&self.idle_seconds),
            "idle_seconds must be 1 to 3600"
        );
        ensure!(
            self.bridge_port >= 1024,
            "bridge_port must be 1024 to 65535"
        );
        ensure!(
            self.bridge_token.len() >= 32
                && self.bridge_token.len() <= 128
                && self.bridge_token.bytes().all(|c| c.is_ascii_alphanumeric()),
            "bridge_token must contain 32 to 128 ASCII letters or digits"
        );
        for target in self.blocked.iter().chain(&self.earning) {
            target.validate()?;
        }
        for browser in &self.browsers {
            Target::App(browser.clone()).validate()?;
        }
        for rule in &self.whitelist {
            SiteRule::parse(rule)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum SiteRule {
    Domain(String),
    Page { url: Url, subtree: bool },
}

fn normalized_host(host: &str) -> &str {
    host.trim_end_matches('.')
}

impl SiteRule {
    pub fn parse(input: &str) -> Result<Self> {
        ensure!(
            !input.is_empty()
                && input.trim() == input
                && input.len() <= 8192
                && !input.chars().any(char::is_control),
            "invalid website rule"
        );
        if !input.contains("://") {
            ensure!(
                !input.contains(['/', '?', '#', ':', '@', '*', '\\']),
                "use a domain or a full http(s) URL"
            );
            let url = Url::parse(&format!("https://{input}")).context("invalid domain")?;
            let host = normalized_host(url.host_str().context("domain is missing")?).to_string();
            ensure!(!host.is_empty(), "domain is missing");
            return Ok(Self::Domain(host));
        }
        let subtree = input.ends_with("/*");
        let source = if subtree {
            &input[..input.len() - 1]
        } else {
            input
        };
        ensure!(
            !source.contains('*'),
            "wildcards are only supported as a trailing /*"
        );
        let mut url = Url::parse(source).context("invalid website URL")?;
        ensure!(
            matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
            "website URLs must use http or https"
        );
        ensure!(
            url.username().is_empty() && url.password().is_none(),
            "website rules cannot contain credentials"
        );
        ensure!(
            !subtree || url.query().is_none(),
            "subtree rules cannot contain a query"
        );
        url.set_fragment(None);
        Ok(Self::Page { url, subtree })
    }

    pub fn matches(&self, candidate: &Url) -> bool {
        if !matches!(candidate.scheme(), "http" | "https") {
            return false;
        }
        let Some(host) = candidate.host_str().map(normalized_host) else {
            return false;
        };
        match self {
            Self::Domain(domain) => {
                host == domain
                    || host
                        .strip_suffix(domain)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            }
            Self::Page { url, subtree } => {
                url.scheme() == candidate.scheme()
                    && url.host_str().map(normalized_host) == Some(host)
                    && url.port_or_known_default() == candidate.port_or_known_default()
                    && if *subtree {
                        candidate.path().starts_with(url.path())
                            || candidate.path() == url.path().trim_end_matches('/')
                    } else {
                        candidate.path() == url.path()
                    }
                    && url.query().is_none_or(|q| candidate.query() == Some(q))
            }
        }
    }
}

pub fn parse_target(kind: &str, value: String) -> Result<Target> {
    let target = match kind {
        "app" => Target::App(value),
        "site" => Target::Site(value),
        _ => bail!("target kind must be app or site"),
    };
    target.validate()?;
    Ok(target)
}
