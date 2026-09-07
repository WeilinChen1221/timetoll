use super::Foreground;
use anyhow::{Result, ensure};
use std::{
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::{LibraryLoader::GetModuleHandleW, SystemInformation::GetTickCount64, Threading::*},
    UI::{
        HiDpi::*,
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
                    -((rect.bottom / 21).min(rect.right / 28).clamp(8, 24)),
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
                let padding = (rect.right.min(rect.bottom) / 10).clamp(1, 16);
                let reserve = if IsWindowVisible(GetDlgItem(hwnd, 1)) != 0 {
                    (rect.bottom / 5).clamp(1, 32) + 16
                } else {
                    0
                };
                rect.top += padding;
                rect.bottom = (rect.bottom - reserve - padding).max(rect.top + 1);
                rect.left += padding;
                rect.right = (rect.right - padding).max(rect.left + 1);
                let mut measured = rect;
                DrawTextW(
                    dc,
                    text.as_ptr(),
                    len,
                    &mut measured,
                    DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
                );
                rect.top +=
                    ((rect.bottom - rect.top - (measured.bottom - measured.top)) / 2).max(0);
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

// DWM bounds exclude invisible resize borders and are physical pixels. The
// per-monitor DPI context keeps SetWindowPos in the same coordinate system.
unsafe fn target_bounds(target: &Foreground, top_inset: Option<u16>) -> Option<RECT> {
    unsafe {
        let hwnd = target.window_id as usize as HWND;
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if target.window_id == 0
            || pid != target.pid
            || IsWindowVisible(hwnd) == 0
            || IsIconic(hwnd) != 0
        {
            return None;
        }
        let mut cloaked = 0u32;
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut _ as _,
            size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return None;
        }
        let mut rect: RECT = std::mem::zeroed();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut rect as *mut _ as _,
            size_of::<RECT>() as u32,
        ) < 0
            && GetWindowRect(hwnd, &mut rect) == 0
        {
            return None;
        }
        let outer = rect;
        let mut client: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut client) == 0 {
            return None;
        }
        let mut origin = POINT {
            x: client.left,
            y: client.top,
        };
        if ClientToScreen(hwnd, &mut origin) == 0 {
            return None;
        }
        rect = RECT {
            left: origin.x,
            top: origin.y,
            right: origin.x + client.right - client.left,
            bottom: origin.y + client.bottom - client.top,
        };
        if let Some(inset) = top_inset {
            let dpi = GetDpiForWindow(hwnd).max(96);
            rect.top = rect
                .top
                .max(outer.top + (u32::from(inset) * dpi).div_ceil(96) as i32);
        } else {
            // Some custom frames extend their client area behind caption buttons.
            let mut title: TITLEBARINFO = std::mem::zeroed();
            title.cbSize = size_of::<TITLEBARINFO>() as u32;
            if GetTitleBarInfo(hwnd, &mut title) != 0
                && title.rgstate[0] & 0x8000 == 0
                && title.rcTitleBar.bottom > title.rcTitleBar.top
            {
                rect.top = rect.top.max(title.rcTitleBar.bottom);
            }
        }
        rect.left = rect.left.max(outer.left);
        rect.top = rect.top.max(outer.top);
        rect.right = rect.right.min(outer.right);
        rect.bottom = rect.bottom.min(outer.bottom);
        (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
    }
}

pub struct Desktop {
    window: Option<(HWND, HWND)>,
    visible: bool,
    target: Option<(HWND, u32)>,
    message: String,
}

impl Desktop {
    pub fn new() -> Result<Self> {
        unsafe {
            if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
                ensure!(
                    !SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
                        .is_null(),
                    "cannot enable per-monitor DPI awareness"
                );
            }
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
            window: None,
            visible: false,
            target: None,
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
            let hwnd = GetAncestor(GetForegroundWindow(), GA_ROOT);
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            if pid == std::process::id() {
                return Some(Foreground {
                    app: "timetoll.exe".into(),
                    pid,
                    window_id: hwnd as usize as u64,
                });
            }
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
                window_id: hwnd as usize as u64,
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

    pub fn show(
        &mut self,
        target: &Foreground,
        message: &str,
        browser: bool,
        top_inset: Option<u16>,
    ) -> Result<bool> {
        unsafe {
            let Some(rect) = target_bounds(target, top_inset) else {
                self.hide(None);
                return Ok(false);
            };
            if self.window.is_none() {
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
                // Save the handle before the next fallible operation for Drop.
                self.window = Some((hwnd, null_mut()));
                let button = CreateWindowExW(
                    0,
                    wide("BUTTON").as_ptr(),
                    wide("Open a new browser tab").as_ptr(),
                    WS_CHILD | WS_TABSTOP | BS_PUSHBUTTON as u32,
                    0,
                    0,
                    260,
                    32,
                    hwnd,
                    1 as HMENU,
                    GetModuleHandleW(null()),
                    null(),
                );
                ensure!(
                    !button.is_null(),
                    "cannot create the new-tab button: {}",
                    std::io::Error::last_os_error()
                );
                self.window = Some((hwnd, button));
            }
            let (hwnd, button) = self.window.unwrap();
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            ensure!(
                SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    rect.left,
                    rect.top,
                    width,
                    height,
                    SWP_SHOWWINDOW | SWP_NOACTIVATE
                ) != 0,
                "cannot position blocking window: {}",
                std::io::Error::last_os_error()
            );
            if self.message != message || !self.visible {
                SetWindowTextW(hwnd, wide(message).as_ptr());
            }
            let button_width = (width - 16).clamp(1, 260);
            let button_height = (height / 5).clamp(1, 32);
            MoveWindow(
                button,
                (width - button_width) / 2,
                (height - button_height - 8).max(0),
                button_width,
                button_height,
                1,
            );
            ShowWindow(button, if browser { SW_SHOWNA } else { SW_HIDE });
            InvalidateRect(hwnd, null(), 1);
            if !self.visible || self.target != Some((target.window_id as usize as HWND, target.pid))
            {
                // Windows may refuse activation. Clicking the overlay gives it
                // keyboard focus; its bounds still cover only the target.
                SetForegroundWindow(hwnd);
            }
            self.target = Some((target.window_id as usize as HWND, target.pid));
            self.message = message.to_string();
            self.visible = true;
        }
        Ok(true)
    }

    pub fn hide(&mut self, restore_pid: Option<u32>) {
        if !self.visible {
            return;
        }
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
            let restore = pid == std::process::id();
            if let Some((hwnd, _)) = self.window {
                ShowWindow(hwnd, SW_HIDE);
            }
            if restore && let Some((target, owner)) = self.target {
                GetWindowThreadProcessId(target, &mut pid);
                if restore_pid == Some(owner)
                    && pid == owner
                    && IsWindowVisible(target) != 0
                    && IsIconic(target) == 0
                {
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
            if let Some((hwnd, _)) = self.window.take() {
                DestroyWindow(hwnd);
            }
        }
    }
}
