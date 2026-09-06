use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use timetoll::{
    bridge,
    config::{Config, Ratio, parse_target},
    engine::{Activity, Policy, account_interval},
    platform::{self, Desktop, Foreground},
    storage::Store,
};

#[derive(Parser)]
#[command(
    version,
    about = "Earn access to distracting apps and websites with focused time"
)]
struct Cli {
    /// Override the directory containing config.toml and state.json
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a configuration with the default 15:10 minute ratio
    Init,
    /// Monitor usage and show blocking windows until Ctrl+C
    Run,
    /// Display banked access time and earning progress
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Add or remove a target in the blocked group
    Block {
        #[command(subcommand)]
        action: TargetAction,
    },
    /// Add or remove a target in the earning group
    Earn {
        #[command(subcommand)]
        action: TargetAction,
    },
    /// Add or remove a website exception to website blocking
    Whitelist {
        #[command(subcommand)]
        action: SiteAction,
    },
    /// Change the ratio in minutes; preserves credit and resets partial progress
    Ratio {
        #[arg(long)]
        earn: u64,
        #[arg(long)]
        unlock: u64,
    },
    /// Print configuration with the bridge token redacted
    Config,
    /// Print identifiers of foreground apps as you switch between them
    Apps,
    /// Print browser extension pairing settings
    Pair,
    /// Check config, storage, and current foreground app detection
    Doctor,
    /// Show the blocking window briefly without changing credit
    Preview {
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=60))]
        seconds: u64,
    },
}

#[derive(Clone, ValueEnum)]
enum Kind {
    App,
    Site,
}
impl Kind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Site => "site",
        }
    }
}

#[derive(Subcommand)]
enum TargetAction {
    Add {
        #[arg(value_enum)]
        kind: Kind,
        value: String,
    },
    Remove {
        #[arg(value_enum)]
        kind: Kind,
        value: String,
    },
}

#[derive(Subcommand)]
enum SiteAction {
    Add { value: String },
    Remove { value: String },
}

