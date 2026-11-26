use super::*;
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_TOPMOST, IsWindowVisible, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SetWindowPos,
};

pub(super) fn volume_percent(span: VolumeTrack, y: f32) -> u8 {
    let height = span.bottom - span.top;
    if height <= 0.0 || !y.is_finite() {
        return 0;
    }
    (((span.bottom - y) / height).clamp(0.0, 1.0) * 100.0).round() as u8
}

/// Prefer the player's right edge, then its left. On narrow displays reserve
/// space for the capsule instead of covering the progress or playback buttons.
pub(super) fn volume_popover_layout(panel: Span, width: f32, scale: f32) -> (Span, Span) {
    let px = |value: f32| (value * scale).round();
    let margin = px(12.0).min(width / 10.0);
    let available = (width - margin * 2.0).max(0.0);
    let capsule = px(44.0).min(available / 3.0);
    let gap = px(8.0).min(available / 10.0);
    let player_width = panel.width().min((available - capsule - gap).max(0.0));
    let mut left = panel
        .left
        .clamp(margin, (width - margin - player_width).max(margin));
    let volume_left = if left + player_width + gap + capsule <= width - margin {
        left + player_width + gap
    } else if left - gap - capsule >= margin {
        left - gap - capsule
    } else {
        left = (width - margin - player_width - gap - capsule).max(margin);
        left + player_width + gap
    };
    (
        Span {
            left,
            right: left + player_width,
        },
        Span {
            left: volume_left,
            right: volume_left + capsule,
        },
    )
}

#[derive(Clone, Copy)]
pub(super) struct IslandGeometry {
    pub(super) left: f32,
    pub(super) top: f32,
    pub(super) width: f32,
    pub(super) scale: f32,
    pub(super) light: bool,
}

impl IslandGeometry {
    pub(super) fn new(span: Span, top: f32, look: &Look) -> Self {
        Self {
            left: span.left,
            top,
            width: span.width(),
            scale: look.scale,
            light: look.light,
        }
    }

    pub(super) fn hit(
        self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        action: IslandAction,
    ) -> IslandHit {
        let left = self.left + x;
        let top = self.top + y;
        IslandHit {
            bounds: (left, top, left + width, top + height),
            action,
        }
    }

