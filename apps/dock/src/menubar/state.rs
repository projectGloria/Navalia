use super::*;

pub(super) fn same_volume_indicator(old: Option<Volume>, new: Option<Volume>) -> bool {
    old.map(|v| (v.level, v.muted)) == new.map(|v| (v.level, v.muted))
}

impl MenuBar {
    pub fn tray_changed(&mut self, gfx: &Gfx) -> Result<()> {
        let Some(host) = &self.tray_host else {
            return Ok(());
        };
        let items: Vec<TrayIcon> = host.icons().into_iter().filter(TrayIcon::shown).collect();
        if self.tray_items == items {
            return Ok(());
        }
        self.tray_icons
            .retain(|id, _| items.iter().any(|item| item.id() == *id));
        self.tray_items = items;
        self.refresh(gfx)
    }

    /// The tray icon under (`x`, `y`) on the bar row, if any.
    pub fn set_covered(&mut self, gfx: &Gfx, covered: bool) -> Result<()> {
        if self.covered == covered {
            return Ok(());
        }
        self.covered = covered;
        if !self.smart || self.hide_setting {
            return Ok(());
        }
        if covered {
            // It is on screen now: hide it the way a leaving pointer does.
            self.auto_hide = true;
            self.revealed = true;
            let mut point = POINT::default();
            // SAFETY: Read-only cursor query.
            let on_bar = unsafe { GetCursorPos(&mut point) }.is_ok()
                && point.x >= self.bounds.left
                && point.x < self.bounds.right
                && point.y >= self.bounds.top
                && point.y < self.bounds.bottom;
            if on_bar || self.panel.is_some() || self.volume_drag.is_some() {
                // In use: it goes once the pointer leaves, as usual.
                return self.place();
            }
            return self.hover(gfx, false);
        }
        let was_hidden = self.auto_hide && !self.revealed;
        self.auto_hide = false;
        self.revealed = false;
        self.hide_until = None;
        self.animation_epoch = self.animation_epoch.wrapping_add(1);
        self.exit_animation = None;
        self.place()?;
        if was_hidden && self.visible && system::animations_enabled() {
            // `place` showed it in its final state; slide it in from there.
            self.surface
                .SetOffset(Vector3::new(0.0, -self.reveal_lift(), 0.0))?;
            self.surface.SetOpacity(0.0)?;
            if let Err(error) = self.animate_visibility(gfx, true) {
                navalia_shell::log::line(&format!("menu bar animation: {error:#}"));
                self.reset_surface()?;
            }
        }
        // Content is not refreshed while hidden.
        self.refresh(gfx)
    }

    pub fn accessories_changed(&mut self, gfx: &Gfx) -> Result<()> {
        if self.accessories_enabled && self.visible && (!self.auto_hide || self.revealed) {
            self.refresh(gfx)?;
        }
        Ok(())
    }

    pub fn restart_accessories(&mut self, gfx: &Gfx) -> Result<()> {
        // Resume/audio notifications and the minute tick must never start a
        // disabled reader, including after a previously enabled one failed.
        if !self.accessories_enabled {
            return Ok(());
        }
        self.accessory_retry_at = Instant::now() + Duration::from_secs(60);
        self.accessories = None;
        self.accessories = accessories::Monitor::new(
            self.hwnd.0 as isize,
            ACCESSORIES_CHANGED,
            self.visible && (!self.auto_hide || self.revealed),
        )
        .inspect_err(|error| navalia_shell::log::line(&format!("accessories: {error:#}")))
        .ok();
        self.refresh(gfx)
    }

    pub fn registry_changed(&mut self, gfx: &Gfx) -> Result<()> {
        if let Some(watch) = &mut self.privacy_watch {
            watch.arm();
        }
        if let Some(watch) = &mut self.settings_watch {
            watch.arm();
        }
        if let Some(watch) = &mut self.search_watch {
            watch.arm();
        }
        // Only reread registry-backed indicators. This event must not trigger
        // network/audio queries, provider scans or accessory discovery.
        self.status.buttons = status::taskbar_buttons();
        self.status.battery_percentage = status::battery_percentage_enabled();
        self.status.privacy = status::privacy();
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        look.status = self.status;
        self.apply_look(gfx, look)
    }