fn main() {
    if let Err(error) = execute(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn execute(cli: Cli) -> Result<()> {
    let store = Store::new(cli.data_dir)?;
    match cli.command {
        Command::Init => {
            store.init()?;
            println!("Created {}", store.dir.join("config.toml").display());
        }
        Command::Run => run(&store)?,
        Command::Apps => platform::watch()?,
        Command::Preview { seconds } => {
            let mut desktop = Desktop::new()?;
            let restore = desktop.foreground().map(|f| f.pid);
            let running = shutdown_handler()?;
            let start = Instant::now();
            while running.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(seconds)
            {
                desktop.show(&format!("TimeToll\n\nBlocking window preview\n\nThis closes automatically after {seconds} seconds."), false)?;
                desktop.pump();
                std::thread::sleep(Duration::from_millis(50));
            }
            desktop.hide(restore);
        }
        Command::Status { json } => {
            let config = store.config()?;
            let mut state = store.ledger()?;
            state.set_ratio(config.ratio);
            if json {
                println!("{}", serde_json::to_string_pretty(&state)?);
            } else {
                println!(
                    "Banked access: {}\nEarning progress: {} / {}\nRatio: {} earns {}",
                    time_text(state.balance_ms),
                    time_text(state.progress_ms),
                    time_text(config.ratio.earn_seconds * 1000),
                    time_text(config.ratio.earn_seconds * 1000),
                    time_text(config.ratio.unlock_seconds * 1000)
                );
            }
        }
        Command::Config => {
            let mut config = store.config()?;
            config.bridge_token = "<redacted; use timetoll pair>".into();
            println!("{}", toml::to_string_pretty(&config)?);
        }
        Command::Pair => {
            let config = store.config()?;
            println!(
                "Set these values in the TimeToll browser extension options:\n\nBridge URL: http://127.0.0.1:{}\nToken: {}\n\nUse `timetoll apps` to find the browser's app identifier.\nLoad extension/chromium for Chrome, Edge, Brave, Arc, Opera or Vivaldi.\nSee README.md for Firefox installation.",
                config.bridge_port, config.bridge_token
            );
        }
        Command::Doctor => {
            let config = store.config()?;
            store.ledger()?;
            println!(
                "Config and state: OK\nData directory: {}\nBridge: 127.0.0.1:{}",
                store.dir.display(),
                config.bridge_port
            );
            let mut desktop = Desktop::new()?;
            desktop.pump();
            let front = desktop
                .foreground()
                .context("could not identify the foreground app")?;
            println!(
                "Foreground app: {}\nIdle time: {:.1}s\nUse `timetoll preview` to check the overlay, then `timetoll run`.",
                front.app,
                desktop.idle_seconds()
            );
        }
        command => {
            let _lock = store.lock("config.lock")?;
            let mut config = store.config()?;
            match command {
                Command::Block { action } => change_target(&mut config.blocked, action)?,
                Command::Earn { action } => change_target(&mut config.earning, action)?,
                Command::Whitelist { action } => match action {
                    SiteAction::Add { value } => {
                        timetoll::config::SiteRule::parse(&value)?;
                        if !config.whitelist.contains(&value) {
                            config.whitelist.push(value);
                        }
                    }
                    SiteAction::Remove { value } => {
                        let n = config.whitelist.len();
                        config.whitelist.retain(|v| v != &value);
                        ensure!(n != config.whitelist.len(), "whitelist rule was not found");
                    }
                },
                Command::Ratio { earn, unlock } => {
                    config.ratio = Ratio {
                        earn_seconds: earn
                            .checked_mul(60)
                            .context("earning duration is too large")?,
                        unlock_seconds: unlock
                            .checked_mul(60)
                            .context("unlock duration is too large")?,
                    };
                }
                _ => unreachable!(),
            }
            store.save_config(&config)?;
            println!("Configuration saved. A running monitor reloads rules within one second.");
        }
    }
    Ok(())
}

fn change_target(targets: &mut Vec<timetoll::config::Target>, action: TargetAction) -> Result<()> {
    match action {
        TargetAction::Add { kind, value } => {
            let target = parse_target(kind.as_str(), value)?;
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        TargetAction::Remove { kind, value } => {
            let target = parse_target(kind.as_str(), value)?;
            let n = targets.len();
            targets.retain(|t| t != &target);
            ensure!(targets.len() != n, "target was not found");
        }
    }
    Ok(())
}

fn shutdown_handler() -> Result<Arc<AtomicBool>> {
    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || signal.store(false, Ordering::Relaxed))?;
    Ok(running)
}

fn time_text(ms: u64) -> String {
    format!("{}m {}s", ms / 60000, ms / 1000 % 60)
}

fn run(store: &Store) -> Result<()> {
    let _lock = store.lock("monitor.lock")?;
    let mut config = store.config()?;
    let mut policy = Policy::new(&config)?;
    let mut ledger = store.ledger()?;
    ledger.set_ratio(config.ratio);
    let mut desktop = Desktop::new()?;
    let running = shutdown_handler()?;
    let browser = bridge::start(
        config.bridge_port,
        config.bridge_token.clone(),
        config.browsers.clone(),
        running.clone(),
    )?;
    let mut last_foreground: Option<Foreground> = None;
    let mut previous = Activity::Neutral;
    let mut previous_active = false;
    let mut previous_time = Instant::now();
    let mut checkpoint = Instant::now();
    let mut reload = Instant::now();
    let mut overlay = false;
    let mut config_error = String::new();
    let mut last_log = String::new();
    store.save_ledger(&ledger)?;
    println!(
        "TimeToll is running. Press Ctrl+C in this terminal to stop.\nBanked access: {}",
        time_text(ledger.balance_ms)
    );
    let result = (|| -> Result<()> {
        while running.load(Ordering::Relaxed) {
            desktop.pump();
            let now = Instant::now();
            let elapsed = now
                .duration_since(previous_time)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            previous_time = now;
            if reload.elapsed() >= Duration::from_secs(1) {
                match store.config() {
                    Ok(updated) => {
                        if updated.bridge_port != config.bridge_port
                            || updated.bridge_token != config.bridge_token
                            || updated.browsers != config.browsers
                        {
                            if config_error != "bridge" {
                                eprintln!(
                                    "Bridge settings changed. Restart timetoll run to apply them."
                                );
                                config_error = "bridge".into();
                            }
                        } else {
                            config_error.clear();
                        }
                        if ledger.ratio != updated.ratio {
                            ledger.set_ratio(updated.ratio);
                            store.save_ledger(&ledger)?;
                        }
                        // The listener retains its launch-time settings until restart.
                        config = Config {
                            bridge_port: config.bridge_port,
                            bridge_token: config.bridge_token.clone(),
                            browsers: config.browsers.clone(),
                            ..updated
                        };
                        policy = Policy::new(&config)?;
                    }
                    Err(error) => {
                        let message = error.to_string();
                        if message != config_error {
                            eprintln!("Keeping previous rules: {error:#}");
                            config_error = message;
                        }
                    }
                }
                reload = now;
            }
            let foreground = desktop.foreground();
            let is_self = foreground
                .as_ref()
                .is_some_and(|f| f.pid == std::process::id());
            let current = if is_self && overlay {
                last_foreground.clone()
            } else {
                foreground.filter(|f| f.pid != std::process::id())
            };
            let activity = if let Some(front) = &current {
                let shared = browser.lock().unwrap();
                policy.classify(&front.app, shared.url(&front.app))
            } else {
                Activity::Neutral
            };
            let active = !is_self && desktop.idle_seconds() < config.idle_seconds as f64;
            let old_balance = ledger.balance_ms;
            account_interval(
                &mut ledger,
                &previous,
                &activity,
                elapsed,
                active && previous_active,
            );
            let blocked = ledger.blocked(&activity);
            if blocked {
                let is_browser = current.as_ref().is_some_and(|front| {
                    config
                        .browsers
                        .iter()
                        .any(|b| b.eq_ignore_ascii_case(&front.app))
                });
                let message = match &activity {
                    Activity::UnknownBrowser => "TimeToll\n\nWaiting for the browser extension\n\nPair the extension using timetoll pair.\nSwitch to another app to continue.".to_string(),
                    _ => format!("TimeToll\n\nAccess is locked\n\nUse an earning app or website for {} to earn {}.\nProgress: {}\n\nSwitch apps with Cmd+Tab or Alt+Tab.", time_text(config.ratio.earn_seconds * 1000), time_text(config.ratio.unlock_seconds * 1000), time_text(ledger.progress_ms)),
                };
                desktop.show(&message, is_browser)?;
                overlay = true;
            } else {
                desktop.hide(current.as_ref().map(|f| f.pid));
                overlay = false;
            }
            if desktop.take_new_tab()
                && let Some(front) = &current
            {
                browser.lock().unwrap().request_new_tab(front.app.clone());
            }
            let log = match &activity {
                Activity::Neutral => "Neutral".into(),
                Activity::Earning(_) => "Earning".into(),
                Activity::Spending(_) if blocked => "Locked".into(),
                Activity::Spending(_) => "Using banked access".into(),
                Activity::UnknownBrowser => "Waiting for browser extension".into(),
            };
            if log != last_log {
                println!("{log}. Banked access: {}", time_text(ledger.balance_ms));
                last_log = log;
            }
            if ledger.balance_ms > old_balance
                || (old_balance > 0 && ledger.balance_ms == 0)
                || checkpoint.elapsed() >= Duration::from_secs(1)
            {
                store.save_ledger(&ledger)?;
                checkpoint = now;
            }
            previous = activity;
            previous_active = active && !blocked;
            if current.is_some() {
                last_foreground = current;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Ok(())
    })();
    running.store(false, Ordering::Relaxed);
    desktop.hide(last_foreground.map(|f| f.pid));
    store.save_ledger(&ledger)?;
    result
}
