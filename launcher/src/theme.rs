use iced::overlay::menu;
use iced::widget::{
    button, checkbox, container, pick_list, progress_bar, scrollable, slider, svg, text_input,
};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

// Colors
pub const ACCENT_ORANGE: Color = Color::from_rgb(1.0, 0.658, 0.27);
pub const ACCENT_GREEN: Color = Color::from_rgb(0.2, 0.8, 0.2);
pub const PANEL_BG: Color = Color::from_rgba(0.035, 0.035, 0.035, 0.65);
pub const SOLID_BG: Color = Color::from_rgb(0.08, 0.08, 0.08);
pub const HOVER_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.1);

pub fn card_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL_BG)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 10.0,
        },
        text_color: Some(Color::WHITE),
        ..Default::default()
    }
}

pub fn danger_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    if status == button::Status::Pressed {
        return button::Style {
            background: Some(Background::Color(Color::from_rgb(0.8, 0.2, 0.2))),
            border: Border {
                color: Color::from_rgb(1.0, 0.4, 0.4),
                width: 1.0,
                radius: 8.0.into(),
            },
            text_color: Color::WHITE,
            ..Default::default()
        };
    }
    button::Style {
        background: Some(Background::Color(Color::from_rgb(0.8, 0.2, 0.2))),
        border: Border {
            color: Color::from_rgb(1.0, 0.4, 0.4),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Color::WHITE,
        ..Default::default()
    }
}

pub fn glass_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL_BG)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 10.0,
        },
        text_color: Some(Color::WHITE),
        ..Default::default()
    }
}

pub fn news_panel_style(theme: &Theme) -> container::Style {
    glass_container(theme)
}

pub fn popup_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SOLID_BG)),
        border: Border {
            color: Color::from_rgba(1.0, 0.658, 0.27, 0.2),
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, -2.0),
            blur_radius: 15.0,
        },
        text_color: Some(Color::WHITE),
        ..Default::default()
    }
}

// Small Action Buttons
pub fn icon_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            text_color: ACCENT_ORANGE,
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1))),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        _ => button::Style {
            background: None,
            text_color: Color::from_rgb(0.5, 0.5, 0.5),
            ..Default::default()
        },
    }
}

// Play Button
pub fn play_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(Color::from_rgba(
            0.035, 0.035, 0.035, 0.8,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 0.658, 0.27, 0.3),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Color::WHITE,
        ..button::Style::default()
    };

    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(Color::from_rgba(0.05, 0.05, 0.05, 0.9))),
            border: Border {
                color: ACCENT_ORANGE,
                width: 1.0,
                radius: 8.0.into(),
            },
            text_color: ACCENT_ORANGE,
            shadow: Shadow {
                color: Color::from_rgba(1.0, 0.658, 0.27, 0.2),
                blur_radius: 10.0,
                ..Default::default()
            },
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(ACCENT_ORANGE)),
            text_color: Color::BLACK,
            ..base
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(Color::from_rgba(0.1, 0.1, 0.1, 0.5))),
            text_color: Color::from_rgb(0.5, 0.5, 0.5),
            ..button::Style::default()
        },
        _ => base,
    }
}

// Active Play Button (Stop Mode) - Using a reddish tint
pub fn play_button_style_active(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(Color::from_rgba(0.5, 0.1, 0.1, 0.5))),
        border: Border {
            color: Color::from_rgb(0.8, 0.3, 0.3),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Color::WHITE,
        ..button::Style::default()
    };

    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(Color::from_rgba(0.7, 0.2, 0.2, 0.6))),
            border: Border {
                color: Color::from_rgb(1.0, 0.4, 0.4),
                ..base.border
            },
            text_color: Color::WHITE,
            shadow: Shadow {
                color: Color::from_rgba(0.8, 0.2, 0.2, 0.3),
                blur_radius: 10.0,
                ..Default::default()
            },
            ..base
        },
        _ => base,
    }
}

pub fn secondary_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1))),
            border: Border {
                color: ACCENT_ORANGE,
                width: 1.0,
                radius: 8.0.into(),
            },
            text_color: ACCENT_ORANGE,
            ..button::Style::default()
        },
        _ => button::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.3))),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
                width: 1.0,
                radius: 1.0.into(),
            },
            text_color: Color::from_rgb(0.7, 0.7, 0.7),
            ..button::Style::default()
        },
    }
}

pub fn ghost_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(HOVER_BG)),
            text_color: Color::WHITE,
            border: Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..button::Style::default()
        },
        _ => button::Style {
            background: None,
            text_color: Color::from_rgb(0.8, 0.8, 0.8),
            ..button::Style::default()
        },
    }
}

pub fn orange_bar_style(_theme: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1)),
        bar: Background::Color(ACCENT_ORANGE),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
    }
}

pub fn primary_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(ACCENT_ORANGE)),
        text_color: Color::BLACK,
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(Color::from_rgb(1.0, 0.7, 0.4))),
            ..base
        },
        _ => base,
    }
}

pub fn active_tab_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 0.658, 0.27, 0.15))),
        border: Border {
            color: ACCENT_ORANGE,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: ACCENT_ORANGE,
        ..Default::default()
    }
}

