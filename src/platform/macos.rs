use super::Foreground;
use anyhow::{Result, ensure};
use std::ffi::{CStr, CString, c_char};

unsafe extern "C" {
    fn tt_init() -> bool;
    fn tt_pump();
    fn tt_foreground(buffer: *mut c_char, size: usize, pid: *mut u32) -> bool;
    fn tt_idle_seconds() -> f64;
    fn tt_show(message: *const c_char, browser: bool);
    fn tt_hide(restore_pid: u32);
    fn tt_take_new_tab() -> bool;
}

// AppKit calls stay on the CLI's main thread.
pub struct Desktop {
    _main_thread: std::marker::PhantomData<*mut ()>,
}

impl Desktop {
    pub fn new() -> Result<Self> {
        ensure!(
            unsafe { tt_init() },
            "cannot initialize AppKit; run in a logged-in desktop session"
        );
        Ok(Self {
            _main_thread: std::marker::PhantomData,
        })
    }
    pub fn pump(&mut self) {
        unsafe { tt_pump() }
    }
    pub fn foreground(&self) -> Option<Foreground> {
        let mut buffer = [0 as c_char; 1024];
        let mut pid = 0;
        if unsafe { tt_foreground(buffer.as_mut_ptr(), buffer.len(), &mut pid) } {
            Some(Foreground {
                app: unsafe { CStr::from_ptr(buffer.as_ptr()) }
                    .to_string_lossy()
                    .into_owned(),
                pid,
            })
        } else {
            None
        }
    }
    pub fn idle_seconds(&self) -> f64 {
        unsafe { tt_idle_seconds() }
    }
    pub fn show(&mut self, message: &str, browser: bool) -> Result<()> {
        let message = CString::new(message.replace('\0', "")).unwrap();
        unsafe { tt_show(message.as_ptr(), browser) }
        Ok(())
    }
    pub fn hide(&mut self, restore_pid: Option<u32>) {
        unsafe { tt_hide(restore_pid.unwrap_or(0)) }
    }
    pub fn take_new_tab(&mut self) -> bool {
        unsafe { tt_take_new_tab() }
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        self.hide(None);
    }
}
