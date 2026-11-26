//! Minimal macOS-style menu bar across the top of the screen.
//!
//! Hiding the Windows taskbar also hides its clock and status area; this bar
//! brings them back: the name of the app in front, the battery, a control
//! center button (opens the Windows quick settings: network, volume,
//! brightness) and the date and time (opens the notification center).
//!
//! Cost while idle: one timer that fires once a minute, on the minute, to
//! redraw the clock. Everything else is event-driven.
//!
//! With auto-hide on, the bar reserves no space and stays off screen. Resting
//! the pointer briefly at the top edge reveals it.
//!
//! Status items, right to left as on a Mac: clock, control center, search,
//! network, volume, battery, then Task View and Widgets. Search, Task View and
//! Widgets appear only if the user has them on the Windows taskbar. Network
//! and volume changes arrive through separate notifications; nothing polls.

use anyhow::Result;
use navalia_config::{IslandStyle, Language};
use navalia_shell::{
    accessories::{Kind as AccessoryKind, Snapshot as AccessorySnapshot},
    appbar::AppBar,
    launcher,
    status::{self, AudioWatch, Network, NetworkWatch, Privacy, TaskbarButtons, Volume},
    system::{self, Battery},
    tray::{self, IconKey, TrayClick, TrayHost, TrayIcon},
    watch::RegistryWatch,
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    UI::{
        Color,
        Composition::{
            CompositionBatchTypes, CompositionBrush, CompositionRoundedRectangleGeometry,
            CompositionScopedBatch, CompositionStretch, CompositionSurfaceBrush, Compositor,
            ContainerVisual, Desktop::DesktopWindowTarget, ShapeVisual, SpriteVisual,
            VisualCollection,
        },
    },
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SYSTEMTIME, WPARAM},
        Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute},
        System::{
            LibraryLoader::GetModuleHandleW, SystemInformation::GetLocalTime,
            WinRT::Composition::ICompositorDesktopInterop,
        },
        UI::Input::KeyboardAndMouse::{
            GetCapture, GetDoubleClickTime, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT,
            TrackMouseEvent,
        },
        UI::WindowsAndMessaging::{
            CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, HHOOK,
            IDC_ARROW, KillTimer, LoadCursorW, MSLLHOOKSTRUCT, PBT_POWERSETTINGCHANGE,
            PostMessageW, RegisterClassW, SetCursor, SetTimer, SetWindowsHookExW,
            UnhookWindowsHookEx, WH_MOUSE_LL, WM_APP, WM_CAPTURECHANGED, WM_LBUTTONDOWN,
            WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST,
            WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SETTINGCHANGE, WM_TIMECHANGE, WM_TIMER,
            WM_XBUTTONDOWN, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
            WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
    },
    core::{BOOL, Interface, h, w},
};
use windows_numerics::{Vector2, Vector3};

use crate::{
    accessories, app,
    appearance::{self, Material},
    bluetooth, edge_watch,
    gfx::{Gfx, Ink, LabelBrush},
    mascot,
    media::{self, Control as MediaControl, Track},
    scene::Theme,
    strings,
    usage::{
        self, ActiveProviders, Monitor as UsageMonitor, Provider, ProviderUsage, Snapshot, State,
    },
};

// Logical pixels.
const HEIGHT: f32 = 28.0;
const SIDE_MARGIN: f32 = 14.0;
const ITEM_GAP: f32 = 16.0;
/// Extra clickable room around an item.
const HIT_SLOP: f32 = 7.0;

const MEDIA_CHANGED: u32 = WM_APP + 28;
const VOLUME_CAPTURE_LOST: u32 = WM_APP + 31;
const AUDIO_CHANGED: u32 = WM_APP + 32;
const AUDIO_TICK: u32 = WM_APP + 33;
const BLUETOOTH_CHANGED: u32 = WM_APP + 34;
const TRAY_ICONS_CHANGED: u32 = WM_APP + 35;
const VOLUME_TIMER: usize = 4;
const PROGRESS_TIMER: usize = 5;
/// The system monitor's samples (issue #64): every 2 s while it shows, and
/// a first one soon after it appears.
const MONITOR_TIMER: usize = 6;
const MONITOR_EVERY_MS: u32 = 2_000;
const MONITOR_FIRST_MS: u32 = 500;
/// Its two lines per column, smaller than the bar's text so both fit.
const MONITOR_FONT: f32 = 10.0;
/// How far each line reaches past the middle of the bar, in logical pixels.
const MONITOR_TUCK: f32 = 1.0;
const MONITOR_COLUMN_GAP: f32 = 8.0;
/// The network lost this long is worth a word from the mascot; shorter
/// drops (Wi-Fi roaming, a router restarting) say nothing.
const NETWORK_TIMER: usize = 7;
const OFFLINE_GRACE: Duration = Duration::from_secs(8);
const ACCESSORIES_CHANGED: u32 = WM_APP + 30;

