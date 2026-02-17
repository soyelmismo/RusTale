use crate::config::GameSettings;
use crate::game::{LauncherStatus, progress::ProgressPayload};
use crate::{Message, theme, util};
use iced::widget::{ProgressBar, Space, button, column, container, row, svg};
use iced::{Alignment, Element, Length};

pub fn view<'a>(
    status: &'a LauncherStatus,
    settings: &'a GameSettings,
    resolved_version: Option<i32>,
    progress_data: &'a Option<ProgressPayload>,
    localization: &'a crate::lang::Localization,
    is_disabled: bool,
    server_patch_progress: f32,
    show_server_patch_progress: bool,
    current_step: Option<usize>,
    total_steps: Option<usize>,
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
            theme::play_button_style_active(&palette, t, bs) // Ahora es dinamico-danger
        },
        
        LauncherStatus::Downloading | LauncherStatus::Migrating => {
            theme::play_button_style_active(&palette, t, bs)
        },
        
        LauncherStatus::NeedsUpdate => {
            theme::update_button_style(&palette, t, bs) // Ahora es dinamico-accent
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

    // Progress bars container using ProgressPayload
    let progress_bars_container = if matches!(status, LauncherStatus::Downloading | LauncherStatus::Migrating | LauncherStatus::Busy | LauncherStatus::Checking) {
        if let Some(data) = progress_data {
            // Translate status text dynamically using message_key with arguments
            let status_msg = if !data.message_args.is_empty() {
                let args: Vec<&str> = data.message_args.iter().map(|s| s.as_str()).collect();
                localization.ta(&data.message_key, &args)
            } else {
                localization.t(&data.message_key).to_string()
            };
            
            // Extract download stats if available
            let (total_bytes, downloaded_bytes, speed_str, eta_str) = if let Some(stats) = &data.stats {
                (stats.total_bytes, stats.downloaded_bytes, stats.speed_str.clone(), stats.eta_str.clone())
            } else {
                (0, 0, String::new(), None)
            };
            
            column![
                // Status and progress info row
                row![
                    theme::text_micro(status_msg, ctx),
                    Space::new().width(Length::Fill),
                    // Show step information if available
                    if let (Some(step), Some(total)) = (current_step, total_steps) {
                        theme::text_micro(format!("Step {}/{}", step, total), ctx)
                    } else {
                        theme::text_micro(format!("{:.0}%", data.global_progress * 100.0), ctx)
                    }
                ],
                
                // Primary progress bar (Global Progress)
                container(
                    ProgressBar::new(0.0..=1.0, data.global_progress)
                        .style(move |t| theme::accent_bar_style(&palette, t))
                ).height(6).width(Length::Fill),
                
                // Secondary progress bar (Step Progress) - only show if significantly different
                if (data.step_progress - data.global_progress).abs() > 0.05 {
                    Element::from(
                        container(
                            ProgressBar::new(0.0..=1.0, data.step_progress)
                                .style(move |t| theme::sub_bar_style(&palette, t))
                        ).height(2).width(Length::Fill)
                    )
                } else {
                    Element::from(Space::new().height(2))
                },
                
                // Download statistics row
                if total_bytes > 0 {
                    row![
                        theme::text_micro(format!("{}/{}", 
                            crate::game::patch_api::utils::format_bytes(downloaded_bytes), 
                            crate::game::patch_api::utils::format_bytes(total_bytes)
                        ), ctx),
                        Space::new().width(Length::Fill),
                        if !speed_str.is_empty() {
                            theme::text_micro(&speed_str, ctx)
                        } else {
                            theme::text_micro("", ctx)
                        },
                        if let Some(eta) = &eta_str {
                            theme::text_micro(format!("ETA: {}", eta), ctx)
                        } else {
                            theme::text_micro("", ctx)
                        }
                    ]
                } else if !speed_str.is_empty() {
                    row![
                        theme::text_micro(&speed_str, ctx),
                        Space::new().width(Length::Fill),
                        theme::text_micro("", ctx)
                    ]
                } else {
                    row![]
                }
            ].spacing(5)
        } else {
            // Indeterminate state (initializing)
            column![
               theme::text_micro(localization.t("launcher.status.initializing"), ctx),
               container(
                   ProgressBar::new(0.0..=1.0, 0.0)
                       .style(move |t| theme::accent_bar_style(&palette, t))
               ).height(4).width(Length::Fill)
            ].spacing(5)
        }
    } else {
        column![] // Empty when idle/ready
    };

    // Status text widget (using translated message from payload or fallback)
    let status_text_widget = if let Some(data) = progress_data {
        let status_msg = if !data.message_args.is_empty() {
            let args: Vec<&str> = data.message_args.iter().map(|s| s.as_str()).collect();
            localization.ta(&data.message_key, &args)
        } else {
            localization.t(&data.message_key).to_string()
        };
        theme::text_body(status_msg, ctx)
    } else {
        theme::text_body(localization.t("launcher.status.ready"), ctx)
    };

    // Server patch progress (separate system)
    let server_patch_container = if show_server_patch_progress {
        column![
            row![
                theme::text_micro("Patching Server...", ctx),
                Space::new().width(Length::Fill),
                theme::text_micro(format!("{:.0}%", server_patch_progress), ctx)
            ],
            container(
                ProgressBar::new(0.0..=1.0, server_patch_progress / 100.0)
                    .style(move |t| theme::accent_bar_style(&palette, t))
            )
            .height(4)
            .width(Length::Fill)
            .style(move |t| theme::container_style_transparent(&palette, t))
        ]
        .spacing(5)
    } else {
        column![]
    };

    container(
        column![
            info_section,
            progress_bars_container,
            server_patch_container,
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
