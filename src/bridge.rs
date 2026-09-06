use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    collections::HashMap,
    io::Read,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tiny_http::{Header, Method, Response, Server};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub app: String,
    pub url: String,
}

#[derive(Default)]
pub struct BrowserState {
    reports: HashMap<String, (String, Instant)>,
    new_tab: Option<(String, Instant)>,
}

impl BrowserState {
    pub fn url(&self, app: &str) -> Option<&str> {
        self.reports
            .get(&app.to_lowercase())
            .filter(|(_, time)| time.elapsed() < Duration::from_secs(4))
            .map(|(url, _)| url.as_str())
    }

    pub fn request_new_tab(&mut self, app: String) {
        self.new_tab = Some((app.to_lowercase(), Instant::now()));
    }

    fn update(&mut self, report: Report) -> bool {
        let app = report.app.to_lowercase();
        self.reports
            .insert(app.clone(), (report.url, Instant::now()));
        if self.new_tab.as_ref().is_some_and(|(requested, time)| {
            requested == &app && time.elapsed() < Duration::from_secs(10)
        }) {
            self.new_tab = None;
            true
        } else {
            false
        }
    }
}

pub type SharedBrowserState = Arc<Mutex<BrowserState>>;

pub fn start(
    port: u16,
    token: String,
    browsers: Vec<String>,
    running: Arc<AtomicBool>,
) -> Result<SharedBrowserState> {
    let server = Server::http(("127.0.0.1", port))
        .map_err(|e| anyhow::anyhow!("cannot start browser bridge on port {port}: {e}"))?;
    let state = Arc::new(Mutex::new(BrowserState::default()));
    let shared = state.clone();
    thread::Builder::new()
        .name("browser-bridge".into())
        .spawn(move || {
            while running.load(Ordering::Relaxed) {
                let mut request = match server.recv_timeout(Duration::from_millis(250)) {
                    Ok(Some(request)) => request,
                    Ok(None) => continue,
                    Err(_) => break,
                };
                // No CORS permission is granted. Ordinary websites cannot send the
                // authenticated JSON request; extensions use their host permission.
                let authorized = request.headers().iter().any(|h| {
                    h.field.equiv("Authorization") && h.value.as_str() == format!("Bearer {token}")
                });
                let content_type = request.headers().iter().any(|h| {
                    h.field.equiv("Content-Type")
                        && h.value.as_str().split(';').next() == Some("application/json")
                });
                let origin_allowed = request
                    .headers()
                    .iter()
                    .filter(|h| h.field.equiv("Origin"))
                    .all(|h| {
                        h.value.as_str().starts_with("chrome-extension://")
                            || h.value.as_str().starts_with("moz-extension://")
                    });
                let host_allowed = request.headers().iter().any(|h| {
                    h.field.equiv("Host") && h.value.as_str() == format!("127.0.0.1:{port}")
                });
                let (status, body) =
                    if request.method() != &Method::Post || request.url() != "/v1/activity" {
                        (404, "{\"error\":\"not found\"}".to_string())
                    } else if !authorized || !origin_allowed || !host_allowed {
                        (403, "{\"error\":\"unauthorized\"}".to_string())
                    } else if !content_type || request.body_length().is_none_or(|n| n > 16384) {
                        (400, "{\"error\":\"invalid request\"}".to_string())
                    } else {
                        let mut bytes = Vec::new();
                        let result = request
                            .as_reader()
                            .take(16385)
                            .read_to_end(&mut bytes)
                            .map_err(anyhow::Error::from)
                            .and_then(|_| parse_report(&bytes, &browsers));
                        match result {
                            Ok(report) => {
                                let new_tab = shared.lock().unwrap().update(report);
                                (200, format!("{{\"ok\":true,\"new_tab\":{new_tab}}}"))
                            }
                            Err(_) => (400, "{\"error\":\"invalid report\"}".to_string()),
                        }
                    };
                let response = Response::from_string(body)
                    .with_status_code(status)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
            }
        })?;
    Ok(state)
}

pub fn parse_report(bytes: &[u8], browsers: &[String]) -> Result<Report> {
    ensure!(bytes.len() <= 16384, "report too large");
    let report: Report = serde_json::from_slice(bytes)?;
    ensure!(
        browsers.iter().any(|b| b.eq_ignore_ascii_case(&report.app)),
        "unknown browser"
    );
    ensure!(report.url.len() <= 8192, "URL too long");
    let url = url::Url::parse(&report.url).context("invalid tab URL")?;
    ensure!(
        matches!(
            url.scheme(),
            "http"
                | "https"
                | "chrome"
                | "edge"
                | "about"
                | "file"
                | "chrome-extension"
                | "moz-extension"
                | "brave"
                | "vivaldi"
                | "opera"
        ),
        "unsupported tab URL"
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_expire_and_new_tab_requests_are_browser_specific_and_one_shot() {
        let mut state = BrowserState::default();
        state.request_new_tab("chrome.exe".into());
        assert!(!state.update(Report {
            app: "firefox.exe".into(),
            url: "about:blank".into()
        }));
        assert!(state.update(Report {
            app: "CHROME.EXE".into(),
            url: "https://example.com".into()
        }));
        assert!(!state.update(Report {
            app: "chrome.exe".into(),
            url: "https://example.com".into()
        }));
        assert_eq!(state.url("CHROME.EXE"), Some("https://example.com"));
        state.reports.get_mut("chrome.exe").unwrap().1 = Instant::now() - Duration::from_secs(5);
        assert!(state.url("chrome.exe").is_none());
    }

    #[test]
    fn expired_new_tab_commands_do_not_open_tabs_later() {
        let mut state = BrowserState {
            new_tab: Some((
                "chrome.exe".into(),
                Instant::now() - Duration::from_secs(11),
            )),
            ..BrowserState::default()
        };
        assert!(!state.update(Report {
            app: "chrome.exe".into(),
            url: "about:blank".into()
        }));
    }
}