const CLOCK_TIMER: usize = 1;
const POINTER_TIMER: usize = 2;
const POINTER_CHECK_MS: u32 = 100;
const POINTER_IDLE_MS: u32 = 200;
const HIDE_ANIMATION_TIMER: usize = 3;
const REVEAL_MS: u32 = 180;
const HIDE_MS: u32 = 130;
// Recovery only; normal completion comes from the compositor, not this timer.
const HIDE_FALLBACK_MS: u32 = HIDE_MS + 250;
const HIDE_ANIMATION_DONE: u32 = WM_APP + 29;
#[cfg_attr(not(test), allow(dead_code))]
const ISLAND_SINGLE_WIDTH: f32 = 145.0;
const ISLAND_EXPANDED_WIDTH: f32 = 440.0;
const ISLAND_ROW_HEIGHT: f32 = 118.0;
const BUBBLE_VERTICAL_INSET: f32 = 2.5;
const BUBBLE_MUSIC_WIDTH: f32 = 164.0;
const BUBBLE_PROVIDER_WIDTH: f32 = 62.0;
const BUBBLE_USAGE_PADDING: f32 = 16.0;
const PANEL_GAP: f32 = 5.0;
const PANEL_RADIUS: f32 = 18.0;
const PANEL_SHADOW_MARGIN: f32 = 20.0;
const WM_POWERBROADCAST: u32 = 0x0218;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// AppBar notifications need a message id; the bar ignores them.
#[allow(dead_code)]
const APPBAR_CALLBACK: u32 = WM_APP + 20;
/// Posted by the network watch; audio uses its own coalesced notification.
const STATUS_CHANGED: u32 = WM_APP + 21;
/// Posted by the bar to itself to update from the message loop.
const TICK: u32 = WM_APP + 22;
const USAGE_CHANGED: u32 = WM_APP + 23;
const DISMISS_ISLAND: u32 = WM_APP + 24;
/// Privacy activity or the user's taskbar preferences changed.
const REGISTRY_CHANGED: u32 = WM_APP + 25;
/// `wParam` of WM_SETTINGCHANGE when the work area changes, which every
/// AppBar reservation (the dock's, this bar's) causes.
const SPI_SETWORKAREA: usize = 0x002F;
/// A `TICK` is already posted.
static TICK_PENDING: AtomicBool = AtomicBool::new(false);
static AUDIO_TICK_PENDING: AtomicBool = AtomicBool::new(false);
static TRIGGERS_LEFT: AtomicI32 = AtomicI32::new(0);
static TRIGGERS_RIGHT: AtomicI32 = AtomicI32::new(0);
static PANEL_LEFT: AtomicI32 = AtomicI32::new(0);
static PANEL_RIGHT: AtomicI32 = AtomicI32::new(0);
static BAR_TOP: AtomicI32 = AtomicI32::new(0);
static BAR_BOTTOM: AtomicI32 = AtomicI32::new(0);
static PANEL_TOP: AtomicI32 = AtomicI32::new(0);
static PANEL_BOTTOM: AtomicI32 = AtomicI32::new(0);
static DOCK_LEFT: AtomicI32 = AtomicI32::new(0);
static DOCK_RIGHT: AtomicI32 = AtomicI32::new(0);
/// The hook exists only while the island is expanded.
static ISLAND_HOOK_WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Segoe Fluent Icons code points.
pub(crate) mod glyph {
    pub const ETHERNET: char = '\u{E839}';
    pub const OFFLINE: char = '\u{F384}';
    pub const VOLUME: [char; 4] = ['\u{E992}', '\u{E993}', '\u{E994}', '\u{E995}'];
    pub const MUTE: char = '\u{E74F}';
    pub const SEARCH: char = '\u{E721}';
    pub const TASK_VIEW: char = '\u{E7C4}';
    /// The taskbar's "show hidden icons" arrow.
    pub const HIDDEN_ICONS: char = '\u{E70E}';
    pub const MICROPHONE: char = '\u{E720}';
    pub const CAMERA: char = '\u{E722}';
    pub const MOUSE: char = '\u{E962}';
    pub const HEADSET: char = '\u{E95B}';
    pub const BLUETOOTH: char = '\u{E702}';
    /// The bell of the Windows 11 notification center ("Ringer").
    pub const NOTIFICATIONS: char = '\u{EA8F}';
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    AccessoryIndicator,
    QuickSettings,
    Notifications,
    /// The clock: the user's own target, or the notification center.
    Clock,
    /// The battery: power settings.
    Power,
    /// The Wi-Fi, Ethernet or offline icon: the list of networks.
    Network,
    Sound,
    Search,
    TaskView,
    HiddenIcons,
    InputLanguage,
    Microphone,
    Camera,
    Bluetooth,
    /// The system monitor: Task Manager, which lists what uses each.
    SystemMonitor,
    /// A live Windows 11 notification-area icon.
    TrayItem(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuBarWidgets {
    pub island_style: IslandStyle,
    /// The notification bell at the right end (`Config::menu_bar_notifications`).
    pub notifications: bool,
    /// CPU, memory and network speed (`Config::menu_bar_system_monitor`).
    pub system_monitor: bool,
}

impl Default for MenuBarWidgets {
    fn default() -> Self {
        Self {
            island_style: IslandStyle::Notch,
            notifications: false,
            system_monitor: false,
        }
    }
}

/// Network, volume and taskbar buttons, read only when Windows reports a change.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Status {
    bluetooth_on: bool,
    network: Network,
    volume: Option<Volume>,
    buttons: TaskbarButtons,
    privacy: Privacy,
    battery_percentage: bool,
}

/// Full-width taskbar backdrop configuration when auto-hide is off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskbarBackdrop {
    pub style: navalia_config::TaskbarStyle,
    pub color: Option<(u8, u8, u8)>,
    pub height: i32,
    pub below_hwnd: isize,
}

