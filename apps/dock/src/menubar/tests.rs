use super::*;

mod island_tests {
    use super::*;

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic preferences, no Windows settings writes"]
    fn native_status_items_follow_windows_preferences_without_overrides() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        let mut look = bar.look.clone().context("initial look")?;
        look.screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        look.battery = Some(Battery {
            percent: 75,
            charging: false,
            saver: false,
        });
        look.accessories = [None, None];
        look.status.bluetooth_on = false;
        let mut battery_width = [0.0; 2];
        for mask in 0..16 {
            look.status.buttons = TaskbarButtons {
                search: mask & 1 != 0,
                task_view: mask & 2 != 0,
                widgets: mask & 4 != 0,
            };
            look.status.battery_percentage = mask & 8 != 0;
            bar.apply_look(&gfx, look.clone())?;
            for (action, on) in [
                (Action::Search, look.status.buttons.search),
                (Action::TaskView, look.status.buttons.task_view),
            ] {
                assert_eq!(
                    bar.zones.iter().filter(|(_, _, a)| *a == action).count(),
                    usize::from(on)
                );
            }
            // The battery is the one Power zone. Its percentage must add
            // label width, without introducing another clickable control.
            let mut power = bar.zones.iter().filter(|(_, _, a)| *a == Action::Power);
            let battery = power.next().context("battery zone")?;
            assert!(power.next().is_none());
            battery_width[usize::from(look.status.battery_percentage)] = battery.1 - battery.0;
        }
        assert!(battery_width[1] > battery_width[0]);
        // No phantom battery or percentage on a desktop without a battery.
        look.battery = None;
        bar.apply_look(&gfx, look)?;
        let count = |action: Action| bar.zones.iter().filter(|(_, _, a)| *a == action).count();
        // Only the control center opens all of quick settings; the network
        // icon has its own list (issue #55).
        assert_eq!(count(Action::QuickSettings), 1);
        assert_eq!(count(Action::Network), 1);
        assert_eq!(count(Action::Power), 0);

