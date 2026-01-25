use crate::Message;
use crate::config::ProfilesConfig;
use crate::theme;
use crate::util;
use iced::widget::{Space, button, column, container, row, svg, text, text_input, tooltip};
use iced::{Alignment, Color, Element, Length};

pub fn view<'a>(
    profiles: &'a ProfilesConfig,
    editing_profile: &'a Option<(Option<String>, String)>,
    editing_uuid: &'a Option<(String, String)>,
    dropdown_open: bool,
    localization: &'a crate::lang::Localization,
) -> Element<'a, Message> {
    let active_profile = profiles.get_active_profile();
    let profile_name = active_profile
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| localization.t("profile.select").to_string());

    let mut dropdown_content = column![].spacing(2).padding(5);

    for profile in &profiles.profiles {
        let is_selected = active_profile
            .as_ref()
            .map(|p| p.id == profile.id)
            .unwrap_or(false);

        let is_being_edited_name = if let Some((Some(id), _)) = editing_profile {
            id == &profile.id
        } else {
            false
        };

        let is_being_edited_uuid = if let Some((id, _)) = editing_uuid {
            id == &profile.id
        } else {
            false
        };

        if is_being_edited_name {
            let (_, current_name) = editing_profile.as_ref().unwrap();
            dropdown_content = dropdown_content.push(
                container(
                    row![
                        text_input(localization.t("profile.name_placeholder"), current_name)
                            .on_input(Message::ProfileNameChanged)
                            .on_submit(Message::SaveProfileName)
                            .style(theme::text_input_style)
                            .padding(5)
                            .width(Length::Fill),
                        button(
                            svg(util::icons::icon(util::icons::CHECK))
                                .width(12)
                                .height(12)
                                .style(theme::svg_accent),
                        )
                        .on_press(Message::SaveProfileName)
                        .style(theme::icon_button_style)
                        .padding(4),
                        button(
                            svg(util::icons::icon(util::icons::X))
                                .width(12)
                                .height(12)
                                .style(theme::svg_accent),
                        )
                        .on_press(Message::CancelProfileEdit)
                        .style(theme::icon_button_style)
                        .padding(4),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(5),
            );
        } else if is_being_edited_uuid {
            let (_, current_uuid_val) = editing_uuid.as_ref().unwrap();
            dropdown_content = dropdown_content.push(
                container(
                    row![
                        text_input("UUID...", current_uuid_val)
                            .on_input(Message::ProfileUUIDChanged)
                            .on_submit(Message::SaveProfileUUID)
                            .style(theme::text_input_style)
                            .padding(5)
                            .width(Length::Fill),
                        // Copy Button
                        tooltip(
                            button(
                                svg(util::icons::icon(util::icons::COPY))
                                    .width(12)
                                    .height(12)
                                    .style(theme::svg_accent),
                            )
                            .on_press(Message::CopyUUID(current_uuid_val.clone()))
                            .style(theme::icon_button_style)
                            .padding(4),
                            "Copy UUID",
                            tooltip::Position::Top,
                        )
                        .style(theme::container_style_transparent),
                        // Generate Random UUID Button
                        tooltip(
                            button(
                                svg(util::icons::icon(util::icons::DICE))
                                    .width(12)
                                    .height(12)
                                    .style(theme::svg_accent),
                            )
                            .on_press(Message::GenerateRandomUUID)
                            .style(theme::icon_button_style)
                            .padding(4),
                            "Generate Random UUID",
                            tooltip::Position::Top,
                        )
                        .style(theme::container_style_transparent),
                        // Save Button
                        button(
                            svg(util::icons::icon(util::icons::CHECK))
                                .width(12)
                                .height(12)
                                .style(theme::svg_accent),
                        )
                        .on_press(Message::SaveProfileUUID)
                        .style(theme::icon_button_style)
                        .padding(4),
                        // Cancel Button
                        button(
                            svg(util::icons::icon(util::icons::X))
                                .width(12)
                                .height(12)
                                .style(theme::svg_accent),
                        )
                        .on_press(Message::CancelProfileUUIDEdit)
                        .style(theme::icon_button_style)
                        .padding(4),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(5)
                .style(theme::active_tab_container_style),
            );
        } else {
            dropdown_content = dropdown_content.push(
                button(
                    row![
                        text(&profile.name).size(13).width(Length::Fill),
                        // UUID Button
                        tooltip(
                            button(
                                svg(util::icons::icon(util::icons::PERSON))
                                    .width(12)
                                    .height(12)
                                    .style(theme::svg_accent),
                            )
                            .on_press(Message::EditProfileUUID(profile.id.clone()))
                            .style(theme::icon_button_style)
                            .padding(4),
                            "View/Edit UUID",
                            tooltip::Position::Top,
                        )
                        .style(theme::container_style_transparent),
                        button(
                            svg(util::icons::icon(util::icons::EDIT))
                                .width(12)
                                .height(12)
                                .style(theme::svg_accent),
                        )
                        .on_press(Message::EditProfile(profile.id.clone()))
                        .style(theme::icon_button_style)
                        .padding(4),
                        button(
                            svg(util::icons::icon(util::icons::TRASH))
                                .width(12)
                                .height(12)
                                .style(theme::svg_accent),
                        )
                        .on_press(Message::DeleteProfile(profile.id.clone()))
                        .style(theme::icon_button_style)
                        .padding(4),
                    ]
                    .align_y(Alignment::Center)
                    .spacing(8),
                )
                .on_press(Message::ProfileSelected(profile.clone()))
                .width(Length::Fill)
                .style(if is_selected {
                    theme::active_tab_style
                } else {
                    theme::ghost_button_style
                })
                .padding(8),
            );
        }
    }

    // Row for adding new profile
    if let Some((None, current_name)) = editing_profile {
        dropdown_content = dropdown_content.push(
            container(
                row![
                    text_input(localization.t("profile.new_name_placeholder"), current_name)
                        .on_input(Message::ProfileNameChanged)
                        .on_submit(Message::SaveProfileName)
                        .style(theme::text_input_style)
                        .padding(5)
                        .width(Length::Fill),
                    button(
                        svg(util::icons::icon(util::icons::CHECK))
                            .width(12)
                            .height(12)
                            .style(theme::svg_accent),
                    )
                    .on_press(Message::SaveProfileName)
                    .style(theme::icon_button_style)
                    .padding(4),
                    button(
                        svg(util::icons::icon(util::icons::X))
                            .width(12)
                            .height(12)
                            .style(theme::svg_accent),
                    )
                    .on_press(Message::CancelProfileEdit)
                    .style(theme::icon_button_style)
                    .padding(4),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(5),
        );
    }

    dropdown_content = dropdown_content.push(Space::new().height(5));
    dropdown_content = dropdown_content.push(
        button(
            row![
                svg(util::icons::icon(util::icons::PLUS))
                    .width(14)
                    .height(14),
                text(localization.t("profile.add")).size(13)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .on_press(Message::AddProfile)
        .width(Length::Fill)
        .style(theme::primary_button_style)
        .padding(10),
    );

    column![
        text(localization.t("profile.title"))
            .size(10)
            .color(Color::from_rgb(0.5, 0.5, 0.5)),
        button(
            row![
                text(profile_name).size(14).width(Length::Fill),
                svg(util::icons::icon(if dropdown_open {
                    util::icons::X
                } else {
                    util::icons::CHEVRON_RIGHT
                }))
                .width(12)
                .height(12)
                .style(theme::svg_muted)
            ]
            .align_y(Alignment::Center)
        )
        .on_press(Message::ToggleProfileDropdown)
        .width(Length::Fill)
        .style(theme::dropdown_trigger_style)
        .padding(10),
        if dropdown_open {
            container(dropdown_content)
                .width(Length::Fill)
                .style(theme::popup_container)
        } else {
            container(Space::new())
        }
    ]
    .spacing(5)
    .into()
}
