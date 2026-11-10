use std::{
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

use anyhow::{Result, ensure};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::GetDpiForWindow,
        Input::{
            KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE},
            *,
        },
        Shell::*,
        WindowsAndMessaging::*,
    },
};

use super::Snapshot;

const TRAY_MESSAGE: u32 = WM_APP + 1;
// shellapi.h defines this as an expression, omitted by windows-sys bindings.
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
static TOGGLE: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static RECREATE: AtomicBool = AtomicBool::new(false);
static CLICKED: AtomicBool = AtomicBool::new(false);
static CLICK_POSITION: AtomicU64 = AtomicU64::new(0);

// Raw input is queued asynchronously. A synchronous low-level mouse hook here
// would stall system-wide input during the monitor's 250 ms sleep and file I/O.
// Only subscribe while the panel is open, so a closed tray receives no motion.
struct MouseInput;

impl MouseInput {
    fn new(window: HWND) -> Result<Self> {
        let device = RAWINPUTDEVICE {
            usUsagePage: 1, // Generic desktop controls
            usUsage: 2,     // Mouse
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: window,
        };
        ensure!(
            unsafe { RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) } != 0,
            "cannot observe outside clicks: {}",
            std::io::Error::last_os_error()
        );
        Ok(Self)
    }
}

impl Drop for MouseInput {
    fn drop(&mut self) {
        let device = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 2,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: null_mut(),
        };
        unsafe { RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) };
    }
}

fn is_dismissal_click(input: &RAWINPUT) -> bool {
    input.header.dwType == RIM_TYPEMOUSE
        && unsafe { input.data.mouse.Anonymous.Anonymous.usButtonFlags }
            & (RI_MOUSE_LEFT_BUTTON_DOWN | RI_MOUSE_RIGHT_BUTTON_DOWN) as u16
            != 0
}

unsafe fn record_mouse_input(handle: HRAWINPUT) {
    unsafe {
        let mut input: RAWINPUT = std::mem::zeroed();
        let mut size = size_of::<RAWINPUT>() as u32;
        let read = GetRawInputData(
            handle,
            RID_INPUT,
            (&mut input as *mut RAWINPUT).cast(),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        );
        if read != u32::MAX
            && read as usize >= size_of::<RAWINPUTHEADER>() + size_of::<RAWMOUSE>()
            && is_dismissal_click(&input)
        {
            // Use the queued event position, not the cursor's later position
            // after a click-and-move between monitor ticks. Preserve signs for
            // monitors to the left of or above the primary display.
            let position = GetMessagePos();
            let point = POINT {
                x: position as i16 as i32,
                y: (position >> 16) as i16 as i32,
            };
            CLICK_POSITION.store(
                (point.x as u32 as u64) << 32 | point.y as u32 as u64,
                Ordering::Relaxed,
            );
            CLICKED.store(true, Ordering::Relaxed);
        }
    }
}

