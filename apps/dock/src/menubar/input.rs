use super::*;

impl MenuBar {
    /// A right click on an icon: closes the island and returns the icon's
    /// menu, which the caller shows outside the app borrow.
    pub fn right_click(&mut self, gfx: &Gfx, x: f32, y: f32) -> Result<Option<ItemMenu>> {
        let Some(menu) = self.item_menu_at(x, y) else {
            return Ok(None);
        };
        self.dismiss_island(gfx)?;
        Ok(Some(menu))
    }

    pub fn hover(&mut self, gfx: &Gfx, inside: bool) -> Result<()> {
        if self.volume_drag.is_some() && !inside {
            return Ok(());
        }
        if !inside && self.panel.take().is_some() {
            self.refresh(gfx)?;
        }
        if !self.auto_hide || self.revealed == inside {
            return Ok(());
        }
        let was_hiding = self.hide_until.is_some();
        let animated = self.visible && system::animations_enabled();
        self.animation_epoch = self.animation_epoch.wrapping_add(1);
        self.exit_animation = None;
        self.revealed = inside;
        self.hide_until = (!inside && animated)
            .then(|| Instant::now() + Duration::from_millis(u64::from(HIDE_FALLBACK_MS)));
        if !animated {
            self.reset_surface()?;
        } else if inside && !was_hiding {
            self.surface
                .SetOffset(Vector3::new(0.0, -self.reveal_lift(), 0.0))?;
            self.surface.SetOpacity(0.0)?;
        }
        self.place()?;
        if animated && let Err(error) = self.animate_visibility(gfx, inside) {
            navalia_shell::log::line(&format!("menu bar animation: {error:#}"));
            self.hide_until = None;
            self.reset_surface()?;
            self.place()?;
        }
        if inside {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            // SAFETY: `track` is a valid structure with its size initialized.
            unsafe { TrackMouseEvent(&mut track)? };
        }
        self.refresh(gfx)
    }

