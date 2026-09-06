use super::Foreground;
use anyhow::{Result, ensure};
use std::{
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{LibraryLoader::GetModuleHandleW, SystemInformation::GetTickCount64, Threading::*},
    UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::*,
    },
};

static NEW_TAB: AtomicBool = AtomicBool::new(false);
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_CLOSE => 0,
            WM_COMMAND if w & 0xffff == 1 => {
                NEW_TAB.store(true, Ordering::Relaxed);
                0
            }
            WM_PAINT => {
                let mut paint: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut paint);
                let mut rect: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rect);
                FillRect(dc, &rect, GetStockObject(BLACK_BRUSH) as HBRUSH);
                SetTextColor(dc, 0x00ffffff);
                SetBkMode(dc, TRANSPARENT as i32);
                let font = CreateFontW(
                    -24,
                    0,
                    0,
                    0,
                    FW_NORMAL as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    DEFAULT_PITCH as u32,
                    wide("Segoe UI").as_ptr(),
                );
                let old_font = SelectObject(dc, font);
                let mut text = [0u16; 2048];
                let len = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
                let center = (rect.bottom - rect.top) / 2;
                rect.top = center - 180;
                rect.bottom = center + 140;
                rect.left += 40;
                rect.right -= 40;
                DrawTextW(
                    dc,
                    text.as_ptr(),
                    len,
                    &mut rect,
                    DT_CENTER | DT_WORDBREAK | DT_NOPREFIX,
                );
                SelectObject(dc, old_font);
                DeleteObject(font);
                EndPaint(hwnd, &paint);
                0
            }
            _ => DefWindowProcW(hwnd, message, w, l),
        }
    }
}

unsafe extern "system" fn collect_monitor(
    _: HMONITOR,
    _: HDC,
    rect: *mut RECT,
    data: LPARAM,
) -> i32 {
    unsafe {
        (&mut *(data as *mut Vec<RECT>)).push(*rect);
    }
    1
}

pub struct Desktop {
    windows: Vec<(HWND, HWND)>,
    visible: bool,
    last_foreground: std::cell::Cell<HWND>,
    message: String,
}

impl Desktop {
    pub fn new() -> Result<Self> {
        unsafe {
            SetProcessDPIAware();
            let class = wide("TimeTollOverlay");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: GetModuleHandleW(null()),
                lpszClassName: class.as_ptr(),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                ..std::mem::zeroed()
            };
            ensure!(
                RegisterClassW(&wc) != 0,
                "cannot register overlay window: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(Self {
            windows: vec![],
            visible: false,
            last_foreground: std::cell::Cell::new(null_mut()),
            message: String::new(),
        })
    }

    pub fn pump(&mut self) {
        unsafe {
            let mut message: MSG = std::mem::zeroed();
            while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    pub fn foreground(&self) -> Option<Foreground> {
        unsafe {
            let hwnd = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            if pid == std::process::id() {
                return Some(Foreground {
                    app: "timetoll.exe".into(),
                    pid,
                });
            }
            self.last_foreground.set(hwnd);
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut path = [0u16; 32768];
            let mut len = path.len() as u32;
            let result = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len);
            CloseHandle(process);
            if result == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&path[..len as usize]);
            Some(Foreground {
                app: path.rsplit('\\').next()?.to_string(),
                pid,
            })
        }
    }

    pub fn idle_seconds(&self) -> f64 {
        unsafe {
            let mut input = LASTINPUTINFO {
                cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                dwTime: 0,
            };
            if GetLastInputInfo(&mut input) == 0 {
                return f64::INFINITY;
            }
            (GetTickCount64() as u32).wrapping_sub(input.dwTime) as f64 / 1000.0
        }
    }

    pub fn show(&mut self, message: &str, browser: bool) -> Result<()> {
        unsafe {
            let mut monitors: Vec<RECT> = vec![];
            EnumDisplayMonitors(
                null_mut(),
                null(),
                Some(collect_monitor),
                &mut monitors as *mut _ as LPARAM,
            );
            ensure!(
                !monitors.is_empty(),
                "cannot find a display for the blocking window"
            );
            if monitors.len() != self.windows.len() {
                for (hwnd, _) in self.windows.drain(..) {
                    DestroyWindow(hwnd);
                }
                for _ in &monitors {
                    let hwnd = CreateWindowExW(
                        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                        wide("TimeTollOverlay").as_ptr(),
                        wide(message).as_ptr(),
                        WS_POPUP,
                        0,
                        0,
                        1,
                        1,
                        null_mut(),
                        null_mut(),
                        GetModuleHandleW(null()),
                        null(),
                    );
                    ensure!(
                        !hwnd.is_null(),
                        "cannot create blocking window: {}",
                        std::io::Error::last_os_error()
                    );
                    let button = CreateWindowExW(
                        0,
                        wide("BUTTON").as_ptr(),
                        wide("Open a new browser tab").as_ptr(),
                        WS_CHILD | WS_TABSTOP | BS_PUSHBUTTON as u32,
                        0,
                        0,
                        260,
                        42,
                        hwnd,
                        1 as HMENU,
                        GetModuleHandleW(null()),
                        null(),
                    );
                    self.windows.push((hwnd, button));
                    ensure!(
                        !button.is_null(),
                        "cannot create the new-tab button: {}",
                        std::io::Error::last_os_error()
                    );
                }
                self.visible = false;
            }
            let changed = self.message != message;
            for ((hwnd, button), rect) in self.windows.iter().zip(&monitors) {
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                SetWindowPos(
                    *hwnd,
                    HWND_TOPMOST,
                    rect.left,
                    rect.top,
                    width,
                    height,
                    SWP_SHOWWINDOW | SWP_NOACTIVATE,
                );
                if changed || !self.visible {
                    SetWindowTextW(*hwnd, wide(message).as_ptr());
                    InvalidateRect(*hwnd, null(), 1);
                }
                MoveWindow(*button, (width - 260) / 2, height / 2 + 160, 260, 42, 1);
                ShowWindow(*button, if browser { SW_SHOWNA } else { SW_HIDE });
            }
            let foreground = GetForegroundWindow();
            let ours = self.windows.iter().any(|(hwnd, _)| *hwnd == foreground);
            if !ours && let Some((hwnd, _)) = self.windows.first() {
                // Windows may refuse foreground activation. The window still
                // covers the desktop; clicking it gives it keyboard focus.
                SetForegroundWindow(*hwnd);
            }
            self.message = message.to_string();
            self.visible = true;
        }
        Ok(())
    }

    pub fn hide(&mut self, restore_pid: Option<u32>) {
        if !self.visible {
            return;
        }
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
            let restore = pid == std::process::id();
            for (hwnd, _) in &self.windows {
                ShowWindow(*hwnd, SW_HIDE);
            }
            if restore {
                let target = self.last_foreground.get();
                GetWindowThreadProcessId(target, &mut pid);
                if restore_pid == Some(pid) {
                    SetForegroundWindow(target);
                }
            }
        }
        self.visible = false;
    }

    pub fn take_new_tab(&mut self) -> bool {
        NEW_TAB.swap(false, Ordering::Relaxed)
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe {
            for (hwnd, _) in self.windows.drain(..) {
                DestroyWindow(hwnd);
            }
        }
    }
}