fn contains(rect: RECT, point: POINT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if message == TASKBAR_CREATED.load(Ordering::Relaxed) && message != 0 {
            RECREATE.store(true, Ordering::Relaxed);
            return 0;
        }
        match message {
            WM_INPUT => {
                if IsWindowVisible(hwnd) != 0 {
                    record_mouse_input(l as HRAWINPUT);
                }
                // Required cleanup for foreground raw-input messages.
                DefWindowProcW(hwnd, message, w, l)
            }
            TRAY_MESSAGE => {
                if matches!(
                    l as u32 & 0xffff,
                    NIN_SELECT | NIN_KEYSELECT | WM_CONTEXTMENU
                ) {
                    TOGGLE.store(true, Ordering::Relaxed);
                }
                0
            }
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
            WM_CLOSE => {
                ShowWindow(hwnd, SW_HIDE);
                0
            }
            WM_PAINT => {
                let mut paint: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut paint);
                let mut rect: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rect);
                FillRect(dc, &rect, GetSysColorBrush(COLOR_WINDOW));
                SetTextColor(dc, GetSysColor(COLOR_WINDOWTEXT));
                SetBkMode(dc, TRANSPARENT as i32);
                let scale = |value: i32| value * GetDpiForWindow(hwnd).max(96) as i32 / 96;
                let font = status_font(hwnd);
                let old = SelectObject(dc, font);
                let mut text = [0u16; 512];
                let len = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
                for (row, line) in text[..len as usize]
                    .split(|c| *c == b'\n' as u16)
                    .enumerate()
                {
                    let mut line_rect = RECT {
                        left: scale(18),
                        top: scale(16 + row as i32 * 28),
                        right: rect.right - scale(18),
                        bottom: scale(42 + row as i32 * 28),
                    };
                    DrawTextW(
                        dc,
                        line.as_ptr(),
                        line.len() as i32,
                        &mut line_rect,
                        DT_SINGLELINE | DT_NOPREFIX | DT_VCENTER,
                    );
                }
                SelectObject(dc, old);
                DeleteObject(font);
                EndPaint(hwnd, &paint);
                0
            }
            _ => DefWindowProcW(hwnd, message, w, l),
        }
    }
}

unsafe fn status_font(hwnd: HWND) -> HFONT {
    unsafe {
        CreateFontW(
            -(14 * GetDpiForWindow(hwnd).max(96) as i32 / 96),
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
        )
    }
}

pub struct Tray {
    window: HWND,
    icon: HICON,
    mouse_input: Option<MouseInput>,
    text: String,
    // Owned and used by the same thread as Desktop's Windows message pump.
    _main_thread: std::marker::PhantomData<*mut ()>,
}

