//! The notification area (system tray) icons of other apps, for the bar.
//!
//! Windows has no API that lists another app's tray icons, and on Windows 11
//! Explorer no longer exposes them to UI Automation. But an app does not
//! talk to the taskbar directly: `Shell_NotifyIcon` finds the first window of
//! class `Shell_TrayWnd` and sends it a `WM_COPYDATA` describing the change.
//! Like ManagedShell-based taskbars (RetroBar, Cairo) with Explorer running,
//! [`TrayHost`] puts a hidden, topmost window of that class in front of
//! Explorer's: it keeps a copy of each icon (its picture, tooltip and where
//! to send clicks) and forwards every message to Explorer unchanged, so
//! Explorer stays in sync and takes over again when the dock stops.
//!
//! Icons added before the host existed are asked for again by posting
//! `TaskbarCreated`, the message Explorer broadcasts after it restarts.

use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};

use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT,
            RECT, WPARAM,
        },
        System::{
            DataExchange::COPYDATASTRUCT,
            LibraryLoader::GetModuleHandleW,
            Threading::{GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
        UI::WindowsAndMessaging::{
            AllowSetForegroundWindow, ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW,
            DestroyWindow, DispatchMessageW, EnumWindows, FindWindowW, GetClassNameW, GetMessageW,
            GetPropW, GetWindowRect, GetWindowThreadProcessId, HICON, HWND_TOPMOST, IsWindow,
            KillTimer, MSG, MSGFLT_ALLOW, PostMessageW, PostQuitMessage, RegisterClassW,
            RegisterWindowMessageW, RemovePropW, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SendMessageTimeoutW, SendNotifyMessageW,
            SetPropW, SetTimer, SetWindowPos, TranslateMessage, WINDOW_EX_STYLE, WM_CLOSE,
            WM_COMMAND, WM_CONTEXTMENU, WM_COPYDATA, WM_DESTROY, WM_HOTKEY, WM_LBUTTONDBLCLK,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_TIMER, WM_USER,
            WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
    },
    core::{BOOL, PCWSTR, w},
};

use crate::icons::{IconImage, icon_image};

/// Property that marks the dock's own `Shell_TrayWnd`, so the dock (and its
/// watchdog, another process) still find Explorer's taskbar behind it.
pub const HOST_MARKER: PCWSTR = w!("NavaliaTrayHost");
const TRAY_CLASS: PCWSTR = w!("Shell_TrayWnd");

/// `WM_COPYDATA` kinds sent to the taskbar: appbar calls are 0, tray icon
/// changes 1, `Shell_NotifyIconGetRect` 3. Only 1 is read; all are forwarded.
const COPYDATA_TRAY: usize = 1;
const TRAY_SIGNATURE: u32 = 0x3475_3423;

// Offsets in the message: a signature and the `NIM_*` call, then a
// `NOTIFYICONDATAW` laid out for 32-bit processes (handles as 32 bits),
// whatever the sender's bitness.
const HEADER: usize = 8;
const NID_HWND: usize = HEADER + 4;
const NID_ID: usize = HEADER + 8;
const NID_FLAGS: usize = HEADER + 12;
const NID_CALLBACK: usize = HEADER + 16;
const NID_ICON: usize = HEADER + 20;
const NID_TIP: usize = HEADER + 24;
const TIP_CHARS: usize = 128;
const NID_STATE: usize = HEADER + 280;
const NID_STATE_MASK: usize = HEADER + 284;
const NID_VERSION: usize = HEADER + 800;
const NID_GUID: usize = HEADER + 936;

const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
const NIF_MESSAGE: u32 = 0x01;
const NIF_ICON: u32 = 0x02;
const NIF_TIP: u32 = 0x04;
const NIF_STATE: u32 = 0x08;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x01;
/// `NIN_SELECT`: what a version 3+ icon gets after a left click.
const NIN_SELECT: u32 = WM_USER;

const HOST_TIMER: usize = 1;
/// How often the host checks it is still in front of Explorer's taskbar,
/// and drops icons whose app died without removing them.
const HOST_TICK_MS: u32 = 250;
/// How long Explorer may take to answer a forwarded message.
const FORWARD_TIMEOUT_MS: u32 = 1_000;

/// Who an icon is: apps name it by a GUID, or by their window and a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconKey {
    Guid(u128),
    Window { hwnd: isize, uid: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrayIcon {
    pub key: IconKey,
    /// The window that gets the icon's clicks.
    pub owner: isize,
    /// The process that owned `owner` when it was recorded: a handle Windows
    /// reuses for another process's window no longer counts as alive.
    pub owner_pid: u32,
    /// The owner's executable file name, lowercase (`discord.exe`); empty
    /// when it could not be read. Unlike `owner`, it survives restarts.
    pub program: String,
    pub uid: u32,
    /// The message the owner wants clicks as; 0 when it gave none.
    pub callback: u32,
    /// `NOTIFYICON_VERSION*` the owner asked for; changes the click messages.
    pub version: u32,
    pub tip: String,
    pub hidden: bool,
    pub image: Option<Arc<IconImage>>,
    /// Changes whenever the picture does, to key cached drawings.
    pub serial: u64,
}

impl TrayIcon {
    /// A stable text id for caches.
    pub fn id(&self) -> String {
        match self.key {
            IconKey::Guid(guid) => format!("{guid:032x}"),
            IconKey::Window { hwnd, uid } => format!("{hwnd:x}:{uid}"),
        }
    }

    /// An id that stays the same when the app or the computer restarts, for
    /// remembering the user's choices about the icon (hiding it): the
    /// program, then the GUID the app names the icon by or its number.
    pub fn preference_key(&self) -> String {
        let program = if self.program.is_empty() {
            "?"
        } else {
            &self.program
        };
        match self.key {
            IconKey::Guid(guid) => format!("tray:{program}:{guid:032x}"),
            IconKey::Window { uid, .. } => format!("tray:{program}:{uid}"),
        }
    }

    /// A name for the icon in menus: the first line of its tooltip, or the
    /// program without `.exe`.
    pub fn display_name(&self) -> String {
        let tip = self.tip.lines().next().unwrap_or("").trim();
        if !tip.is_empty() {
            return tip.to_owned();
        }
        program_label(&self.program)
    }

    /// Worth a place on the bar: visible, with a picture and a way to click it.
    pub fn shown(&self) -> bool {
        !self.hidden && self.image.is_some() && self.callback != 0
    }
}

/// One `Shell_NotifyIcon` call, as read from the message.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Notify {
    message: u32,
    hwnd: isize,
    uid: u32,
    flags: u32,
    callback: u32,
    icon: isize,
    tip: Option<String>,
    state: u32,
    state_mask: u32,
    version: u32,
    guid: Option<u128>,
}

impl Notify {
    fn key(&self) -> IconKey {
        match self.guid {
            Some(guid) => IconKey::Guid(guid),
            None => IconKey::Window {
                hwnd: self.hwnd,
                uid: self.uid,
            },
        }
    }
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let field = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
}

/// A 32-bit handle widened as Windows does: sign-extended.
fn read_handle(bytes: &[u8], at: usize) -> Option<isize> {
    read_u32(bytes, at).map(|value| value as i32 as isize)
}

/// Reads a tray message; `None` if it is not one or is cut short. Fields past
/// a short message read as absent.
fn parse_notify(bytes: &[u8]) -> Option<Notify> {
    if read_u32(bytes, 0)? != TRAY_SIGNATURE {
        return None;
    }
    let message = read_u32(bytes, 4)?;
    let hwnd = read_handle(bytes, NID_HWND)?;
    let uid = read_u32(bytes, NID_ID)?;
    let flags = read_u32(bytes, NID_FLAGS)?;
    let callback = read_u32(bytes, NID_CALLBACK).unwrap_or(0);
    let icon = read_handle(bytes, NID_ICON).unwrap_or(0);
    let tip = (flags & NIF_TIP != 0)
        .then(|| bytes.get(NID_TIP..NID_TIP + TIP_CHARS * 2))
        .flatten()
        .map(|raw| {
            let units: Vec<u16> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .take_while(|&unit| unit != 0)
                .collect();
            String::from_utf16_lossy(&units)
        });
    let guid = (flags & NIF_GUID != 0)
        .then(|| bytes.get(NID_GUID..NID_GUID + 16))
        .flatten()
        .and_then(|raw| raw.try_into().ok())
        .map(u128::from_le_bytes)
        .filter(|&guid| guid != 0);
    Some(Notify {
        message,
        hwnd,
        uid,
        flags,
        callback,
        icon,
        tip,
        state: read_u32(bytes, NID_STATE).unwrap_or(0),
        state_mask: read_u32(bytes, NID_STATE_MASK).unwrap_or(0),
        version: read_u32(bytes, NID_VERSION).unwrap_or(0),
        guid,
    })
}

/// Applies one call to the icon list. `image` is the picture already read
/// from the call's icon (only looked at with `NIF_ICON`); `accepted` is
/// whether Explorer took the call. Returns whether anything changed.
fn apply(
    icons: &mut Vec<TrayIcon>,
    notify: &Notify,
    image: Option<Arc<IconImage>>,
    accepted: bool,
    serial: &AtomicU64,
) -> bool {
    let key = notify.key();
    let found = icons.iter().position(|icon| icon.key == key);
    match (notify.message, found) {
        (NIM_DELETE, Some(index)) => {
            icons.remove(index);
            true
        }
        (NIM_SETVERSION, Some(index)) => {
            icons[index].version = notify.version;
            false
        }
        (NIM_ADD | NIM_MODIFY, Some(index)) => update(&mut icons[index], notify, image, serial),
        // Explorer refuses an add it already has: icons re-sent after
        // `TaskbarCreated` are kept if they carry a window and visible/callback
        // data, while truly rejected empty adds are dropped.
        (NIM_ADD, None)
            if accepted
                || (notify.hwnd != 0 && notify.flags & (NIF_ICON | NIF_MESSAGE | NIF_TIP) != 0) =>
        {
            icons.push(new_icon(notify, image, serial));
            true
        }
        // A change to an icon added before the host existed: kept if
        // Explorer knows it and the call says where its clicks go.
        (NIM_MODIFY, None) if accepted && notify.flags & NIF_MESSAGE != 0 => {
            icons.push(new_icon(notify, image, serial));
            true
        }
        // The version of an icon added before the host existed: kept, not
        // shown (no picture or click message yet), so the clicks that later
        // reach it use the format its owner asked for.
        (NIM_SETVERSION, None) if accepted => {
            let mut icon = new_icon(notify, None, serial);
            icon.version = notify.version;
            icons.push(icon);
            false
        }
        _ => false,
    }
}

fn new_icon(notify: &Notify, image: Option<Arc<IconImage>>, serial: &AtomicU64) -> TrayIcon {
    let mut icon = TrayIcon {
        key: notify.key(),
        owner: notify.hwnd,
        owner_pid: 0,
        program: String::new(),
        uid: notify.uid,
        callback: 0,
        version: 0,
        tip: String::new(),
        hidden: false,
        image: None,
        serial: 0,
    };
    update(&mut icon, notify, image, serial);
    icon
}

fn update(
    icon: &mut TrayIcon,
    notify: &Notify,
    image: Option<Arc<IconImage>>,
    serial: &AtomicU64,
) -> bool {
    let before = icon.clone();
    if notify.hwnd != 0 {
        if icon.owner != notify.hwnd || icon.owner_pid == 0 {
            icon.owner_pid = window_pid(notify.hwnd);
            icon.program = process_program(icon.owner_pid);
        }
        icon.owner = notify.hwnd;
        icon.uid = notify.uid;
    }
    if notify.flags & NIF_MESSAGE != 0 {
        icon.callback = notify.callback;
    }
    if let Some(tip) = &notify.tip {
        icon.tip.clone_from(tip);
    }
    if notify.flags & NIF_STATE != 0 && notify.state_mask & NIS_HIDDEN != 0 {
        icon.hidden = notify.state & NIS_HIDDEN != 0;
    }
    if notify.flags & NIF_ICON != 0 && icon.image != image {
        icon.image = image;
        icon.serial = serial.fetch_add(1, Ordering::Relaxed) + 1;
    }
    *icon != before
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayClick {
    Left,
    Double,
    Right,
}

/// The mouse messages Explorer sends for a click, in order.
fn click_events(click: TrayClick, version: u32) -> Vec<u32> {
    let modern = version >= 3;
    let mut events = match click {
        TrayClick::Left => vec![WM_LBUTTONDOWN, WM_LBUTTONUP],
        TrayClick::Double => vec![WM_LBUTTONDBLCLK, WM_LBUTTONUP],
        TrayClick::Right => vec![WM_RBUTTONDOWN, WM_RBUTTONUP],
    };
    match click {
        TrayClick::Left if modern => events.push(NIN_SELECT),
        TrayClick::Right if modern => events.push(WM_CONTEXTMENU),
        _ => {}
    }
    events
}

/// The callback's parameters: version 4 icons get the anchor point and the
/// event with the icon's id; older ones the id and the event.
fn callback_params(version: u32, uid: u32, event: u32, (x, y): (i32, i32)) -> (usize, isize) {
    if version >= 4 {
        let anchor = (x as u16 as usize) | ((y as u16 as usize) << 16);
        let event = ((event & 0xFFFF) | ((uid & 0xFFFF) << 16)) as isize;
        (anchor, event)
    } else {
        (uid as usize, event as isize)
    }
}

/// Clicks `icon` as the taskbar would, `anchor` being the screen point the
/// app may open its menu or flyout at. `false` if its app is gone.
pub fn click(icon: &TrayIcon, click: TrayClick, anchor: (i32, i32)) -> bool {
    let owner = HWND(icon.owner as *mut _);
    // SAFETY: Read-only check on a window handle.
    if icon.callback == 0 || !unsafe { IsWindow(Some(owner)) }.as_bool() {
        return false;
    }
    let mut pid = 0u32;
    // SAFETY: Read-only lookups; the app may come to the foreground, as it
    // does when the real taskbar sends the click.
    unsafe {
        GetWindowThreadProcessId(owner, Some(&mut pid));
        if pid != 0 {
            let _ = AllowSetForegroundWindow(pid);
        }
    }
    for event in click_events(click, icon.version) {
        let (wparam, lparam) = callback_params(icon.version, icon.uid, event, anchor);
        // SAFETY: Notifications without pointers; they return at once.
        let sent =
            unsafe { SendNotifyMessageW(owner, icon.callback, WPARAM(wparam), LPARAM(lparam)) };
        if sent.is_err() {
            return false;
        }
    }
    true
}

struct Shared {
    icons: Mutex<Vec<TrayIcon>>,
    serial: AtomicU64,
    /// A change notification is on its way to the bar.
    posted: AtomicBool,
    /// Counts changes to `icons`; `seen` is the count the bar last read.
    /// The host's timer posts again while they differ, so a notification
    /// the bar dropped (it was busy) cannot leave it showing stale icons.
    version: AtomicU64,
    seen: AtomicU64,
    notify_window: isize,
    notify_message: u32,
}

impl Shared {
    fn changed(&self) {
        self.version.fetch_add(1, Ordering::AcqRel);
        if !self.posted.swap(true, Ordering::AcqRel) {
            self.post();
        }
    }

    /// Posts again if the bar has not read the latest icons.
    fn repost_unseen(&self) {
        if self.version.load(Ordering::Acquire) != self.seen.load(Ordering::Acquire) {
            self.posted.store(true, Ordering::Release);
            self.post();
        }
    }

    fn post(&self) {
        // SAFETY: A payload-free notification to the bar's window.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.notify_window as *mut _)),
                self.notify_message,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

thread_local! {
    /// The host's state, for its window procedure; the host has one thread.
    static HOST: RefCell<Option<Arc<Shared>>> = const { RefCell::new(None) };
}

fn host_shared() -> Option<Arc<Shared>> {
    HOST.with(|host| host.borrow().clone())
}

/// Receives the other apps' tray icons while it lives; see the module docs.
pub struct TrayHost {
    shared: Arc<Shared>,
    window: isize,
    thread: Option<JoinHandle<()>>,
}

impl TrayHost {
    /// Starts the host on its own thread. `notify_message` is posted to
    /// `notify_window` when the icons change, then again only after
    /// [`TrayHost::icons`] has been called. `None` if the window could not be
    /// made.
    pub fn start(notify_window: isize, notify_message: u32) -> Option<Self> {
        let shared = Arc::new(Shared {
            icons: Mutex::new(Vec::new()),
            serial: AtomicU64::new(0),
            posted: AtomicBool::new(false),
            version: AtomicU64::new(0),
            seen: AtomicU64::new(0),
            notify_window,
            notify_message,
        });
        let (ready, created) = mpsc::channel();
        let thread_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("navalia-tray".into())
            .spawn(move || run_host(thread_shared, &ready))
            .ok()?;
        match created.recv() {
            Ok(Some(window)) => Some(Self {
                shared,
                window,
                thread: Some(thread),
            }),
            _ => {
                let _ = thread.join();
                None
            }
        }
    }

    /// The icons now, in the order they were added; allows the next change
    /// notification.
    pub fn icons(&self) -> Vec<TrayIcon> {
        // Read the count first: a change made while copying posts again.
        let version = self.shared.version.load(Ordering::Acquire);
        self.shared.posted.store(false, Ordering::Release);
        self.shared.seen.store(version, Ordering::Release);
        self.shared
            .icons
            .lock()
            .map(|icons| icons.clone())
            .unwrap_or_default()
    }

    /// Drops the icons of apps that are gone now, instead of at the next
    /// tick; for a click that found its app closed.
    pub fn drop_dead_icons(&self) {
        drop_orphans(&self.shared);
    }
}

impl Drop for TrayHost {
    fn drop(&mut self) {
        // SAFETY: Asks the host's own thread to close its window.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.window as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

static HOST_HWND: AtomicIsize = AtomicIsize::new(0);
static YIELD_DEPTH: AtomicUsize = AtomicUsize::new(0);
static ZORDER_LOCK: Mutex<()> = Mutex::new(());

/// Temporarily places the tray host behind Explorer's `Shell_TrayWnd` so
/// `SHAppBarMessage` (which resolves `FindWindowW("Shell_TrayWnd", NULL)` and
/// allocates shared memory for that window's PID) talks directly to Explorer.
pub(crate) fn with_explorer_in_front<T>(f: impl FnOnce() -> T) -> T {
    struct YieldGuard {
        moved_host: Option<HWND>,
    }

    impl Drop for YieldGuard {
        fn drop(&mut self) {
            if let Some(host) = self.moved_host {
                // SAFETY: Read-only validity check followed by z-order restore on our own window.
                unsafe {
                    if IsWindow(Some(host)).as_bool() {
                        let _ = SetWindowPos(
                            host,
                            Some(HWND_TOPMOST),
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                        );
                    }
                }
            }
            YIELD_DEPTH.fetch_sub(1, Ordering::SeqCst);
        }
    }

    let _lock = ZORDER_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    YIELD_DEPTH.fetch_add(1, Ordering::SeqCst);
    let host_raw = HOST_HWND.load(Ordering::Acquire);
    let moved_host = if host_raw != 0
        && let Some(explorer) = crate::taskbar::explorer_tray()
    {
        let host = HWND(host_raw as *mut _);
        // SAFETY: Places our hidden tray host directly behind Explorer's `Shell_TrayWnd`.
        unsafe {
            let _ = SetWindowPos(
                host,
                Some(explorer),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
        Some(host)
    } else {
        None
    };
    let _guard = YieldGuard { moved_host };
    f()
}

fn run_host(shared: Arc<Shared>, ready: &mpsc::Sender<Option<isize>>) {
    HOST.with(|host| *host.borrow_mut() = Some(shared));
    let Some(window) = create_host_window() else {
        let _ = ready.send(None);
        return;
    };
    HOST_HWND.store(window.0 as isize, Ordering::Release);
    let _ = ready.send(Some(window.0 as isize));
    keep_in_front(window);
    ask_for_icons();
    let mut msg = MSG::default();
    // SAFETY: A standard message loop for the host's window on this thread.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    let _ = HOST_HWND.compare_exchange(window.0 as isize, 0, Ordering::AcqRel, Ordering::Relaxed);
    HOST.with(|host| *host.borrow_mut() = None);
}

fn create_host_window() -> Option<HWND> {
    // SAFETY: Registers and creates a window of this module's procedure;
    // the class may remain from an earlier host in this process.
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(host_proc),
            hInstance: instance.into(),
            lpszClassName: TRAY_CLASS,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return None;
        }
        let rect = explorer_rect().unwrap_or_default();
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0),
            TRAY_CLASS,
            None,
            WS_POPUP,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        let _ = SetPropW(window, HOST_MARKER, Some(HANDLE(std::ptr::dangling_mut())));
        // Apps running elevated must still reach the tray.
        let _ = ChangeWindowMessageFilterEx(window, WM_COPYDATA, MSGFLT_ALLOW, None);
        SetTimer(Some(window), HOST_TIMER, HOST_TICK_MS, None);
        Some(window)
    }
}

fn explorer_rect() -> Option<RECT> {
    let explorer = crate::taskbar::explorer_tray()?;
    let mut rect = RECT::default();
    // SAFETY: Read-only lookup into a local.
    unsafe { GetWindowRect(explorer, &mut rect) }.ok()?;
    Some(rect)
}

/// Stays the window apps find first, with Explorer's taskbar area (some
/// apps place things by it) and its task band property (see `taskband`).
fn keep_in_front(window: HWND) {
    if YIELD_DEPTH.load(Ordering::Acquire) > 0 {
        return;
    }
    // SAFETY: Lookups and changes on the host's own hidden window.
    unsafe {
        if FindWindowW(TRAY_CLASS, None).ok() != Some(window) {
            let _ = SetWindowPos(
                window,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
        let Some(explorer) = crate::taskbar::explorer_tray() else {
            return;
        };
        let (mut theirs, mut ours) = (RECT::default(), RECT::default());
        if GetWindowRect(explorer, &mut theirs).is_ok()
            && GetWindowRect(window, &mut ours).is_ok()
            && theirs != ours
        {
            let _ = SetWindowPos(
                window,
                None,
                theirs.left,
                theirs.top,
                theirs.right - theirs.left,
                theirs.bottom - theirs.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        let band = GetPropW(explorer, crate::taskband::PROPERTY);
        if GetPropW(window, crate::taskband::PROPERTY) != band {
            let _ = SetPropW(window, crate::taskband::PROPERTY, Some(band));
        }
    }
}

/// Drops the icons of windows that are gone: an app that crashed never
/// removes its icon.
fn drop_orphans(shared: &Shared) {
    let Ok(mut icons) = shared.icons.lock() else {
        return;
    };
    let before = icons.len();
    // A handle that now belongs to another process was reused: the icon's
    // owner is gone.
    icons.retain(|icon| {
        // SAFETY: Read-only check on a window handle.
        unsafe { IsWindow(Some(HWND(icon.owner as *mut _))) }.as_bool()
            && (icon.owner_pid == 0 || window_pid(icon.owner) == icon.owner_pid)
    });
    let changed = icons.len() != before;
    drop(icons);
    if changed {
        shared.changed();
    }
}

/// The process that owns `window`; 0 if it is gone or not a window.
fn window_pid(window: isize) -> u32 {
    if window == 0 {
        return 0;
    }
    let mut pid = 0u32;
    // SAFETY: Read-only lookup; an invalid handle leaves `pid` at 0.
    unsafe { GetWindowThreadProcessId(HWND(window as *mut _), Some(&mut pid)) };
    pid
}

/// The file name of the executable `pid` runs, lowercase; empty if unknown.
fn process_program(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    // SAFETY: Opens the process only to read its image name, then closes it.
    let image = unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let read = crate::paths::query_process_image(process);
        let _ = CloseHandle(process);
        read
    };
    image
        .as_deref()
        .and_then(|path| path.rsplit('\\').next())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// `discord.exe` as `Discord`, for an icon with no tooltip.
pub fn program_label(program: &str) -> String {
    let stem = program.strip_suffix(".exe").unwrap_or(program);
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Whether `pid` runs a `navalia*` executable from the folder Navalia is
/// installed in (the dock, its watchdog and workers, Settings).
fn is_navalia_process(pid: u32, folder: Option<&std::path::Path>) -> bool {
    let Some(folder) = folder else {
        return false;
    };
    // SAFETY: Opens the process only to read its image name, then closes it.
    let image = unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let read = crate::paths::query_process_image(process);
        let _ = CloseHandle(process);
        let Some(read) = read else {
            return false;
        };
        std::path::PathBuf::from(read)
    };
    image
        .parent()
        .is_some_and(|parent| parent.as_os_str().eq_ignore_ascii_case(folder.as_os_str()))
        && image
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().starts_with("navalia"))
}

/// Posts `TaskbarCreated` to the other apps' windows, as Explorer does after
/// it restarts: each adds its icons again, now through the host. Navalia's
/// own processes are left out: for the dock it means taking the taskbar
/// again, and the others have no icons to add.
fn ask_for_icons() {
    unsafe extern "system" fn collect(hwnd: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` points to the Vec below, alive for the enumeration.
        unsafe { (*(found.0 as *mut Vec<HWND>)).push(hwnd) };
        true.into()
    }
    // SAFETY: Registers a well-known message name.
    let created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    if created == 0 {
        return;
    }
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: The callback only pushes into `windows`, which outlives the call.
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&raw mut windows as isize));
    }
    // SAFETY: Read-only lookup of this process's id.
    let own = unsafe { GetCurrentProcessId() };
    let folder = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
    let mut navalia = std::collections::HashMap::new();
    for window in windows {
        let pid = window_pid(window.0 as isize);
        if pid == own || pid == 0 || is_taskbar(window) {
            continue;
        }
        if *navalia
            .entry(pid)
            .or_insert_with(|| is_navalia_process(pid, folder.as_deref()))
        {
            continue;
        }
        // SAFETY: A payload-free post.
        let _ = unsafe { PostMessageW(Some(window), created, WPARAM(0), LPARAM(0)) };
    }
}

fn is_taskbar_created(msg: u32) -> bool {
    use std::sync::OnceLock;
    static CREATED: OnceLock<u32> = OnceLock::new();
    // SAFETY: Registers a well-known message name.
    msg == *CREATED.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

fn is_taskbar(window: HWND) -> bool {
    let mut class = [0u16; 32];
    // SAFETY: `class` is a valid buffer.
    let len = unsafe { GetClassNameW(window, &mut class) };
    matches!(
        String::from_utf16_lossy(&class[..len.max(0) as usize]).as_str(),
        "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

/// Sends a message on to Explorer's taskbar; `None` if there is none or it
/// did not answer in time.
fn forward(msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let explorer = crate::taskbar::explorer_tray()?;
    let mut result = 0usize;
    // SAFETY: Forwards the message as received; Windows copies `WM_COPYDATA`
    // payloads across processes, and the original stays valid until we return.
    let sent = unsafe {
        SendMessageTimeoutW(
            explorer,
            msg,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG,
            FORWARD_TIMEOUT_MS,
            Some(&mut result),
        )
    };
    (sent.0 != 0).then_some(LRESULT(result as isize))
}

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_COPYDATA => {
            let answer = forward(msg, w, l);
            // SAFETY: For `WM_COPYDATA`, `l` points to a COPYDATASTRUCT whose
            // `cbData` bytes at `lpData` stay valid for this call.
            let payload = unsafe {
                let data = (l.0 as *const COPYDATASTRUCT).as_ref();
                data.filter(|data| data.dwData == COPYDATA_TRAY && !data.lpData.is_null())
                    .map(|data| {
                        std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize)
                    })
            };
            if let Some(notify) = payload.and_then(parse_notify) {
                let accepted = answer.is_some_and(|result| result.0 != 0);
                note(&notify, accepted);
                // Without Explorer, the call still reached a tray: ours.
                return answer.unwrap_or(LRESULT(1));
            }
            answer.unwrap_or(LRESULT(0))
        }
        // Commands, hotkeys, private and registered messages apps send the
        // taskbar (show the desktop, for one) are Explorer's to answer. Its
        // own `TaskbarCreated` broadcast is not sent back to it.
        WM_COMMAND | WM_HOTKEY | WM_USER.. if !is_taskbar_created(msg) => {
            forward(msg, w, l).unwrap_or(LRESULT(0))
        }
        WM_TIMER if w.0 == HOST_TIMER => {
            keep_in_front(hwnd);
            if let Some(shared) = host_shared() {
                drop_orphans(&shared);
                shared.repost_unseen();
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: The host's own window, on its thread.
            unsafe {
                let _ = KillTimer(Some(hwnd), HOST_TIMER);
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: Properties this module set on its own window.
            unsafe {
                let _ = RemovePropW(hwnd, HOST_MARKER);
                let _ = RemovePropW(hwnd, crate::taskband::PROPERTY);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        // SAFETY: Default handling for our own window.
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

/// Records a tray call. The picture is read before taking the lock: the
/// procedure can be re-entered while it waits on Explorer.
fn note(notify: &Notify, accepted: bool) {
    let Some(shared) = host_shared() else {
        return;
    };
    let image = (notify.flags & NIF_ICON != 0 && notify.icon != 0)
        .then(|| icon_image(HICON(notify.icon as *mut _)))
        .flatten()
        .map(Arc::new);
    let changed = match shared.icons.lock() {
        Ok(mut icons) => apply(&mut icons, notify, image, accepted, &shared.serial),
        Err(_) => false,
    };
    if changed {
        shared.changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NID_SIZE: usize = 956;

    fn message(call: u32, flags: u32, fill: impl FnOnce(&mut [u8])) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER + NID_SIZE];
        let mut put = |at: usize, value: u32| {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        };
        put(0, TRAY_SIGNATURE);
        put(4, call);
        put(HEADER, NID_SIZE as u32);
        put(NID_HWND, 0xFFFF_1234);
        put(NID_ID, 7);
        put(NID_FLAGS, flags);
        put(NID_CALLBACK, 0x8001);
        put(NID_ICON, 0x0004_0010);
        fill(&mut bytes);
        bytes
    }

    fn put_tip(bytes: &mut [u8], tip: &str) {
        for (index, unit) in tip.encode_utf16().enumerate() {
            let at = NID_TIP + index * 2;
            bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    fn image(shade: u8) -> Option<Arc<IconImage>> {
        Some(Arc::new(IconImage {
            width: 1,
            height: 1,
            bgra: vec![shade, shade, shade, 255],
        }))
    }

    #[test]
    fn the_bar_is_told_again_until_it_reads_the_latest_icons() {
        let shared = Arc::new(Shared {
            icons: Mutex::new(Vec::new()),
            serial: AtomicU64::new(0),
            posted: AtomicBool::new(false),
            version: AtomicU64::new(0),
            seen: AtomicU64::new(0),
            notify_window: 0,
            notify_message: 0,
        });
        let host = TrayHost {
            shared: Arc::clone(&shared),
            window: 0,
            thread: None,
        };
        let unseen =
            || shared.version.load(Ordering::Acquire) != shared.seen.load(Ordering::Acquire);
        assert!(!unseen());
        // The first notice is dropped by a busy bar: still unseen, so the
        // host's tick posts again.
        shared.changed();
        shared.changed();
        assert!(unseen());
        assert!(shared.posted.load(Ordering::Acquire));
        let _ = host.icons();
        assert!(!unseen());
        assert!(!shared.posted.load(Ordering::Acquire));
        shared.repost_unseen();
        assert!(!shared.posted.load(Ordering::Acquire));
        drop(host);
    }

    #[test]
    fn reads_an_add_with_its_tip_and_sign_extended_handles() -> Result<(), String> {
        let bytes = message(NIM_ADD, NIF_MESSAGE | NIF_ICON | NIF_TIP, |bytes| {
            put_tip(bytes, "Discord");
        });
        let notify = parse_notify(&bytes).ok_or("not read")?;
        assert_eq!(notify.message, NIM_ADD);
        assert_eq!(notify.hwnd, 0xFFFF_1234_u32 as i32 as isize);
        assert_eq!(notify.uid, 7);
        assert_eq!(notify.callback, 0x8001);
        assert_eq!(notify.icon, 0x0004_0010);
        assert_eq!(notify.tip.as_deref(), Some("Discord"));
        assert_eq!(notify.guid, None);
        Ok(())
    }

    #[test]
    fn rejects_other_payloads_and_tolerates_short_ones() {
        let mut bytes = message(NIM_ADD, NIF_TIP | NIF_GUID, |_| {});
        bytes[0] = 0;
        assert_eq!(parse_notify(&bytes), None);
        assert_eq!(parse_notify(&[0u8; 6]), None);

        let short = message(NIM_MODIFY, NIF_TIP | NIF_GUID, |_| {});
        let notify = parse_notify(&short[..NID_ICON + 4]);
        assert!(notify.is_some_and(|notify| notify.tip.is_none() && notify.guid.is_none()));
    }

    #[test]
    fn an_icon_named_by_guid_is_keyed_by_it() -> Result<(), String> {
        let bytes = message(NIM_ADD, NIF_GUID, |bytes| {
            bytes[NID_GUID] = 0xAB;
        });
        let notify = parse_notify(&bytes).ok_or("not read")?;
        assert_eq!(notify.key(), IconKey::Guid(0xAB));
        Ok(())
    }

    #[test]
    fn follows_an_icon_from_add_to_delete() -> Result<(), String> {
        let serial = AtomicU64::new(0);
        let mut icons = Vec::new();
        let add = parse_notify(&message(
            NIM_ADD,
            NIF_MESSAGE | NIF_ICON | NIF_TIP,
            |bytes| {
                put_tip(bytes, "Steam");
            },
        ))
        .ok_or("not read")?;
        assert!(apply(&mut icons, &add, image(1), false, &serial));
        assert_eq!(icons.len(), 1);
        assert!(icons[0].shown());
        assert_eq!(icons[0].tip, "Steam");
        let first = icons[0].serial;

        // The same picture again changes nothing; a new one is a new serial.
        assert!(!apply(&mut icons, &add, image(1), true, &serial));
        let modify = parse_notify(&message(NIM_MODIFY, NIF_ICON, |_| {})).ok_or("not read")?;
        assert!(apply(&mut icons, &modify, image(2), true, &serial));
        assert_ne!(icons[0].serial, first);
        assert_eq!(icons[0].tip, "Steam");

        let hide = parse_notify(&message(NIM_MODIFY, NIF_STATE, |bytes| {
            bytes[NID_STATE] = NIS_HIDDEN as u8;
            bytes[NID_STATE_MASK] = NIS_HIDDEN as u8;
        }))
        .ok_or("not read")?;
        assert!(apply(&mut icons, &hide, None, true, &serial));
        assert!(!icons[0].shown());

        let version = parse_notify(&message(NIM_SETVERSION, 0, |bytes| {
            bytes[NID_VERSION] = 4;
        }))
        .ok_or("not read")?;
        apply(&mut icons, &version, None, true, &serial);
        assert_eq!(icons[0].version, 4);

        let delete = parse_notify(&message(NIM_DELETE, 0, |_| {})).ok_or("not read")?;
        assert!(apply(&mut icons, &delete, None, true, &serial));
        assert!(icons.is_empty());
        Ok(())
    }

    #[test]
    fn a_change_to_an_unknown_icon_is_kept_only_when_explorer_has_it() -> Result<(), String> {
        let serial = AtomicU64::new(0);
        let mut icons = Vec::new();
        let picture_only = parse_notify(&message(NIM_MODIFY, NIF_ICON, |_| {})).ok_or("x")?;
        assert!(!apply(&mut icons, &picture_only, image(1), true, &serial));
        let full = parse_notify(&message(NIM_MODIFY, NIF_MESSAGE | NIF_ICON, |_| {})).ok_or("x")?;
        assert!(!apply(&mut icons, &full, image(1), false, &serial));
        assert!(apply(&mut icons, &full, image(1), true, &serial));
        assert_eq!(icons.len(), 1);
        Ok(())
    }

    #[test]
    fn rejected_empty_add_is_not_retained() -> Result<(), String> {
        let serial = AtomicU64::new(0);
        let mut icons = Vec::new();
        let empty_add = parse_notify(&message(NIM_ADD, 0, |_| {})).ok_or("x")?;
        assert!(!apply(&mut icons, &empty_add, None, false, &serial));
        assert!(icons.is_empty());
        Ok(())
    }

    #[test]
    fn a_version_set_before_the_icon_is_known_applies_to_its_clicks() -> Result<(), String> {
        let serial = AtomicU64::new(0);
        let mut icons = Vec::new();
        let version = parse_notify(&message(NIM_SETVERSION, 0, |bytes| {
            bytes[NID_VERSION] = 4;
        }))
        .ok_or("x")?;
        assert!(!apply(&mut icons, &version, None, false, &serial));
        assert!(icons.is_empty(), "a version Explorer refused is not kept");
        assert!(!apply(&mut icons, &version, None, true, &serial));
        assert!(!icons[0].shown(), "nothing to draw yet");
        let full = parse_notify(&message(NIM_MODIFY, NIF_MESSAGE | NIF_ICON, |_| {})).ok_or("x")?;
        assert!(apply(&mut icons, &full, image(1), true, &serial));
        assert!(icons[0].shown());
        assert_eq!(icons[0].version, 4);
        Ok(())
    }

    #[test]
    fn clicks_follow_the_icon_version() {
        assert_eq!(
            click_events(TrayClick::Left, 0),
            [WM_LBUTTONDOWN, WM_LBUTTONUP]
        );
        assert_eq!(
            click_events(TrayClick::Left, 4),
            [WM_LBUTTONDOWN, WM_LBUTTONUP, NIN_SELECT]
        );
        assert_eq!(
            click_events(TrayClick::Right, 3),
            [WM_RBUTTONDOWN, WM_RBUTTONUP, WM_CONTEXTMENU]
        );
        assert_eq!(
            click_events(TrayClick::Double, 4),
            [WM_LBUTTONDBLCLK, WM_LBUTTONUP]
        );

        assert_eq!(
            callback_params(0, 7, WM_RBUTTONUP, (10, 20)),
            (7, WM_RBUTTONUP as isize)
        );
        let (anchor, event) = callback_params(4, 7, WM_CONTEXTMENU, (-5, 1050));
        assert_eq!(anchor & 0xFFFF, (-5i32) as u16 as usize);
        assert_eq!(anchor >> 16, 1050);
        assert_eq!(event, (WM_CONTEXTMENU | (7 << 16)) as isize);
    }

    #[test]
    fn with_explorer_in_front_returns_value_and_resets_depth_on_panic() {
        let value = with_explorer_in_front(|| {
            assert!(YIELD_DEPTH.load(Ordering::Acquire) > 0);
            42
        });
        assert_eq!(value, 42);
        assert_eq!(YIELD_DEPTH.load(Ordering::Acquire), 0);

        let panicked = std::panic::catch_unwind(|| {
            with_explorer_in_front(|| -> () {
                assert!(YIELD_DEPTH.load(Ordering::Acquire) > 0);
                panic!("simulated failure");
            });
        });
        assert!(panicked.is_err());
        assert_eq!(YIELD_DEPTH.load(Ordering::Acquire), 0);
    }

    #[test]
    #[ignore = "Puts a tray host in front of the live taskbar for a moment"]
    fn the_host_collects_live_icons() {
        let Some(host) = TrayHost::start(0, 0) else {
            return;
        };
        std::thread::sleep(std::time::Duration::from_secs(3));
        for icon in host.icons() {
            println!(
                "{} {:?} shown={} version={}",
                icon.id(),
                icon.tip,
                icon.shown(),
                icon.version
            );
        }
    }
}