/// What the bar was last drawn for.
#[derive(Clone, PartialEq)]
struct Look {
    accessories: [Option<AccessoryReading>; 2],
    /// The system monitor's lines (see `MenuBar::monitor_lines`) while on.
    monitor: Option<[String; 4]>,
    language: Language,
    screen: RECT,
    scale: f32,
    /// Horizontal screen X range reserved for the centered bottom dock (`------- XXX ----------`).
    dock_cutout: Option<(i32, i32)>,
    taskbar_backdrop: TaskbarBackdrop,
    /// The theme's material; `light` and `glass` are its other two halves
    /// (see `appearance::Look`).
    material: Material,
    light: bool,
    /// See `system::transparency_enabled`.
    glass: bool,
    app_name: String,
    clock: String,
    /// The input language indicator, when more than one is installed.
    input: Option<String>,
    battery: Option<Battery>,
    status: Status,
    widgets: MenuBarWidgets,
    usage: Snapshot,
    usage_enabled: bool,
    active_providers: ActiveProviders,
    panel: Option<Island>,
    usage_refreshing: bool,
    track: Option<Track>,
    tray_items: Vec<TrayIcon>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AccessoryReading {
    kind: AccessoryKind,
    percent: Option<u8>,
}

// Compare only visible readings: unrelated names/devices do not redraw the bar.
fn accessory_readings(snapshot: &AccessorySnapshot) -> [Option<AccessoryReading>; 2] {
    snapshot.compact().map(|device| {
        device.map(|device| AccessoryReading {
            kind: device.kind,
            percent: device.percent,
        })
    })
}

impl Look {
    pub(super) fn panel_content_height(&self) -> f32 {
        match self.panel {
            Some(Island::Music) => 180.0,
            Some(Island::Usage) => {
                74.0 + Provider::ALL
                    .into_iter()
                    .filter(|provider| self.active_providers.has(*provider))
                    .map(|provider| usage_row_height(self.usage.get(provider)))
                    .sum::<f32>()
            }
            None => 0.0,
        }
    }