        // The native registry handler reads the real Windows values without
        // replacing the media/provider snapshot or resetting the clock timer.
        bar.registry_changed(&gfx)?;
        let current = bar.look.as_ref().context("updated look")?;
        assert!(current.status.buttons == status::taskbar_buttons());
        assert_eq!(
            current.status.battery_percentage,
            status::battery_percentage_enabled()
        );
        Ok(())
    }

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic preferences, nothing is opened"]
    fn native_notification_bell_takes_the_right_end_only_when_chosen() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        let mut look = bar.look.clone().context("initial look")?;
        look.screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        for notifications in [false, true, false] {
            look.widgets.notifications = notifications;
            bar.apply_look(&gfx, look.clone())?;
            // Zones are pushed from the right edge inward.
            let order: Vec<Action> = bar.zones.iter().map(|(_, _, a)| *a).collect();
            let expected: &[Action] = if notifications {
                &[Action::Notifications, Action::Clock, Action::QuickSettings]
            } else {
                &[Action::Clock, Action::QuickSettings]
            };
            assert_eq!(order.get(..expected.len()), Some(expected));
            assert_eq!(
                order
                    .iter()
                    .filter(|a| **a == Action::Notifications)
                    .count(),
                usize::from(notifications)
            );
        }
        // The clock target is not drawn: choosing one keeps the same look.
        let drawn = bar.look.clone();
        bar.set_clock_target(Some("ms-clock:"));
        assert_eq!(bar.clock_target.as_deref(), Some("ms-clock:"));
        assert!(bar.look == drawn);
        bar.set_clock_target(None);
        assert_eq!(bar.clock_target, None);
        Ok(())
    }

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic network states"]
    fn native_network_moments_wait_out_brief_drops_and_tell_once() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, false, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.cue = None;
        bar.online = None;
        let act = |bar: &mut MenuBar| bar.cue.take().map(|(act, _)| act);
        // The first reading is only the starting point.
        bar.status.network = Network::Ethernet;
        bar.watch_network();
        assert_eq!(act(&mut bar), None);
        // A brief drop says nothing.
        bar.status.network = Network::Offline;
        bar.watch_network();
        assert_eq!(act(&mut bar), None);
        bar.status.network = Network::Ethernet;
        bar.watch_network();
        assert_eq!(act(&mut bar), None);
        // Down for the grace period: told once.
        bar.status.network = Network::Offline;
        bar.watch_network();
        bar.offline_since = Some(Instant::now() - OFFLINE_GRACE);
        bar.watch_network();
        assert_eq!(act(&mut bar), Some(mascot::Act::Offline));
        bar.watch_network();
        assert_eq!(act(&mut bar), None);
        // Back: relief, also once.
        bar.status.network = Network::Ethernet;
        bar.watch_network();
        assert_eq!(act(&mut bar), Some(mascot::Act::Online));
        bar.watch_network();
        assert_eq!(act(&mut bar), None);
        // A hidden bar hands the moment over without a point (the dock's).
        bar.cue = Some((mascot::Act::Online, 10));
        assert!(bar.take_cue().is_some_and(|(_, point)| point.is_none()));
        Ok(())
    }

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic coverage changes"]
    fn native_intelligent_bar_hides_only_while_covered_and_reserves_nothing() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, false, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.set_visible(true)?;
        assert!(bar.reserved_strip().is_some());
        bar.set_smart_hide(true)?;
        // Intelligent: shown, but maximized windows get the whole screen.
        assert!(!bar.auto_hide && bar.reserved_strip().is_none());
        bar.set_covered(&gfx, true)?;
        assert!(bar.auto_hide);
        bar.set_covered(&gfx, false)?;
        assert!(!bar.auto_hide && !bar.revealed && bar.hide_until.is_none());
        // Automatically hide wins: coverage changes nothing.
        bar.set_auto_hide(true)?;
        bar.set_covered(&gfx, true)?;
        assert!(bar.auto_hide);
        bar.set_covered(&gfx, false)?;
        assert!(bar.auto_hide);
        // Back to intelligent while covered: hidden at once.
        bar.set_covered(&gfx, true)?;
        bar.set_auto_hide(false)?;
        assert!(bar.auto_hide);
        // Always visible: reserves its strip again and shows.
        bar.set_smart_hide(false)?;
        assert!(!bar.auto_hide && bar.reserved_strip().is_some());
        bar.set_visible(false)?;
        Ok(())
    }

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic monitor readings"]
    fn native_system_monitor_keeps_its_place_and_samples_only_while_shown() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let widgets = MenuBarWidgets {
            system_monitor: true,
            ..MenuBarWidgets::default()
        };
        let mut bar = MenuBar::create(&gfx, false, false, false, widgets)?;
        bar.set_visible(false)?;
        // Hidden, nothing is sampled.
        assert!(bar.monitor.is_none());
        let mut look = bar.look.clone().context("initial look")?;
        look.screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let lines = |cpu: &str, down: &str| {
            [
                format!("CPU {cpu}"),
                "RAM 48%".to_owned(),
                "↑ 12 KB/s".to_owned(),
                format!("↓ {down}"),
            ]
        };
        let zone = |bar: &MenuBar| {
            bar.zones
                .iter()
                .find(|(_, _, action)| *action == Action::SystemMonitor)
                .map(|(left, right, _)| (*left, *right))
        };
        look.monitor = None;
        bar.apply_look(&gfx, look.clone())?;
        assert_eq!(zone(&bar), None);
        look.monitor = Some(lines("5%", "0 KB/s"));
        bar.apply_look(&gfx, look.clone())?;
        let narrow = zone(&bar).context("monitor zone")?;
        // The last zone pushed is the leftmost: the monitor sits left of
        // every status icon.
        assert_eq!(
            bar.zones.last().map(|(_, _, action)| *action),
            Some(Action::SystemMonitor)
        );
        look.monitor = Some(lines("100%", "35,5 MB/s"));
        bar.apply_look(&gfx, look.clone())?;
        let wide = zone(&bar).context("monitor zone")?;
        assert!(wide.0 < narrow.0 && wide.1 == narrow.1);
        // Narrower digits again: the columns keep their widest size, so the
        // rest of the bar does not shift back and forth.
        look.monitor = Some(lines("5%", "0 KB/s"));
        bar.apply_look(&gfx, look)?;
        assert_eq!(zone(&bar), Some(wide));
        Ok(())
    }

    #[test]
    #[ignore = "Hidden compositor fixture; synthetic Bluetooth states, no hardware changes"]
    fn native_bluetooth_indicator_appears_only_for_an_enabled_system_radio() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.bluetooth = None;
        let mut look = bar.look.clone().context("initial look")?;
        look.screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        for on in [false, true, false] {
            look.status.bluetooth_on = on;
            bar.status.bluetooth_on = on;
            bar.apply_look(&gfx, look.clone())?;
            assert_eq!(
                bar.zones
                    .iter()
                    .filter(|(_, _, action)| *action == Action::Bluetooth)
                    .count(),
                usize::from(on)
            );
        }
        // The native event handler updates the cached Look instead of running
        // provider/process scans or re-reading unrelated Windows preferences.
        look.status.bluetooth_on = true;
        bar.apply_look(&gfx, look)?;
        bar.bluetooth_changed(&gfx)?;
        assert!(
            !bar.zones
                .iter()
                .any(|(_, _, action)| *action == Action::Bluetooth)
        );
        Ok(())
    }

    #[test]
    #[ignore = "Native popup layout/input regression; hidden window, no audio writes"]
    fn native_popup_overflow_and_layout_changes_keep_input_in_sync() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        // SAFETY: All composition objects stay on this test's STA thread.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.audio = None;
        let mut look = bar.look.clone().context("initial look")?;
        look.screen = RECT {
            left: -1920,
            top: -768,
            right: 0,
            bottom: 0,
        };
        look.track = None;
        look.panel = Some(Island::Usage);
        look.usage_enabled = true;
        look.active_providers.0.fill(true);
        for provider in Provider::ALL {
            look.usage.providers[provider as usize] = ProviderUsage {
                state: State::Ready,
                windows: [300, 10_080]
                    .into_iter()
                    .map(|minutes| usage::Window {
                        label: None,
                        minutes,
                        used_percent: 30,
                        resets_at: None,
                    })
                    .collect(),
                fetched_at: Some(std::time::SystemTime::now()),
            };
        }
        let opencode = &mut look.usage.providers[Provider::OpenCode as usize];
        opencode.windows.push(usage::Window {
            label: None,
            minutes: 43_200,
            used_percent: 10,
            resets_at: None,
        });
        for style in [IslandStyle::Notch, IslandStyle::Bubble] {
            look.widgets.island_style = style;
            for scale in [1.0, 1.5, 2.0] {
                look.scale = scale;
                for light in [false, true] {
                    look.light = light;
                    bar.usage_scroll = 0.0;
                    bar.build(&gfx, &look)?;
                    assert!(bar.visual_height <= 769, "popup must fit a 768 px display");
                    if scale == 1.0 {
                        assert!(bar.usage_viewport.is_none());
                        continue;
                    }
                    let viewport = bar
                        .usage_viewport
                        .as_ref()
                        .context("overflow needs a viewport")?;
                    let bounds = viewport.bounds;
                    let maximum = viewport.maximum;
                    assert!(maximum > 0.0);
                    let body = viewport.content.clone();
                    let down = viewport.down.clone();
                    let root = bar.surface.Children()?.First()?.Current()?;
                    for _ in 0..20 {
                        assert!(bar.scroll_usage(48.0)?);
                    }
                    assert_eq!(bar.surface.Children()?.First()?.Current()?, root);
                    assert_eq!(bar.usage_scroll, maximum);
                    assert_eq!(body.Offset()?.Y, -(maximum * scale).round());
                    assert_eq!(down.Opacity()?, 0.22);
                    let up = bar
                        .island_hits
                        .iter()
                        .find(|hit| matches!(hit.action, IslandAction::ScrollUsage(-1)))
                        .context("scroll up button")?;
                    let (left, top, right, bottom) = up.bounds;
                    assert!(
                        left >= bounds.0
                            && top >= bounds.1
                            && right <= bounds.2
                            && bottom <= bounds.3
                    );
                    bar.click(&gfx, (left + right) / 2.0, (top + bottom) / 2.0)?;
                    assert!(bar.usage_scroll < maximum);
                    assert_eq!(bar.surface.Children()?.First()?.Current()?, root);
                    assert!(bar.scroll_usage(-10_000.0)?);
                    assert_eq!(bar.usage_scroll, 0.0);
                }
            }
        }
        // A busy refresh is not another active input target.
        look.usage_refreshing = true;
        bar.build(&gfx, &look)?;
        assert!(
            !bar.island_hits
                .iter()
                .any(|hit| matches!(hit.action, IslandAction::Refresh))
        );
        look.usage_refreshing = false;
        bar.scroll_usage(10_000.0)?;
        look.active_providers.0.fill(false);
        look.active_providers.0[Provider::Codex as usize] = true;
        bar.build(&gfx, &look)?;
        assert_eq!(bar.usage_scroll, 0.0); // A refresh that removes rows clamps the offset.
        assert!(bar.usage_viewport.is_none());

        look.panel = Some(Island::Music);
        look.track = Some(Track::test_fixture());
        bar.panel = look.panel;
        bar.status.volume = Some(Volume {
            level: 2,
            muted: false,
            percent: 50,
        });
        look.status = bar.status;
        bar.apply_look(&gfx, look.clone())?;
        bar.apply_volume_reading(
            &gfx,
            Some(Volume {
                level: 0,
                muted: true,
                percent: 0,
            }),
        )?;
        // Mute/glyph changes render the current snapshot; a full refresh would
        // replace this monitor and scan live processes/system status again.
        assert!(
            bar.look
                .as_ref()
                .is_some_and(|current| current.screen == look.screen
                    && current.status.volume.is_some_and(|volume| volume.muted))
        );
        let span = bar
            .island_hits
            .iter()
            .find_map(|hit| match hit.action {
                IslandAction::Volume(span) => Some(span),
                _ => None,
            })
            .context("slider")?;
        bar.volume_drag = Some(span); // Never capture the user's mouse in a test.
        bar.queue_volume(span, span.top);
        look.scale = 1.5;
        bar.apply_look(&gfx, look.clone())?;
        assert!(
            bar.volume_drag.is_none()
                && bar.pending_volume.is_none()
                && bar.volume_target.is_none()
        );
        assert!(bar.release_volume(Some(span.bottom))?); // Late up must not activate another button.
        let span = bar
            .island_hits
            .iter()
            .find_map(|hit| match hit.action {
                IslandAction::Volume(span) => Some(span),
                _ => None,
            })
            .context("new slider")?;
        bar.volume_drag = Some(span);
        bar.queue_volume(span, span.bottom);
        // Move the capsule by changing its anchor style, without changing the
        // monitor/DPI. A provider count can now leave the capsule in place.
        look.widgets.island_style = IslandStyle::Notch;
        bar.apply_look(&gfx, look.clone())?;
        assert!(bar.volume_drag.is_none() && bar.pending_volume.is_none());

        // A trigger that cannot fit must not leave a latent open popup.
        look.screen.right = look.screen.left + 160;
        bar.apply_look(&gfx, look.clone())?;
        assert!(bar.panel.is_none() && bar.look.as_ref().is_some_and(|look| look.panel.is_none()));
        let mut wide = bar.look.clone().context("crowded look")?;
        wide.screen.right = wide.screen.left + 1920;
        bar.apply_look(&gfx, wide)?;
        assert!(bar.panel_span.width() <= 0.0 && bar.usage_viewport.is_none());
        Ok(())
    }

    #[test]
    fn vertical_volume_coordinates_increase_upwards_and_clamp_at_both_ends() {
        let span = VolumeTrack {
            center_x: 200.0,
            top: 100.0,
            bottom: 300.0,
        };
        for (y, expected) in [
            (-100.0, 100),
            (100.0, 100),
            (200.0, 50),
            (299.0, 1),
            (300.0, 0),
            (500.0, 0),
            (f32::NAN, 0),
            (f32::INFINITY, 0),
        ] {
            assert_eq!(volume_percent(span, y), expected);
        }
        assert_eq!(volume_percent(VolumeTrack::default(), 1.0), 0);
    }

    #[test]
    fn volume_capsule_stays_beside_the_player_and_inside_each_screen_edge() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for (left, right, width) in [
                (12.0, 372.0, 800.0),
                (428.0, 788.0, 800.0),
                (220.0, 580.0, 800.0),
                (12.0, 380.0, 392.0),
            ] {
                let (music, capsule) = volume_popover_layout(
                    Span {
                        left: left * scale,
                        right: right * scale,
                    },
                    width * scale,
                    scale,
                );
                assert!(music.left >= 0.0 && capsule.left >= 0.0);
                assert!(music.right <= width * scale && capsule.right <= width * scale);
                assert!(
                    music.right < capsule.left || capsule.right < music.left,
                    "never cover playback controls"
                );
                assert!(music.width() > capsule.width());
            }
        }
        let panel = Span {
            left: 428.0,
            right: 788.0,
        };
        let (music, capsule) = volume_popover_layout(panel, 800.0, 1.0);
        assert!(music == panel && capsule.right < music.left);
    }

    #[test]
    fn playback_timestamps_include_hours_without_affecting_short_tracks() {
        for (seconds, expected) in [
            (0, "0:00"),
            (65, "1:05"),
            (3599, "59:59"),
            (3600, "1:00:00"),
            (3661, "1:01:01"),
        ] {
            assert_eq!(media_time(seconds), expected);
        }
    }

    #[test]
    fn percentage_changes_keep_the_same_indicator_but_mute_and_device_changes_do_not() {
        let volume = Volume {
            level: 2,
            muted: false,
            percent: 40,
        };
        assert!(same_volume_indicator(
            Some(volume),
            Some(Volume {
                percent: 60,
                ..volume
            })
        ));
        assert!(!same_volume_indicator(
            Some(volume),
            Some(Volume {
                muted: true,
                ..volume
            })
        ));
        assert!(!same_volume_indicator(
            Some(volume),
            Some(Volume { level: 3, ..volume })
        ));
        assert!(!same_volume_indicator(Some(volume), None));
        assert!(!same_volume_indicator(None, Some(volume)));
    }

    #[test]
    #[ignore = "Native compositor coverage; hidden test window, no audio writes"]
    fn native_music_artwork_and_volume_fit_both_styles_and_release_drag_state() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.audio = None; // This fixture can never change the user's audio.
        bar.status.volume = Some(Volume {
            level: 2,
            muted: false,
            percent: 42,
        });
        let mut look = bar.look.clone().context("initial look")?;
        look.status = bar.status;
        look.panel = Some(Island::Music);
        look.track = Some(Track::test_fixture());
        for style in [IslandStyle::Notch, IslandStyle::Bubble] {
            look.widgets.island_style = style;
            for scale in [1.0, 1.25, 1.5, 2.0] {
                look.scale = scale;
                for (light, glass) in [(true, true), (false, true), (true, false), (false, false)] {
                    look.light = light;
                    look.glass = glass;
                    bar.build(&gfx, &look)?;
                    assert!(bar.cover.is_some());
                    assert!(bar.progress_controls.is_some());
                    let capsule = bar
                        .volume_controls
                        .as_ref()
                        .context("volume capsule should be visible with the player")?
                        .geometry;
                    let progress = bar.progress_controls.as_ref().context("progress")?;
                    assert_eq!(
                        progress.width,
                        progress.geometry.width - progress.geometry.px(52.0),
                        "the progress track keeps only a balanced right inset"
                    );
                    assert!(
                        progress.geometry.left + progress.geometry.width <= capsule.left
                            || progress.geometry.left >= capsule.left + capsule.width,
                        "the volume capsule stays beside the player"
                    );
                    let brush = bar.cover.as_ref().context("missing cover")?.1.clone();
                    bar.build(&gfx, &look)?;
                    assert_eq!(bar.cover.as_ref().context("cached cover")?.1, brush);
                    let volume = bar
                        .island_hits
                        .iter()
                        .find(|hit| matches!(hit.action, IslandAction::Volume(_)))
                        .context("missing volume hit")?;
                    let (left, top, right, bottom) = volume.bounds;
                    assert!(left >= bar.panel_span.left && right <= bar.panel_span.right);
                    assert!(
                        top >= HEIGHT * scale
                            && bottom <= (HEIGHT + PANEL_GAP + look.panel_height()) * scale + 1.0
                    );
                    let geometry = bar
                        .volume_controls
                        .as_ref()
                        .context("volume controls")?
                        .geometry;
                    // Click the visible speaker's centre, not merely somewhere
                    // inside an independently specified hit rectangle.
                    let speaker = bar
                        .island_hits
                        .iter()
                        .find(|hit| {
                            hit.at(
                                geometry.left + geometry.width / 2.0,
                                geometry.top + geometry.px(155.0),
                            )
                        })
                        .context("speaker has no hit target")?;
                    assert!(matches!(speaker.action, IslandAction::Mute));
                    let slider = bar
                        .island_hits
                        .iter()
                        .find(|hit| hit.at((left + right) / 2.0, geometry.top + geometry.px(82.0)))
                        .context("slider has no hit target")?;
                    assert!(matches!(slider.action, IslandAction::Volume(_)));
                }
            }
        }
        // Commit the synthetic player to input/cache state after the layout
        // matrix. build() alone does not update the open panel or cached Look.
        bar.apply_look(&gfx, look.clone())?;
        let image = Arc::new(navalia_shell::icons::IconImage {
            width: 8,
            height: 8,
            bgra: [20, 40, 80, 255].repeat(64),
        });
        look.track.as_mut().context("track")?.artwork = Some(image.clone());
        bar.build(&gfx, &look)?;
        assert!(
            bar.cover
                .as_ref()
                .and_then(|(source, _)| source.as_ref())
                .is_some_and(|source| Arc::ptr_eq(source, &image))
        );
        // Compare the old full redraw path with retained updates without ever
        // writing the real audio endpoint. Tree identity is the regression
        // assertion; timings are informational, not a flaky speed threshold.
        let started = Instant::now();
        for step in 0..64 {
            bar.status.volume = Some(Volume {
                level: 2,
                muted: false,
                percent: 40 + step % 20,
            });
            look.status = bar.status;
            bar.build(&gfx, &look)?;
        }
        let full_redraw = started.elapsed();
        bar.look = Some(look.clone());
        let root = bar.surface.Children()?.First()?.Current()?;
        let mut update = look.track.clone().context("track for progress update")?;
        update.timeline = Track::test_fixture().timeline;
        bar.media = Some(media::Watch::test_snapshot(update));
        bar.media_changed(&gfx)?;
        assert_eq!(
            bar.surface.Children()?.First()?.Current()?,
            root,
            "position events retain the visual tree"
        );
        bar.visible = true; // The native window stays hidden throughout the fixture.
        bar.revealed = true;
        bar.sync_progress_watch();
        assert!(bar.progress_timer);
        bar.visible = false;
        bar.sync_progress_watch();
        assert!(
            !bar.progress_timer,
            "hidden popups must have no progress timer"
        );
        let started = Instant::now();
        for step in 0..64 {
            bar.apply_volume_reading(
                &gfx,
                Some(Volume {
                    level: 2,
                    muted: false,
                    percent: 40 + step % 20,
                }),
            )?;
            assert_eq!(bar.surface.Children()?.First()?.Current()?, root);
            assert_eq!(
                bar.volume_controls
                    .as_ref()
                    .context("retained volume")?
                    .label_percent,
                40 + step % 20
            );
        }
        eprintln!(
            "64 volume updates: full redraw {:?}, retained {:?} (no audio writes)",
            full_redraw,
            started.elapsed()
        );
        let span = VolumeTrack {
            center_x: 200.0,
            top: 100.0,
            bottom: 300.0,
        };
        bar.volume_drag = Some(span); // No SetCapture in a test.
        bar.queue_volume(span, 260.0);
        bar.pointer_moved(&gfx, 999.0, 140.0)?;
        let player = bar.surface.Children()?.First()?.Current()?;
        bar.refresh_backdrops(&gfx)?;
        assert_eq!(bar.surface.Children()?.First()?.Current()?, player);
        assert!(bar.volume_drag.is_some());
        assert_eq!(bar.pending_volume, Some(80));
        assert_eq!(
            bar.volume_controls
                .as_ref()
                .context("immediate percentage")?
                .label_percent,
            80
        );
        assert_eq!(
            bar.volume_controls
                .as_ref()
                .context("immediate fill")?
                .position_percent,
            Some(80)
        );
        let controls = bar.volume_controls.as_ref().context("vertical fill")?;
        assert!((controls.fill.Size()?.Y - controls.height * 0.8).abs() < 0.01);
        assert!(
            (controls.fill.Offset()?.Y + controls.fill.Size()?.Y - controls.height).abs() < 0.01,
            "fill stays attached to the bottom"
        );
        let label = controls.value.Size()?;
        assert!(
            (controls.value.Offset()?.X + label.X / 2.0 - controls.geometry.width / 2.0).abs()
                < 0.01
        );
        // An endpoint acknowledgement must not drag the fill backwards while
        // a newer pointer target is still waiting for its coalesced write.
        bar.apply_volume_reading(
            &gfx,
            Some(Volume {
                level: 2,
                muted: false,
                percent: 50,
            }),
        )?;
        assert_eq!(
            bar.volume_controls
                .as_ref()
                .context("optimistic fill")?
                .position_percent,
            Some(80)
        );
        assert_eq!(bar.surface.Children()?.First()?.Current()?, root);
        assert!(bar.release_volume(Some(300.0))?);
        assert!(bar.pending_volume.is_none() && bar.volume_drag.is_none());
        assert!(!bar.release_volume(None)?);
        assert!(bar.release_volume(Some(200.0))?); // Late up after capture loss/timer.
        assert!(!bar.release_volume(Some(200.0))?);
        bar.status.volume = None;
        look.status = bar.status;
        // Wallpaper/DWM changes renew only the host sample. Cached content and
        // the open music popup remain intact when the theme itself is unchanged.
        bar.look = Some(look.clone());
        let previous_material = bar.surface.Children()?.First()?.Current()?;
        bar.refresh_backdrops(&gfx)?;
        assert!(bar.look.as_ref() == Some(&look));
        bar.apply_look(&gfx, look.clone())?;
        assert_eq!(
            bar.surface.Children()?.First()?.Current()?,
            previous_material
        );
        bar.build(&gfx, &look)?;
        assert!(
            !bar.island_hits
                .iter()
                .any(|hit| matches!(hit.action, IslandAction::Volume(_) | IslandAction::Mute))
        );
        look.track.as_mut().context("track")?.timeline = None;
        bar.apply_look(&gfx, look)?;
        assert!(bar.progress_controls.is_none() && !bar.progress_timer);
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows composition; hidden window and no provider requests"]
    fn opencode_only_measured_go_appears_in_both_island_styles() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        // SAFETY: The dispatcher and hidden composition objects belong to this thread.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, false, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        let mut look = bar.look.clone().context("initial look")?;
        look.track = None;
        look.active_providers = ActiveProviders::default();
        look.active_providers.0[Provider::OpenCode as usize] = true;
        // Yesterday's saved quota is not evidence that Codex is running now.
        look.usage.providers[Provider::Codex as usize] = ProviderUsage {
            state: State::Ready,
            windows: vec![usage::Window {
                label: None,
                minutes: 300,
                used_percent: 42,
                resets_at: None,
            }],
            fetched_at: Some(std::time::SystemTime::now() - Duration::from_secs(24 * 60 * 60)),
        };
        let quota: Vec<_> = [300, 10_080, 43_200]
            .into_iter()
            .map(|minutes| usage::Window {
                label: None,
                minutes,
                used_percent: 15,
                resets_at: Some(chrono::Utc::now() + chrono::Duration::hours(2)),
            })
            .collect();
        for style in [IslandStyle::Notch, IslandStyle::Bubble] {
            look.widgets.island_style = style;
            for scale in [1.0, 1.25, 1.5, 2.0] {
                look.scale = scale;
                for (light, glass) in [(true, true), (false, true), (true, false), (false, false)] {
                    look.light = light;
                    look.glass = glass;
                    look.panel = Some(Island::Usage);
                    look.usage_enabled = true;
                    look.usage.providers[Provider::OpenCode as usize] = ProviderUsage::default();
                    bar.apply_look(&gfx, look.clone())?;
                    assert!(bar.island_hits.is_empty() && bar.panel.is_none());
                    assert_eq!(bar.triggers_span.width(), 0.0);
                    look.usage.providers[Provider::OpenCode as usize] = ProviderUsage {
                        state: State::Ready,
                        windows: quota.clone(),
                        fetched_at: Some(std::time::SystemTime::now()),
                    };
                    bar.apply_look(&gfx, look.clone())?;
                    let current = bar.look.as_ref().context("rendered Go")?;
                    let providers = island_providers(current);
                    assert_eq!(providers.len(), 1);
                    assert!(providers.iter().all(|entry| entry.0 == Provider::OpenCode));
                    assert_eq!(
                        current.panel_content_height(),
                        74.0 + ISLAND_ROW_HEIGHT + 26.0
                    );
                    assert!(bar.panel_span.width() > 0.0);
                    assert!(
                        bar.island_hits
                            .iter()
                            .any(|hit| matches!(hit.action, IslandAction::Toggle(Island::Usage)))
                    );
                    assert!(
                        bar.island_hits
                            .iter()
                            .any(|hit| matches!(hit.action, IslandAction::Refresh))
                    );
                }
            }
        }
        // Removing Go credentials/quota closes its open popup and removes the trigger.
        look.usage.providers[Provider::OpenCode as usize] = ProviderUsage {
            state: State::SignIn,
            ..ProviderUsage::default()
        };
        bar.apply_look(&gfx, look)?;
        assert!(bar.panel.is_none() && bar.panel_span.width() == 0.0);
        assert!(bar.island_hits.is_empty() && bar.usage_viewport.is_none());
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows desktop composition; keeps the bar hidden"]
    fn accessory_opt_out_blocks_startup_resume_and_retry() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, true, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        assert!(!bar.accessories_enabled && bar.accessories.is_none());
        bar.restart_accessories(&gfx)?;
        bar.tick(&gfx)?;
        assert!(bar.accessories.is_none());
        bar.set_accessories_enabled(&gfx, true)?;
        assert!(bar.accessories.is_some());
        bar.set_accessories_enabled(&gfx, false)?;
        bar.accessory_retry_at = Instant::now() - Duration::from_secs(1);
        bar.restart_accessories(&gfx)?;
        bar.tick(&gfx)?;
        assert!(!bar.accessories_enabled && bar.accessories.is_none());
        assert!(
            bar.look
                .as_ref()
                .is_some_and(|look| look.accessories == [None, None])
        );
        Ok(())
    }

    #[test]
    fn accessory_status_ignores_hidden_metadata_and_hides_incomplete_readings() {
        use navalia_shell::accessories::{Device, State};
        let mut snapshot = AccessorySnapshot {
            state: State::Ready,
            ..Default::default()
        };
        assert_eq!(accessory_readings(&snapshot), [None, None]);
        snapshot.devices = vec![
            Device {
                id: "mouse".into(),
                name: "Mouse".into(),
                kind: AccessoryKind::Mouse,
                percent: Some(0),
                conflicting: false,
                active_audio: false,
            },
            Device {
                id: "headset".into(),
                name: "Headset".into(),
                kind: AccessoryKind::Headset,
                percent: None,
                conflicting: false,
                active_audio: true,
            },
        ];
        let before = accessory_readings(&snapshot);
        assert!(before[0].is_some_and(|reading| reading.percent == Some(0)));
        assert!(before[1].is_some_and(|reading| reading.percent.is_none()));
        snapshot.devices[0].name = "New name".into();
        assert!(accessory_readings(&snapshot) == before);
        for state in [State::Loading, State::Unavailable] {
            snapshot.state = state;
            assert_eq!(accessory_readings(&snapshot), [None, None]);
        }
    }

    #[test]
    #[ignore = "Requires Windows desktop composition; keeps the test bar hidden"]
    fn accessory_indicators_render_without_controls_or_popups() -> Result<()> {
        use anyhow::Context;
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, true, true, MenuBarWidgets::default())?;
        bar.set_visible(false)?;
        bar.accessories = None;
        let mut look = bar.look.clone().context("missing initial menu bar")?;
        look.track = None;
        for (scale, material) in [1.0, 1.25, 1.5, 2.0].into_iter().zip(Material::ALL) {
            for light in [true, false] {
                for glass in [true, false] {
                    look.scale = scale;
                    look.material = material;
                    look.light = light;
                    look.glass = glass;
                    look.accessories = [None, None];
                    bar.build(&gfx, &look)?;
                    let root: ContainerVisual =
                        bar.surface.Children()?.First()?.Current()?.cast()?;
                    let empty_visuals = root.Children()?.Count()?;
                    let controls = bar.zones.len();
                    look.accessories = [
                        Some(AccessoryReading {
                            kind: AccessoryKind::Mouse,
                            percent: Some(76),
                        }),
                        Some(AccessoryReading {
                            kind: AccessoryKind::Headset,
                            percent: Some(58),
                        }),
                    ];
                    bar.build(&gfx, &look)?;
                    let root: ContainerVisual =
                        bar.surface.Children()?.First()?.Current()?.cast()?;
                    assert_eq!(root.Children()?.Count()?, empty_visuals + 4);
                    assert_eq!(bar.zones.len(), controls);
                    assert!(bar.island_hits.is_empty());
                    assert!(bar.outside_click_hook.is_none());
                    assert_eq!(bar.panel, None);
                }
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows desktop composition"]
    fn changing_ui_language_redraws_unchanged_menu_bar_content() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        struct RestoreLanguage(Language);
        impl Drop for RestoreLanguage {
            fn drop(&mut self) {
                strings::set_language(self.0);
            }
        }
        let _restore = RestoreLanguage(strings::language());
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        strings::set_language(Language::Es);
        let mut bar = MenuBar::create(&gfx, false, true, true, MenuBarWidgets::default())?;
        assert!(
            bar.look
                .as_ref()
                .is_some_and(|look| look.language == Language::Es)
        );
        for language in [
            Language::En,
            Language::Fr,
            Language::Zh,
            Language::Hi,
            Language::Ar,
            Language::Bn,
            Language::Pt,
            Language::Es,
        ] {
            let previous = bar.surface.Children()?.First()?.Current()?;
            strings::set_language(language);
            bar.refresh(&gfx)?;
            assert_ne!(
                previous,
                bar.surface.Children()?.First()?.Current()?,
                "translation-only changes must rebuild the content"
            );
            assert!(
                bar.look
                    .as_ref()
                    .is_some_and(|look| look.language == language)
            );
            // Exercise Windows shaping/font fallback and compact surface bounds,
            // including mixed Arabic + Latin text and Hindi/Bengali conjuncts.
            for text in [
                strings::text().usage_title,
                strings::text().media_title,
                strings::text().usage_claude_sign_in,
            ] {
                let label = gfx.text_brush_fitted(text, 1.0, 13.0, false, Ink::Dark, 160.0)?;
                assert!(label.width > 0.0 && label.width <= 160.0);
                assert!(label.height > 0.0 && label.height < 100.0);
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows desktop composition"]
    fn auto_hidden_bar_can_reveal_and_hide_repeatedly() -> Result<()> {
        use windows::Win32::{
            System::WinRT::{
                CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
                DispatcherQueueOptions,
            },
            UI::WindowsAndMessaging::IsWindowVisible,
        };
        // SAFETY: The dispatcher and bar belong to this test thread.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, true, true, MenuBarWidgets::default())?;
        assert!(
            !unsafe { IsWindowVisible(bar.hwnd) }.as_bool(),
            "auto-hide must apply before the first frame"
        );
        bar.set_auto_hide(true)?;
        for _ in 0..3 {
            assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            assert_eq!(
                bar.surface.Opacity()?,
                0.0,
                "hidden content must stay transparent"
            );
            let point = POINT {
                x: (bar.bounds.left + bar.bounds.right) / 2,
                y: bar.bounds.top,
            };
            bar.edge_rested(&gfx, point)?;
            assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            // A queued leave from before the reveal must not hide it while
            // the pointer is still at the edge, including its first pixel.
            bar.pointer_left_at(&gfx, point)?;
            assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            let surface = bar.surface.clone();
            bar.set_app_name(&gfx, "Navalia regression test")?;
            assert_eq!(
                bar.surface, surface,
                "redraws must preserve the animation parent"
            );
            let outside = POINT {
                y: bar.bounds.bottom,
                ..point
            };
            bar.pointer_left_at(&gfx, outside)?;
            if bar.hide_until.is_some() {
                // Returning during the exit cancels it. Its already queued
                // completion must not close the bar after that reversal.
                bar.pointer_left_at(&gfx, point)?;
                let old_epoch = bar.animation_epoch;
                bar.finish_hide_at(
                    &gfx,
                    Instant::now() + Duration::from_secs(1),
                    outside,
                    Some(old_epoch.wrapping_sub(1)),
                )?;
                assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
                bar.pointer_left_at(&gfx, outside)?;
                // An old completion must also be ignored during a newer exit.
                bar.finish_hide_at(
                    &gfx,
                    Instant::now() + Duration::from_secs(1),
                    outside,
                    Some(old_epoch),
                )?;
                assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
                let epoch = bar.animation_epoch;
                bar.finish_hide_at(&gfx, Instant::now(), outside, None)?;
                assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
                bar.finish_hide_at(&gfx, Instant::now(), outside, Some(epoch))?;
            }
        }
        assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
        bar.set_auto_hide(false)?;
        assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
        bar.set_auto_hide(true)?;
        assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
        bar.edge_rested(
            &gfx,
            POINT {
                x: bar.bounds.right / 2,
                y: bar.bounds.top,
            },
        )?;
        bar.set_visible(false)?;
        assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
        assert!(bar.hide_until.is_none());
        bar.set_visible(true)?;
        assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows desktop composition; runs visible transitions"]
    fn compositor_completion_hides_only_the_current_exit() -> Result<()> {
        use windows::Win32::{
            System::WinRT::{
                CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
                DispatcherQueueOptions,
            },
            UI::WindowsAndMessaging::{
                DispatchMessageW, IsWindowVisible, MSG, PM_REMOVE, PeekMessageW,
            },
        };
        // SAFETY: The dispatcher, queue, window and compositor are test-owned.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let mut bar = MenuBar::create(&gfx, false, true, true, MenuBarWidgets::default())?;
        bar.set_app_name(&gfx, "Navalia animation test")?;
        bar.set_auto_hide(true)?;
        let inside = POINT {
            x: bar.bounds.left + 100,
            y: bar.bounds.top,
        };
        let outside = POINT {
            y: bar.bounds.bottom + 50,
            ..inside
        };
        let pump = |bar: &mut MenuBar, duration: Duration, point: POINT| -> Result<usize> {
            let until = Instant::now() + duration;
            let mut completions = 0;
            let mut msg = MSG::default();
            while Instant::now() < until {
                // Pump the dispatcher so real compositor completion events run.
                // Pointer state is deterministic; no mouse input is injected.
                while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                    if msg.hwnd == bar.hwnd && msg.message == HIDE_ANIMATION_DONE {
                        completions += 1;
                        bar.finish_hide_at(&gfx, Instant::now(), point, Some(msg.wParam.0))?;
                    } else if msg.hwnd == bar.hwnd
                        && msg.message == WM_TIMER
                        && msg.wParam.0 == HIDE_ANIMATION_TIMER
                    {
                        bar.finish_hide_at(&gfx, Instant::now(), point, None)?;
                    } else {
                        unsafe { DispatchMessageW(&msg) };
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(completions)
        };
        pump(&mut bar, Duration::from_millis(300), outside)?;
        for _ in 0..3 {
            bar.hover(&gfx, true)?;
            pump(&mut bar, Duration::from_millis(500), inside)?;
            assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            bar.hover(&gfx, false)?;
            if system::animations_enabled() {
                assert!(unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            }
            let completions = pump(&mut bar, Duration::from_millis(500), outside)?;
            if system::animations_enabled() {
                assert_eq!(completions, 1, "exit must complete through the compositor");
            }
            assert!(!unsafe { IsWindowVisible(bar.hwnd) }.as_bool());
            assert_eq!(bar.surface.Opacity()?, 0.0);
        }
        Ok(())
    }

    #[test]
    fn clicking_usage_replaces_music_and_clicking_again_closes_it() {
        let open = toggle_panel(Some(Island::Music), Island::Usage);
        assert_eq!(open, Some(Island::Usage));
        assert_eq!(toggle_panel(open, Island::Usage), None);
    }

    #[test]
    fn usage_panel_covers_the_last_provider_even_when_the_trigger_is_wide() {
        let screen = 1920.0;
        let triggers = trigger_layout(
            Span {
                left: 210.0,
                right: 1540.0,
            },
            screen / 2.0,
            1.25,
            true,
            5,
        );
        let Some(usage) = triggers[1].1 else {
            panic!("Usage trigger should fit");
        };
        let panel = panel_span(usage, screen, 550.0);
        assert!(panel.left <= usage.left && panel.right >= usage.right);
    }

    #[test]
    fn narrow_bar_still_gives_each_trigger_its_own_clickable_span() {
        let bounds = Span {
            left: 200.0,
            right: 390.0,
        };
        let [(_, Some(music)), (_, Some(usage))] = trigger_layout(bounds, 300.0, 1.5, true, 3)
        else {
            panic!("Both triggers should fit");
        };
        assert!(
            music.left >= bounds.left && music.right < usage.left && usage.right <= bounds.right
        );
    }

    #[test]
    fn bubble_layout_aligns_to_right_status_edge_and_stays_compact() {
        let bounds = Span {
            left: 210.0,
            right: 1420.0,
        };
        let [(_, Some(short_music)), (_, Some(short_usage))] =
            bubble_trigger_layout(bounds, 1.0, Some(92.0), 2)
        else {
            panic!("Both bubble segments should fit");
        };
        let [(_, Some(long_music)), (_, Some(long_usage))] =
            bubble_trigger_layout(bounds, 1.0, Some(240.0), 2)
        else {
            panic!("Both bubble segments should fit");
        };
        assert_eq!(short_usage.right, bounds.right);
        assert_eq!(long_usage.right, bounds.right);
        assert!(short_music.width() < long_music.width());
        assert_eq!(long_music.width(), BUBBLE_MUSIC_WIDTH);
        assert!(short_music.left >= bounds.left && short_music.right < short_usage.left);

        let short_bubble = Span {
            left: short_music.left,
            right: short_usage.right,
        };
        let long_bubble = Span {
            left: long_music.left,
            right: long_usage.right,
        };
        let short_panel = bubble_panel_span(short_bubble, 1920.0, 360.0);
        let long_panel = bubble_panel_span(long_bubble, 1920.0, 360.0);
        assert_eq!(short_panel.left, long_panel.left);
        assert_eq!(short_panel.right, long_panel.right);
    }

    #[test]
    fn losing_a_session_closes_its_panel_without_opening_the_other() {
        assert_eq!(available_panel(Some(Island::Music), false, true), None);
        assert_eq!(available_panel(Some(Island::Usage), true, false), None);
    }

    #[test]
    #[ignore = "Requires Windows desktop composition"]
    fn popup_surface_builds_in_both_themes_with_and_without_transparency() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };

        // SAFETY: The controller initializes the test thread's own queue.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        for (light, glass) in [(true, true), (false, true), (true, false), (false, false)] {
            let root = gfx.compositor.CreateContainerVisual()?;
            let theme = Theme::for_popup(light, glass);
            let geometry = IslandGeometry {
                left: 90.0,
                top: HEIGHT + PANEL_GAP,
                width: 360.0,
                scale: 1.0,
                light,
            };
            popup_surface(
                &gfx,
                theme,
                &root.Children()?,
                geometry,
                136.0,
                geometry.px(PANEL_RADIUS),
            )?;
        }
        Ok(())
    }
}

/// Where the pointer is, for a tray icon to open its menu at.
mod mascot_tests {
    use super::*;
    use crate::usage::{ProviderUsage, Snapshot, State, Window};

    fn at(percent: Option<u8>) -> Snapshot {
        let mut snapshot = Snapshot::default();
        if let Some(percent) = percent {
            snapshot.providers[Provider::Claude as usize] = ProviderUsage {
                state: State::Ready,
                windows: vec![Window {
                    label: None,
                    minutes: 300,
                    used_percent: percent,
                    resets_at: None,
                }],
                fetched_at: None,
            };
        }
        snapshot
    }

    fn moment(before: Option<u8>, now: Option<u8>) -> Option<mascot::Act> {
        let mut active = ActiveProviders::default();
        active.0[Provider::Claude as usize] = true;
        usage_moment(&at(before), &at(now), &active).map(|(_, act)| act)
    }

    #[test]
    fn usage_moment_should_fire_only_when_a_line_is_crossed() {
        assert_eq!(moment(Some(60), Some(0)), Some(mascot::Act::Fresh));
        assert_eq!(moment(Some(85), Some(91)), Some(mascot::Act::Nervous));
        assert_eq!(moment(Some(95), Some(100)), Some(mascot::Act::Exhausted));
        // Straight to 100 is "exhausted", not "nervous".
        assert_eq!(moment(Some(50), Some(100)), Some(mascot::Act::Exhausted));
        // Staying above a line, a small reset, or the first figure: nothing.
        assert_eq!(moment(Some(92), Some(95)), None);
        assert_eq!(moment(Some(10), Some(0)), None);
        assert_eq!(moment(None, Some(0)), None);
        assert_eq!(moment(None, Some(100)), None);
    }

    #[test]
    fn usage_moment_ignores_failed_reads_and_different_quota_windows() {
        let before = at(Some(95));
        let mut after = at(Some(0));
        let mut active = ActiveProviders::default();
        active.0[Provider::Claude as usize] = true;
        after.providers[Provider::Claude as usize].windows[0].minutes = 10080;
        assert!(usage_moment(&before, &after, &active).is_none());
        after.providers[Provider::Claude as usize].windows[0].minutes = 300;
        after.providers[Provider::Claude as usize].state = State::Unavailable;
        assert!(usage_moment(&before, &after, &active).is_none());
    }

    #[test]
    fn usage_moment_ignores_out_of_order_measurements() {
        let mut before = at(Some(95));
        let mut after = at(Some(0));
        let mut active = ActiveProviders::default();
        active.0[Provider::Claude as usize] = true;
        let now = std::time::SystemTime::now();
        before.providers[Provider::Claude as usize].fetched_at = Some(now);
        after.providers[Provider::Claude as usize].fetched_at =
            now.checked_sub(std::time::Duration::from_secs(60));
        assert!(usage_moment(&before, &after, &active).is_none());
    }

    #[test]
    fn tray_item_spans_place_icons_left_to_right_within_left_wing() {
        let spans = tray_item_spans(14.0, 488.0, 16.0, 10.0, 3);
        assert_eq!(spans.len(), 3);
        assert!(spans[0].0 < spans[1].0 && spans[1].0 < spans[2].0);
        assert!(spans.iter().all(|&(_, right)| right <= 488.0));

        // Stops before dock cutout when crowded.
        let crowded = tray_item_spans(14.0, 40.0, 16.0, 10.0, 5);
        assert_eq!(crowded, vec![(14.0, 30.0)]);
    }
}
