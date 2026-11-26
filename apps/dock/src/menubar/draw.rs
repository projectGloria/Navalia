use super::*;

pub(crate) fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

pub(crate) fn add_visual(
    compositor: &Compositor,
    layers: &VisualCollection,
    brush: &CompositionBrush,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Result<SpriteVisual> {
    let visual = compositor.CreateSpriteVisual()?;
    visual.SetSize(Vector2 {
        X: width,
        Y: height,
    })?;
    visual.SetOffset(Vector3 {
        X: x.round(),
        Y: y.round(),
        Z: 0.0,
    })?;
    visual.SetBrush(brush)?;
    layers.InsertAtTop(&visual)?;
    Ok(visual)
}

#[expect(
    clippy::too_many_arguments,
    reason = "A rounded shape needs its rectangle, radius and two colors"
)]
pub(crate) fn add_rounded_box(
    compositor: &Compositor,
    layers: &VisualCollection,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    fill: Color,
    border: Option<Color>,
) -> Result<()> {
    retained_rounded_box(
        compositor, layers, x, y, width, height, radius, fill, border,
    )
    .map(|_| ())
}

#[expect(
    clippy::too_many_arguments,
    reason = "Same rectangle/color inputs as add_rounded_box; retain geometry for small UI updates"
)]
pub(super) fn retained_rounded_box(
    compositor: &Compositor,
    layers: &VisualCollection,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    fill: Color,
    border: Option<Color>,
) -> Result<(ShapeVisual, CompositionRoundedRectangleGeometry)> {
    let geometry = compositor.CreateRoundedRectangleGeometry()?;
    let inset = if border.is_some() { 0.5 } else { 0.0 };
    geometry.SetOffset(Vector2 { X: inset, Y: inset })?;
    geometry.SetSize(Vector2 {
        X: (width - inset * 2.0).max(0.0),
        Y: (height - inset * 2.0).max(0.0),
    })?;
    geometry.SetCornerRadius(Vector2 {
        X: radius.min(width / 2.0),
        Y: radius.min(height / 2.0),
    })?;
    let shape = compositor.CreateSpriteShapeWithGeometry(&geometry)?;
    shape.SetFillBrush(&compositor.CreateColorBrushWithColor(fill)?)?;
    if let Some(border) = border {
        shape.SetStrokeBrush(&compositor.CreateColorBrushWithColor(border)?)?;
        shape.SetStrokeThickness(1.0)?;
    }
    let visual = compositor.CreateShapeVisual()?;
    visual.SetSize(Vector2 {
        X: width,
        Y: height,
    })?;
    visual.SetOffset(Vector3 {
        X: x.round(),
        Y: y.round(),
        Z: 0.0,
    })?;
    visual.Shapes()?.Append(&shape)?;
    layers.InsertAtTop(&visual)?;
    Ok((visual, geometry))
}

#[expect(
    clippy::too_many_arguments,
    reason = "Typography and placement are chosen together for panel labels"
)]
pub(super) fn add_panel_text(
    gfx: &Gfx,
    layers: &VisualCollection,
    value: &str,
    x: f32,
    y: f32,
    scale: f32,
    font_size: f32,
    bold: bool,
    ink: Ink,
    opacity: f32,
) -> Result<LabelBrush> {
    let label = gfx.text_brush_sized(value, scale, font_size, bold, ink)?;
    add_visual(
        &gfx.compositor,
        layers,
        &label.brush.cast()?,
        x,
        y,
        label.width,
        label.height,
    )?
    .SetOpacity(opacity)?;
    Ok(label)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Right-aligned panel labels choose typography and placement together"
)]
pub(super) fn add_panel_text_right(
    gfx: &Gfx,
    layers: &VisualCollection,
    value: &str,
    right: f32,
    y: f32,
    scale: f32,
    font_size: f32,
    bold: bool,
    ink: Ink,
    opacity: f32,
) -> Result<LabelBrush> {
    let label = gfx.text_brush_sized(value, scale, font_size, bold, ink)?;
    add_visual(
        &gfx.compositor,
        layers,
        &label.brush.cast()?,
        right - label.width,
        y,
        label.width,
        label.height,
    )?
    .SetOpacity(opacity)?;
    Ok(label)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Each provider row draws its own colors, data and measured rectangle"
)]
pub(super) fn draw_usage_row(
    gfx: &Gfx,
    layers: &VisualCollection,
    provider: Provider,
    usage: &ProviderUsage,
    accent: Color,
    scale: f32,
    light: bool,
    usage_interval_minutes: u32,
    island_width: f32,
    y: f32,
) -> Result<Option<(f32, f32, f32, f32)>> {
    let c = &gfx.compositor;
    let px = |logical: f32| (logical * scale).round();
    let ink = if light { Ink::Dark } else { Ink::Light };
    let text = strings::text();
    let left = px(23.0);
    let width = island_width - px(46.0);
    let right = left + width;
    add_rounded_box(
        c,
        layers,
        left + px(12.0),
        y + px(11.0),
        px(32.0),
        px(32.0),
        px(10.0),
        rgba(accent.R, accent.G, accent.B, if light { 31 } else { 41 }),
        None,
    )?;
    if let Some(image) = usage::logo(provider) {
        let icon_size = px(20.0) as u32;
        let image_brush = gfx.icon_brush(image, icon_size)?;
        let brush: CompositionBrush = if !matches!(
            provider,
            Provider::Claude | Provider::Antigravity | Provider::Gemini
        ) && light
        {
            let tinted = c.CreateMaskBrush()?;
            tinted.SetMask(&image_brush)?;
            tinted.SetSource(&c.CreateColorBrushWithColor(rgba(27, 34, 45, 255))?)?;
            tinted.cast()?
        } else {
            image_brush.cast()?
        };
        add_visual(
            c,
            layers,
            &brush,
            left + px(18.0),
            y + px(17.0),
            icon_size as f32,
            icon_size as f32,
        )?;
    }
    add_panel_text(
        gfx,
        layers,
        provider.name(),
        left + px(53.0),
        y + px(10.0),
        scale,
        14.0,
        true,
        ink,
        1.0,
    )?;
    if let Some(main) = usage.five_hour_window() {
        let left_pct = main.left_percent();
        let now = chrono::Utc::now();
        let exhausted_countdown = (left_pct == 0).then(|| main.reset_countdown(now)).flatten();
        let big_value = exhausted_countdown
            .clone()
            .unwrap_or_else(|| format!("{left_pct}%"));
        let sub_label = if exhausted_countdown.is_some() {
            text.usage_resets
        } else {
            text.usage_left
        };
        add_panel_text(
            gfx,
            layers,
            &main.title(),
            left + px(53.0),
            y + px(30.0),
            scale,
            11.5,
            false,
            ink,
            0.72,
        )?;
        add_panel_text_right(
            gfx,
            layers,
            &big_value,
            right - px(14.0),
            y + px(6.0),
            scale,
            20.0,
            true,
            ink,
            1.0,
        )?;
        add_panel_text_right(
            gfx,
            layers,
            sub_label,
            right - px(15.0),
            y + px(31.0),
            scale,
            11.0,
            false,
            ink,
            0.7,
        )?;
        let track_left = left + px(14.0);
        let track_width = width - px(28.0);
        let track_y = y + px(53.0);
        add_rounded_box(
            c,
            layers,
            track_left,
            track_y,
            track_width,
            px(6.0),
            px(3.0),
            if light {
                rgba(54, 73, 95, 43)
            } else {
                rgba(224, 235, 248, 48)
            },
            None,
        )?;
        let fill = track_width * f32::from(left_pct) / 100.0;
        if fill > 0.0 {
            add_rounded_box(
                c,
                layers,
                track_left,
                track_y,
                fill,
                px(6.0),
                px(3.0),
                accent,
                None,
            )?;
        }
        if let Some(reset) = main.reset_text() {
            add_panel_text_right(
                gfx,
                layers,
                &format!("{} {reset}", text.usage_resets),
                right - px(14.0),
                y + px(64.0),
                scale,
                11.0,
                false,
                ink,
                0.78,
            )?;
        }
        if usage.state == State::Unavailable
            || usage
                .age_minutes()
                .is_some_and(|minutes| minutes > u64::from(usage_interval_minutes.max(5)))
        {
            add_panel_text(
                gfx,
                layers,
                text.usage_stale,
                left + px(14.0),
                y + px(64.0),
                scale,
                11.0,
                false,
                ink,
                0.7,
            )?;
        }
        for (index, secondary) in usage
            .windows
            .iter()
            .filter(|w| !std::ptr::eq(*w, main))
            .enumerate()
        {
            add_visual(
                c,
                layers,
                &c.CreateColorBrushWithColor(if light {
                    rgba(50, 68, 88, 24)
                } else {
                    rgba(228, 236, 247, 29)
                })?
                .cast()?,
                left + px(14.0),
                y + px(83.0 + index as f32 * 26.0),
                width - px(28.0),
                px(1.0),
            )?;
            add_panel_text(
                gfx,
                layers,
                &format!(
                    "{} · {}% {}",
                    secondary.title(),
                    secondary.left_percent(),
                    text.usage_left
                ),
                left + px(14.0),
                y + px(87.0 + index as f32 * 26.0),
                scale,
                11.5,
                true,
                ink,
                0.94,
            )?;
            if let Some(reset) = secondary.reset_text() {
                add_panel_text_right(
                    gfx,
                    layers,
                    &format!("{} {reset}", text.usage_resets),
                    right - px(14.0),
                    y + px(87.0 + index as f32 * 26.0),
                    scale,
                    11.5,
                    false,
                    ink,
                    0.76,
                )?;
            }
        }
    } else {
        let message = match usage.state {
            State::Loading => text.usage_loading,
            State::SignIn if provider == Provider::Claude => text.usage_claude_sign_in,
            State::SignIn => text.usage_sign_in,
            State::Ready | State::Unavailable => text.usage_unavailable,
        };
        if matches!(usage.state, State::SignIn | State::Unavailable) {
            let button_label =
                gfx.text_brush_sized(text.usage_sign_in_button, scale, 11.5, true, ink)?;
            let button_width = button_label.width + px(20.0);
            let button_height = px(27.0);
            let button_left = right - px(14.0) - button_width;
            let button_top = y + px(34.0);
            let fill = if light {
                rgba(0, 0, 0, 18)
            } else {
                rgba(255, 255, 255, 27)
            };
            let border = if light {
                rgba(35, 57, 83, 31)
            } else {
                rgba(223, 237, 255, 37)
            };
            add_rounded_box(
                c,
                layers,
                button_left,
                button_top,
                button_width,
                button_height,
                px(13.5),
                fill,
                Some(border),
            )?;
            add_visual(
                c,
                layers,
                &button_label.brush.cast()?,
                button_left + px(10.0),
                button_top + (button_height - button_label.height) / 2.0,
                button_label.width,
                button_label.height,
            )?;
            let max_msg_width = (button_left - (left + px(53.0)) - px(10.0)).max(px(40.0));
            let label = gfx.text_brush_fitted(message, scale, 12.0, false, ink, max_msg_width)?;
            add_visual(
                c,
                layers,
                &label.brush.cast()?,
                left + px(53.0),
                y + px(40.0),
                label.width,
                label.height,
            )?
            .SetOpacity(0.72)?;
            return Ok(Some((button_left, button_top, button_width, button_height)));
        }
        add_panel_text(
            gfx,
            layers,
            message,
            left + px(53.0),
            y + px(43.0),
            scale,
            12.0,
            false,
            ink,
            0.72,
        )?;
    }
    Ok(None)
}

