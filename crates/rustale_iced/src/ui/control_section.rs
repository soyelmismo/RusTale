use crate::config::GameSettings;
use crate::game::LauncherStatus;
use crate::{Message, theme, util};
use iced::widget::{ProgressBar, Space, button, column, container, row, svg};
use iced::{Alignment, Color, Element, Length};

pub fn view<'a>(
    status: &'a LauncherStatus,
    settings: &'a GameSettings,
    resolved_version: Option<u32>,
    status_text: &'a str,
    progress: f32,
    step_progress: f32,
    current_step: Option<usize>,
    total_steps: Option<usize>,
    eta: Option<&'a String>,
    localization: &'a rustale_shared::lang::Localization,
    is_disabled: bool,
    ctx: theme::UIContext,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let play_button_text = match status {
        LauncherStatus::Playing => localization.t("launcher.stop"),
        LauncherStatus::Downloading => localization.t("launcher.status.cancel"),
        LauncherStatus::Checking => localization.t("launcher.status.checking"),
        LauncherStatus::Migrating => localization.t("launcher.status.cancel"),
        LauncherStatus::NeedsInstall => localization.t("launcher.play"),
        LauncherStatus::NeedsUpdate => localization.t("launcher.update"),
        LauncherStatus::Busy => localization.t("launcher.status.busy"),
        LauncherStatus::OfflineReady => localization.t("launcher.status.offline_ready"),
        _ => localization.t("launcher.play"),
    };

    let play_icon = match status {
        LauncherStatus::Playing => util::icons::STOP,
        LauncherStatus::Downloading | LauncherStatus::Migrating => util::icons::X,
        _ => util::icons::PLAY,
    };

    let is_long_text = play_button_text.len() > 9;
    let (icon_size, spacing_val) = if is_long_text { (16.0, 6) } else { (20.0, 10) };

    let is_busy = matches!(
        status,
        LauncherStatus::Checking
            | LauncherStatus::Busy
            | LauncherStatus::Downloading
            | LauncherStatus::Migrating
    );

    let navigation_locked = is_disabled || is_busy;

    let version_display = if settings.game_version == 0 {
        match resolved_version {
            Some(v) if v > 0 => format!("{} (Latest)", v),
            _ => "Latest".to_string(),
        }
    } else {
        settings.game_version.to_string()
    };

    let info_section = column![
        row![
            theme::text_micro(localization.t("launcher.info.channel"), ctx),
            Space::new().width(Length::Fill),
            theme::text_small(&settings.channel, ctx)
        ]
        .width(Length::Fill),
        row![
            theme::text_micro(localization.t("launcher.info.version"), ctx),
            Space::new().width(Length::Fill),
            theme::text_small(version_display, ctx)
        ]
        .width(Length::Fill),
    ]
    .spacing(5);

    let mut play_btn = button(
        container(
            row![
                theme::svg(
                    svg(util::svg_handle(play_icon))
                        .width(icon_size)
                        .height(icon_size)
                        .style(move |t, s| theme::svg_accent(&palette, t, s))
                        .opacity(ctx.palette.text_primary.a),
                    ctx
                ),
                theme::text_title(play_button_text, ctx)
            ]
            .spacing(spacing_val)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |t| theme::container_style_transparent(&palette, t)),
    )
    .style(move |t, bs| match status {
        _ if is_disabled => theme::play_button_style(&palette, t, bs),
        LauncherStatus::Checking | LauncherStatus::Busy => {
            theme::blocked_button_style(&palette, t, bs)
        }
        LauncherStatus::Playing => theme::play_button_style_active(&palette, t, bs),
        LauncherStatus::Downloading | LauncherStatus::Migrating => {
            theme::play_button_style_active(&palette, t, bs)
        }
        LauncherStatus::NeedsUpdate => theme::update_button_style(&palette, t, bs),
        _ => theme::play_button_style(&palette, t, bs),
    })
    .width(Length::Fill)
    .height(Length::Fill);

    if !is_disabled {
        play_btn = match status {
            LauncherStatus::Downloading | LauncherStatus::Migrating => {
                play_btn.on_press(Message::CancelAction)
            }
            LauncherStatus::Checking | LauncherStatus::Busy => play_btn,
            _ => play_btn.on_press(Message::StartGame),
        };
    }

    let settings_btn = button(
        container(
            svg(util::svg_handle(util::icons::SETTINGS))
                .width(18)
                .height(18)
                .style(move |t, s| theme::svg_accent(&palette, t, s))
                .opacity(ctx.palette.text_primary.a),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |t| theme::container_style_transparent(&palette, t)),
    )
    .style(move |t, s| {
        if navigation_locked {
            theme::blocked_button_style(&palette, t, s)
        } else {
            theme::secondary_button_style(&palette, t, s)
        }
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let settings_btn = if !navigation_locked {
        settings_btn.on_press(Message::OpenSettings)
    } else {
        settings_btn
    };

    let mods_btn = button(
        container(
            svg(util::svg_handle(util::icons::PUZZLE))
                .width(18)
                .height(18)
                .style(move |t, s| theme::svg_accent(&palette, t, s))
                .opacity(ctx.palette.text_primary.a),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |t| theme::container_style_transparent(&palette, t)),
    )
    .style(move |t, s| {
        if navigation_locked {
            theme::blocked_button_style(&palette, t, s)
        } else {
            theme::secondary_button_style(&palette, t, s)
        }
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let mods_btn = if !navigation_locked {
        mods_btn.on_press(Message::OpenMods)
    } else {
        mods_btn
    };

    // Server panel button: always enabled (independent of game state)
    let server_btn = button(
        container(
            svg(util::svg_handle(util::icons::SHELL))
                .width(18)
                .height(18)
                .style(move |t, s| theme::svg_accent(&palette, t, s))
                .opacity(ctx.palette.text_primary.a),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |t| theme::container_style_transparent(&palette, t)),
    )
    .style(move |t, s| theme::secondary_button_style(&palette, t, s))
    .width(Length::Fill)
    .height(Length::Fill)
    .on_press(Message::OpenServerPanel);

    let actions = row![
        theme::magic_button(play_btn.into(), ctx),
        column![
            theme::magic_button(settings_btn.into(), ctx),
            theme::magic_button(mods_btn.into(), ctx),
            theme::magic_button(server_btn.into(), ctx)
        ]
        .width(45)
        .spacing(8)
    ]
    .spacing(10)
    .height(120);

    let progress_bars_container = if is_busy {
        // ── Step label row: "Step 2 / 4" on the left, "45%" on the right ──────
        let step_label = match (current_step, total_steps) {
            (Some(cur), Some(tot)) if tot > 0 => {
                format!("{} {}/{}", localization.t("launcher.status.step"), cur, tot)
            }
            _ => status_text.to_string(),
        };

        let header_row = row![
            theme::text_micro(step_label, ctx),
            Space::new().width(Length::Fill),
            theme::text_micro(format!("{:.0}%", progress * 100.0), ctx)
        ]
        .width(Length::Fill);

        // ── Bar 1: Segmented global progress ─────────────────────────────────
        // Draws `total_steps` equal-width blocks separated by 2px gaps.
        // Blocks before current_step are fully filled; current block is filled
        // proportionally to step_progress; remaining blocks are empty.
        let segmented_bar: Element<'_, Message> =
            match (current_step, total_steps) {
                (Some(cur), Some(tot)) if tot > 1 => {
                    // Build a row of `tot` small containers acting as bar segments.
                    // Gap between segments is 2px; each segment fills equally.
                    let accent = palette.accent;
                    let surface = Color {
                        r: accent.r * 0.15,
                        g: accent.g * 0.15,
                        b: accent.b * 0.15,
                        a: accent.a * 0.4,
                    };

                    let mut segments: Vec<Element<'_, Message>> = Vec::with_capacity(tot * 2 - 1);
                    for i in 1..=tot {
                        // Fill level for this segment
                        let fill = if i < cur {
                            1.0_f32 // completed
                        } else if i == cur {
                            step_progress.clamp(0.0, 1.0) // in-progress
                        } else {
                            0.0_f32 // not started
                        };

                        let seg: Element<'_, Message> = container(
                            ProgressBar::new(0.0..=1.0, fill)
                                .style(move |t| theme::accent_bar_style(&palette, t))
                        )
                        .height(6)
                        .width(Length::Fill)
                        .style(move |_| container::Style {
                            background: Some(surface.into()),
                            border: iced::Border {
                                radius: 3.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                        .into();

                        segments.push(seg);

                        // Add gap between segments (not after the last one)
                        if i < tot {
                            segments.push(Space::new().width(2).into());
                        }
                    }

                    // Wrap in a row
                    let seg_row = segments
                        .into_iter()
                        .fold(
                            iced::widget::Row::new().width(Length::Fill).align_y(Alignment::Center),
                            |r, e| r.push(e),
                        );

                    seg_row.into()
                }
                // Fallback: single plain progress bar (no step info yet)
                _ => container(
                    ProgressBar::new(0.0..=1.0, progress)
                        .style(move |t| theme::accent_bar_style(&palette, t)),
                )
                .height(6)
                .width(Length::Fill)
                .into(),
            };

        // ── Bar 2: Sub-progress of the current step ───────────────────────────
        // Only shown when we have real step data and step_progress is meaningful.
        let has_step_data = matches!((current_step, total_steps), (Some(_), Some(t)) if t > 0);
        let step_bar_row: Element<'_, Message> = if has_step_data {
            row![
                theme::text_micro(status_text, ctx),
                Space::new().width(Length::Fill),
                if let Some(eta_str) = eta {
                    Element::from(theme::text_micro(format!("ETA: {}", eta_str), ctx))
                } else {
                    Element::from(Space::new().width(0))
                }
            ]
            .width(Length::Fill)
            .into()
        } else {
            // No step data: show plain status + ETA
            row![
                theme::text_micro(status_text, ctx),
                Space::new().width(Length::Fill),
                if let Some(eta_str) = eta {
                    Element::from(theme::text_micro(format!("ETA: {}", eta_str), ctx))
                } else {
                    Element::from(Space::new().width(0))
                }
            ]
            .width(Length::Fill)
            .into()
        };

        column![header_row, segmented_bar, step_bar_row]
            .spacing(5)
    } else {
        column![]
    };

    // Show status text as body label only when idle and non-empty
    let idle_status_label = if !is_busy && !status_text.is_empty() {
        column![theme::text_body(status_text, ctx)]
    } else {
        column![]
    };

    container(
        column![
            info_section,
            progress_bars_container,
            idle_status_label,
            actions
        ]
        .spacing(15),
    )
    .width(Length::Fill)
    .height(Length::Shrink)
    .style(move |t| theme::container_style_transparent(&ctx.palette, t))
    .into()
}