    pub(super) fn px(self, logical: f32) -> f32 {
        (logical * self.scale).round()
    }
    pub(super) fn ink(self) -> Ink {
        if self.light { Ink::Dark } else { Ink::Light }
    }
    pub(super) fn control_fill(self) -> Color {
        if self.light {
            rgba(0, 0, 0, 18)
        } else {
            rgba(255, 255, 255, 27)
        }
    }
    pub(super) fn border(self) -> Color {
        if self.light {
            rgba(35, 57, 83, 31)
        } else {
            rgba(223, 237, 255, 37)
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn trigger_layout(
    bounds: Span,
    center: f32,
    scale: f32,
    music: bool,
    providers: usize,
) -> [(Island, Option<Span>); 2] {
    let px = |v: f32| (v * scale).round();
    let mut result = [(Island::Music, None), (Island::Usage, None)];
    let gap = if music && providers > 0 { px(8.0) } else { 0.0 };
    let music_width = if music { px(214.0) } else { 0.0 };
    let usage_width = match providers {
        0 => 0.0,
        1 => px(ISLAND_SINGLE_WIDTH),
        count => px(134.0 * count as f32),
    };
    let total = music_width + usage_width;
    let available = (bounds.width() - gap).min(total);
    if total == 0.0 || available < px(if gap > 0.0 { 85.0 } else { 42.0 }) {
        return result;
    }
    let music_width = if gap > 0.0 {
        (available * music_width / total)
            .round()
            .clamp(px(42.0), available - px(42.0))
    } else if music {
        available
    } else {
        0.0
    };
    let left = (center - (available + gap) / 2.0)
        .clamp(
            bounds.left,
            (bounds.right - available - gap).max(bounds.left),
        )
        .round();
    if music {
        result[0].1 = Some(Span {
            left,
            right: left + music_width,
        });
    }
    if providers > 0 {
        result[1].1 = Some(Span {
            left: left + music_width + gap,
            right: left + available + gap,
        });
    }
    result
}

pub(super) fn bubble_trigger_layout(
    bounds: Span,
    scale: f32,
    music_width: Option<f32>,
    providers: usize,
) -> [(Island, Option<Span>); 2] {
    let px = |v: f32| (v * scale).round();
    let mut result = [(Island::Music, None), (Island::Usage, None)];
    let music = music_width.is_some();
    let gap = if music && providers > 0 { px(8.0) } else { 0.0 };
    let music_width = music_width
        .map(|width| width.clamp(px(36.0), px(BUBBLE_MUSIC_WIDTH)))
        .unwrap_or(0.0);
    let usage_width = if providers > 0 {
        px(BUBBLE_USAGE_PADDING + BUBBLE_PROVIDER_WIDTH * providers as f32)
    } else {
        0.0
    };
    let total = music_width + usage_width;
    let available = (bounds.width() - gap).min(total);
    if total == 0.0 || available < px(if gap > 0.0 { 72.0 } else { 36.0 }) {
        return result;
    }
    let music_width = if gap > 0.0 {
        (available * music_width / total)
            .round()
            .clamp(px(36.0), available - px(36.0))
    } else if music {
        available
    } else {
        0.0
    };
    let left = (bounds.right - available - gap).max(bounds.left).round();
    if music {
        result[0].1 = Some(Span {
            left,
            right: left + music_width,
        });
    }
    if providers > 0 {
        result[1].1 = Some(Span {
            left: left + music_width + gap,
            right: left + available + gap,
        });
    }
    result
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn panel_span(trigger: Span, screen_width: f32, desired: f32) -> Span {
    let width = desired.max(trigger.width()).min(screen_width);
    let left = ((trigger.left + trigger.right - width) / 2.0)
        .clamp(0.0, screen_width - width)
        .round();
    Span {
        left,
        right: left + width,
    }
}

pub(super) fn bubble_panel_span(anchor: Span, screen_width: f32, desired: f32) -> Span {
    let width = desired.max(anchor.width()).min(screen_width);
    let left = (anchor.right - width)
        .clamp(0.0, screen_width - width)
        .round();
    Span {
        left,
        right: left + width,
    }
}

pub(super) fn left_bubble_panel_span(anchor: Span, screen_width: f32, desired: f32) -> Span {
    let width = desired.max(anchor.width()).min(screen_width);
    let left = anchor.left.clamp(0.0, screen_width - width).round();
    Span {
        left,
        right: left + width,
    }
}

pub(super) type IslandProvider<'a> = (Provider, &'a ProviderUsage, Color);

pub(super) fn cursor_anchor() -> (i32, i32) {
    let mut point = POINT::default();
    // SAFETY: Writes the cursor position into a local.
    let _ = unsafe { GetCursorPos(&mut point) };
    (point.x, point.y)
}

pub(super) fn tray_item_spans(
    start_x: f32,
    max_x: f32,
    side: f32,
    gap: f32,
    count: usize,
) -> Vec<(f32, f32)> {
    let mut spans = Vec::new();
    let mut left = start_x;
    for _ in 0..count {
        let right = left + side;
        if right > max_x {
            break;
        }
        spans.push((left, right));
        left = right + gap;
    }
    spans
}

impl MenuBar {
    pub(super) fn reveal_lift(&self) -> f32 {
        (self.bounds.bottom - self.bounds.top) as f32 * 0.3
    }

    /// Puts the bar back on top without moving or showing it. Explorer lifts
    /// its auto-hidden taskbar above every topmost window when a shell flyout
    /// opens, and even ghosted it then covers the bar's icons.
    pub fn raise(&self) {
        // SAFETY: Our own live window; IsWindowVisible only reads its state.
        unsafe {
            if !IsWindowVisible(self.hwnd).as_bool() {
                return;
            }
            let z_after = if self.taskbar_backdrop.below_hwnd != 0 {
                HWND(self.taskbar_backdrop.below_hwnd as *mut _)
            } else {
                HWND_TOPMOST
            };
            let _ = SetWindowPos(
                self.hwnd,
                Some(z_after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    pub(super) fn place(&mut self) -> Result<()> {
        self.sync_progress_watch();
        let bounds = self.bounds;
        let bar_height = bounds.bottom - bounds.top;
        if bar_height <= 0 {
            return Ok(());
        }
        let hidden = self.auto_hide && !self.revealed;
        if !self.visible || !self.auto_hide {
            self.hide_until = None;
            self.exit_animation = None;
        }
        let on_screen = self.visible && (!hidden || self.hide_until.is_some());
        if let Some(monitor) = &self.accessories {
            monitor.set_visible(on_screen && !hidden);
        }
        self.sync_monitor(self.visible && !hidden);
        if !on_screen {
            // Keep the hidden tree transparent. Restoring opacity here races
            // HWND hiding and flashes its last opaque frame on the next show.
            self.park_surface()?;
        } else if !self.auto_hide {
            self.reset_surface()?;
        }
        let show = if on_screen {
            SWP_SHOWWINDOW
        } else {
            SWP_HIDEWINDOW
        };
        let window_top = bounds.bottom - self.visual_height;
        let z_after = if self.taskbar_backdrop.below_hwnd != 0 {
            HWND(self.taskbar_backdrop.below_hwnd as *mut _)
        } else {
            HWND_TOPMOST
        };
        // SAFETY: Our own live window.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(z_after),
                bounds.left,
                window_top,
                bounds.right - bounds.left,
                self.visual_height,
                SWP_NOACTIVATE | show,
            )?;
            if on_screen && (self.auto_hide || self.panel.is_some()) {
                // TrackMouseEvent may report a leave immediately after showing
                // a window. Poll at 100 ms during drags or hide countdowns, and
                // relax to 200 ms when the bar or popup is stationary.
                let interval = if self.volume_drag.is_some() || self.hide_until.is_some() {
                    POINTER_CHECK_MS
                } else {
                    POINTER_IDLE_MS
                };
                SetTimer(Some(self.hwnd), POINTER_TIMER, interval, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), POINTER_TIMER);
            }
            let _ = KillTimer(Some(self.hwnd), HIDE_ANIMATION_TIMER);
            if let Some(deadline) = self.hide_until {
                let wait = deadline.saturating_duration_since(Instant::now());
                SetTimer(
                    Some(self.hwnd),
                    HIDE_ANIMATION_TIMER,
                    (wait.as_millis() as u32).max(1),
                    None,
                );
            }
        }
        edge_watch::set_menu_bar((self.visible && hidden).then_some(bounds));
        self.sync_outside_click_hook()
    }

    /// The edge watcher has verified the pointer's dwell on this bar.
    pub fn input_switcher_anchor(&self, x: f32, y: f32) -> Option<POINT> {
        let bar_y = (self.visual_height - (self.bounds.bottom - self.bounds.top)).max(0) as f32;
        if y < bar_y || y >= self.visual_height as f32 {
            return None;
        }
        self.zones
            .iter()
            .find(|(left, right, action)| {
                x >= *left && x < *right && matches!(action, Action::InputLanguage)
            })
            .map(|(left, right, _)| POINT {
                x: self.bounds.left + ((left + right) * 0.5) as i32,
                y: self.bounds.top - 6,
            })
    }

    /// The icon under (`x`, `y`) and what its right-click menu offers.
    pub fn item_menu_at(&self, x: f32, y: f32) -> Option<ItemMenu> {
        let bar_y = (self.visual_height - (self.bounds.bottom - self.bounds.top)).max(0) as f32;
        if y < bar_y || y >= self.visual_height as f32 {
            return None;
        }
        let (left, right, action) = self
            .zones
            .iter()
            .find(|(left, right, _)| x >= *left && x < *right)?;
        let (key, tray) = match action {
            Action::TrayItem(idx) => {
                let item = self.look.as_ref()?.tray_items.get(*idx)?.clone();
                (item.preference_key(), Some(item))
            }
            other => (other.preference_key()?.to_owned(), None),
        };
        Some(ItemMenu {
            key,
            tray,
            anchor: POINT {
                x: self.bounds.left + ((left + right) * 0.5) as i32,
                y: self.bounds.top - 6,
            },
            pointer: cursor_anchor(),
        })
    }

    /// Renew the host sample without rebuilding an open island or its controls.
    pub(super) fn anchor(&self, item: Action) -> Option<POINT> {
        if !self.shown() {
            return None;
        }
        let (left, right, _) = self.zones.iter().find(|(_, _, action)| *action == item)?;
        Some(POINT {
            x: self.bounds.left + ((left + right) / 2.0) as i32,
            y: self.bounds.bottom,
        })
    }

    /// Under the battery icon, for the battery's moments.
    pub fn battery_anchor(&self) -> Option<POINT> {
        self.anchor(Action::Power)
    }
}