pub(super) fn usage_row_height(usage: &ProviderUsage) -> f32 {
    ISLAND_ROW_HEIGHT + usage.windows.len().saturating_sub(2) as f32 * 26.0
}

impl MenuBar {
    pub(super) fn build(&mut self, gfx: &Gfx, look: &Look) -> Result<()> {
        let c = &gfx.compositor;
        let scale = look.scale;
        let px = |logical: f32| (logical * scale).round();
        let screen = look.screen;
        let has_backdrop = look.taskbar_backdrop.style != navalia_config::TaskbarStyle::Clear;
        let bar_h = if has_backdrop && look.taskbar_backdrop.height > 0 {
            (look.taskbar_backdrop.height as f32).max(px(HEIGHT))
        } else {
            px(HEIGHT)
        };
        let (width, height) = ((screen.right - screen.left) as f32, bar_h);
        let panel_h = px(look.panel_height());
        let visual_height = if look.panel.is_some() {
            px(PANEL_SHADOW_MARGIN) + panel_h + px(PANEL_GAP) + height
        } else {
            height
        };
        let bar_y = if look.panel.is_some() {
            visual_height - height
        } else {
            0.0
        };
        let cutout = look.dock_cutout.and_then(|(l, r)| {
            let left = ((l - screen.left) as f32).clamp(0.0, width);
            let right = ((r - screen.left) as f32).clamp(left, width);
            (right > left).then_some((left, right))
        });
        let effective_light = if has_backdrop {
            look.taskbar_backdrop
                .color
                .map_or(look.light, crate::scene::is_light_rgb)
        } else {
            look.light
        };
        let ink = if effective_light {
            Ink::Dark
        } else {
            Ink::Light
        };

        let root = c.CreateContainerVisual()?;
        root.SetSize(Vector2 {
            X: width,
            Y: visual_height,
        })?;
        let layers = root.Children()?;

        if has_backdrop
            && let Some(backdrop) = crate::scene::taskbar_bar_visual(
                gfx,
                width,
                height,
                bar_y,
                look.taskbar_backdrop.style,
                look.taskbar_backdrop.color,
                look.light,
                look.glass,
            )?
        {
            layers.InsertAtTop(&backdrop)?;
        }

        let place = |layers: &windows::UI::Composition::VisualCollection,
                     item: &LabelBrush,
                     left: f32|
         -> Result<SpriteVisual> {
            let visual = c.CreateSpriteVisual()?;
            visual.SetSize(Vector2 {
                X: item.width,
                Y: item.height,
            })?;
            visual.SetOffset(Vector3 {
                X: left.round(),
                Y: (bar_y + (height - item.height) / 2.0).round(),
                Z: 0.0,
            })?;
            visual.SetBrush(&item.brush)?;
            layers.InsertAtTop(&visual)?;
            Ok(visual)
        };

        self.zones.clear();
        let left_wing_start = px(SIDE_MARGIN);
        let left_wing_max = cutout.map_or(
            (width * 0.5 - px(12.0)).max(left_wing_start),
            |(c_left, _)| (c_left - px(12.0)).max(left_wing_start),
        );
        let right_wing_min = cutout.map_or(px(SIDE_MARGIN), |(_, c_right)| {
            (c_right + px(12.0)).min(width - px(SIDE_MARGIN))
        });

        // Right wing, from the right edge inward, stopping before the centered dock.
        let slop = px(HIT_SLOP);
        let mut right = width - px(SIDE_MARGIN);
        let status = look.status;
        let symbol = |layers: &[(char, f32)]| gfx.glyph_brush(layers, scale, ink);
        let mut items: Vec<(LabelBrush, Action)> = Vec::new();
        // The notification bell, when chosen, takes the very end of the bar.
        if look.widgets.notifications {
            items.push((
                symbol(&[(glyph::NOTIFICATIONS, 1.0)])?,
                Action::Notifications,
            ));
        }
        items.push((
            gfx.text_brush(&look.clock, scale, false, ink)?,
            Action::Clock,
        ));
        items.push((gfx.control_center_brush(scale, ink)?, Action::QuickSettings));
        // Privacy indicators, as the Windows tray shows them: something is
        // using the microphone or camera right now.
        let privacy = status.privacy;
        for (on, glyph, action) in [
            (privacy.microphone, glyph::MICROPHONE, Action::Microphone),
            (privacy.camera, glyph::CAMERA, Action::Camera),
        ] {
            if on {
                items.push((symbol(&[(glyph, 1.0)])?, action));
            }
        }
        if status.buttons.search {
            items.push((symbol(&[(glyph::SEARCH, 1.0)])?, Action::Search));
        }
        if status.bluetooth_on {
            items.push((symbol(&[(glyph::BLUETOOTH, 1.0)])?, Action::Bluetooth));
        }
        // The control center already opens all of quick settings: the network
        // icon goes straight to the list of networks (issue #55).
        match status.network {
            Network::Wifi { bars, has_internet } => {
                items.push((
                    gfx.wifi_brush(bars, has_internet, scale, ink)?,
                    Action::Network,
                ));
            }
            Network::Ethernet => {
                items.push((symbol(&[(glyph::ETHERNET, 1.0)])?, Action::Network));
            }
            Network::Offline => {
                items.push((symbol(&[(glyph::OFFLINE, 1.0)])?, Action::Network));
            }
        }
        if let Some(volume) = status.volume {
            let speaker = if volume.muted {
                glyph::MUTE
            } else {
                glyph::VOLUME[usize::from(volume.level.min(3))]
            };
            items.push((symbol(&[(speaker, 1.0)])?, Action::Sound));
        }
        let mut battery_label = None;
        if let Some(battery) = look.battery {
            if status.battery_percentage {
                battery_label = Some((
                    items.len(),
                    gfx.text_brush(&format!("{}%", battery.percent), scale, false, ink)?,
                ));
            }
            items.push((
                gfx.battery_brush(battery.percent, battery.charging, battery.saver, scale, ink)?,
                Action::Power,
            ));
        }
        let mut accessory_labels = Vec::new();
        for device in look.accessories.into_iter().flatten().rev() {
            let value = device
                .percent
                .map_or_else(|| "—".to_owned(), |percent| format!("{percent}%"));
            accessory_labels.push((items.len(), gfx.text_brush(&value, scale, false, ink)?));
            let icon = match device.kind {
                AccessoryKind::Mouse => glyph::MOUSE,
                AccessoryKind::Headset => glyph::HEADSET,
            };
            items.push((symbol(&[(icon, 1.0)])?, Action::AccessoryIndicator));
        }
        if let Some(language) = &look.input {
            items.push((
                gfx.text_brush(language, scale, false, ink)?,
                Action::InputLanguage,
            ));
        }
        if status.buttons.task_view {
            items.push((symbol(&[(glyph::TASK_VIEW, 1.0)])?, Action::TaskView));
        }
        let hidden = |action: Action| {
            action
                .preference_key()
                .is_some_and(|key| look.hidden.iter().any(|item| item == key))
        };
        for (index, (item, action)) in items.iter().enumerate() {
            if hidden(*action) {
                continue;
            }
            let label = battery_label
                .iter()
                .chain(accessory_labels.iter())
                .find(|(item_index, _)| *item_index == index)
                .map(|(_, label)| label);
            let total_width = item.width + label.map_or(0.0, |l| px(6.0) + l.width);
            if right - total_width < right_wing_min {
                break;
            }
            let zone_right = right;
            if let Some(label) = label {
                right -= label.width;
                place(&layers, label, right)?;
                right -= px(6.0);
            }
            let left = right - item.width;
            place(&layers, item, left)?;
            // The battery icon and its percentage form one control.
            // Accessory readings do nothing on a click; their zone is there
            // for the right-click menu that hides them.
            self.zones.push((left - slop, zone_right + slop, *action));
            right = left - px(ITEM_GAP);
        }

        // Live third-party tray icons (such as Antigravity, Discord, Steam)
        // placed on the left side of the bar, left-to-right, preceded by the
        // hidden-icons arrow at the leftmost position.
        let tray_side = px(16.0).max(12.0);
        let tray_gap = px(10.0);
        let tray_slop = px(4.0);
        let arrow = symbol(&[(glyph::HIDDEN_ICONS, 1.0)])?;
        let tray_start =
            if !hidden(Action::HiddenIcons) && left_wing_start + arrow.width <= left_wing_max {
                place(&layers, &arrow, left_wing_start)?;
                self.zones.push((
                    left_wing_start - tray_slop,
                    left_wing_start + arrow.width + tray_slop,
                    Action::HiddenIcons,
                ));
                left_wing_start + arrow.width + tray_gap
            } else {
                left_wing_start
            };
        let tray_spans = tray_item_spans(
            tray_start,
            left_wing_max,
            tray_side,
            tray_gap,
            look.tray_items.len().min(20),
        );
        for (idx, &(icon_left, icon_right)) in tray_spans.iter().enumerate() {
            let tray_item = &look.tray_items[idx];
            let id = tray_item.id();
            let cached = match self.tray_icons.get(&id) {
                Some((serial, brush)) if *serial == tray_item.serial => brush.clone(),
                _ => {
                    let brush = tray_item
                        .image
                        .as_deref()
                        .and_then(|img| gfx.image_surface(img).ok())
                        .and_then(|surface| c.CreateSurfaceBrushWithSurface(&surface).ok())
                        .inspect(|brush| {
                            let _ = brush.SetStretch(CompositionStretch::Uniform);
                        });
                    self.tray_icons
                        .insert(id, (tray_item.serial, brush.clone()));
                    brush
                }
            };
            if let Some(brush) = cached {
                let visual = c.CreateSpriteVisual()?;
                visual.SetSize(Vector2 {
                    X: tray_side,
                    Y: tray_side,
                })?;
                visual.SetOffset(Vector3 {
                    X: icon_left.round(),
                    Y: (bar_y + (height - tray_side) / 2.0).round(),
                    Z: 0.0,
                })?;
                visual.SetBrush(&brush)?;
                layers.InsertAtTop(&visual)?;
            } else {
                let fallback = symbol(&[('\u{E73A}', 1.0)])?;
                place(
                    &layers,
                    &fallback,
                    icon_left + (tray_side - fallback.width) / 2.0,
                )?;
            }
            self.zones.push((
                icon_left - tray_slop,
                icon_right + tray_slop,
                Action::TrayItem(idx),
            ));
        }
        let left_wing_end = tray_spans
            .last()
            .map_or(tray_start, |(_, icon_right)| *icon_right + tray_gap);

        // The system monitor (issue #64), left of the status icons: two
        // small stacked columns, CPU over memory, then upload over download.
        if let Some(lines) = &look.monitor {
            let brushes = lines
                .iter()
                .map(|line| gfx.text_brush_sized(line, scale, MONITOR_FONT, false, ink))
                .collect::<Result<Vec<_>>>()?;
            if self.monitor_widths.0 != scale {
                self.monitor_widths = (scale, [0.0; 2]);
            }
            for (column, pair) in brushes.chunks(2).enumerate() {
                let widest = pair.iter().map(|brush| brush.width).fold(0.0, f32::max);
                self.monitor_widths.1[column] = self.monitor_widths.1[column].max(widest);
            }
            let widths = self.monitor_widths.1;
            let needed = widths[0] + widths[1] + px(MONITOR_COLUMN_GAP);
            if right - needed >= right_wing_min {
                let zone_right = right;
                let mut left = right;
                for column in [1, 0] {
                    left -= widths[column];
                    for (row, brush) in brushes[column * 2..column * 2 + 2].iter().enumerate() {
                        let top = if row == 0 {
                            bar_y + height / 2.0 - brush.height + px(MONITOR_TUCK)
                        } else {
                            bar_y + height / 2.0 - px(MONITOR_TUCK)
                        };
                        let visual = c.CreateSpriteVisual()?;
                        visual.SetSize(Vector2 {
                            X: brush.width,
                            Y: brush.height,
                        })?;
                        visual.SetOffset(Vector3 {
                            X: left.round(),
                            Y: top.round(),
                            Z: 0.0,
                        })?;
                        visual.SetBrush(&brush.brush)?;
                        layers.InsertAtTop(&visual)?;
                    }
                    if column == 1 {
                        left -= px(MONITOR_COLUMN_GAP);
                    }
                }
                self.zones
                    .push((left - slop, zone_right + slop, Action::SystemMonitor));
                right = left - px(ITEM_GAP);
            }
        }

        self.island_hits.clear();
        self.volume_controls = None;
        self.progress_controls = None;
        self.usage_viewport = None;
        self.triggers_span = Span::default();
        self.music_trigger_span = Span::default();
        self.usage_trigger_span = Span::default();
        self.panel_span = Span::default();
        self.draw_island(
            gfx,
            look,
            effective_light,
            &layers,
            width,
            height,
            bar_y,
            left_wing_end,
            right_wing_min,
            right,
            cutout,
        )?;
        if let Some(drag) = self.volume_drag {
            let new_span = self.island_hits.iter().find_map(|hit| match hit.action {
                IslandAction::Volume(span) => Some(span),
                _ => None,
            });
            if new_span != Some(drag) {
                self.release_volume(None)?;
            }
        }
        self.surface.SetSize(Vector2 {
            X: width,
            Y: visual_height,
        })?;
        let contents = self.surface.Children()?;
        contents.RemoveAll()?;
        contents.InsertAtTop(&root)?;
        let has_panel = self.panel_span.width() > 0.0;
        self.visual_height = if has_panel { visual_height } else { height } as i32;
        self.bounds = RECT {
            left: screen.left,
            top: screen.bottom - height as i32,
            right: screen.right,
            bottom: screen.bottom,
        };
        let active_triggers = match look.panel {
            Some(Island::Music) => self.music_trigger_span,
            Some(Island::Usage) => self.usage_trigger_span,
            None => self.triggers_span,
        };
        TRIGGERS_LEFT.store(screen.left + active_triggers.left as i32, Ordering::Relaxed);
        TRIGGERS_RIGHT.store(
            screen.left + active_triggers.right as i32,
            Ordering::Relaxed,
        );
        PANEL_LEFT.store(screen.left + self.panel_span.left as i32, Ordering::Relaxed);
        PANEL_RIGHT.store(
            screen.left + self.panel_span.right as i32,
            Ordering::Relaxed,
        );
        BAR_TOP.store(self.bounds.top, Ordering::Relaxed);
        BAR_BOTTOM.store(self.bounds.bottom, Ordering::Relaxed);
        let panel_top = if has_panel {
            self.bounds.top - (px(PANEL_GAP) + panel_h) as i32
        } else {
            self.bounds.top
        };
        PANEL_TOP.store(panel_top, Ordering::Relaxed);
        PANEL_BOTTOM.store(self.bounds.top, Ordering::Relaxed);
        if let Some((cl, cr)) = cutout {
            DOCK_LEFT.store(screen.left + cl as i32, Ordering::Relaxed);
            DOCK_RIGHT.store(screen.left + cr as i32, Ordering::Relaxed);
        } else {
            DOCK_LEFT.store(0, Ordering::Relaxed);
            DOCK_RIGHT.store(0, Ordering::Relaxed);
        }
        self.place()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The island is laid out between the measured bar items and dock cutout"
    )]
    pub(super) fn draw_island(
        &mut self,
        gfx: &Gfx,
        look: &Look,
        effective_light: bool,
        layers: &VisualCollection,
        width: f32,
        bar_height: f32,
        bar_y: f32,
        left_wing_end: f32,
        left_end: f32,
        right_start: f32,
        cutout: Option<(f32, f32)>,
    ) -> Result<()> {
        let px = |logical: f32| (logical * look.scale).round();
        let style = look.widgets.island_style;
        let cover = if let Some(track) = &look.track {
            let cached =
                self.cover
                    .as_ref()
                    .is_some_and(|(source, _)| match (source, &track.artwork) {
                        (Some(old), Some(new)) => Arc::ptr_eq(old, new),
                        (None, None) => true,
                        _ => false,
                    });
            if !cached {
                self.cover = media::artwork(track)
                    .map(|image| {
                        let surface = gfx.image_surface(image)?;
                        Ok::<_, anyhow::Error>((
                            track.artwork.clone(),
                            gfx.compositor.CreateSurfaceBrushWithSurface(&surface)?,
                        ))
                    })
                    .transpose()?;
            }
            self.cover.as_ref().map(|(_, brush)| brush.clone())
        } else {
            self.cover = None;
            None
        };
        let prepared_music = if style == IslandStyle::Bubble
            && let Some(track) = &look.track
        {
            Some(prepare_bubble_music(gfx, look, track)?)
        } else {
            None
        };
        let providers = island_providers(look);
        let prepared_usage = if !providers.is_empty() {
            Some(prepare_usage_strip(gfx, look, effective_light, &providers)?)
        } else {
            None
        };

        let music = bubble_trigger_layout(
            Span {
                left: left_end,
                right: right_start,
            },
            look.scale,
            prepared_music.as_ref().map(|music| music.width),
            0,
        )[0]
        .1;

        let usage = prepared_usage.as_ref().and_then(|strip| {
            let right_max = music.map_or(right_start, |m| m.left - px(12.0));
            let left_bounds = Span {
                left: (left_wing_end + px(4.0)).min(cutout.map_or(left_end, |(c_left, _)| c_left)),
                right: cutout.map_or(left_end, |(c_left, _)| {
                    (c_left - px(12.0)).max(left_wing_end)
                }),
            };
            if look.usage_position == AiUsagePosition::Left {
                let w = strip.fitting_width(left_bounds.width());
                (w > 0.0).then(|| {
                    let left = (left_bounds.left + (left_bounds.width() - w) / 2.0).round();
                    Span {
                        left,
                        right: left + w,
                    }
                })
            } else {
                let right_avail = (right_max - left_end).max(0.0);
                let w = strip.fitting_width(right_avail);
                if w > 0.0 {
                    let left = (left_end + (right_avail - w) / 2.0).round();
                    Some(Span {
                        left,
                        right: left + w,
                    })
                } else {
                    let lw = strip.fitting_width(left_bounds.width());
                    (lw > 0.0).then(|| {
                        let left = (left_bounds.left + (left_bounds.width() - lw) / 2.0).round();
                        Span {
                            left,
                            right: left + lw,
                        }
                    })
                }
            }
        });

        // The shelf takes what is left right of the dock: up to the music
        // island, and on the wider side of the AI tools when they sit here too.
        let edge = px(SHELF_EDGE_GAP);
        let shelf_end = music.map_or(right_start, |span| span.left) - edge;
        let shelf_region = match usage.filter(|span| span.left >= left_end) {
            Some(strip) => {
                let before = Span {
                    left: left_end,
                    right: strip.left - edge,
                };
                let after = Span {
                    left: strip.right + edge,
                    right: shelf_end,
                };
                if after.width() > before.width() {
                    after
                } else {
                    before
                }
            }
            None => Span {
                left: left_end,
                right: shelf_end,
            },
        };
        self.draw_shelf(
            gfx,
            look,
            effective_light,
            layers,
            shelf_region,
            bar_y,
            bar_height,
        )?;

        self.music_trigger_span = music.unwrap_or_default();
        self.usage_trigger_span = usage.unwrap_or_default();
        if music.is_none() && usage.is_none() {
            return Ok(());
        }
        self.triggers_span = Span {
            left: [music, usage]
                .into_iter()
                .flatten()
                .map(|s| s.left)
                .fold(f32::INFINITY, f32::min),
            right: [music, usage]
                .into_iter()
                .flatten()
                .map(|s| s.right)
                .fold(f32::NEG_INFINITY, f32::max),
        };

        let c = &gfx.compositor;
        if let Some(span) = music
            && let Some(track) = &look.track
        {
            let pill_geom = IslandGeometry::new(span, bar_y, look);
            let (compact, compact_height) =
                bubble_surface(gfx, look, layers, pill_geom, bar_height)?;
            let geometry = IslandGeometry::new(span, 0.0, look);
            match style {
                IslandStyle::Notch => draw_compact_player(
                    gfx,
                    &compact,
                    geometry,
                    geometry.width,
                    compact_height,
                    track,
                    cover.as_ref(),
                )?,
                IslandStyle::Bubble => {
                    if let Some(prepared) = &prepared_music {
                        draw_bubble_player(
                            gfx,
                            &compact,
                            geometry,
                            compact_height,
                            track,
                            prepared,
                            cover.as_ref(),
                        )?;
                    }
                }
            }
            self.island_hits.push(IslandHit {
                bounds: (span.left, bar_y, span.right, bar_y + bar_height),
                action: IslandAction::Toggle(Island::Music),
            });
        }

        if let Some(span) = usage
            && let Some(strip) = &prepared_usage
        {
            let anchors = draw_usage_strip(
                gfx,
                c,
                layers,
                strip,
                span,
                bar_y,
                bar_height,
                look.scale,
                effective_light,
            )?;
            self.usage_anchors = anchors
                .into_iter()
                .map(|(provider, x)| (provider, look.screen.left + x as i32))
                .collect();
            self.island_hits.push(IslandHit {
                bounds: (span.left, bar_y, span.right, bar_y + bar_height),
                action: IslandAction::Toggle(Island::Usage),
            });
        }

        if let Some(kind) = look.panel {
            let anchor = match kind {
                Island::Music => music,
                Island::Usage => usage,
            };
            let Some(span) = anchor else {
                return Ok(());
            };
            let desired = px(match kind {
                Island::Music => 360.0,
                Island::Usage => ISLAND_EXPANDED_WIDTH,
            });
            self.panel_span = if kind == Island::Usage && span.left < width * 0.5 {
                left_bubble_panel_span(span, width, desired)
            } else {
                bubble_panel_span(span, width, desired)
            };
            let volume_span = if kind == Island::Music && look.status.volume.is_some() {
                let (music, volume) = volume_popover_layout(self.panel_span, width, look.scale);
                self.panel_span = music;
                Some(volume)
            } else {
                None
            };
            let height = px(look.panel_height());
            let popup_top = bar_y - px(PANEL_GAP) - height;
            let geometry = IslandGeometry::new(self.panel_span, popup_top, look);
            let theme = Theme::for_popup(look.light, look.glass);
            let content = popup_surface(gfx, theme, layers, geometry, height, px(PANEL_RADIUS))?;
            match kind {
                Island::Music => {
                    if let Some(track) = &look.track {
                        self.draw_player(gfx, &content, geometry, track, cover.as_ref())?;
                    }
                    if let Some(span) = volume_span {
                        let volume_geometry = IslandGeometry::new(span, geometry.top, look);
                        let volume_content = popup_surface(
                            gfx,
                            theme,
                            layers,
                            volume_geometry,
                            height,
                            span.width() / 2.0,
                        )?;
                        self.draw_volume_capsule(gfx, &volume_content, volume_geometry)?;
                        // Keep the small bridge between surfaces interactive,
                        // so moving towards the volume does not dismiss it.
                        self.panel_span.left = self.panel_span.left.min(span.left);
                        self.panel_span.right = self.panel_span.right.max(span.right);
                    }
                }
                Island::Usage => {
                    self.draw_usage_panel(gfx, &content, geometry, height, look, &providers)?
                }
            }
        }
        Ok(())
    }

    pub(super) fn draw_usage_panel(
        &mut self,
        gfx: &Gfx,
        content: &VisualCollection,
        geometry: IslandGeometry,
        island_height: f32,
        look: &Look,
        providers: &[IslandProvider<'_>],
    ) -> Result<()> {
        let c = &gfx.compositor;
        let scale = geometry.scale;
        let px = |v| geometry.px(v);
        let island_width = geometry.width;
        let ink = geometry.ink();
        let title = strings::text();
        add_panel_text(
            gfx,
            content,
            title.usage_title,
            px(28.0),
            px(15.0),
            scale,
            13.0,
            true,
            ink,
            0.94,
        )?;
        let button_text = if look.usage_refreshing {
            title.usage_refreshing
        } else {
            title.usage_refresh
        };
        let button_label = gfx.text_brush_sized(button_text, scale, 11.5, true, ink)?;
        let button_width = button_label.width + px(20.0);
        let button_height = px(27.0);
        let button_left = island_width - px(23.0) - button_width;
        let button_top = px(8.0);
        add_rounded_box(
            c,
            content,
            button_left,
            button_top,
            button_width,
            button_height,
            px(13.5),
            geometry.control_fill(),
            Some(geometry.border()),
        )?;
        add_visual(
            c,
            content,
            &button_label.brush.cast()?,
            button_left + px(10.0),
            button_top + (button_height - button_label.height) / 2.0,
            button_label.width,
            button_label.height,
        )?;
        if !look.usage_refreshing {
            self.island_hits.push(geometry.hit(
                button_left,
                button_top,
                button_width,
                button_height,
                IslandAction::Refresh,
            ));
        }
        let body_top = px(42.0);
        let body_height = (island_height - body_top - px(26.0)).max(0.0);
        let content_height = px((look.panel_content_height() - 68.0).max(0.0));
        let maximum = ((content_height - body_height) / scale).max(0.0);
        self.usage_scroll = self.usage_scroll.clamp(0.0, maximum);
        let viewport = c.CreateContainerVisual()?;
        viewport.SetOffset(Vector3::new(0.0, body_top, 0.0))?;
        viewport.SetSize(Vector2::new(island_width, body_height))?;
        viewport.SetClip(&c.CreateInsetClip()?)?;
        let body = c.CreateContainerVisual()?;
        body.SetSize(Vector2::new(island_width, content_height))?;
        body.SetOffset(Vector3::new(0.0, -px(self.usage_scroll), 0.0))?;
        viewport.Children()?.InsertAtTop(&body)?;
        content.InsertAtTop(&viewport)?;
        let rows = body.Children()?;
        let mut row_top = 6.0;
        for (provider, usage, accent) in providers {
            let y = px(row_top);
            row_top += usage_row_height(usage);
            let inset = px(23.0);
            add_visual(
                c,
                &rows,
                &c.CreateColorBrushWithColor(geometry.border())?.cast()?,
                inset,
                y - px(6.0),
                island_width - 2.0 * inset,
                1.0,
            )?;
            if let Some((bx, by, bw, bh)) = draw_usage_row(
                gfx,
                &rows,
                *provider,
                usage,
                *accent,
                scale,
                look.light,
                look.usage_interval_minutes,
                island_width,
                y,
            )? {
                let panel_top = body_top + by - px(self.usage_scroll);
                let panel_bottom = panel_top + bh;
                let visible_top = panel_top.max(body_top);
                let visible_bottom = panel_bottom.min(body_top + body_height);
                if visible_bottom > visible_top {
                    self.island_hits.push(geometry.hit(
                        bx,
                        visible_top,
                        bw,
                        visible_bottom - visible_top,
                        IslandAction::SignIn(*provider),
                    ));
                }
            }
        }
        let bounds = (
            geometry.left,
            geometry.top + body_top,
            geometry.left + island_width,
            geometry.top + body_top + body_height,
        );
        if maximum > 0.0 {
            let [up, down] = [
                ('\u{E70E}', body_top + px(2.0), -1),
                ('\u{E70D}', body_top + body_height - px(22.0), 1),
            ]
            .map(|(symbol, top, direction)| -> Result<SpriteVisual> {
                let label = gfx.glyph_brush(&[(symbol, 1.0)], scale * 0.7, ink)?;
                let visual = add_visual(
                    c,
                    content,
                    &label.brush.cast()?,
                    island_width - px(22.0) + (px(18.0) - label.width) / 2.0,
                    top + (px(20.0) - label.height) / 2.0,
                    label.width,
                    label.height,
                )?;
                self.island_hits.push(geometry.hit(
                    island_width - px(22.0),
                    top,
                    px(18.0),
                    px(20.0),
                    IslandAction::ScrollUsage(direction),
                ));
                Ok(visual)
            });
            self.usage_viewport = Some(UsageViewport {
                content: body,
                maximum,
                scale,
                bounds,
                up: up?,
                down: down?,
            });
            self.position_usage_body()?;
        }
        let newest_age = providers
            .iter()
            .filter_map(|provider| provider.1.age_minutes())
            .min();
        if let Some(minutes) = newest_age {
            let status = format!("{} {minutes} min", title.usage_updated);
            let label = gfx.text_brush_sized(&status, scale, 11.0, false, ink)?;
            add_visual(
                c,
                content,
                &label.brush.cast()?,
                (island_width - label.width) / 2.0,
                island_height - px(21.0),
                label.width,
                label.height,
            )?
            .SetOpacity(0.53)?;
        }
        Ok(())
    }

    pub(super) fn draw_player(
        &mut self,
        gfx: &Gfx,
        layers: &VisualCollection,
        geometry: IslandGeometry,
        track: &Track,
        cover: Option<&CompositionSurfaceBrush>,
    ) -> Result<()> {
        let px = |v| geometry.px(v);
        let c = &gfx.compositor;
        let ink = geometry.ink();
        add_panel_text(
            gfx,
            layers,
            strings::text().media_title,
            px(26.0),
            px(15.0),
            geometry.scale,
            12.5,
            true,
            ink,
            0.88,
        )?;
        let chevron = gfx.glyph_brush(&[('', 0.72)], geometry.scale, ink)?;
        let x = geometry.width - px(22.0) - chevron.width;
        add_visual(
            c,
            layers,
            &chevron.brush.cast()?,
            x,
            (px(46.0) - chevron.height) / 2.0,
            chevron.width,
            chevron.height,
        )?;
        self.island_hits.push(geometry.hit(
            x - px(6.0),
            px(7.0),
            chevron.width + px(12.0),
            px(32.0),
            IslandAction::Toggle(Island::Music),
        ));
        let tile = px(34.0);
        if let Some(cover) = cover {
            add_visual(c, layers, &cover.cast()?, px(25.0), px(46.0), tile, tile)?;
        }
        let available = geometry.width - px(91.0);
        let title = gfx.text_brush_fitted(
            track_title(track),
            geometry.scale,
            13.0,
            true,
            ink,
            available,
        )?;
        let title_y = if track.artist.trim().is_empty() {
            px(63.0) - title.height / 2.0
        } else {
            px(47.0)
        };
        add_visual(
            c,
            layers,
            &title.brush.cast()?,
            px(69.0),
            title_y,
            title.width,
            title.height,
        )?;
        if !track.artist.trim().is_empty() {
            let artist =
                gfx.text_brush_fitted(&track.artist, geometry.scale, 11.0, false, ink, available)?;
            add_visual(
                c,
                layers,
                &artist.brush.cast()?,
                px(69.0),
                px(65.0),
                artist.width,
                artist.height,
            )?
            .SetOpacity(0.69)?;
        }
        let center = geometry.width / 2.0;
        for (control, glyph, enabled, offset) in [
            (MediaControl::Previous, '\u{E892}', track.previous, -44.0),
            (
                MediaControl::PlayPause,
                if track.playing {
                    '\u{E769}'
                } else {
                    '\u{E768}'
                },
                track.toggle,
                0.0,
            ),
            (MediaControl::Next, '\u{E893}', track.next, 44.0),
        ] {
            let size = px(34.0);
            let x = center + px(offset) - size / 2.0;
            let y = px(89.0);
            let main = matches!(control, MediaControl::PlayPause);
            if main {
                add_rounded_box(
                    c,
                    layers,
                    x,
                    y,
                    size,
                    size,
                    size / 2.0,
                    geometry.control_fill(),
                    Some(geometry.border()),
                )?;
            }
            let symbol = gfx.glyph_brush(&[(glyph, 1.0)], geometry.scale, ink)?;
            add_visual(
                c,
                layers,
                &symbol.brush.cast()?,
                x + (size - symbol.width) / 2.0,
                y + (size - symbol.height) / 2.0,
                symbol.width,
                symbol.height,
            )?
            .SetOpacity(if enabled { 1.0 } else { 0.30 })?;
            if enabled {
                self.island_hits
                    .push(geometry.hit(x, y, size, size, IslandAction::Media(control)));
            }
        }
        self.draw_progress(gfx, layers, geometry, track)?;
        Ok(())
    }

    pub(super) fn draw_volume_capsule(
        &mut self,
        gfx: &Gfx,
        layers: &VisualCollection,
        geometry: IslandGeometry,
    ) -> Result<()> {
        let px = |v| geometry.px(v);
        let c = &gfx.compositor;
        let ink = geometry.ink();
        if let Some(volume) = self.status.volume {
            let symbol = if volume.muted {
                glyph::MUTE
            } else {
                glyph::VOLUME[volume.level.min(3) as usize]
            };
            let label = gfx.glyph_brush(&[(symbol, 1.0)], geometry.scale, ink)?;
            add_visual(
                c,
                layers,
                &label.brush.cast()?,
                (geometry.width - label.width) / 2.0,
                px(155.0) - label.height / 2.0,
                label.width,
                label.height,
            )?;
            self.island_hits.push(geometry.hit(
                (geometry.width - px(28.0)) / 2.0,
                px(141.0),
                px(28.0),
                px(28.0),
                IslandAction::Mute,
            ));
            let left = (geometry.width - px(4.0)) / 2.0;
            let top = px(34.0);
            let height = px(96.0);
            let percent = self.volume_target.unwrap_or(volume.percent);
            add_rounded_box(
                c,
                layers,
                left,
                top,
                px(4.0),
                height,
                px(2.0),
                geometry.control_fill(),
                None,
            )?;
            let (_, fill) = retained_rounded_box(
                c,
                layers,
                left,
                top,
                px(4.0),
                height,
                px(2.0),
                if geometry.light {
                    rgba(40, 48, 58, 220)
                } else {
                    rgba(240, 244, 250, 235)
                },
                None,
            )?;
            let label =
                gfx.text_brush_sized(&format!("{percent}%"), geometry.scale, 10.5, false, ink)?;
            let value = add_visual(
                c,
                layers,
                &label.brush.cast()?,
                (geometry.width - label.width) / 2.0,
                px(12.0),
                label.width,
                label.height,
            )?;
            value.SetOpacity(0.8)?;
            let mut controls = VolumeControls {
                fill,
                value,
                geometry,
                height,
                position_percent: None,
                label_percent: percent,
            };
            controls.move_to(percent)?;
            self.volume_controls = Some(controls);
            self.island_hits.push(geometry.hit(
                (geometry.width - px(28.0)) / 2.0,
                top - px(8.0),
                px(28.0),
                height + px(16.0),
                IslandAction::Volume(VolumeTrack {
                    center_x: geometry.left + geometry.width / 2.0,
                    top: geometry.top + top,
                    bottom: geometry.top + top + height,
                }),
            ));
        }
        Ok(())
    }

    pub(super) fn draw_progress(
        &mut self,
        gfx: &Gfx,
        layers: &VisualCollection,
        geometry: IslandGeometry,
        track: &Track,
    ) -> Result<()> {
        let px = |v| geometry.px(v);
        let c = &gfx.compositor;
        let width = (geometry.width - px(52.0)).max(px(1.0));
        let Some(timeline) = track.timeline else {
            let text = if self.status.volume.is_some() {
                strings::text().media_no_progress
            } else {
                strings::text().media_no_output
            };
            let label =
                gfx.text_brush_fitted(text, geometry.scale, 11.0, false, geometry.ink(), width)?;
            add_visual(
                c,
                layers,
                &label.brush.cast()?,
                px(26.0),
                px(135.0),
                label.width,
                label.height,
            )?
            .SetOpacity(0.55)?;
            return Ok(());
        };
        add_rounded_box(
            c,
            layers,
            px(26.0),
            px(139.0),
            width,
            px(4.0),
            px(2.0),
            geometry.control_fill(),
            None,
        )?;
        let (_, fill) = retained_rounded_box(
            c,
            layers,
            px(26.0),
            px(139.0),
            width,
            px(4.0),
            px(2.0),
            if geometry.light {
                rgba(40, 48, 58, 220)
            } else {
                rgba(240, 244, 250, 235)
            },
            None,
        )?;
        let label = gfx.text_brush_sized("0:00", geometry.scale, 10.0, false, geometry.ink())?;
        let elapsed = add_visual(
            c,
            layers,
            &label.brush.cast()?,
            px(26.0),
            px(148.0),
            label.width,
            label.height,
        )?;
        let total = add_visual(
            c,
            layers,
            &label.brush.cast()?,
            px(26.0),
            px(148.0),
            label.width,
            label.height,
        )?;
        elapsed.SetOpacity(0.6)?;
        total.SetOpacity(0.6)?;
        let mut controls = ProgressControls {
            fill,
            elapsed,
            total,
            geometry,
            width,
            elapsed_seconds: None,
            total_seconds: None,
        };
        controls.paint(gfx, timeline, Instant::now())?;
        self.progress_controls = Some(controls);
        Ok(())
    }
}

