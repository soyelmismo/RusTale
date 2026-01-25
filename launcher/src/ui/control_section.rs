use crate::config::GameSettings;
use crate::game::LauncherStatus;
use crate::{Message, theme, util};
use iced::widget::{ProgressBar, Space, button, column, container, row, svg, text};
use iced::{Alignment, Color, Element, Font, Length};

pub fn view<'a>(
    status: &'a LauncherStatus,
    settings: &'a GameSettings,
    resolved_version: Option<i32>,
    download_progress: f32,
    sub_progress: f32,
    status_text: &'a str,
    localization: &'a crate::lang::Localization,
    is_disabled: bool,
) -> Element<'a, Message> {
    let play_button_text = match status {
        LauncherStatus::Playing => localization.t("launcher.stop"),
        LauncherStatus::Downloading => localization.t("launcher.status.downloading"),
        LauncherStatus::Checking => localization.t("launcher.status.checking"),
        LauncherStatus::Migrating => localization.t("launcher.status.migrating"),
        LauncherStatus::NeedsInstall => localization.t("launcher.play"),
        LauncherStatus::NeedsUpdate => localization.t("launcher.update"),
        _ => localization.t("launcher.play"),
    };

    let play_icon = match status {
        LauncherStatus::Playing => util::icons::STOP,
        _ => util::icons::PLAY,
    };

    let mut play_btn = button(
        container(
            row![
                svg(util::icons::icon(play_icon))
                    .width(20)
                    .height(20)
                    .style(theme::svg_accent),
                text(play_button_text).size(16).font(Font::MONOSPACE)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .style(match status {
        _ if is_disabled => theme::play_button_style,
        LauncherStatus::Playing => theme::play_button_style_active,
        LauncherStatus::NeedsUpdate => theme::update_button_style,
        _ => theme::play_button_style,
    })
    .width(Length::Fill)
    .height(50);

    if !is_disabled
        && !matches!(
            status,
            LauncherStatus::Downloading
                | LauncherStatus::Checking
                | LauncherStatus::Busy
                | LauncherStatus::Migrating
        )
    {
        play_btn = play_btn.on_press(Message::StartGame);
    }

    let mut settings_btn = button(
        container(
            row![
                svg(util::icons::icon(util::icons::SETTINGS))
                    .width(16)
                    .height(16)
                    .style(theme::svg_accent)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .width(50)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .style(theme::secondary_button_style)
    .height(50);

    if !is_disabled {
        settings_btn = settings_btn.on_press(Message::OpenSettings);
    }

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
                .color(Color::from_rgb(0.5, 0.5, 0.5)),
            Space::new().width(Length::Fill),
            text(&settings.channel).size(12).color(Color::WHITE),
        ]
        .width(Length::Fill),
        row![
            text(localization.t("launcher.info.version"))
                .size(10)
                .color(Color::from_rgb(0.5, 0.5, 0.5)),
            Space::new().width(Length::Fill),
            text(version_display).size(12).color(Color::WHITE),
        ]
        .width(Length::Fill),
    ]
    .spacing(5);

    let mut mods_btn = button(
        container(
            svg(util::icons::icon(util::icons::PUZZLE))
                .width(16)
                .height(16)
                .style(theme::svg_accent),
        )
        .width(50)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .style(theme::secondary_button_style)
    .height(50);

    if !is_disabled {
        mods_btn = mods_btn.on_press(Message::Mods(
            crate::ui::mods_modal::ModsMessage::RefreshLocal,
        ));
    }

    let actions = row![play_btn, settings_btn, mods_btn].spacing(10);

    column![
        info_section,
        if *status == LauncherStatus::Downloading || *status == LauncherStatus::Migrating {
            column![
                column![
                    row![
                        text(if *status == LauncherStatus::Migrating {
                            "Moving files..."
                        } else {
                            localization.t("launcher.status.general")
                        })
                        .size(11)
                        .color(Color::from_rgb(0.7, 0.7, 0.7)),
                        Space::new().width(Length::Fill),
                        text(format!("{:.0}%", download_progress))
                            .size(11)
                            .color(Color::WHITE),
                    ],
                    container(
                        ProgressBar::new(0.0..=100.0, download_progress)
                            .style(theme::orange_bar_style)
                    )
                    .height(6)
                    .width(Length::Fill),
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
                                    .color(Color::from_rgb(0.5, 0.5, 0.5)),
                                Space::new().width(Length::Fill),
                                text(format!("{:.0}%", sub_progress))
                                    .size(10)
                                    .color(Color::from_rgb(0.8, 0.8, 0.8)),
                            ],
                            container(
                                ProgressBar::new(0.0..=100.0, sub_progress)
                                    .style(theme::sub_bar_style)
                            )
                            .height(3)
                            .width(Length::Fill),
                        ]
                        .spacing(2),
                    )
                }
            ]
            .spacing(10)
        } else {
            column![]
        },
        text(status_text).size(14).color(Color::WHITE),
        actions
    ]
    .spacing(15)
    .into()
}
