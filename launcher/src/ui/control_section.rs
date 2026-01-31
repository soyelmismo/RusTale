use crate::config::GameSettings;
use crate::game::LauncherStatus;
use crate::{Message, theme, util};
use iced::widget::{ProgressBar, Space, button, column, container, row, svg};
use iced::{Alignment, Element, Length};

pub fn view<'a>(
    status: &'a LauncherStatus,
    settings: &'a GameSettings,
    resolved_version: Option<i32>,
    download_progress: f32,
    sub_progress: f32,
    status_text: &'a str,
    localization: &'a crate::lang::Localization,
    is_disabled: bool,
    server_patch_progress: f32,
    show_server_patch_progress: bool,
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
        _ => localization.t("launcher.play"),
    };

    let play_icon = match status {
        LauncherStatus::Playing => util::icons::STOP,
        LauncherStatus::Downloading | LauncherStatus::Migrating => util::icons::X,
        _ => util::icons::PLAY,
    };

    let is_long_text = play_button_text.len() > 9;
    let (icon_size, spacing_val) = if is_long_text {
        (16.0, 6)
    } else {
        (20.0, 10)
    };

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
                    svg(util::icons::icon(play_icon))
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
            theme::blocked_button_style(&palette, t, bs) // Nuevo estilo para estados intermedios
        },
        
        LauncherStatus::Playing => {
            theme::play_button_style_active(&palette, t, bs) // Ahora es dinámico-danger
        },
        
        LauncherStatus::Downloading | LauncherStatus::Migrating => {
            theme::danger_button_style(&palette, t, bs)
        },
        
        LauncherStatus::NeedsUpdate => {
            theme::update_button_style(&palette, t, bs) // Ahora es dinámico-accent
        },
        
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
            svg(util::icons::icon(util::icons::SETTINGS))
                .width(18)
                .height(18)
                .style(move |t, s| theme::svg_accent(&palette, t, s))
                .opacity(ctx.palette.text_primary.a)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |t| theme::container_style_transparent(&palette, t)),
    )
    .style(move |t, s| theme::secondary_button_style(&palette, t, s))
    .width(Length::Fill)
    .height(Length::Fill);
    let settings_btn = if !is_disabled {
        settings_btn.on_press(Message::OpenSettings)
    } else {
        settings_btn
    };

    let mods_btn = button(
        container(
            svg(util::icons::icon(util::icons::PUZZLE))
                .width(18)
                .height(18)
                .style(move |t, s| theme::svg_accent(&palette, t, s))
                .opacity(ctx.palette.text_primary.a)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |t| theme::container_style_transparent(&palette, t)),
    )
    .style(move |t, s| theme::secondary_button_style(&palette, t, s))
    .width(Length::Fill)
    .height(Length::Fill);
    let mods_btn = if !is_disabled {
        mods_btn.on_press(Message::Mods(
            crate::ui::mods_modal::ModsMessage::RefreshLocal,
        ))
    } else {
        mods_btn
    };

    let actions = row![
        theme::magic_button(play_btn.into(), ctx),
        column![
            theme::magic_button(settings_btn.into(), ctx),
            theme::magic_button(mods_btn.into(), ctx)
        ]
        .width(45)
        .spacing(8)
    ]
    .spacing(10)
    .height(90);

    // Aplicar LSD al status text
    let status_text_widget = theme::text_body(status_text, ctx);

    container(
        column![
            info_section,
            if *status == LauncherStatus::Downloading || *status == LauncherStatus::Migrating {
                column![
                    row![
                        theme::text_micro(localization.t("launcher.status.step"), ctx),
                        Space::new().width(Length::Fill),
                        theme::text_micro(format!("{:.0}%", download_progress), ctx)
                    ],
                    container(
                        ProgressBar::new(0.0..=100.0, download_progress)
                            .style(move |t| theme::accent_bar_style(&palette, t))
                    )
                    .height(4)
                    .width(Length::Fill)
                    .style(move |t| theme::container_style_transparent(&palette, t))
                ]
                .spacing(5)
            } else {
                column![]
            },
            if *status == LauncherStatus::Downloading || *status == LauncherStatus::Migrating {
                column![
                    row![
                        theme::text_micro(localization.t("launcher.status.step"), ctx),
                        Space::new().width(Length::Fill),
                        theme::text_micro(format!("{:.0}%", sub_progress), ctx)
                    ],
                    container(
                        ProgressBar::new(0.0..=100.0, sub_progress)
                            .style(move |t| theme::sub_bar_style(&palette, t))
                    )
                    .height(3)
                    .width(Length::Fill)
                    .style(move |t| theme::container_style_transparent(&palette, t))
                ]
                .spacing(2)
            } else if show_server_patch_progress {
                column![
                    row![
                        theme::text_micro("Patching Server...", ctx),
                        Space::new().width(Length::Fill),
                        theme::text_micro(format!("{:.0}%", server_patch_progress), ctx)
                    ],
                    container(
                        ProgressBar::new(0.0..=100.0, server_patch_progress)
                            .style(move |t| theme::accent_bar_style(&palette, t))
                    )
                    .height(4)
                    .width(Length::Fill)
                    .style(move |t| theme::container_style_transparent(&palette, t))
                ]
                .spacing(5)
            } else {
                column![]
            },
            status_text_widget,
            actions
        ]
        .spacing(15)
    )
    .width(Length::Fill)
    .height(Length::Shrink)
    .style(move |t| theme::container_style_transparent(&ctx.palette, t))
    .into()
}