pub fn modal_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.08, 0.08, 0.08))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, 10.0),
            blur_radius: 30.0,
        },
        text_color: Some(Color::WHITE),
        ..Default::default()
    }
}

pub fn sidebar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.2))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.05),
            width: 0.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn footer_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.3))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.05),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn text_input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let base = text_input::Style {
        background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.3)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 6.0.into(),
        },
        icon: Color::WHITE,
        placeholder: Color::from_rgb(0.5, 0.5, 0.5),
        value: Color::WHITE,
        selection: Color::from_rgba(1.0, 0.658, 0.27, 0.3),
    };

    match status {
        text_input::Status::Focused { .. } => text_input::Style {
            border: Border {
                color: ACCENT_ORANGE,
                ..base.border
            },
            ..base
        },
        text_input::Status::Hovered => text_input::Style {
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
                ..base.border
            },
            ..base
        },
        _ => base,
    }
}

pub fn slider_style(_theme: &Theme, _status: slider::Status) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(ACCENT_ORANGE),
                Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1)),
            ),
            width: 4.0,
            border: Border {
                radius: 2.0.into(),
                ..Default::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 8.0 },
            background: Background::Color(Color::WHITE),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    }
}

pub fn checkbox_style(_theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let base = checkbox::Style {
        background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.3)),
        icon_color: Color::BLACK,
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(Color::WHITE),
    };
    match status {
        checkbox::Status::Active { is_checked } | checkbox::Status::Hovered { is_checked } => {
            if is_checked {
                checkbox::Style {
                    background: Background::Color(ACCENT_ORANGE),
                    icon_color: Color::BLACK,
                    border: Border {
                        color: ACCENT_ORANGE,
                        ..base.border
                    },
                    ..base
                }
            } else {
                if matches!(status, checkbox::Status::Hovered { .. }) {
                    checkbox::Style {
                        border: Border {
                            color: Color::WHITE,
                            ..base.border
                        },
                        ..base
                    }
                } else {
                    base
                }
            }
        }
        _ => base,
    }
}

pub fn sub_bar_style(_theme: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(Color::TRANSPARENT),
        // Blanco con 40% de opacidad (0.4)
        bar: Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.4)),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
    }
}

pub fn update_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(Color::from_rgba(0.035, 0.05, 0.1, 0.8))),
        border: Border {
            color: Color::from_rgba(0.27, 0.65, 1.0, 0.3),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Color::WHITE,
        ..button::Style::default()
    };

    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(Color::from_rgba(0.05, 0.1, 0.2, 0.9))),
            border: Border {
                color: Color::from_rgb(0.4, 0.7, 1.0),
                width: 1.0,
                radius: 8.0.into(),
            },
            text_color: Color::from_rgb(0.4, 0.7, 1.0),
            shadow: Shadow {
                color: Color::from_rgba(0.2, 0.5, 1.0, 0.2),
                blur_radius: 10.0,
                ..Default::default()
            },
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(Color::from_rgb(0.27, 0.6, 1.0))),
            text_color: Color::BLACK,
            ..base
        },
        _ => base,
    }
}

pub fn dropdown_trigger_style(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        text_color: Color::WHITE,
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.3))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    };

    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            border: Border {
                color: ACCENT_ORANGE,
                ..base.border
            },
            ..base
        },
        _ => base,
    }
}

pub fn pick_list_style(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let base = pick_list::Style {
        text_color: Color::WHITE,
        placeholder_color: Color::from_rgb(0.5, 0.5, 0.5),
        handle_color: Color::from_rgb(0.7, 0.7, 0.7),
        background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.3)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 8.0.into(),
        },
    };

    match status {
        pick_list::Status::Hovered | pick_list::Status::Opened { .. } => pick_list::Style {
            border: Border {
                color: ACCENT_ORANGE,
                ..base.border
            },
            handle_color: ACCENT_ORANGE,
            ..base
        },
        _ => base,
    }
}

pub fn menu_style(_theme: &Theme) -> menu::Style {
    menu::Style {
        text_color: Color::WHITE,
        background: Background::Color(SOLID_BG),
        border: Border {
            color: Color::from_rgba(1.0, 0.658, 0.27, 0.2),
            width: 1.0,
            radius: 8.0.into(),
        },
        selected_text_color: Color::BLACK,
        selected_background: Background::Color(ACCENT_ORANGE),
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 10.0,
        },
    }
}

pub fn svg_muted(_theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(Color::from_rgb(0.5, 0.5, 0.5)),
    }
}

pub fn svg_accent(_theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(ACCENT_ORANGE),
    }
}

pub fn scrollable_style(_theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let is_hovered = matches!(status, scrollable::Status::Hovered { .. });

    let rail = scrollable::Rail {
        background: Some(Background::Color(Color::TRANSPARENT)),
        border: Border {
            radius: 10.0.into(),
            ..Default::default()
        },
        scroller: scrollable::Scroller {
            background: Background::Color(if is_hovered {
                ACCENT_ORANGE
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.1)
            }),
            border: Border {
                radius: 10.0.into(),
                ..Default::default()
            },
        },
    };

    scrollable::Style {
        container: container::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            ..Default::default()
        },
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5)),
            border: Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            shadow: Shadow::default(),
            icon: Color::WHITE,
        },
    }
}
