use std::ffi::{CString, c_char};

use anyhow::{Result, ensure};

use super::Snapshot;

unsafe extern "C" {
    fn tt_tray_init() -> bool;
    fn tt_tray_update(state: *const c_char, balance: *const c_char, progress: *const c_char);
    fn tt_tray_destroy();
}

// Like Desktop, this owner must stay on the AppKit main thread.
pub struct Tray {
    previous: Option<Snapshot>,
    _main_thread: std::marker::PhantomData<*mut ()>,
}

impl Tray {
    pub fn new() -> Result<Self> {
        ensure!(
            unsafe { tt_tray_init() },
            "cannot create menu bar status item"
        );
        Ok(Self {
            previous: None,
            _main_thread: std::marker::PhantomData,
        })
    }

    pub fn update(&mut self, snapshot: &Snapshot) {
        if self.previous.as_ref() == Some(snapshot) {
            return;
        }
        // Snapshot text is generated locally and contains no user input.
        let state = CString::new(snapshot.state.as_str()).unwrap();
        let balance = CString::new(snapshot.balance.as_str()).unwrap();
        let progress = CString::new(snapshot.progress.as_str()).unwrap();
        unsafe { tt_tray_update(state.as_ptr(), balance.as_ptr(), progress.as_ptr()) };
        self.previous = Some(snapshot.clone());
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe { tt_tray_destroy() };
    }
}