    pub fn tick(&mut self, gfx: &Gfx) -> Result<()> {
        self.update_status();
        self.watch_network();
        let audio_changed = self
            .audio
            .as_ref()
            .is_some_and(AudioWatch::take_default_changed);
        let retry_due = Instant::now() >= self.accessory_retry_at
            && self.accessories.as_ref().is_none_or(|monitor| {
                monitor.snapshot().state == navalia_shell::accessories::State::Unavailable
            });
        if self.accessories_enabled && (audio_changed || retry_due) {
            self.restart_accessories(gfx)?;
        }
        self.schedule_next_minute();
        self.refresh(gfx)
    }

    /// Ordinary audio notifications do not requery network, registry, providers
    /// or clock. A default-device change still restarts enabled accessories.
    pub fn audio_changed(&mut self, gfx: &Gfx) -> Result<()> {
        let volume = self.audio.as_mut().and_then(AudioWatch::volume);
        if self
            .audio
            .as_ref()
            .is_some_and(AudioWatch::take_default_changed)
            && self.accessories_enabled
        {
            self.status.volume = volume;
            self.restart_accessories(gfx)?;
        }
        self.apply_volume_reading(gfx, volume)
    }

    pub(super) fn apply_volume_reading(&mut self, gfx: &Gfx, volume: Option<Volume>) -> Result<()> {
        self.status.volume = volume;
        if let Some(look) = &self.look
            && same_volume_indicator(look.status.volume, volume)
        {
            if let Some(volume) = volume
                && let Some(controls) = &mut self.volume_controls
            {
                controls.paint(gfx, self.volume_target.unwrap_or(volume.percent))?;
            }
            if let Some(look) = &mut self.look {
                look.status.volume = volume;
            }
            return Ok(());
        }
        // Availability, mute and speaker-glyph changes affect layout, but do
        // not justify a new provider/process or system-status scan either.
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        look.status.volume = volume;
        self.apply_look(gfx, look)
    }

    pub fn bluetooth_changed(&mut self, gfx: &Gfx) -> Result<()> {
        let on = self
            .bluetooth
            .as_ref()
            .is_some_and(bluetooth::Monitor::is_on);
        self.status.bluetooth_on = on;
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        if look.status.bluetooth_on == on {
            return Ok(());
        }
        look.status.bluetooth_on = on;
        self.apply_look(gfx, look)
    }

    pub(super) fn update_status(&mut self) {
        self.status = Status {
            bluetooth_on: self
                .bluetooth
                .as_ref()
                .is_some_and(bluetooth::Monitor::is_on),
            network: status::network(),
            volume: self.audio.as_mut().and_then(AudioWatch::volume),
            buttons: status::taskbar_buttons(),
            privacy: status::privacy(),
            battery_percentage: status::battery_percentage_enabled(),
        };
    }

    /// One wake-up per minute, aligned to the minute, instead of polling.
    pub(super) fn schedule_next_minute(&self) {
        // SAFETY: Read-only query, then a timer on our own window (setting an
        // existing timer id replaces it).
        unsafe {
            let now: SYSTEMTIME = GetLocalTime();
            let elapsed = u32::from(now.wSecond) * 1000 + u32::from(now.wMilliseconds);
            SetTimer(
                Some(self.hwnd),
                CLOCK_TIMER,
                60_000 - elapsed.min(59_900) + 50,
                None,
            );
        }
    }

    pub fn refresh_backdrops(&self, gfx: &Gfx) -> Result<()> {
        gfx.refresh_backdrops(&self.surface)?;
        Ok(())
    }

    /// Redraws the bar if anything it shows has changed.
    pub fn refresh(&mut self, gfx: &Gfx) -> Result<()> {
        let Some(monitor) = system::primary_monitor() else {
            return Ok(());
        };
        let active_providers = if self.usage.is_some() {
            usage::active_providers()
        } else {
            ActiveProviders::default()
        };
        let track = self.media.as_ref().and_then(media::Watch::snapshot);
        self.panel = available_panel(self.panel, track.is_some(), active_providers.any());
        if !active_providers.any() {
            if let Some(usage) = &mut self.usage {
                usage.mark_inactive();
            }
        } else if self.visible
            && (!self.auto_hide || self.revealed)
            && let Some(usage) = &mut self.usage
        {
            usage.refresh(false);
        }
        let appearance = appearance::menu_bar();
        let look = Look {
            accessories: self.accessories.as_ref().map_or([None, None], |monitor| {
                accessory_readings(&monitor.snapshot())
            }),
            monitor: self.monitor_lines(),
            language: strings::language(),
            screen: monitor.screen,
            scale: monitor.scale,
            dock_cutout: self.dock_cutout,
            taskbar_backdrop: self.taskbar_backdrop,
            material: appearance.material,
            light: appearance.light,
            glass: appearance.glass,
            app_name: self.app_name.clone(),
            clock: system::clock_text(),
            input: system::input_language(),
            battery: system::battery(),
            status: self.status,
            widgets: self.widgets,
            usage: self
                .usage
                .as_ref()
                .map_or_else(Snapshot::default, UsageMonitor::snapshot),
            usage_enabled: self.usage.is_some() && active_providers.any(),
            active_providers,
            panel: self.panel,
            usage_refreshing: self.usage.as_ref().is_some_and(UsageMonitor::is_refreshing),
            track,
            tray_items: self.tray_items.clone(),
        };
        self.apply_look(gfx, look)
    }

