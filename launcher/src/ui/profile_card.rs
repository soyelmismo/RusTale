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
    ctx: theme::UIContext,
) -> Element<'a, Message> {
    let palette = ctx.palette;
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
                            .style(move |t, status| theme::text_input_style(&palette, t, status))
                            .padding(5)
                            .width(Length::Fill),
                        button(theme::svg(
                            svg(util::icons::icon(util::icons::CHECK))
                                .width(12)
                                .height(12)
                                .style(move |t, s| theme::svg_accent(&palette, t, s)),
                            ctx
                        ))
                        .on_press(Message::SaveProfileName)
                        .style(move |t, s| theme::icon_button_style(&palette, t, s))
                        .padding(4),
                        button(theme::svg(
                            svg(util::icons::icon(util::icons::X))
                                .width(12)
                                .height(12)
                                .style(move |t, s| theme::svg_accent(&palette, t, s)),
                            ctx
                        ))
                        .on_press(Message::CancelProfileEdit)
                        .style(move |t, s| theme::icon_button_style(&palette, t, s))
                        .padding(4),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(5),
            );
        } else if is_being_edited_uuid {
            let (_, curr_uuid) = editing_uuid.as_ref().unwrap();
            dropdown_content = dropdown_content.push(
                container(
                    row![
                        text_input("UUID...", curr_uuid)
                            .on_input(Message::ProfileUUIDChanged)
                            .on_submit(Message::SaveProfileUUID)
                            .style(move |t, s| theme::text_input_style(&palette, t, s))
                            .padding(5)
                            .width(Length::Fill),
                        tooltip(
                            button(theme::svg(
                                svg(util::icons::icon(util::icons::COPY))
                                    .width(12)
                                    .height(12)
                                    .style(move |t, s| theme::svg_accent(&palette, t, s)),
                                ctx
                            ))
                            .on_press(Message::CopyUUID(curr_uuid.clone()))
                            .style(move |t, s| theme::icon_button_style(&palette, t, s))
                            .padding(4),
                            "Copy UUID",
                            tooltip::Position::Top
                        )
                        .style(move |t| theme::container_style_transparent(&palette, t)),
                        tooltip(
                            button(theme::svg(
                                svg(util::icons::icon(util::icons::DICE))
                                    .width(12)
                                    .height(12)
                                    .style(move |t, s| theme::svg_accent(&palette, t, s)),
                                ctx
                            ))
                            .on_press(Message::GenerateRandomUUID)
                            .style(move |t, s| theme::icon_button_style(&palette, t, s))
                            .padding(4),
                            "Generate Random UUID",
                            tooltip::Position::Top
                        )
                        .style(move |t| theme::container_style_transparent(&palette, t)),
                        button(theme::svg(
                            svg(util::icons::icon(util::icons::CHECK))
                                .width(12)
                                .height(12)
                                .style(move |t, s| theme::svg_accent(&palette, t, s)),
                            ctx
                        ))
                        .on_press(Message::SaveProfileUUID)
                        .style(move |t, s| theme::icon_button_style(&palette, t, s))
                        .padding(4),
                        button(theme::svg(
                            svg(util::icons::icon(util::icons::X))
                                .width(12)
                                .height(12)
                                .style(move |t, s| theme::svg_accent(&palette, t, s)),
                            ctx
                        ))
                        .on_press(Message::CancelProfileUUIDEdit)
                        .style(move |t, s| theme::icon_button_style(&palette, t, s))
                        .padding(4),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(5)
                .style(move |t| theme::active_tab_container_style(&palette, t)),
            );
        } else {
            dropdown_content = dropdown_content.push(
                button(
                    row![
                        theme::text(text(&profile.name).size(13).width(Length::Fill), ctx),
                        tooltip(
                            button(theme::svg(
                                svg(util::icons::icon(util::icons::PERSON))
                                    .width(12)
                                    .height(12)
                                    .style(move |t, s| theme::svg_accent(&palette, t, s)),
                                ctx
                            ))
                            .on_press(Message::EditProfileUUID(profile.id.clone()))
                            .style(move |t, s| theme::icon_button_style(&palette, t, s))
                            .padding(4),
                            "View/Edit UUID",
                            tooltip::Position::Top
                        )
                        .style(move |t| theme::container_style_transparent(&palette, t)),
                        button(theme::svg(
                            svg(util::icons::icon(util::icons::EDIT))
                                .width(12)
                                .height(12)
                                .style(move |t, s| theme::svg_accent(&palette, t, s)),
                            ctx
                        ))
                        .on_press(Message::EditProfile(profile.id.clone()))
                        .style(move |t, s| theme::icon_button_style(&palette, t, s))
                        .padding(4),
                        button(theme::svg(
                            svg(util::icons::icon(util::icons::TRASH))
                                .width(12)
                                .height(12)
                                .style(move |t, s| theme::svg_accent(&palette, t, s)),
                            ctx
                        ))
                        .on_press(Message::DeleteProfile(profile.id.clone()))
                        .style(move |t, s| theme::icon_button_style(&palette, t, s))
                        .padding(4),
                    ]
                    .align_y(Alignment::Center)
                    .spacing(8),
                )
                .on_press(Message::ProfileSelected(profile.clone()))
                .width(Length::Fill)
                .style(move |t, s| {
                    if is_selected {
                        theme::active_tab_style(&palette, t, s)
                    } else {
                        theme::ghost_button_style(&palette, t, s)
                    }
                })
                .padding(8),
            );
        }
    }

    if let Some((None, curr_name)) = editing_profile {
        dropdown_content = dropdown_content.push(
            container(
                row![
                    text_input(localization.t("profile.new_name_placeholder"), curr_name)
                        .on_input(Message::ProfileNameChanged)
                        .on_submit(Message::SaveProfileName)
                        .style(move |t, s| theme::text_input_style(&palette, t, s))
                        .padding(5)
                        .width(Length::Fill),
                    button(theme::svg(
                        svg(util::icons::icon(util::icons::CHECK))
                            .width(12)
                            .height(12)
                            .style(move |t, s| theme::svg_accent(&palette, t, s)),
                        ctx
                    ))
                    .on_press(Message::SaveProfileName)
                    .style(move |t, s| theme::icon_button_style(&palette, t, s))
                    .padding(4),
                    button(theme::svg(
                        svg(util::icons::icon(util::icons::X))
                            .width(12)
                            .height(12)
                            .style(move |t, s| theme::svg_accent(&palette, t, s)),
                        ctx
                    ))
                    .on_press(Message::CancelProfileEdit)
                    .style(move |t, s| theme::icon_button_style(&palette, t, s))
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
                theme::svg(
                    svg(util::icons::icon(util::icons::PLUS))
                        .width(14)
                        .height(14),
                    ctx
                ),
                theme::text(text(localization.t("profile.add")).size(13), ctx)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .on_press(Message::AddProfile)
        .width(Length::Fill)
        .style(move |t, s| theme::primary_button_style(&palette, t, s))
        .padding(10),
    );

    column![
        theme::text(
            text(localization.t("profile.title"))
                .size(10)
                .color(Color::from_rgb(0.5, 0.5, 0.5)),
            ctx
        ),
        button(
            row![
                theme::text(text(profile_name).size(14).width(Length::Fill), ctx),
                theme::svg(
                    svg(util::icons::icon(if dropdown_open {
                        util::icons::X
                    } else {
                        util::icons::CHEVRON_RIGHT
                    }))
                    .width(12)
                    .height(12)
                    .style(move |t, s| theme::svg_muted(&palette, t, s)),
                    ctx
                )
            ]
            .align_y(Alignment::Center)
        )
        .on_press(Message::ToggleProfileDropdown)
        .width(Length::Fill)
        .style(move |t, s| theme::dropdown_trigger_style(&palette, t, s))
        .padding(10),
        if dropdown_open {
            container(dropdown_content)
                .width(Length::Fill)
                .style(move |t| theme::modal_container(&palette, t))
        } else {
            container(Space::new())
        }
    ]
    .spacing(5)
    .into()
}
