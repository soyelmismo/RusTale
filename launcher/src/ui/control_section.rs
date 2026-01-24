use crate::config::GameSettings;
use crate::game::LauncherStatus;
use crate::{Message, theme, util};
use iced::widget::{ProgressBar, Space, button, column, container, row, svg, text};
use iced::{Alignment, Color, Element, Font, Length};

pub fn view<'a>(
    status: &'a LauncherStatus,
    settings: &'a GameSettings,
    download_progress: f32,
    sub_progress: f32,
    status_text: &'a str,
    localization: &'a crate::lang::Localization,
) -> Element<'a, Message> {
    let play_button_text = match status {
        LauncherStatus::Playing => localization.t("launcher.stop"),
        LauncherStatus::Downloading => localization.t("launcher.status.downloading"),
        LauncherStatus::Checking => localization.t("launcher.status.checking"),
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
        LauncherStatus::Playing => theme::play_button_style_active,
        LauncherStatus::NeedsUpdate => theme::update_button_style,
        _ => theme::play_button_style,
    })
    .width(Length::Fill)
    .height(50);

    if !matches!(
        status,
        LauncherStatus::Downloading | LauncherStatus::Checking | LauncherStatus::Busy
    ) {
        play_btn = play_btn.on_press(Message::StartGame);
    }

    let settings_btn = button(
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
    .on_press(Message::OpenSettings)
    .style(theme::secondary_button_style)
    .height(50);

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
            text(settings.game_version.to_string())
                .size(12)
                .color(Color::WHITE),
        ]
        .width(Length::Fill),
    ]
    .spacing(5);

    let mods_btn = button(
        container(
            svg(util::icons::icon(util::icons::FOLDER)) // O usa un icono de CAJA/CUBO si añades uno
                .width(16)
                .height(16)
                .style(theme::svg_accent),
        )
        .width(50)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .on_press(Message::Mods(
        crate::ui::mods_modal::ModsMessage::RefreshLocal,
    ))
    .style(theme::secondary_button_style)
    .height(50);

    let actions = row![play_btn, settings_btn, mods_btn].spacing(10);

    column![
        info_section,
        if *status == LauncherStatus::Downloading {
            column![
                column![
                    row![
                        text(localization.t("launcher.status.general"))
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
                        ProgressBar::new(0.0..=100.0, sub_progress).style(theme::sub_bar_style)
                    )
                    .height(3)
                    .width(Length::Fill),
                ]
                .spacing(2)
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
