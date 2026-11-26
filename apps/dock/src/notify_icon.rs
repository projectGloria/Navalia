//! Navalia's own icon in the notification area (system tray).
//!
//! While the dock runs, its icon sits beside the other apps' tray icons: in
//! the menu bar when it is on (its tray host keeps a copy), and in Explorer's
//! notification area otherwise. A left click opens Settings; a right click
//! offers Settings and Quit. Quitting takes the dock's usual exit path, which
//! gives the Windows taskbar back.
//!
//! The icon window lives on the UI thread. The menu bar forwards clicks with
//! `SendNotifyMessageW`, which runs this window procedure at once and maybe
//! while the app state is borrowed, so clicks are posted back to this window
//! and handled from the message loop.

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
                NOTIFYICON_VERSION_4, NOTIFYICONDATAW, NOTIFYICONDATAW_0, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW, DestroyIcon,
                DestroyWindow, GetSystemMetrics, HICON, IDI_APPLICATION, IMAGE_ICON,
                LR_DEFAULTCOLOR, LoadIconW, LoadImageW, MSGFLT_ALLOW, PostMessageW, RegisterClassW,
                RegisterWindowMessageW, SM_CXSMICON, SM_CYSMICON, WM_APP, WM_CONTEXTMENU, WM_USER,
                WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::menu::Placement;

const CLASS: PCWSTR = w!("Navalia.NotifyIcon");
const ICON_ID: u32 = 1;
/// The resource id `build.rs` gives the app icon (winresource's default).
const APP_ICON_RESOURCE: usize = 1;
/// What the notification area sends for clicks on the icon.
const CALLBACK: u32 = WM_APP + 1;
/// A click, posted back to this window to handle outside any borrow.
const OPEN_SETTINGS: u32 = WM_APP + 2;
const SHOW_MENU: u32 = WM_APP + 3;
/// `NIN_SELECT` and `NIN_KEYSELECT`: a left click, or Enter/Space on the icon.
const NIN_SELECT: u32 = WM_USER;
const NIN_KEYSELECT: u32 = WM_USER + 1;

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static ICON: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// Navalia's tray icon; removed when dropped.
pub struct NotifyIcon {
    window: HWND,
    icon: HICON,
    /// Whether `icon` was loaded from the executable and must be destroyed.
    owned: bool,
}

impl NotifyIcon {
    /// Creates the icon window and adds the icon. `None` if the window could
    /// not be made; a failed add is retried when the taskbar is (re)created.
    pub fn create() -> Option<Self> {
        // SAFETY: Window creation with a valid class and static strings; the
        // icon handle is checked before use.
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: CLASS,
                ..Default::default()
            });
            let created = RegisterWindowMessageW(w!("TaskbarCreated"));
            TASKBAR_CREATED.store(created, Ordering::Relaxed);
            // A top-level window (not message-only), so it gets the
            // `TaskbarCreated` broadcast after Explorer restarts.
            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                CLASS,
                w!("Navalia"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;
            if created != 0 {
                let _ = ChangeWindowMessageFilterEx(window, created, MSGFLT_ALLOW, None);
            }
            let loaded = LoadImageW(
                Some(instance.into()),
                PCWSTR(APP_ICON_RESOURCE as *const u16),
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                LR_DEFAULTCOLOR,
            )
            .ok()
            .filter(|handle| !handle.is_invalid());
            let (icon, owned) = match loaded {
                Some(handle) => (HICON(handle.0), true),
                None => (LoadIconW(None, IDI_APPLICATION).unwrap_or_default(), false),
            };
            WINDOW.store(window.0 as isize, Ordering::Relaxed);
            ICON.store(icon.0 as isize, Ordering::Relaxed);
            add();
            Some(Self {
                window,
                icon,
                owned,
            })
        }
    }
}

impl Drop for NotifyIcon {
    fn drop(&mut self) {
        remove();
        WINDOW.store(0, Ordering::Relaxed);
        ICON.store(0, Ordering::Relaxed);
        // SAFETY: The window and icon were created by `create` on this thread.
        unsafe {
            let _ = DestroyWindow(self.window);
            if self.owned {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}

/// Adds the icon again, for a tray host that just started: hosts only learn
/// of icons added after them, and they ask every app but Navalia to re-add.
pub fn announce_again() {
    if WINDOW.load(Ordering::Relaxed) != 0 {
        remove();
        add();
    }
}

fn data() -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: HWND(WINDOW.load(Ordering::Relaxed) as *mut _),
        uID: ICON_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: CALLBACK,
        hIcon: HICON(ICON.load(Ordering::Relaxed) as *mut _),
        Anonymous: NOTIFYICONDATAW_0 {
            uVersion: NOTIFYICON_VERSION_4,
        },
        ..Default::default()
    };
    for (slot, unit) in data.szTip.iter_mut().zip("Navalia".encode_utf16()) {
        *slot = unit;
    }
    data
}

fn add() {
    let data = data();
    // SAFETY: `data` is a fully initialized structure of the size it states.
    unsafe {
        if Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
        } else {
            navalia_shell::log::line("could not add the Navalia tray icon");
        }
    }
}

fn remove() {
    let data = data();
    // SAFETY: As in `add`; removing an icon that is not there is harmless.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

/// Where a menu opened from `anchor` fits: below it near the top of its
/// screen (the menu bar), above it otherwise (Explorer's taskbar).
fn placement_for(anchor: POINT) -> Placement {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: Read-only queries with a valid, sized output structure.
    let found = unsafe {
        GetMonitorInfoW(
            MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST),
            &mut info,
        )
        .as_bool()
    };
    if found && anchor.y < (info.rcMonitor.top + info.rcMonitor.bottom) / 2 {
        Placement::Below
    } else {
        Placement::Above
    }
}

/// Splits `NOTIFYICON_VERSION_4`'s anchor, two signed 16-bit coordinates.
fn anchor_of(packed: usize) -> POINT {
    POINT {
        x: i32::from(packed as u16 as i16),
        y: i32::from((packed >> 16) as u16 as i16),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        CALLBACK => {
            // Version 4: the event in the low word of `lParam`, the anchor in `wParam`.
            let event = (l.0 as usize & 0xFFFF) as u32;
            let next = match event {
                NIN_SELECT | NIN_KEYSELECT => Some(OPEN_SETTINGS),
                WM_CONTEXTMENU => Some(SHOW_MENU),
                _ => None,
            };
            if let Some(next) = next {
                // SAFETY: Posting to this live window; no pointers are carried.
                unsafe {
                    let _ = PostMessageW(Some(hwnd), next, w, LPARAM(0));
                }
            }
        }
        OPEN_SETTINGS => crate::app::open_settings(),
        SHOW_MENU => {
            let anchor = anchor_of(w.0);
            crate::app::show_navalia_menu(anchor, placement_for(anchor));
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => add(),
        // SAFETY: Default handling for this window's other messages.
        _ => return unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
    LRESULT(0)
}

#[cfg(test)]
mod tests {
    use super::anchor_of;

    #[test]
    fn anchor_keeps_negative_coordinates_of_left_monitors() {
        let packed = (0xFFF6_usize << 16) | 0xFF9C;
        let point = anchor_of(packed);
        assert_eq!((point.x, point.y), (-100, -10));
        let point = anchor_of((1040 << 16) | 1900);
        assert_eq!((point.x, point.y), (1900, 1040));
    }
}
