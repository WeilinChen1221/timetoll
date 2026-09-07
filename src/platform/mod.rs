use anyhow::Result;

#[derive(Clone, Debug)]
pub struct Foreground {
    pub app: String,
    pub pid: u32,
    /// Native window ID, or zero when the app has no visible window.
    pub window_id: u64,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Desktop;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Desktop;

#[cfg(not(any(target_os = "macos", windows)))]
pub struct Desktop;

#[cfg(not(any(target_os = "macos", windows)))]
impl Desktop {
    pub fn new() -> Result<Self> {
        anyhow::bail!("desktop monitoring supports macOS and Windows only")
    }
    pub fn pump(&mut self) {}
    pub fn foreground(&self) -> Option<Foreground> {
        None
    }
    pub fn idle_seconds(&self) -> f64 {
        f64::INFINITY
    }
    pub fn show(&mut self, _: &Foreground, _: &str, _: bool, _: Option<u16>) -> Result<bool> {
        anyhow::bail!("unsupported desktop")
    }
    pub fn hide(&mut self, _: Option<u32>) {}
    pub fn take_new_tab(&mut self) -> bool {
        false
    }
}

pub fn watch() -> Result<()> {
    let mut desktop = Desktop::new()?;
    println!("Switch to an app to see its identifier. Press Ctrl+C to stop.");
    let mut previous = String::new();
    loop {
        desktop.pump();
        if let Some(front) = desktop.foreground()
            && front.app != previous
        {
            println!("{}  pid={}", front.app, front.pid);
            previous = front.app;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}
