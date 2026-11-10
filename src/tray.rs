use timetoll::engine::{Activity, Ledger};

use crate::time_text;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Tray;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Tray;

#[cfg(not(any(target_os = "macos", windows)))]
pub struct Tray;
#[cfg(not(any(target_os = "macos", windows)))]
impl Tray {
    pub fn new() -> anyhow::Result<Self> {
        anyhow::bail!("status tray supports macOS and Windows only")
    }
    pub fn update(&mut self, _: &Snapshot) {}
    pub fn handle_events(&mut self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub state: String,
    pub balance: String,
    pub progress: String,
}

impl Snapshot {
    pub fn new(ledger: &Ledger, activity: &Activity, active: bool) -> Self {
        // A lock or missing browser report explains why access is unavailable,
        // even when an overlay owns focus or the user has become idle.
        let state = match activity {
            Activity::UnknownBrowser => "waiting for browser extension",
            Activity::Spending(_) if ledger.blocked(activity) => "locked",
            _ if !active => "idle",
            Activity::Earning(_) => "earning",
            Activity::Spending(_) => "spending",
            Activity::Neutral => "neutral",
        };
        Self {
            state: format!("State: {state}"),
            balance: format!("Banked access: {}", time_text(ledger.balance_ms)),
            progress: format!(
                "Earning progress: {} / {}",
                time_text(ledger.progress_ms),
                time_text(ledger.ratio.earn_seconds * 1000)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use timetoll::{config::Ratio, engine::account_interval};

    #[test]
    fn state_explains_accounting_and_access() {
        for (activity, balance_ms, active, expected) in [
            (Activity::Neutral, 0, true, "neutral"),
            (Activity::Earning("editor".into()), 0, true, "earning"),
            (Activity::Earning("editor".into()), 0, false, "idle"),
            (Activity::Spending("game".into()), 1000, true, "spending"),
            (Activity::Spending("game".into()), 1000, false, "idle"),
            (Activity::Spending("game".into()), 0, false, "locked"),
            (
                Activity::UnknownBrowser,
                0,
                false,
                "waiting for browser extension",
            ),
        ] {
            let ledger = Ledger {
                balance_ms,
                ..Default::default()
            };
            assert_eq!(
                Snapshot::new(&ledger, &activity, active).state,
                format!("State: {expected}")
            );
        }
    }

    #[test]
    fn display_follows_rewards_exhaustion_and_ratio_reload() {
        let earning = Activity::Earning("editor".into());
        let spending = Activity::Spending("game".into());
        let mut ledger = Ledger {
            progress_ms: 899_000,
            ..Default::default()
        };
        let snapshot = Snapshot::new(&ledger, &earning, true);
        assert_eq!(snapshot.balance, "Banked access: 0m 0s");
        assert_eq!(snapshot.progress, "Earning progress: 14m 59s / 15m 0s");
        account_interval(&mut ledger, &earning, &earning, 1000, true);
        let snapshot = Snapshot::new(&ledger, &earning, true);
        assert_eq!(snapshot.balance, "Banked access: 10m 0s");
        assert_eq!(snapshot.progress, "Earning progress: 0m 0s / 15m 0s");
        ledger.advance(&spending, 600_000);
        assert_eq!(
            Snapshot::new(&ledger, &spending, true).state,
            "State: locked"
        );
        ledger.advance(&earning, 1500);
        ledger.set_ratio(Ratio {
            earn_seconds: 30,
            unlock_seconds: 10,
        });
        assert_eq!(
            Snapshot::new(&ledger, &earning, true).progress,
            "Earning progress: 0m 0s / 0m 30s"
        );
    }
}
