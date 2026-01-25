use crate::config::GameSettings;
use crate::game::LauncherStatus;
use crate::{Message, theme, util};
use iced::widget::{ProgressBar, Space, button, column, container, row, svg, text};
use iced::{Alignment, Element, Font, Length};

pub fn view<'a>(
    status: &'a LauncherStatus,
    settings: &'a GameSettings,
    resolved_version: Option<i32>,
    download_progress: f32,
    sub_progress: f32,
    status_text: &'a str,
    localization: &'a crate::lang::Localization,
    is_disabled: bool,
    palette: &'a theme::Palette,
) -> Element<'a, Message> {
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
    let (font_size, icon_size, spacing_val) = if is_long_text {
        (13, 16.0, 6)
    } else {
        (16, 20.0, 10)
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
            text(localization.t("launcher.info.channel"))
                .size(10)
                .color(palette.text_secondary),
            Space::new().width(Length::Fill),
            text(&settings.channel).size(12).color(palette.text_primary)
        ]
        .width(Length::Fill),
        row![
            text(localization.t("launcher.info.version"))
                .size(10)
                .color(palette.text_secondary),
            Space::new().width(Length::Fill),
            text(version_display).size(12).color(palette.text_primary)
        ]
        .width(Length::Fill),
    ]
    .spacing(5);

    let mut play_btn = button(
        container(
            row![
                svg(util::icons::icon(play_icon))
                    .width(icon_size)
                    .height(icon_size)
                    .style(move |t, s| theme::svg_accent(palette, t, s)),
                text(play_button_text)
                    .size(font_size + 4)
                    .font(Font::MONOSPACE)
                    .align_y(iced::alignment::Vertical::Center)
            ]
            .spacing(spacing_val)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .style(move |t, bs| match status {
        _ if is_disabled => theme::play_button_style(palette, t, bs),
        LauncherStatus::Playing => theme::play_button_style_active(palette, t, bs),
        LauncherStatus::Downloading | LauncherStatus::Migrating => {
            theme::danger_button_style(palette, t, bs)
        }
        LauncherStatus::NeedsUpdate => theme::update_button_style(palette, t, bs),
        _ => theme::play_button_style(palette, t, bs),
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
                .style(move |t, s| theme::svg_accent(palette, t, s)),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .style(move |t, s| theme::secondary_button_style(palette, t, s))
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
                .style(move |t, s| theme::svg_accent(palette, t, s)),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .style(move |t, s| theme::secondary_button_style(palette, t, s))
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
        play_btn,
        column![settings_btn, mods_btn].width(45).spacing(8)
    ]
    .spacing(10)
    .height(90);

    column![
        info_section,
        if *status == LauncherStatus::Downloading || *status == LauncherStatus::Migrating {
            column![
                column![
                    row![
                        text(if *status == LauncherStatus::Migrating {
                            localization.t("launcher.status.migrating")
                        } else {
                            localization.t("launcher.status.general")
                        })
                        .size(11)
                        .color(palette.text_secondary),
                        Space::new().width(Length::Fill),
                        text(format!("{:.0}%", download_progress))
                            .size(11)
                            .color(palette.text_primary)
                    ],
                    container(
                        ProgressBar::new(0.0..=100.0, download_progress)
                            .style(move |t| theme::orange_bar_style(palette, t))
                    )
                    .height(6)
                    .width(Length::Fill)
                ]
                .spacing(3),
                if *status == LauncherStatus::Migrating {
                    Element::from(column![])
                } else {
                    Element::from(
                        column![
                            row![
                                text(localization.t("launcher.status.step"))
                                    .size(10)
                                    .color(palette.text_secondary),
                                Space::new().width(Length::Fill),
                                text(format!("{:.0}%", sub_progress))
                                    .size(10)
                                    .color(palette.text_secondary)
                            ],
                            container(
                                ProgressBar::new(0.0..=100.0, sub_progress)
                                    .style(move |t| theme::sub_bar_style(palette, t))
                            )
                            .height(3)
                            .width(Length::Fill)
                        ]
                        .spacing(2),
                    )
                }
            ]
            .spacing(10)
        } else {
            column![]
        },
        text(status_text).size(14).color(palette.text_primary),
        actions
    ]
    .spacing(15)
    .into()
}
