use anyhow::Result;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute, queue,
    style::Print,
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    io::{self, IsTerminal, Write},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use timetoll::{engine::Ledger, storage::Store};

use crate::{shutdown_handler, time_text};

fn read_status(store: &Store) -> Result<Ledger> {
    let config = store.config()?;
    let mut state = store.ledger()?;
    state.set_ratio(config.ratio);
    Ok(state)
}

fn status_text(state: &Ledger) -> String {
    format!(
        "Banked access: {}\nEarning progress: {} / {}\nRatio: {} earns {}",
        time_text(state.balance_ms),
        time_text(state.progress_ms),
        time_text(state.ratio.earn_seconds * 1000),
        time_text(state.ratio.earn_seconds * 1000),
        time_text(state.ratio.unlock_seconds * 1000)
    )
}

pub fn show(store: &Store, json: bool, watch: bool) -> Result<()> {
    let mut state = read_status(store)?;
    if !watch {
        let text = if json {
            serde_json::to_string_pretty(&state)?
        } else {
            status_text(&state)
        };
        writeln!(io::stdout().lock(), "{text}")?;
        return Ok(());
    }

    let running = shutdown_handler()?;
    let mut output = WatchOutput::new()?;
    while running.load(Ordering::Relaxed) {
        let next_refresh = Instant::now() + Duration::from_secs(1);
        output.render(&status_text(&state))?;
        while running.load(Ordering::Relaxed) && Instant::now() < next_refresh {
            std::thread::sleep(Duration::from_millis(50));
        }
        if running.load(Ordering::Relaxed) {
            state = read_status(store)?;
        }
    }
    Ok(())
}

struct WatchOutput {
    stdout: io::Stdout,
    terminal: bool,
}

impl WatchOutput {
    fn new() -> Result<Self> {
        let stdout = io::stdout();
        let mut output = Self {
            terminal: stdout.is_terminal(),
            stdout,
        };
        if output.terminal {
            execute!(output.stdout, EnterAlternateScreen, Hide)?;
        }
        Ok(output)
    }

    fn render(&mut self, text: &str) -> Result<()> {
        if self.terminal {
            let mut stdout = self.stdout.lock();
            queue!(
                stdout,
                MoveTo(0, 0),
                Clear(ClearType::All),
                Print("TimeToll status. Refreshing every second. Press Ctrl+C to stop.")
            )?;
            for (row, line) in text.lines().enumerate() {
                queue!(stdout, MoveTo(0, row as u16 + 2), Print(line))?;
            }
            stdout.flush()?;
        } else {
            let mut stdout = self.stdout.lock();
            writeln!(stdout, "{text}\n")?;
            stdout.flush()?;
        }
        Ok(())
    }
}

impl Drop for WatchOutput {
    fn drop(&mut self) {
        if self.terminal {
            // Restore the terminal on Ctrl+C and on read or output errors.
            let _ = execute!(self.stdout, Show, LeaveAlternateScreen);
        }
    }
}