    /// Samples only while the monitor is on and the bar shows; a hidden or
    /// auto-hidden bar costs nothing (issue #64).
    pub(super) fn sync_monitor(&mut self, shown: bool) {
        let wanted = self.widgets.system_monitor && shown;
        if wanted == self.monitor.is_some() {
            return;
        }
        if wanted {
            let mut sampler = navalia_shell::monitor::Sampler::default();
            // Memory reads at once; CPU and network keep their last values
            // until the first sample, half a second later.
            self.monitor_reading.memory = sampler.sample().memory;
            self.monitor = Some(sampler);
            // SAFETY: A timer on our own window.
            unsafe {
                SetTimer(Some(self.hwnd), MONITOR_TIMER, MONITOR_FIRST_MS, None);
            }
        } else {
            self.monitor = None;
            // SAFETY: A timer on our own window.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), MONITOR_TIMER);
            }
        }
    }

    /// A new sample of the system monitor; redraws if what it shows changed.
    pub fn monitor_tick(&mut self, gfx: &Gfx) -> Result<()> {
        let Some(sampler) = &mut self.monitor else {
            return Ok(());
        };
        self.monitor_reading = sampler.sample();
        // SAFETY: A timer on our own window; after the first sample, the
        // regular interval.
        unsafe {
            SetTimer(Some(self.hwnd), MONITOR_TIMER, MONITOR_EVERY_MS, None);
        }
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        look.monitor = self.monitor_lines();
        self.apply_look(gfx, look)
    }

    /// CPU and memory, then upload and download, as the bar shows them.
    pub(super) fn monitor_lines(&self) -> Option<[String; 4]> {
        if !self.widgets.system_monitor {
            return None;
        }
        let reading = self.monitor_reading;
        let comma = matches!(
            strings::language(),
            Language::Es | Language::Fr | Language::Pt
        );
        let share = |label: &str, value: Option<u8>| {
            value.map_or_else(|| format!("{label} —"), |value| format!("{label} {value}%"))
        };
        let speed = |arrow: char, value: Option<u64>| {
            value.map_or_else(
                || format!("{arrow} —"),
                |value| {
                    format!(
                        "{arrow} {}",
                        navalia_shell::monitor::format_rate(value, comma)
                    )
                },
            )
        };
        Some([
            share("CPU", reading.cpu),
            share("RAM", reading.memory),
            speed('↑', reading.up),
            speed('↓', reading.down),
        ])
    }

    /// Track changes only need to update the contents of the island.
    pub fn media_changed(&mut self, gfx: &Gfx) -> Result<()> {
        let Some(mut look) = self.look.clone() else {
            return self.refresh(gfx);
        };
        let track = self.media.as_ref().and_then(media::Watch::snapshot);
        let timeline_only = look
            .track
            .as_ref()
            .zip(track.as_ref())
            .is_some_and(|(old, new)| {
                old.same_content(new) && old.timeline.is_some() == new.timeline.is_some()
            });
        look.track = track;
        if timeline_only {
            self.look = Some(look);
            return self.progress_tick(gfx);
        }
        self.panel = available_panel(self.panel, look.track.is_some(), look.usage_enabled);
        look.panel = self.panel;
        self.apply_look(gfx, look)
    }

    pub(super) fn apply_look(&mut self, gfx: &Gfx, mut look: Look) -> Result<()> {
        look.active_providers = look.usage.visible_providers(look.active_providers);
        look.usage_enabled &= look.active_providers.any();
        self.panel = available_panel(look.panel, look.track.is_some(), look.usage_enabled);
        look.panel = self.panel;
        if look.panel != Some(Island::Usage) {
            self.usage_scroll = 0.0;
        }
        let moved = self
            .look
            .as_ref()
            .is_some_and(|old| old.screen != look.screen || old.scale != look.scale);
        if look.panel != Some(Island::Music) || moved {
            self.release_volume(None)?;
        }
        if self.look.as_ref() == Some(&look) {
            return Ok(());
        }
        let moment = self
            .look
            .as_ref()
            .and_then(|old| usage_moment(&old.usage, &look.usage, &look.active_providers));
        self.build(gfx, &look)?;
        // A crowded bar can have no room even for a compact trigger. Do not
        // keep a ghost popup that reappears by itself when space returns.
        if look.panel.is_some() && self.panel_span.width() <= 0.0 {
            self.panel = None;
            look.panel = None;
            self.build(gfx, &look)?;
        }
        self.look = Some(look);
        self.sync_progress_watch();
        if let Some((provider, act)) = moment
            && let Some(&(_, x)) = self.usage_anchors.iter().find(|(p, _)| *p == provider)
        {
            self.cue = Some((act, x));
        }
        Ok(())
    }

    /// A moment worth the crab, found by the last update: the act and the
    /// screen x of the provider in the island. Taken once.
    /// The moment waiting for the mascot, with the point under the bar
    /// where it happens, or `None` when the bar does not show now.
    pub fn take_cue(&mut self) -> Option<(mascot::Act, Option<POINT>)> {
        let (act, x) = self.cue.take()?;
        Some((
            act,
            self.shown().then_some(POINT {
                x,
                y: self.bounds.bottom,
            }),
        ))
    }

    /// Whether the bar is on screen now.
    pub(super) fn watch_network(&mut self) {
        let online = !matches!(self.status.network, Network::Offline);
        let before = self.online.replace(online);
        if online {
            if self.offline_told {
                self.network_cue(mascot::Act::Online);
            }
            self.offline_since = None;
            self.offline_told = false;
            // SAFETY: A timer on our own window.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), NETWORK_TIMER);
            }
            return;
        }
        if before == Some(true) {
            self.offline_since = Some(Instant::now());
            // Fires a tick, which reads the network again.
            // SAFETY: A timer on our own window.
            unsafe {
                SetTimer(
                    Some(self.hwnd),
                    NETWORK_TIMER,
                    OFFLINE_GRACE.as_millis() as u32,
                    None,
                );
            }
        } else if !self.offline_told
            && self
                .offline_since
                .is_some_and(|since| since.elapsed() + Duration::from_millis(250) >= OFFLINE_GRACE)
        {
            self.offline_told = true;
            // SAFETY: A timer on our own window.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), NETWORK_TIMER);
            }
            self.network_cue(mascot::Act::Offline);
        }
    }

    pub(super) fn network_cue(&mut self, act: mascot::Act) {
        let x = self
            .zones
            .iter()
            .find(|(_, _, action)| *action == Action::Network)
            .map_or(
                self.bounds.left + (self.bounds.right - self.bounds.left) / 2,
                |(left, right, _)| self.bounds.left + ((left + right) / 2.0) as i32,
            );
        self.cue = Some((act, x));
    }

    pub(super) fn sync_progress_watch(&mut self) {
        let shown =
            self.panel == Some(Island::Music) && self.visible && (!self.auto_hide || self.revealed);
        if let Some(media) = &self.media {
            media.show_progress(shown);
        }
        let ticking = shown
            && self.progress_controls.is_some()
            && self
                .look
                .as_ref()
                .and_then(|look| look.track.as_ref())
                .and_then(|track| track.timeline)
                .is_some_and(|timeline| timeline.advancing(Instant::now()));
        if ticking != self.progress_timer {
            // This timer only advances cached position/time labels while the
            // player popup is visible. No API calls, artwork reads or polling.
            unsafe {
                self.progress_timer = if ticking {
                    SetTimer(Some(self.hwnd), PROGRESS_TIMER, 1000, None) != 0
                } else {
                    let _ = KillTimer(Some(self.hwnd), PROGRESS_TIMER);
                    false
                };
            }
        }
    }

    pub fn progress_tick(&mut self, gfx: &Gfx) -> Result<()> {
        if let Some(timeline) = self
            .look
            .as_ref()
            .and_then(|look| look.track.as_ref())
            .and_then(|track| track.timeline)
            && let Some(controls) = &mut self.progress_controls
        {
            controls.paint(gfx, timeline, Instant::now())?;
        }
        self.sync_progress_watch();
        Ok(())
    }
}