    pub(super) fn animate_visibility(&mut self, gfx: &Gfx, shown: bool) -> Result<()> {
        let c = &gfx.compositor;
        let ease = if shown {
            c.CreateCubicBezierEasingFunction(Vector2::new(0.2, 0.8), Vector2::new(0.2, 1.0))?
        } else {
            c.CreateCubicBezierEasingFunction(Vector2::new(0.4, 0.0), Vector2::new(1.0, 1.0))?
        };
        let duration = TimeSpan {
            Duration: i64::from(if shown { REVEAL_MS } else { HIDE_MS }) * 10_000,
        };
        let slide = c.CreateScalarKeyFrameAnimation()?;
        slide.SetDuration(duration)?;
        slide.InsertExpressionKeyFrame(0.0, h!("this.StartingValue"))?;
        slide.InsertKeyFrameWithEasingFunction(
            1.0,
            if shown { 0.0 } else { self.reveal_lift() },
            &ease,
        )?;
        let fade = c.CreateScalarKeyFrameAnimation()?;
        fade.SetDuration(duration)?;
        fade.InsertExpressionKeyFrame(0.0, h!("this.StartingValue"))?;
        fade.InsertKeyFrameWithEasingFunction(1.0, if shown { 1.0 } else { 0.0 }, &ease)?;
        let batch = (!shown)
            .then(|| c.CreateScopedBatch(CompositionBatchTypes::Animation))
            .transpose()?;
        let started = (|| -> Result<()> {
            self.surface.StartAnimation(h!("Offset.Y"), &slide)?;
            self.surface.StartAnimation(h!("Opacity"), &fade)?;
            Ok(())
        })();
        if let Some(batch) = batch {
            let window = self.hwnd.0 as isize;
            let epoch = self.animation_epoch;
            let completed = batch.Completed(&TypedEventHandler::new(move |_, _| {
                // SAFETY: Only scalar state is posted back to the UI thread.
                // A cancelled transition's already queued reply is ignored.
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(window as *mut _)),
                        HIDE_ANIMATION_DONE,
                        WPARAM(epoch),
                        LPARAM(0),
                    );
                }
                Ok(())
            }));
            // Register before ending the scope. End it even on failure so
            // unrelated later animations cannot become part of this exit.
            let ended = batch.End();
            started?;
            ended?;
            let completed = completed?;
            self.exit_animation = Some(ExitAnimation { batch, completed });
        } else {
            started?;
        }
        Ok(())
    }

    pub(super) fn park_surface(&self) -> Result<()> {
        self.surface.StopAnimation(h!("Offset.Y"))?;
        self.surface.StopAnimation(h!("Opacity"))?;
        self.surface
            .SetOffset(Vector3::new(0.0, self.reveal_lift(), 0.0))?;
        self.surface.SetOpacity(0.0)?;
        Ok(())
    }

    pub(super) fn reset_surface(&self) -> Result<()> {
        self.surface.StopAnimation(h!("Offset.Y"))?;
        self.surface.StopAnimation(h!("Opacity"))?;
        self.surface.SetOffset(Vector3::new(0.0, 0.0, 0.0))?;
        self.surface.SetOpacity(1.0)?;
        Ok(())
    }

    pub fn finish_hide(&mut self, gfx: &Gfx, completed_epoch: Option<usize>) -> Result<()> {
        let mut point = POINT::default();
        // SAFETY: Read-only cursor query before completing an exit transition.
        unsafe { GetCursorPos(&mut point)? };
        self.finish_hide_at(gfx, Instant::now(), point, completed_epoch)
    }

    pub(super) fn finish_hide_at(
        &mut self,
        gfx: &Gfx,
        now: Instant,
        point: POINT,
        completed_epoch: Option<usize>,
    ) -> Result<()> {
        // A queued timer from a cancelled transition cannot hide a reopened bar.
        if completed_epoch.is_some_and(|epoch| epoch != self.animation_epoch) {
            return Ok(());
        }
        let Some(deadline) = self.hide_until else {
            return Ok(());
        };
        self.pointer_left_at(gfx, point)?;
        if self.hide_until.is_none() {
            return Ok(());
        }
        if completed_epoch.is_some() || now >= deadline {
            self.hide_until = None;
            self.exit_animation = None;
        }
        self.place()
    }

    /// A leave notification can already be queued when the hidden window is
    /// shown. Check the pointer rather than hiding a newly revealed bar on it.
    pub fn check_pointer(&mut self, gfx: &Gfx) -> Result<()> {
        if self.panel.is_some() {
            // SAFETY: Read-only Escape state; checked only while a popup is open.
            if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x1B) } < 0 {
                return self.dismiss_island(gfx);
            }
        }
        let mut point = POINT::default();
        // SAFETY: Read-only cursor query in the same DPI context as `bounds`.
        unsafe { GetCursorPos(&mut point)? };
        self.pointer_left_at(gfx, point)
    }

    pub(super) fn pointer_left_at(&mut self, gfx: &Gfx, point: POINT) -> Result<()> {
        let x = (point.x - self.bounds.left) as f32;
        if let Some(span) = self.volume_drag {
            let y = (point.y - self.bounds.top) as f32;
            self.queue_volume(span, y);
            // A nonactivating background window may not receive mouse-up
            // outside its visible bounds. Reuse the existing popup pointer
            // check to finish the gesture without ever stealing app focus.
            let pressed = unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
                let primary = if GetSystemMetrics(SM_SWAPBUTTON) == 0 {
                    0x01
                } else {
                    0x02
                };
                windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(primary) < 0
            };
            if pressed {
                return self.paint_volume_preview(gfx);
            }
            self.release_volume(Some(y))?;
        }
        let within_bar = point.x >= self.bounds.left
            && point.x < self.bounds.right
            && point.y >= self.bounds.top
            && point.y < self.bounds.bottom;
        // Include the gap between the island and its popup so crossing it
        // does not close the popup before the pointer reaches its controls.
        let within_panel = self.panel.is_some()
            && x >= self.panel_span.left
            && x < self.panel_span.right
            && point.y >= PANEL_TOP.load(Ordering::Relaxed)
            && point.y < self.bounds.top;
        if (within_bar || within_panel) && self.hide_until.is_some() {
            self.hover(gfx, true)?;
        } else if !within_bar && !within_panel {
            self.hover(gfx, false)?;
        }
        Ok(())
    }

    /// Leaving the island sideways still stays inside the full-width bar window.
    pub fn pointer_moved(&mut self, gfx: &Gfx, x: f32, y: f32) -> Result<()> {
        if let Some(span) = self.volume_drag {
            self.queue_volume(span, y);
            return self.paint_volume_preview(gfx);
        }
        let bar_y = (self.visual_height - (self.bounds.bottom - self.bounds.top)).max(0) as f32;
        let active_trigger = match self.panel {
            Some(Island::Music) => self.music_trigger_span,
            Some(Island::Usage) => self.usage_trigger_span,
            None => self.triggers_span,
        };
        let span = if y >= bar_y {
            active_trigger
        } else {
            self.panel_span
        };
        if self.panel.is_some() && (x < span.left || x >= span.right) {
            self.dismiss_island(gfx)?;
        }
        self.hover(gfx, true)
    }

    pub fn dismiss_island(&mut self, gfx: &Gfx) -> Result<()> {
        self.release_volume(None)?;
        if self.panel.take().is_some() {
            self.refresh(gfx)?;
        }
        Ok(())
    }

    pub fn edge_rested(&mut self, gfx: &Gfx, point: POINT) -> Result<()> {
        if self.visible && self.auto_hide && !self.revealed {
            if navalia_shell::tracker::caption_button_at(point, self.bounds) {
                edge_watch::retry_on_move(edge_watch::Target::MenuBar);
            } else {
                self.hover(gfx, true)?;
            }
        }
        Ok(())
    }

    pub(super) fn sync_outside_click_hook(&mut self) -> Result<()> {
        let needed =
            self.panel_span.width() > 0.0 && self.visible && (!self.auto_hide || self.revealed);
        if needed && self.outside_click_hook.is_none() {
            // SAFETY: This process owns the hook procedure and installs the hook
            // only while the panel is visible.
            let module = unsafe { GetModuleHandleW(None)? };
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_MOUSE_LL,
                    Some(outside_click_proc),
                    Some(module.into()),
                    0,
                )?
            };
            ISLAND_HOOK_WINDOW.store(self.hwnd.0 as isize, Ordering::Relaxed);
            self.outside_click_hook = Some(hook);
        } else if !needed && let Some(hook) = self.outside_click_hook {
            // SAFETY: This hook was installed by this MenuBar.
            unsafe { UnhookWindowsHookEx(hook)? };
            self.outside_click_hook = None;
            ISLAND_HOOK_WINDOW.store(0, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Privacy activity or the user's taskbar preferences changed.
    pub fn click(&mut self, gfx: &Gfx, x: f32, y: f32) -> Result<()> {
        // A click anywhere else on the bar is a click outside an open shell
        // flyout; the items that open one toggle it themselves.
        let on_island = self.island_hits.iter().any(|hit| hit.at(x, y));
        let toggles_flyout = !on_island
            && self.zones.iter().any(|(left, right, action)| {
                x >= *left
                    && x < *right
                    && matches!(
                        action,
                        Action::QuickSettings
                            | Action::Sound
                            | Action::Notifications
                            | Action::Clock
                            | Action::Network
                            | Action::HiddenIcons
                    )
            });
        if !toggles_flyout {
            launcher::dismiss_flyouts();
        }
        if let Some(hit) = self.island_hits.iter().find(|hit| hit.at(x, y)) {
            match hit.action {
                IslandAction::Toggle(island) => {
                    self.panel = toggle_panel(self.panel, island);
                    if self.panel.is_some() {
                        let mut track = TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: self.hwnd,
                            dwHoverTime: 0,
                        };
                        // SAFETY: `track` describes our live window.
                        unsafe { TrackMouseEvent(&mut track)? };
                    }
                }
                IslandAction::Refresh => {
                    let targets = self.target_providers();
                    if let Some(usage) = &mut self.usage {
                        usage.refresh(targets, true);
                    }
                }
                IslandAction::SignIn(provider) => {
                    usage::launch_sign_in(provider);
                    self.dismiss_island(gfx)?;
                    return Ok(());
                }
                IslandAction::Media(control) => {
                    if let Some(media) = &self.media
                        && let Some(track) = self.look.as_ref().and_then(|look| look.track.as_ref())
                    {
                        media.send(track, control);
                    }
                    return Ok(());
                }
                IslandAction::Volume(_) => return Ok(()), // Handled on press/drag.
                IslandAction::Mute => {
                    if let Some(audio) = &mut self.audio {
                        audio.toggle_mute()?;
                    }
                    return self.audio_changed(gfx);
                }
                IslandAction::ScrollUsage(direction) => {
                    self.scroll_usage(f32::from(direction) * 72.0)?;
                    return Ok(());
                }
            }
            return self.refresh(gfx);
        }
        let bar_y = (self.visual_height - (self.bounds.bottom - self.bounds.top)).max(0) as f32;
        if y < bar_y {
            return Ok(());
        }
        self.dismiss_island(gfx)?;
        let hit = self
            .zones
            .iter()
            .find(|(left, right, _)| x >= *left && x < *right);
        // Where a flyout opened from this item should sit: above the bottom bar.
        let anchor = hit.map_or((0, 0), |(left, right, _)| {
            (
                self.bounds.left + ((left + right) / 2.0) as i32,
                self.bounds.top,
            )
        });
        match hit.map(|(_, _, action)| *action) {
            Some(Action::AccessoryIndicator) => {}
            Some(Action::QuickSettings) => launcher::open_quick_settings(),
            Some(Action::Notifications) => launcher::open_notifications(),
            Some(Action::Clock) => match &self.clock_target {
                // A target that no longer opens (an uninstalled app) falls
                // back to what the clock always opened.
                Some(target) if launcher::open(target).is_ok() => {}
                _ => launcher::open_notifications(),
            },
            Some(Action::Power) => launcher::open_power_settings(),
            Some(Action::Network) => launcher::open_network_list(),
            Some(Action::Sound) => launcher::open_quick_settings(),
            Some(Action::Search) => launcher::open_search(),
            Some(Action::TaskView) => launcher::open_task_view(),
            Some(Action::Bluetooth) => launcher::open_bluetooth(),
            Some(Action::SystemMonitor) => {
                let _ = launcher::open("taskmgr.exe");
            }
            Some(Action::HiddenIcons) => launcher::toggle_hidden_icons(anchor),
            Some(Action::InputLanguage) => {} // Opened outside the app borrow.
            // Windows' own pages list which apps use each device.
            Some(Action::Microphone) => {
                let _ = launcher::open("ms-settings:privacy-microphone");
            }
            Some(Action::Camera) => {
                let _ = launcher::open("ms-settings:privacy-webcam");
            }
            Some(Action::TrayItem(idx)) => {
                if let Some(item) = self
                    .look
                    .as_ref()
                    .and_then(|look| look.tray_items.get(idx))
                    .cloned()
                    && !self.click_tray_icon(&item)
                {
                    return self.tray_changed(gfx);
                }
            }
            Some(Action::ShelfItem(idx)) => {
                let Some((path, tray)) = self.look.as_ref().and_then(|look| {
                    let item = look.shelf.get(idx)?;
                    Some((
                        item.path.clone(),
                        look.shelf_tray.get(idx).cloned().flatten(),
                    ))
                }) else {
                    return Ok(());
                };
                // An app that is running with a tray icon answers as that icon
                // would; anything else is opened like a double-click in Explorer.
                match tray {
                    Some(icon) if self.click_tray_icon(&icon) => {}
                    Some(_) => {
                        if let Err(error) = launcher::open(&path) {
                            navalia_shell::log::line(&format!("shelf item {path}: {error}"));
                        }
                        return self.tray_changed(gfx);
                    }
                    None => {
                        if let Err(error) = launcher::open(&path) {
                            navalia_shell::log::line(&format!("shelf item {path}: {error}"));
                        }
                    }
                }
            }
            None => {}
        }
        Ok(())
    }

    /// Clicks a tray icon as Explorer would, telling a double-click from two
    /// clicks. `false` when its app closed without removing the icon.
    fn click_tray_icon(&mut self, item: &TrayIcon) -> bool {
        let now = Instant::now();
        // SAFETY: Reads a system setting.
        let window = Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() }));
        let double = self
            .tray_last_click
            .is_some_and(|(key, at)| key == item.key && now.duration_since(at) <= window);
        // A third click starts over, as with a mouse.
        self.tray_last_click = (!double).then_some((item.key, now));
        let kind = if double {
            TrayClick::Double
        } else {
            TrayClick::Left
        };
        if tray::click(item, kind, cursor_anchor()) {
            return true;
        }
        if let Some(host) = &self.tray_host {
            host.drop_dead_icons();
        }
        false
    }

    pub fn press_volume(&mut self, gfx: &Gfx, x: f32, y: f32) -> Result<()> {
        self.suppress_volume_release = false;
        let Some(hit) = self
            .island_hits
            .iter()
            .find(|hit| hit.at(x, y) && matches!(hit.action, IslandAction::Volume(_)))
        else {
            return Ok(());
        };
        let IslandAction::Volume(span) = hit.action else {
            return Ok(());
        };
        self.volume_drag = Some(span);
        // SAFETY: Capture only for a live slider drag, never to activate the bar.
        unsafe {
            SetCapture(self.hwnd);
        }
        self.queue_volume(span, y);
        self.paint_volume_preview(gfx)
    }

    pub fn scroll_usage_at(&mut self, point: POINT, delta: i16) -> Result<bool> {
        if !self.visible || (self.auto_hide && !self.revealed) || delta == 0 {
            return Ok(false);
        }
        let Some(viewport) = &self.usage_viewport else {
            return Ok(false);
        };
        let (x, y) = (
            (point.x - self.bounds.left) as f32,
            (point.y - self.bounds.top) as f32,
        );
        let (left, top, right, bottom) = viewport.bounds;
        if x < left || x >= right || y < top || y >= bottom {
            return Ok(false);
        }
        self.scroll_usage(-f32::from(delta) / 120.0 * 48.0)
    }

    pub(super) fn scroll_usage(&mut self, delta: f32) -> Result<bool> {
        let Some(viewport) = &self.usage_viewport else {
            return Ok(false);
        };
        let next = (self.usage_scroll + delta).clamp(0.0, viewport.maximum);
        if next != self.usage_scroll {
            self.usage_scroll = next;
            self.position_usage_body()?;
        }
        Ok(true)
    }

    pub(super) fn position_usage_body(&mut self) -> Result<()> {
        let Some(viewport) = &self.usage_viewport else {
            return Ok(());
        };
        let offset = (self.usage_scroll * viewport.scale).round();
        viewport
            .content
            .SetOffset(Vector3::new(0.0, -offset, 0.0))?;
        viewport
            .up
            .SetOpacity(if self.usage_scroll > 0.0 { 0.8 } else { 0.22 })?;
        viewport
            .down
            .SetOpacity(if self.usage_scroll < viewport.maximum {
                0.8
            } else {
                0.22
            })?;
        Ok(())
    }

    pub(super) fn paint_volume_preview(&mut self, gfx: &Gfx) -> Result<()> {
        if let Some(percent) = self.volume_target
            && let Some(controls) = &mut self.volume_controls
        {
            controls.paint(gfx, percent)?;
        }
        Ok(())
    }

    pub(super) fn queue_volume(&mut self, span: VolumeTrack, y: f32) {
        let percent = volume_percent(span, y);
        self.volume_target = Some(percent);
        if let Some(controls) = &mut self.volume_controls {
            // Optimistic GPU-only feedback is independent of endpoint latency.
            // Audio writes still proceed if device loss interrupts this paint;
            // the existing graphics heartbeat owns recovery.
            let _ = controls.move_to(percent);
        }
        if self.pending_volume == Some(percent)
            || (self.pending_volume.is_none()
                && self
                    .status
                    .volume
                    .is_some_and(|v| v.percent == percent && !v.muted))
        {
            return;
        }
        let arm = self.pending_volume.is_none();
        self.pending_volume = Some(percent);
        if arm {
            // At most 30 endpoint writes/second during dragging; no idle timer.
            unsafe {
                SetTimer(Some(self.hwnd), VOLUME_TIMER, 33, None);
            }
        }
    }

    pub fn flush_volume(&mut self) -> Result<()> {
        // SAFETY: Our own short-lived drag timer.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), VOLUME_TIMER);
        }
        if let Some(percent) = self.pending_volume.take()
            && let Some(audio) = &mut self.audio
        {
            audio.set_percent(percent)?;
        }
        Ok(())
    }

    pub fn release_volume(&mut self, y: Option<f32>) -> Result<bool> {
        let Some(span) = self.volume_drag.take() else {
            return Ok(y.is_some() && std::mem::take(&mut self.suppress_volume_release));
        };
        self.suppress_volume_release = true;
        if let Some(y) = y {
            self.queue_volume(span, y);
        }
        // Clear state first: ReleaseCapture posts a capture-lost notification.
        unsafe {
            if GetCapture() == self.hwnd {
                let _ = ReleaseCapture();
            }
        }
        if let Err(error) = self.flush_volume() {
            navalia_shell::log::line(&format!("adjust volume: {error:#}"));
        }
        self.volume_target = None;
        Ok(true)
    }
}