    pub(super) fn panel_height(&self) -> f32 {
        let natural = self.panel_content_height();
        if self.panel == Some(Island::Usage) {
            let available = (self.screen.bottom - self.screen.top) as f32 / self.scale
                - HEIGHT
                - PANEL_GAP
                - PANEL_SHADOW_MARGIN;
            natural.min(available.max(0.0))
        } else {
            natural
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Island {
    Music,
    Usage,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Span {
    left: f32,
    right: f32,
}

impl Span {
    pub(super) fn width(self) -> f32 {
        self.right - self.left
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
struct VolumeTrack {
    center_x: f32,
    top: f32,
    bottom: f32,
}

#[derive(Clone, Copy)]
enum IslandAction {
    Toggle(Island),
    Media(MediaControl),
    Volume(VolumeTrack),
    Mute,
    Refresh,
    ScrollUsage(i8),
}

#[derive(Clone, Copy)]
struct IslandHit {
    bounds: (f32, f32, f32, f32),
    action: IslandAction,
}

/// A retained, clipped body. Wheel/arrow input only changes its transform and
/// arrow opacity; no quota queries or redraws.
struct UsageViewport {
    content: ContainerVisual,
    maximum: f32,
    scale: f32,
    bounds: (f32, f32, f32, f32),
    up: SpriteVisual,
    down: SpriteVisual,
}

impl IslandHit {
    pub(super) fn at(&self, x: f32, y: f32) -> bool {
        let (left, top, right, bottom) = self.bounds;
        x >= left && x < right && y >= top && y < bottom
    }
}

struct ExitAnimation {
    batch: CompositionScopedBatch,
    completed: i64,
}

/// Only these retained visuals move during a volume gesture. Album artwork,
/// track labels and the rest of the menu bar keep their existing visual tree.
struct VolumeControls {
    fill: CompositionRoundedRectangleGeometry,
    value: SpriteVisual,
    geometry: IslandGeometry,
    height: f32,
    position_percent: Option<u8>,
    label_percent: u8,
}

/// Only the progress fill and time labels change while the popup is open.
struct ProgressControls {
    fill: CompositionRoundedRectangleGeometry,
    elapsed: SpriteVisual,
    total: SpriteVisual,
    geometry: IslandGeometry,
    width: f32,
    elapsed_seconds: Option<u64>,
    total_seconds: Option<u64>,
}

impl ProgressControls {
    pub(super) fn paint(
        &mut self,
        gfx: &Gfx,
        timeline: media::Timeline,
        now: Instant,
    ) -> Result<()> {
        let position = timeline.position_at(now);
        let px = |v| self.geometry.px(v);
        let filled = self.width * (position.as_secs_f64() / timeline.duration.as_secs_f64()) as f32;
        self.fill.SetSize(Vector2::new(filled, px(4.0)))?;
        self.fill
            .SetCornerRadius(Vector2::new(px(2.0).min(filled / 2.0), px(2.0)))?;
        for (visual, cached, seconds, right) in [
            (
                &self.elapsed,
                &mut self.elapsed_seconds,
                position.as_secs(),
                false,
            ),
            (
                &self.total,
                &mut self.total_seconds,
                timeline.duration.as_secs(),
                true,
            ),
        ] {
            if *cached == Some(seconds) {
                continue;
            }
            let label = gfx.text_brush_sized(
                &media_time(seconds),
                self.geometry.scale,
                10.0,
                false,
                self.geometry.ink(),
            )?;
            visual.SetBrush(&label.brush.cast::<CompositionBrush>()?)?;
            visual.SetSize(Vector2::new(label.width, label.height))?;
            if right {
                visual.SetOffset(Vector3::new(
                    px(26.0) + self.width - label.width,
                    px(148.0),
                    0.0,
                ))?;
            }
            *cached = Some(seconds);
        }
        Ok(())
    }
}

impl VolumeControls {
    pub(super) fn move_to(&mut self, percent: u8) -> Result<()> {
        if self.position_percent == Some(percent) {
            return Ok(());
        }
        let px = |v| self.geometry.px(v);
        let filled = self.height * f32::from(percent) / 100.0;
        self.fill.SetSize(Vector2::new(px(4.0), filled))?;
        self.fill
            .SetOffset(Vector2::new(0.0, self.height - filled))?;
        self.fill
            .SetCornerRadius(Vector2::new(px(2.0), px(2.0).min(filled / 2.0)))?;
        self.position_percent = Some(percent);
        Ok(())
    }

    pub(super) fn paint(&mut self, gfx: &Gfx, percent: u8) -> Result<()> {
        self.move_to(percent)?;
        if self.label_percent != percent {
            let label = gfx.text_brush_sized(
                &format!("{percent}%"),
                self.geometry.scale,
                10.5,
                false,
                self.geometry.ink(),
            )?;
            self.value
                .SetBrush(&label.brush.cast::<CompositionBrush>()?)?;
            self.value
                .SetSize(Vector2::new(label.width, label.height))?;
            self.value.SetOffset(Vector3::new(
                (self.geometry.width - label.width) / 2.0,
                self.geometry.px(12.0),
                0.0,
            ))?;
            self.label_percent = percent;
        }
        Ok(())
    }
}

impl Drop for ExitAnimation {
    fn drop(&mut self) {
        let _ = self.batch.RemoveCompleted(self.completed);
    }
}

pub struct MenuBar {
    bluetooth: Option<bluetooth::Monitor>,
    accessories: Option<accessories::Monitor>,
    accessories_enabled: bool,
    accessory_retry_at: Instant,
    /// The system monitor's counters, only while it samples (issue #64).
    monitor: Option<navalia_shell::monitor::Sampler>,
    monitor_reading: navalia_shell::monitor::Reading,
    /// Widest each monitor column has been at this scale: digits changing
    /// width must not shift the rest of the bar every two seconds.
    monitor_widths: (f32, [f32; 2]),
    hwnd: HWND,
    _target: DesktopWindowTarget,
    /// Stable parent: content refreshes must not restart reveal animations.
    surface: ContainerVisual,
    appbar: Option<AppBar>,
    app_name: String,
    look: Option<Look>,
    /// Clickable spans, in window pixels.
    zones: Vec<(f32, f32, Action)>,
    visible: bool,
    /// Whether the bar hides now, until the pointer reaches the top edge:
    /// always with "Automatically hide", and with "Intelligent" while the
    /// window in front reaches under it (`covered`).
    auto_hide: bool,
    /// The user's choice of hiding (`Config::menu_bar_auto_hide`).
    hide_setting: bool,
    /// Intelligent hiding (`Config::menu_bar_smart_hide`, issue #61): like
    /// the dock's, the bar hides only while the window in front reaches
    /// under it. It reserves no strip of the screen then, or each switch
    /// would resize every maximized window.
    smart: bool,
    /// The window in front reaches under the bar (see `set_covered`).
    covered: bool,
    /// With auto-hide on: the pointer is at the top edge, so the bar shows.
    revealed: bool,
    hide_until: Option<Instant>,
    animation_epoch: usize,
    exit_animation: Option<ExitAnimation>,
    /// Where the full bar goes on screen.
    bounds: RECT,
    /// Horizontal screen X range occupied by the bottom dock (`------- XXX ----------`).
    dock_cutout: Option<(i32, i32)>,
    taskbar_backdrop: TaskbarBackdrop,
    status: Status,
    widgets: MenuBarWidgets,
    /// What the clock opens (`Config::menu_bar_clock_target`). Not drawn, so
    /// it lives outside `widgets` and changing it redraws nothing.
    clock_target: Option<String>,
    usage: Option<UsageMonitor>,
    /// Where each provider sits in the closed island, as screen x.
    usage_anchors: Vec<(Provider, i32)>,
    /// A moment for the crab, waiting for the dock (see `take_cue`).
    cue: Option<(mascot::Act, i32)>,
    /// Whether the network was up at the last reading (`None` before the
    /// first), since when it is down, and whether that was told.
    online: Option<bool>,
    offline_since: Option<Instant>,
    offline_told: bool,
    media: Option<media::Watch>,
    /// "Show what is playing": off, nothing listens to media sessions.
    media_enabled: bool,
    cover: Option<(
        Option<Arc<navalia_shell::icons::IconImage>>,
        CompositionSurfaceBrush,
    )>,
    volume_drag: Option<VolumeTrack>,
    pending_volume: Option<u8>,
    volume_target: Option<u8>,
    volume_controls: Option<VolumeControls>,
    progress_controls: Option<ProgressControls>,
    progress_timer: bool,
    usage_scroll: f32,
    usage_viewport: Option<UsageViewport>,
    suppress_volume_release: bool,
    panel: Option<Island>,
    visual_height: i32,
    triggers_span: Span,
    panel_span: Span,
    island_hits: Vec<IslandHit>,
    outside_click_hook: Option<HHOOK>,
    /// Held for their callbacks; dropping them unsubscribes.
    _network: Option<NetworkWatch>,
    /// Hears when apps start or stop using the microphone or camera.
    privacy_watch: Option<RegistryWatch>,
    /// Windows' taskbar preferences, including the battery percentage.
    settings_watch: Option<RegistryWatch>,
    /// Search uses its own Windows preference key.
    search_watch: Option<RegistryWatch>,
    /// Energy saver on or off arrives as `WM_POWERBROADCAST`; glass follows.
    _saver_watch: navalia_shell::power::SaverWatch,
    audio: Option<AudioWatch>,
    /// Other apps' notification-area icons, received while the bar lives.
    tray_host: Option<TrayHost>,
    tray_items: Vec<TrayIcon>,
    /// Drawn pictures by icon id, with the picture's serial.
    tray_icons: HashMap<String, (u64, Option<CompositionSurfaceBrush>)>,
    /// The last tray click, to tell a double click.
    tray_last_click: Option<(IconKey, Instant)>,
}

unsafe extern "system" fn bar_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            launcher::note_menu_bar_press();
            app::menu_bar_volume_press((l.0 & 0xFFFF) as i16 as f32, (l.0 >> 16) as i16 as f32);
        }
        WM_LBUTTONUP => {
            let x = (l.0 & 0xFFFF) as i16;
            let y = (l.0 >> 16) as i16;
            if !app::menu_bar_volume_release(Some(f32::from(y))) {
                app::menu_bar_clicked(f32::from(x), f32::from(y));
            }
        }
        WM_RBUTTONUP => {
            let x = (l.0 & 0xFFFF) as i16;
            let y = (l.0 >> 16) as i16;
            if !app::menu_bar_right_clicked(f32::from(x), f32::from(y)) {
                return unsafe { DefWindowProcW(hwnd, msg, w, l) };
            }
        }
        WM_MOUSEMOVE => {
            let x = (l.0 & 0xFFFF) as i16;
            let y = (l.0 >> 16) as i16;
            app::menu_bar_pointer_moved(f32::from(x), f32::from(y));
        }
        WM_MOUSELEAVE => app::menu_bar_check_pointer(),
        WM_MOUSEWHEEL => {
            let delta = (w.0 >> 16) as u16 as i16;
            let point = POINT {
                x: (l.0 & 0xFFFF) as i16 as i32,
                y: (l.0 >> 16) as i16 as i32,
            };
            if app::menu_bar_scrolled(point, delta) {
                return LRESULT(0);
            }
            return unsafe { DefWindowProcW(hwnd, msg, w, l) };
        }
        WM_TIMER if w.0 == POINTER_TIMER => app::menu_bar_check_pointer(),
        WM_TIMER if w.0 == VOLUME_TIMER => app::menu_bar_volume_flush(),
        WM_TIMER if w.0 == PROGRESS_TIMER => app::menu_bar_progress_tick(),
        WM_TIMER if w.0 == MONITOR_TIMER => app::menu_bar_monitor_tick(),
        WM_CAPTURECHANGED => {
            // Avoid reentering the app borrow inside Set/ReleaseCapture.
            unsafe {
                let _ = PostMessageW(Some(hwnd), VOLUME_CAPTURE_LOST, WPARAM(0), LPARAM(0));
            }
        }
        VOLUME_CAPTURE_LOST => {
            // A delayed notification must not cancel a newer drag.
            if unsafe { GetCapture() } != hwnd {
                app::menu_bar_volume_release(None);
            }
        }
        WM_TIMER if w.0 == HIDE_ANIMATION_TIMER => app::menu_bar_finish_hide(None),
        HIDE_ANIMATION_DONE => app::menu_bar_finish_hide(Some(w.0)),
        DISMISS_ISLAND => app::menu_bar_dismiss_island(),
        REGISTRY_CHANGED => app::menu_bar_registry_changed(),
        ACCESSORIES_CHANGED => app::menu_bar_accessories_changed(),
        MEDIA_CHANGED => app::menu_bar_media_changed(),
        BLUETOOTH_CHANGED => app::menu_bar_bluetooth_changed(),
        TRAY_ICONS_CHANGED => app::menu_bar_tray_changed(),
        AUDIO_CHANGED => {
            if !AUDIO_TICK_PENDING.swap(true, Ordering::Relaxed) {
                unsafe {
                    let _ = PostMessageW(Some(hwnd), AUDIO_TICK, WPARAM(0), LPARAM(0));
                }
            }
        }
        AUDIO_TICK => {
            AUDIO_TICK_PENDING.store(false, Ordering::Relaxed);
            app::menu_bar_audio_changed();
        }
        WM_SETCURSOR => {
            // SAFETY: Keep the standard arrow throughout the bar and popups.
            if let Ok(arrow) = unsafe { LoadCursorW(None, IDC_ARROW) } {
                unsafe { SetCursor(Some(arrow)) };
                return LRESULT(1);
            }
            return unsafe { DefWindowProcW(hwnd, msg, w, l) };
        }
        WM_NCHITTEST => {
            let x = (l.0 & 0xFFFF) as i16 as i32;
            let y = (l.0 >> 16) as i16 as i32;
            let bar_top = BAR_TOP.load(Ordering::Relaxed);
            if y < bar_top {
                if y < PANEL_TOP.load(Ordering::Relaxed)
                    || x < PANEL_LEFT.load(Ordering::Relaxed)
                    || x >= PANEL_RIGHT.load(Ordering::Relaxed)
                {
                    return LRESULT(-1);
                }
            } else {
                let dock_left = DOCK_LEFT.load(Ordering::Relaxed);
                let dock_right = DOCK_RIGHT.load(Ordering::Relaxed);
                if dock_right > dock_left && x >= dock_left && x < dock_right {
                    return LRESULT(-1);
                }
            }
            // SAFETY: Default hit testing for the bar wings and the upward island popup.
            return unsafe { DefWindowProcW(hwnd, msg, w, l) };
        }
        // The minute changed, the charger was plugged or unplugged, the
        // network or volume changed, or a Windows setting (taskbar buttons).
        // Nothing the bar shows depends on the work area.
        WM_SETTINGCHANGE if w.0 == SPI_SETWORKAREA => {}
        // Some of these are sent while the dock is busy reserving space, and
        // updating then was dropped. From the message loop the dock is never
        // busy, and a burst of them makes a single update.
        // WM_TIMECHANGE: the clock or time zone was changed.
        WM_TIMER | WM_POWERBROADCAST | WM_TIMECHANGE | STATUS_CHANGED | USAGE_CHANGED
        | WM_SETTINGCHANGE => {
            if msg == WM_POWERBROADCAST && matches!(w.0, 7 | 18) {
                app::menu_bar_accessories_reset();
            }
            if msg == WM_POWERBROADCAST && w.0 == PBT_POWERSETTINGCHANGE as usize {
                // SAFETY: Windows owns the setting payload for this message.
                unsafe {
                    navalia_shell::power::power_setting_changed(l);
                }
            }
            if !TICK_PENDING.swap(true, Ordering::Relaxed) {
                // SAFETY: Posting to our own live window.
                unsafe {
                    let _ = PostMessageW(Some(hwnd), TICK, WPARAM(0), LPARAM(0));
                }
            }
        }
        TICK => {
            TICK_PENDING.store(false, Ordering::Relaxed);
            app::menu_bar_tick();
        }
        // SAFETY: Standard forwarding of unhandled messages.
        _ => return unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
    LRESULT(0)
}

unsafe extern "system" fn outside_click_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0
        && matches!(
            w.0 as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        )
    {
        // SAFETY: Windows provides an MSLLHOOKSTRUCT for mouse hook actions.
        let point = unsafe { (*(l.0 as *const MSLLHOOKSTRUCT)).pt };
        let in_triggers = point.y >= BAR_TOP.load(Ordering::Relaxed)
            && point.y < BAR_BOTTOM.load(Ordering::Relaxed)
            && point.x >= TRIGGERS_LEFT.load(Ordering::Relaxed)
            && point.x < TRIGGERS_RIGHT.load(Ordering::Relaxed);
        let in_panel = point.y >= PANEL_TOP.load(Ordering::Relaxed)
            && point.y < PANEL_BOTTOM.load(Ordering::Relaxed)
            && point.x >= PANEL_LEFT.load(Ordering::Relaxed)
            && point.x < PANEL_RIGHT.load(Ordering::Relaxed);
        if !in_triggers && !in_panel {
            let window = ISLAND_HOOK_WINDOW.load(Ordering::Relaxed);
            if window != 0 {
                // SAFETY: The window remains live until the hook is removed in Drop.
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(window as *mut _)),
                        DISMISS_ISLAND,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        }
    }
    // SAFETY: Never consume the click; the target application must receive it.
    unsafe { CallNextHookEx(None, code, w, l) }
}

mod draw;
mod input;
mod layout;
mod state;
use draw::*;
use layout::*;
#[cfg(test)]
use state::same_volume_indicator;
#[cfg(test)]
mod tests;

impl MenuBar {
    pub fn create(
        gfx: &Gfx,
        usage_enabled: bool,
        media_enabled: bool,
        auto_hide: bool,
        widgets: MenuBarWidgets,
    ) -> Result<Self> {
        let class = w!("NavaliaMenuBar");
        // SAFETY: Standard class registration and window creation on the UI
        // thread; the window is destroyed in `Drop`.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None)?;
            // Registering an existing class fails harmlessly.
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(bar_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class,
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
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
            )?;
            // Without this, the system backdrop brush appears black.
            let enable = BOOL::from(true);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                (&raw const enable).cast(),
                size_of::<BOOL>() as u32,
            );
            hwnd
        };
        let target = unsafe {
            // SAFETY: `hwnd` is the live window created above.
            gfx.compositor
                .cast::<ICompositorDesktopInterop>()?
                .CreateDesktopWindowTarget(hwnd, true)?
        };
        // Our compositor owns the transition; do not combine it with DWM's
        // native show/hide effect, which can present a cached opaque frame.
        system::set_window_transitions(hwnd.0 as isize, false);
        let surface = gfx.compositor.CreateContainerVisual()?;
        target.SetRoot(&surface)?;
        let mut bar = Self {
            accessories: None,
            accessories_enabled: false,
            accessory_retry_at: Instant::now() + Duration::from_secs(60),
            monitor: None,
            monitor_reading: navalia_shell::monitor::Reading::default(),
            monitor_widths: (0.0, [0.0; 2]),
            hwnd,
            _target: target,
            surface,
            appbar: None,
            app_name: String::new(),
            look: None,
            zones: Vec::new(),
            visible: true,
            auto_hide,
            hide_setting: auto_hide,
            smart: false,
            covered: false,
            revealed: false,
            hide_until: None,
            animation_epoch: 0,
            exit_animation: None,
            bounds: RECT::default(),
            dock_cutout: None,
            taskbar_backdrop: TaskbarBackdrop::default(),
            status: Status {
                bluetooth_on: false,
                network: Network::Offline,
                volume: None,
                buttons: TaskbarButtons::default(),
                privacy: Privacy::default(),
                battery_percentage: false,
            },
            widgets,
            clock_target: None,
            usage: usage_enabled.then(|| UsageMonitor::new(hwnd.0 as isize, USAGE_CHANGED)),
            usage_anchors: Vec::new(),
            cue: None,
            online: None,
            offline_since: None,
            offline_told: false,
            media: media_enabled
                .then(|| media::Watch::start(hwnd, MEDIA_CHANGED))
                .flatten(),
            media_enabled,
            cover: None,
            volume_drag: None,
            pending_volume: None,
            volume_target: None,
            volume_controls: None,
            progress_controls: None,
            progress_timer: false,
            usage_scroll: 0.0,
            usage_viewport: None,
            suppress_volume_release: false,
            panel: None,
            visual_height: 0,
            triggers_span: Span::default(),
            panel_span: Span::default(),
            island_hits: Vec::new(),
            outside_click_hook: None,
            bluetooth: bluetooth::Monitor::new(hwnd.0 as isize, BLUETOOTH_CHANGED).ok(),
            _network: NetworkWatch::new(hwnd.0 as isize, STATUS_CHANGED),
            privacy_watch: status::watch_privacy(hwnd.0 as isize, REGISTRY_CHANGED),
            settings_watch: status::watch_taskbar_settings(hwnd.0 as isize, REGISTRY_CHANGED),
            search_watch: status::watch_search_settings(hwnd.0 as isize, REGISTRY_CHANGED),
            _saver_watch: navalia_shell::power::SaverWatch::new(hwnd.0 as isize),
            audio: AudioWatch::new(hwnd.0 as isize, AUDIO_CHANGED),
            tray_host: TrayHost::start(hwnd.0 as isize, TRAY_ICONS_CHANGED),
            tray_items: Vec::new(),
            tray_icons: HashMap::new(),
            tray_last_click: None,
        };
        AUDIO_TICK_PENDING.store(false, Ordering::Relaxed);
        bar.update_status();
        bar.refresh(gfx)?;
        bar.schedule_next_minute();
        Ok(bar)
    }

    pub fn set_app_name(&mut self, gfx: &Gfx, name: &str) -> Result<()> {
        if self.app_name == name {
            return Ok(());
        }
        name.clone_into(&mut self.app_name);
        self.refresh(gfx)
    }

    pub fn set_dock_cutout(&mut self, gfx: &Gfx, cutout: Option<(i32, i32)>) -> Result<()> {
        if self.dock_cutout == cutout {
            return Ok(());
        }
        self.dock_cutout = cutout;
        self.refresh(gfx)
    }

    pub fn set_taskbar_backdrop(&mut self, gfx: &Gfx, backdrop: TaskbarBackdrop) -> Result<()> {
        if self.taskbar_backdrop == backdrop {
            return Ok(());
        }
        self.taskbar_backdrop = backdrop;
        self.refresh(gfx)
    }

    /// Hidden while an app is full screen.
    /// How tall the bar is at `scale`: the strip it keeps at the top of the
    /// primary display unless it hides itself. A side dock starts below it.
    #[allow(dead_code)]
    pub fn strip_height(scale: f32) -> i32 {
        (HEIGHT * scale).round() as i32
    }

    pub fn set_visible(&mut self, visible: bool) -> Result<()> {
        if !visible {
            self.release_volume(None)?;
        }
        if self.visible == visible {
            return Ok(());
        }
        self.visible = visible;
        if !visible && self.auto_hide {
            self.revealed = false;
        }
        self.place()
    }

    pub fn set_auto_hide(&mut self, auto_hide: bool) -> Result<()> {
        if self.hide_setting == auto_hide {
            return Ok(());
        }
        self.hide_setting = auto_hide;
        self.apply_hiding()
    }

    /// Intelligent hiding on or off (see `smart`).
    pub fn set_smart_hide(&mut self, smart: bool) -> Result<()> {
        if self.smart == smart {
            return Ok(());
        }
        self.smart = smart;
        self.apply_hiding()
    }

    /// A change of setting applies at once, without animation.
    pub(super) fn apply_hiding(&mut self) -> Result<()> {
        self.auto_hide = self.hide_setting || (self.smart && self.covered);
        self.revealed = false;
        self.hide_until = None;
        // The reserved strip may change even when hiding does not.
        self.place()
    }

    /// Whether the bar keeps a strip of the screen for itself, so windows
    /// are maximized below it: only while it neither auto-hides nor hides
    /// intelligently.
    #[allow(dead_code)]
    pub(super) fn reserves(&self) -> bool {
        !self.hide_setting && !self.smart
    }

    /// Where the bar is on screen, shown or not.
    pub fn bounds(&self) -> RECT {
        self.bounds
    }

    /// Intelligent hiding: whether the window in front reaches under the
    /// bar. It slides away as when the pointer leaves an auto-hiding bar,
    /// unless the pointer is on it, and slides back once nothing covers it;
    /// meanwhile the top edge reveals it as usual.
    pub fn set_usage_enabled(&mut self, gfx: &Gfx, enabled: bool) -> Result<()> {
        if self.usage.is_some() == enabled {
            return Ok(());
        }
        self.usage = enabled.then(|| UsageMonitor::new(self.hwnd.0 as isize, USAGE_CHANGED));
        self.refresh(gfx)
    }

    /// Shows or hides what is playing. Off, the media watch stops too, so
    /// nothing follows playback changes.
    pub fn set_media_enabled(&mut self, gfx: &Gfx, enabled: bool) -> Result<()> {
        if self.media_enabled == enabled {
            return Ok(());
        }
        self.media_enabled = enabled;
        self.media = enabled
            .then(|| media::Watch::start(self.hwnd, MEDIA_CHANGED))
            .flatten();
        self.refresh(gfx)
    }

    pub fn set_accessories_enabled(&mut self, gfx: &Gfx, enabled: bool) -> Result<()> {
        if self.accessories_enabled == enabled {
            return Ok(());
        }
        self.accessories_enabled = enabled;
        if enabled {
            self.restart_accessories(gfx)
        } else {
            self.accessories = None;
            self.refresh(gfx)
        }
    }

    pub fn set_widgets(&mut self, gfx: &Gfx, widgets: MenuBarWidgets) -> Result<()> {
        if self.widgets == widgets {
            return Ok(());
        }
        self.widgets = widgets;
        self.refresh(gfx)
    }

    /// What clicking the clock opens; `None` for the notification center.
    pub fn set_clock_target(&mut self, target: Option<&str>) {
        if self.clock_target.as_deref() != target {
            self.clock_target = target.map(str::to_owned);
        }
    }

    /// The pointer reached the top edge or left the bar.
    /// The bottom bar never reserves space via AppBar; the dock owns the bottom edge.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn reserved_strip(&self) -> Option<RECT> {
        None
    }

    /// Positions the window along the bottom of the primary display.
    pub(super) fn shown(&self) -> bool {
        self.visible && (!self.auto_hide || self.revealed)
    }
}

impl Drop for MenuBar {
    fn drop(&mut self) {
        self.search_watch = None;
        self.settings_watch = None;
        self.privacy_watch = None;
        self.bluetooth = None;
        self.accessories = None;
        self.exit_animation = None;
        self.media = None;
        ISLAND_HOOK_WINDOW.store(0, Ordering::Relaxed);
        edge_watch::set_menu_bar(None);
        if let Some(hook) = self.outside_click_hook.take() {
            // SAFETY: Do not leave a callback pointing at a destroyed window.
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
        // Give the reserved strip back before the window goes away.
        self.appbar = None;
        // SAFETY: Our own window, destroyed once.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