impl Tray {
    pub fn new() -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(null());
            let class = wide("TimeTollStatus");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                ..std::mem::zeroed()
            };
            ensure!(
                RegisterClassW(&wc) != 0,
                "cannot register tray status window"
            );
            TASKBAR_CREATED.store(
                RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                Ordering::Relaxed,
            );
            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                class.as_ptr(),
                wide("TimeToll").as_ptr(),
                WS_POPUP | WS_BORDER,
                0,
                0,
                360,
                148,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            ensure!(!window.is_null(), "cannot create tray status window");
            let mut tray = Self {
                window,
                icon: null_mut(),
                mouse_input: None,
                text: String::new(),
                _main_thread: std::marker::PhantomData,
            };
            // Draw an hourglass directly into a small BGRA icon. No image
            // decoder or asset files are needed by the installed executable.
            let mut mask = [255u8; 128];
            let mut pixels = [0u8; 32 * 32 * 4];
            for y in 4usize..28 {
                for x in 6usize..26 {
                    let edge = if y < 16 {
                        7 + (y - 4) / 2
                    } else {
                        7 + (27 - y) / 2
                    };
                    if y <= 6 || y >= 25 || x.abs_diff(edge) <= 1 || x.abs_diff(31 - edge) <= 1 {
                        mask[y * 4 + x / 8] &= !(0x80 >> (x % 8));
                        pixels[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4]
                            .copy_from_slice(&[246, 130, 59, 255]);
                    }
                }
            }
            tray.icon = CreateIcon(instance, 32, 32, 1, 32, mask.as_ptr(), pixels.as_ptr());
            ensure!(!tray.icon.is_null(), "cannot create tray icon");
            ensure!(tray.add_icon(), "cannot add taskbar status icon");
            Ok(tray)
        }
    }

    fn icon_data(&self) -> NOTIFYICONDATAW {
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.window;
        data.uID = 1;
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = TRAY_MESSAGE;
        data.hIcon = self.icon;
        let tooltip = if self.text.is_empty() {
            "TimeToll"
        } else {
            &self.text
        };
        for (dest, source) in data.szTip.iter_mut().take(127).zip(tooltip.encode_utf16()) {
            *dest = source;
        }
        data
    }

    fn add_icon(&self) -> bool {
        unsafe {
            let mut data = self.icon_data();
            if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
                return false;
            }
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &data);
            true
        }
    }

    pub fn update(&mut self, snapshot: &Snapshot) {
        let text = format!(
            "TimeToll\n{}\n{}\n{}",
            snapshot.state, snapshot.balance, snapshot.progress
        );
        unsafe {
            // Also release observation if Windows closed the panel directly.
            if IsWindowVisible(self.window) == 0 {
                self.mouse_input = None;
            }
            let clicked = CLICKED.swap(false, Ordering::Relaxed);
            let click = CLICK_POSITION.load(Ordering::Relaxed);
            let mouse = POINT {
                x: (click >> 32) as i32,
                y: click as i32,
            };
            if self.text != text {
                self.text = text;
                SetWindowTextW(self.window, wide(&self.text).as_ptr());
                Shell_NotifyIconW(NIM_MODIFY, &self.icon_data());
                InvalidateRect(self.window, null(), 1);
                if IsWindowVisible(self.window) != 0 {
                    self.position();
                }
            }
            // Explorer recreates the notification area after a shell restart.
            if RECREATE.swap(false, Ordering::Relaxed) && !self.add_icon() {
                RECREATE.store(true, Ordering::Relaxed);
            }
            if TOGGLE.swap(false, Ordering::Relaxed) {
                if IsWindowVisible(self.window) != 0 {
                    self.hide_panel();
                } else {
                    match MouseInput::new(self.window) {
                        Ok(input) => self.mouse_input = Some(input),
                        Err(error) => {
                            eprintln!("{error:#}. Click the tray icon to close the status panel.")
                        }
                    }
                    self.position();
                    ShowWindow(self.window, SW_SHOWNOACTIVATE);
                }
            } else if IsWindowVisible(self.window) != 0 {
                let mut rect: RECT = std::mem::zeroed();
                GetWindowRect(self.window, &mut rect);
                let outside = !contains(rect, mouse) && !contains(self.anchor(), mouse);
                if GetAsyncKeyState(VK_ESCAPE as i32) < 0 || (clicked && outside) {
                    self.hide_panel();
                }
            }
        }
    }

    fn hide_panel(&mut self) {
        self.mouse_input = None;
        unsafe { ShowWindow(self.window, SW_HIDE) };
    }

    unsafe fn anchor(&self) -> RECT {
        unsafe {
            let identifier = NOTIFYICONIDENTIFIER {
                cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
                hWnd: self.window,
                uID: 1,
                ..std::mem::zeroed()
            };
            let mut anchor: RECT = std::mem::zeroed();
            if Shell_NotifyIconGetRect(&identifier, &mut anchor) < 0 {
                let mut point: POINT = std::mem::zeroed();
                GetCursorPos(&mut point);
                anchor = RECT {
                    left: point.x,
                    right: point.x,
                    top: point.y,
                    bottom: point.y,
                };
            }
            anchor
        }
    }

    unsafe fn position(&self) {
        unsafe {
            let anchor = self.anchor();
            let mut monitor: MONITORINFO = std::mem::zeroed();
            monitor.cbSize = size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(
                MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            ) == 0
            {
                return;
            }
            // Move onto the tray's display before measuring its DPI, but avoid
            // moving a visible panel to the icon and back on every timer tick.
            if MonitorFromWindow(self.window, MONITOR_DEFAULTTONEAREST)
                != MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST)
            {
                SetWindowPos(
                    self.window,
                    null_mut(),
                    anchor.left,
                    anchor.top,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE | SWP_NOREDRAW,
                );
            }
            let dpi = GetDpiForWindow(self.window).max(96) as i32;
            let dc = GetDC(self.window);
            let font = status_font(self.window);
            let old = SelectObject(dc, font);
            let mut width = 340 * dpi / 96;
            for line in self.text.lines() {
                let line = wide(line);
                let mut size: SIZE = std::mem::zeroed();
                GetTextExtentPoint32W(dc, line.as_ptr(), line.len() as i32 - 1, &mut size);
                width = width.max(size.cx + 40 * dpi / 96);
            }
            SelectObject(dc, old);
            DeleteObject(font);
            ReleaseDC(self.window, dc);
            let work = monitor.rcWork;
            width = width.min(work.right - work.left);
            let height = 148 * dpi / 96;
            let x = (anchor.right - width).clamp(work.left, work.right - width);
            let y = if anchor.top - height >= work.top {
                anchor.top - height
            } else {
                anchor.bottom
            };
            let y = y.clamp(work.top, (work.bottom - height).max(work.top));
            SetWindowPos(
                self.window,
                HWND_TOPMOST,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            self.mouse_input = None;
            Shell_NotifyIconW(NIM_DELETE, &self.icon_data());
            DestroyWindow(self.window);
            if !self.icon.is_null() {
                DestroyIcon(self.icon);
            }
            UnregisterClassW(wide("TimeTollStatus").as_ptr(), GetModuleHandleW(null()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mouse_button_down_requests_dismissal() {
        let mut input = RAWINPUT::default();
        input.header.dwType = RIM_TYPEMOUSE;
        for (flags, expected) in [
            (0, false), // Motion, including high-polling-rate mice
            (RI_MOUSE_LEFT_BUTTON_DOWN, true),
            (RI_MOUSE_RIGHT_BUTTON_DOWN, true),
            (RI_MOUSE_LEFT_BUTTON_UP, false),
            (RI_MOUSE_RIGHT_BUTTON_UP, false),
            (RI_MOUSE_WHEEL, false),
            (RI_MOUSE_LEFT_BUTTON_DOWN | RI_MOUSE_LEFT_BUTTON_UP, true),
        ] {
            input.data.mouse = RAWMOUSE {
                Anonymous: RAWMOUSE_0 {
                    Anonymous: RAWMOUSE_0_0 {
                        usButtonFlags: flags as u16,
                        usButtonData: 0,
                    },
                },
                ..Default::default()
            };
            assert_eq!(is_dismissal_click(&input), expected);
        }
        input.header.dwType = RIM_TYPEKEYBOARD;
        assert!(!is_dismissal_click(&input));
        input.header.dwType = RIM_TYPEHID;
        assert!(!is_dismissal_click(&input));
    }

    fn registered_mice() -> Vec<RAWINPUTDEVICE> {
        unsafe {
            let mut count = 0;
            let size = size_of::<RAWINPUTDEVICE>() as u32;
            let result = GetRegisteredRawInputDevices(null_mut(), &mut count, size);
            assert!(result != u32::MAX || GetLastError() == ERROR_INSUFFICIENT_BUFFER);
            if count == 0 {
                return vec![];
            }
            let mut devices = vec![RAWINPUTDEVICE::default(); count as usize];
            let written = GetRegisteredRawInputDevices(devices.as_mut_ptr(), &mut count, size);
            assert_ne!(written, u32::MAX);
            devices.truncate(written as usize);
            devices
                .into_iter()
                .filter(|d| d.usUsagePage == 1 && d.usUsage == 2)
                .collect()
        }
    }

    #[test]
    fn raw_mouse_subscription_is_passive_and_released_on_close() {
        struct Window(HWND);
        impl Drop for Window {
            fn drop(&mut self) {
                unsafe { DestroyWindow(self.0) };
            }
        }
        // A hidden OS window exercises registration without an Explorer tray,
        // moving the pointer, injecting input, or depending on physical devices.
        let window = Window(unsafe {
            CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("TimeToll input test").as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        });
        assert!(!window.0.is_null());
        assert!(registered_mice().is_empty());
        for _ in 0..2 {
            let input = MouseInput::new(window.0).unwrap();
            let mice = registered_mice();
            assert_eq!(mice.len(), 1);
            assert_eq!(mice[0].hwndTarget, window.0);
            // Do not capture the mouse or suppress the foreground app's input.
            assert_eq!(mice[0].dwFlags, RIDEV_INPUTSINK);
            drop(input);
            assert!(registered_mice().is_empty());
        }
    }
}