pub(super) fn media_time(seconds: u64) -> String {
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

pub(super) fn toggle_panel(open: Option<Island>, clicked: Island) -> Option<Island> {
    (open != Some(clicked)).then_some(clicked)
}

pub(super) fn available_panel(open: Option<Island>, music: bool, usage: bool) -> Option<Island> {
    open.filter(|island| match island {
        Island::Music => music,
        Island::Usage => usage,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn island_providers(look: &Look) -> Vec<IslandProvider<'_>> {
    Provider::ALL
        .into_iter()
        .filter(|provider| look.active_providers.has(*provider))
        .map(|provider| {
            let accent = match provider {
                Provider::Claude => rgba(217, 119, 87, 255),
                Provider::Codex => rgba(135, 190, 245, 255),
                Provider::Antigravity => rgba(96, 165, 250, 255),
                Provider::Gemini => rgba(111, 146, 255, 255),
                Provider::OpenCode => rgba(132, 196, 166, 255),
                Provider::Zai => rgba(79, 110, 247, 255),
            };
            (provider, look.usage.get(provider), accent)
        })
        .collect()
}

pub(super) fn popup_surface(
    gfx: &Gfx,
    theme: Theme,
    layers: &VisualCollection,
    geometry: IslandGeometry,
    height: f32,
    radius: f32,
) -> Result<VisualCollection> {
    let c = &gfx.compositor;
    let size = Vector2::new(geometry.width, height);
    let panel = theme.panel(gfx, size, radius, geometry.scale)?;
    let popup = c.CreateContainerVisual()?;
    popup.SetSize(size)?;
    popup.SetOffset(Vector3::new(geometry.left.round(), geometry.top, 0.0))?;
    let children = popup.Children()?;
    let mask = c.CreateNineGridBrush()?;
    mask.SetSource(&gfx.rounded_mask(radius)?)?;
    mask.SetInsets(radius.ceil() + 1.0)?;
    children.InsertAtTop(&island_shadow(c, &mask.cast()?, size, geometry)?)?;
    children.InsertAtTop(&panel)?;
    layers.InsertAtTop(&popup)?;
    Ok(children)
}

pub(super) fn island_shadow(
    c: &Compositor,
    mask: &CompositionBrush,
    size: Vector2,
    geometry: IslandGeometry,
) -> Result<SpriteVisual> {
    let shadow = c.CreateDropShadow()?;
    shadow.SetMask(mask)?;
    shadow.SetBlurRadius(geometry.px(12.0))?;
    shadow.SetOffset(Vector3::new(0.0, geometry.px(5.0), 0.0))?;
    shadow.SetColor(rgba(0, 0, 0, 92))?;
    let host = c.CreateSpriteVisual()?;
    host.SetSize(size)?;
    host.SetShadow(&shadow)?;
    Ok(host)
}

#[allow(dead_code)]
pub(super) fn island_surface(
    gfx: &Gfx,
    look: &Look,
    layers: &VisualCollection,
    geometry: IslandGeometry,
    island_height: f32,
) -> Result<VisualCollection> {
    let c = &gfx.compositor;
    let px = |v| geometry.px(v);
    let island_left = geometry.left;
    let island_width = geometry.width;
    let scale = geometry.scale;
    let size = Vector2::new(island_width, island_height);
    let island = c.CreateContainerVisual()?;
    island.SetSize(size)?;
    island.SetOffset(Vector3::new(island_left.round(), 0.0, 0.0))?;
    let content = island.Children()?;
    let silhouette = gfx.island_mask(island_width, island_height, scale)?;
    content.InsertAtTop(&island_shadow(c, &silhouette.cast()?, size, geometry)?)?;

    let tint = match (look.light, look.glass) {
        (true, true) => rgba(247, 249, 252, 243),
        (false, true) => rgba(7, 9, 13, 242),
        (true, false) => rgba(247, 249, 252, 255),
        (false, false) => rgba(10, 12, 17, 255),
    };
    if look.glass {
        // The backdrop cannot be used as a mask-brush source. Inset its
        // rounded clip so it stays entirely inside the curved silhouette.
        let inset = (12.0 * scale)
            .min(island_width / 5.0)
            .min(island_height / 2.0);
        let glass_width = island_width - inset * 2.0;
        let glass = c.CreateContainerVisual()?;
        glass.SetSize(Vector2 {
            X: glass_width,
            Y: island_height,
        })?;
        glass.SetOffset(Vector3 {
            X: inset,
            Y: 0.0,
            Z: 0.0,
        })?;
        let outline = c.CreateRoundedRectangleGeometry()?;
        let lift = px(22.0);
        outline.SetSize(Vector2 {
            X: glass_width,
            Y: island_height + lift,
        })?;
        outline.SetOffset(Vector2 { X: 0.0, Y: -lift })?;
        outline.SetCornerRadius(Vector2 { X: lift, Y: lift })?;
        glass.SetClip(&c.CreateGeometricClipWithGeometry(&outline)?)?;
        add_visual(
            c,
            &glass.Children()?,
            &c.CreateHostBackdropBrush()?.cast()?,
            0.0,
            0.0,
            glass_width,
            island_height,
        )?;
        content.InsertAtTop(&glass)?;
    }
    let masked = c.CreateMaskBrush()?;
    masked.SetSource(&c.CreateColorBrushWithColor(tint)?)?;
    masked.SetMask(&silhouette)?;
    add_visual(
        c,
        &content,
        &masked.cast()?,
        0.0,
        0.0,
        island_width,
        island_height,
    )?;

    layers.InsertAtTop(&island)?;
    Ok(content)
}

pub(super) fn bubble_surface(
    gfx: &Gfx,
    look: &Look,
    layers: &VisualCollection,
    geometry: IslandGeometry,
    bar_height: f32,
) -> Result<(VisualCollection, f32)> {
    let c = &gfx.compositor;
    let inset_y = geometry.px(BUBBLE_VERTICAL_INSET);
    let pill_height = (bar_height - 2.0 * inset_y).max(1.0);
    let radius = pill_height / 2.0;
    let size = Vector2::new(geometry.width, pill_height);
    let bubble = c.CreateContainerVisual()?;
    bubble.SetSize(size)?;
    bubble.SetOffset(Vector3::new(
        geometry.left.round(),
        geometry.top + inset_y,
        0.0,
    ))?;
    let content = bubble.Children()?;

    let tint = match (look.light, look.glass) {
        (true, true) => rgba(247, 249, 252, 243),
        (false, true) => rgba(7, 9, 13, 242),
        (true, false) => rgba(247, 249, 252, 255),
        (false, false) => rgba(10, 12, 17, 255),
    };
    if look.glass {
        let glass = c.CreateContainerVisual()?;
        glass.SetSize(size)?;
        let outline = c.CreateRoundedRectangleGeometry()?;
        outline.SetSize(size)?;
        outline.SetCornerRadius(Vector2 {
            X: radius,
            Y: radius,
        })?;
        glass.SetClip(&c.CreateGeometricClipWithGeometry(&outline)?)?;
        add_visual(
            c,
            &glass.Children()?,
            &c.CreateHostBackdropBrush()?.cast()?,
            0.0,
            0.0,
            geometry.width,
            pill_height,
        )?;
        content.InsertAtTop(&glass)?;
    }
    add_rounded_box(
        c,
        &content,
        0.0,
        0.0,
        geometry.width,
        pill_height,
        radius,
        tint,
        None,
    )?;

    layers.InsertAtTop(&bubble)?;
    Ok((content, pill_height))
}

/// Draws the providers in the closed island and returns where each one's
/// figure sits (its centre, from the segment's left edge), for the crab.
/// The moment an AI plan's figure crosses a line worth showing, comparing
/// the previous update with this one: a limit reset to 0 % (after being in
/// use), past 90 %, or at 100 %. Nothing on the first figures after starting,
/// which have nothing to compare with.
pub(super) fn usage_moment(
    old: &usage::Snapshot,
    new: &usage::Snapshot,
    active: &ActiveProviders,
) -> Option<(Provider, mascot::Act)> {
    Provider::ALL
        .into_iter()
        .filter(|p| active.has(*p))
        .find_map(|provider| {
            let previous = old.get(provider);
            let current = new.get(provider);
            if previous.state != State::Ready || current.state != State::Ready {
                return None;
            }
            let before_window = previous.main_window()?;
            let now_window = current.main_window()?;
            if before_window.minutes != now_window.minutes
                || before_window.label != now_window.label
            {
                return None;
            }
            if let (Some(before), Some(now)) = (previous.fetched_at, current.fetched_at)
                && now <= before
            {
                return None;
            }
            let before = before_window.used_percent;
            let now = now_window.used_percent;
            let act = if now >= 100 && before < 100 {
                mascot::Act::Exhausted
            } else if now >= 90 && before < 90 {
                mascot::Act::Nervous
            } else if now <= 1 && before >= 20 {
                mascot::Act::Fresh
            } else {
                return None;
            };
            Some((provider, act))
        })
}

fn strip_divider_color(effective_light: bool) -> Color {
    if effective_light {
        rgba(0, 0, 0, 32)
    } else {
        rgba(255, 255, 255, 36)
    }
}

impl MenuBar {
    /// The user's shelf: icons only, at the AI tools strip's size, spacing
    /// and dividers, centred in `region`. What does not fit is left out.
    #[expect(
        clippy::too_many_arguments,
        reason = "The shelf is laid out in the space the island leaves on the bar"
    )]
    fn draw_shelf(
        &mut self,
        gfx: &Gfx,
        look: &Look,
        effective_light: bool,
        layers: &VisualCollection,
        region: Span,
        bar_y: f32,
        bar_height: f32,
    ) -> Result<()> {
        if look.shelf.is_empty() {
            return Ok(());
        }
        let c = &gfx.compositor;
        let px = |v: f32| (v * look.scale).round();
        let side = px(STRIP_ICON).max(STRIP_ICON_MIN);
        let gap = px(STRIP_ITEM_GAP);
        let slop = px(4.0);
        let divider_h = px(STRIP_DIVIDER_HEIGHT).max(1.0);
        let divider_color = strip_divider_color(effective_light);
        let ink = if effective_light {
            Ink::Dark
        } else {
            Ink::Light
        };
        let top = bar_y + (bar_height - side) / 2.0;
        let side_px = side as u32;
        let spans = shelf_spans(region, look.shelf.len(), side, gap);
        for (index, (span, item)) in spans.iter().zip(&look.shelf).enumerate() {
            if index > 0 {
                add_visual(
                    c,
                    layers,
                    &c.CreateColorBrushWithColor(divider_color)?.cast()?,
                    span.left - gap / 2.0,
                    bar_y + (bar_height - divider_h) / 2.0,
                    px(1.0).max(1.0),
                    divider_h,
                )?;
            }
            let brush = match (&item.icon, self.shelf_brushes.get(&item.path)) {
                (Some(_), Some((drawn, brush))) if *drawn == side_px => Some(brush.clone()),
                (Some(image), _) => {
                    let brush = gfx.icon_brush(image, side_px)?;
                    self.shelf_brushes
                        .insert(item.path.clone(), (side_px, brush.clone()));
                    Some(brush)
                }
                (None, _) => None,
            };
            if let Some(brush) = brush {
                add_visual(c, layers, &brush.cast()?, span.left, top, side, side)?;
            } else {
                // No picture to show: a folder or a page glyph instead.
                let glyph = if std::path::Path::new(&item.path).is_dir() {
                    '\u{E8B7}'
                } else {
                    '\u{E7C3}'
                };
                let label = gfx.glyph_brush(&[(glyph, 1.0)], look.scale, ink)?;
                add_visual(
                    c,
                    layers,
                    &label.brush.cast()?,
                    span.left + (side - label.width) / 2.0,
                    bar_y + (bar_height - label.height) / 2.0,
                    label.width,
                    label.height,
                )?;
            }
            self.zones.push((
                span.left - slop,
                span.right + slop,
                Action::ShelfItem(index),
            ));
        }
        Ok(())
    }
}

struct UsageStripItem {
    provider: Provider,
    label: LabelBrush,
    has_value: bool,
    width: f32,
}

struct UsageStrip {
    icon_size: f32,
    icon_gap: f32,
    item_gap: f32,
    items: Vec<UsageStripItem>,
}

impl UsageStrip {
    fn fitting_width(&self, max_width: f32) -> f32 {
        let mut used = 0.0;
        for (index, item) in self.items.iter().enumerate() {
            let next = if index == 0 {
                item.width
            } else {
                used + self.item_gap + item.width
            };
            if next > max_width {
                break;
            }
            used = next;
        }
        used
    }
}

fn prepare_usage_strip(
    gfx: &Gfx,
    look: &Look,
    effective_light: bool,
    providers: &[IslandProvider<'_>],
) -> Result<UsageStrip> {
    let px = |v: f32| (v * look.scale).round();
    let ink = if effective_light {
        Ink::Dark
    } else {
        Ink::Light
    };
    let icon_size = px(STRIP_ICON).max(STRIP_ICON_MIN);
    let icon_gap = px(5.0);
    let item_gap = px(STRIP_ITEM_GAP);
    let mut items = Vec::with_capacity(providers.len());
    for (provider, usage, _) in providers {
        let text = usage.compact_text();
        let label = gfx.text_brush_sized(&text, look.scale, 12.0, true, ink)?;
        let width = icon_size + icon_gap + label.width;
        items.push(UsageStripItem {
            provider: *provider,
            label,
            has_value: usage.five_hour_window().is_some(),
            width,
        });
    }
    Ok(UsageStrip {
        icon_size,
        icon_gap,
        item_gap,
        items,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "Strip placement needs bar geometry, scale, and backdrop contrast"
)]
fn draw_usage_strip(
    gfx: &Gfx,
    c: &Compositor,
    layers: &VisualCollection,
    strip: &UsageStrip,
    span: Span,
    bar_y: f32,
    bar_height: f32,
    scale: f32,
    effective_light: bool,
) -> Result<Vec<(Provider, f32)>> {
    let px = |v: f32| (v * scale).round();
    let mut x = span.left;
    let mut anchors = Vec::with_capacity(strip.items.len());
    let divider_color = strip_divider_color(effective_light);
    let divider_h = px(STRIP_DIVIDER_HEIGHT).max(1.0);
    for (index, item) in strip.items.iter().enumerate() {
        let right = x + item.width;
        if right > span.right + 0.5 {
            break;
        }
        if index > 0 {
            let div_x = x - strip.item_gap / 2.0;
            add_visual(
                c,
                layers,
                &c.CreateColorBrushWithColor(divider_color)?.cast()?,
                div_x,
                bar_y + (bar_height - divider_h) / 2.0,
                px(1.0).max(1.0),
                divider_h,
            )?;
        }
        anchors.push((item.provider, x + item.width / 2.0));
        if let Some(image) = usage::logo(item.provider) {
            let icon_px = strip.icon_size as u32;
            let image_brush = gfx.icon_brush(image, icon_px)?;
            let brush: CompositionBrush = if !matches!(
                item.provider,
                Provider::Claude | Provider::Antigravity | Provider::Gemini
            ) && effective_light
            {
                let tinted = c.CreateMaskBrush()?;
                tinted.SetMask(&image_brush)?;
                tinted.SetSource(&c.CreateColorBrushWithColor(rgba(27, 34, 45, 255))?)?;
                tinted.cast()?
            } else {
                image_brush.cast()?
            };
            add_visual(
                c,
                layers,
                &brush,
                x,
                bar_y + (bar_height - strip.icon_size) / 2.0,
                strip.icon_size,
                strip.icon_size,
            )?;
        }
        add_visual(
            c,
            layers,
            &item.label.brush.cast()?,
            x + strip.icon_size + strip.icon_gap,
            bar_y + (bar_height - item.label.height) / 2.0,
            item.label.width,
            item.label.height,
        )?
        .SetOpacity(if item.has_value { 0.95 } else { 0.55 })?;
        x = right + strip.item_gap;
    }
    Ok(anchors)
}

pub(super) fn track_title(track: &Track) -> &str {
    if track.title.trim().is_empty() {
        strings::text().media_title
    } else {
        &track.title
    }
}

pub(super) fn draw_compact_player(
    gfx: &Gfx,
    layers: &VisualCollection,
    geometry: IslandGeometry,
    width: f32,
    height: f32,
    track: &Track,
    cover: Option<&CompositionSurfaceBrush>,
) -> Result<()> {
    let c = &gfx.compositor;
    let px = |v| geometry.px(v);
    let ink = geometry.ink();
    let side = px(18.0).min(height).min(width);
    let x = px(23.0) + side;
    let show_title = width > x + px(18.0);
    let note_x = if show_title {
        px(18.0)
    } else {
        (width - side) / 2.0
    };
    if let Some(cover) = cover {
        add_visual(
            c,
            layers,
            &cover.cast()?,
            note_x,
            (height - side) / 2.0,
            side,
            side,
        )?
        .SetOpacity(if track.playing { 1.0 } else { 0.7 })?;
    }
    if show_title {
        let label = gfx.text_brush_fitted(
            track_title(track),
            geometry.scale,
            11.5,
            true,
            ink,
            width - x - px(13.0),
        )?;
        add_visual(
            c,
            layers,
            &label.brush.cast()?,
            x,
            (height - label.height) / 2.0,
            label.width,
            label.height,
        )?;
    }
    Ok(())
}

pub(super) struct BubbleMusic {
    pub(super) artwork_size: f32,
    pub(super) label: LabelBrush,
    pub(super) width: f32,
}

pub(super) fn prepare_bubble_music(gfx: &Gfx, look: &Look, track: &Track) -> Result<BubbleMusic> {
    let px = |v: f32| (v * look.scale).round();
    let ink = if look.light { Ink::Dark } else { Ink::Light };
    let artwork_size = px(18.0);
    let left_pad = px(10.0);
    let gap = px(6.0);
    let right_pad = px(8.0);
    let max_text =
        (px(BUBBLE_MUSIC_WIDTH) - left_pad - artwork_size - gap - right_pad).max(px(18.0));
    let label = gfx.text_brush_fitted(track_title(track), look.scale, 11.5, true, ink, max_text)?;
    let width = (left_pad + artwork_size + gap + label.width + right_pad)
        .min(px(BUBBLE_MUSIC_WIDTH))
        .ceil();
    Ok(BubbleMusic {
        artwork_size,
        label,
        width,
    })
}

pub(super) fn draw_bubble_player(
    gfx: &Gfx,
    layers: &VisualCollection,
    geometry: IslandGeometry,
    height: f32,
    track: &Track,
    prepared: &BubbleMusic,
    cover: Option<&CompositionSurfaceBrush>,
) -> Result<()> {
    let width = geometry.width;
    let c = &gfx.compositor;
    let px = |v| geometry.px(v);
    let ink = geometry.ink();
    let side = prepared.artwork_size.min(height).min(width);
    let left_pad = px(10.0);
    let gap = px(6.0);
    let right_pad = px(8.0);
    let text_x = left_pad + side + gap;
    let max_label_width = width - text_x - right_pad;
    let show_title = max_label_width >= px(18.0);
    let note_x = if show_title {
        left_pad
    } else {
        (width - side) / 2.0
    };
    if let Some(cover) = cover {
        add_visual(
            c,
            layers,
            &cover.cast()?,
            note_x,
            (height - side) / 2.0,
            side,
            side,
        )?
        .SetOpacity(if track.playing { 1.0 } else { 0.7 })?;
    }
    if show_title {
        let fitted;
        let label = if prepared.label.width <= max_label_width {
            &prepared.label
        } else {
            fitted = gfx.text_brush_fitted(
                track_title(track),
                geometry.scale,
                11.5,
                true,
                ink,
                max_label_width,
            )?;
            &fitted
        };
        add_visual(
            c,
            layers,
            &label.brush.cast()?,
            text_x,
            (height - label.height) / 2.0,
            label.width,
            label.height,
        )?;
    }
    Ok(())
}
