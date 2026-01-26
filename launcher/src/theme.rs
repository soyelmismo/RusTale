use iced::overlay::menu;
use iced::widget::{
    button, checkbox, container, pick_list, progress_bar, scrollable, slider, text_input,
};
use iced::{Background, Border, Color, Element, Padding, Shadow, Theme, Vector};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

// --- CONSTANTS ---
pub const ACCENT_GREEN: Color = Color::from_rgb(0.2, 0.8, 0.2);

// --- PALETTE SYSTEM ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub accent: Color,
    pub background: Color,
    pub surface: Color,
    pub surface_hover: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub success: Color,
    pub danger: Color,
}

pub fn hex_to_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::from_rgb8(r, g, b))
}

pub fn generate_palette(config: &crate::config::ThemeConfig) -> Palette {
    let mut base = hex_to_color(&config.accent_hex).unwrap_or(Color::from_rgb8(255, 168, 69));
    if (config.saturation - 1.0).abs() > 0.01 {
        let gray = base.r * 0.299 + base.g * 0.587 + base.b * 0.114;
        base.r = (gray + (base.r - gray) * config.saturation).clamp(0.0, 1.0);
        base.g = (gray + (base.g - gray) * config.saturation).clamp(0.0, 1.0);
        base.b = (gray + (base.b - gray) * config.saturation).clamp(0.0, 1.0);
    }
    let tint_factor = 0.03 * config.contrast;
    let bg_lum = 0.05 / config.contrast.max(0.1);
    let background = Color::from_rgb(
        (bg_lum + (base.r * tint_factor)).clamp(0.0, 1.0),
        (bg_lum + (base.g * tint_factor)).clamp(0.0, 1.0),
        (bg_lum + (base.b * tint_factor)).clamp(0.0, 1.0),
    );
    let surface = Color::from_rgb(
        (background.r + 0.05).clamp(0.0, 1.0),
        (background.g + 0.05).clamp(0.0, 1.0),
        (background.b + 0.05).clamp(0.0, 1.0),
    );
    let text_lum = (0.9 * config.contrast).clamp(0.0, 1.0);
    let text_primary = Color::from_rgb(text_lum, text_lum, text_lum);
    let text_secondary = Color::from_rgba(text_lum, text_lum, text_lum, 0.6);

    Palette {
        accent: base,
        background,
        surface,
        surface_hover: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
        text_primary,
        text_secondary,
        success: ACCENT_GREEN,
        danger: Color::from_rgb(0.9, 0.2, 0.2),
    }
}

pub fn card_style(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(palette.surface)),
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
        text_color: Some(palette.text_primary),
        ..Default::default()
    }
}

pub fn danger_button_style(
    palette: &Palette,
    _t: &Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(palette.danger)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Color::WHITE,
        ..Default::default()
    }
}

pub fn glass_container(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            palette.background.r,
            palette.background.g,
            palette.background.b,
            0.85,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 10.0,
        },
        text_color: Some(palette.text_primary),
        ..Default::default()
    }
}

pub fn news_panel_style(palette: &Palette, t: &Theme) -> container::Style {
    glass_container(&palette, t)
}

pub fn icon_button_style(palette: &Palette, _t: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            text_color: palette.accent,
            background: Some(Background::Color(palette.surface_hover)),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        _ => button::Style {
            background: None,
            text_color: palette.text_secondary,
            ..Default::default()
        },
    }
}

pub fn play_button_style(palette: &Palette, _t: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(Color::from_rgba(
            palette.background.r,
            palette.background.g,
            palette.background.b,
            0.8,
        ))),
        border: Border {
            color: Color::from_rgba(palette.accent.r, palette.accent.g, palette.accent.b, 0.3),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: palette.text_primary,
        ..button::Style::default()
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                palette.surface.r,
                palette.surface.g,
                palette.surface.b,
                0.9,
            ))),
            border: Border {
                color: palette.accent,
                width: 1.0,
                radius: 8.0.into(),
            },
            text_color: palette.accent,
            shadow: Shadow {
                color: Color::from_rgba(palette.accent.r, palette.accent.g, palette.accent.b, 0.2),
                blur_radius: 10.0,
                ..Default::default()
            },
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(palette.accent)),
            text_color: Color::BLACK,
            ..base
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(Color::from_rgba(0.1, 0.1, 0.1, 0.5))),
            text_color: palette.text_secondary,
            ..button::Style::default()
        },
        _ => base,
    }
}

pub fn play_button_style_active(
    palette: &Palette,
    _t: &Theme,
    status: button::Status,
) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(Color::from_rgba(0.5, 0.1, 0.1, 0.5))),
        border: Border {
            color: Color::from_rgb(0.8, 0.3, 0.3),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: palette.text_primary,
        ..button::Style::default()
    };
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(Color::from_rgba(0.7, 0.2, 0.2, 0.6))),
            border: Border {
                color: Color::from_rgb(1.0, 0.4, 0.4),
                ..base.border
            },
            text_color: palette.text_primary,
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

pub fn secondary_button_style(
    palette: &Palette,
    _t: &Theme,
    status: button::Status,
) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(palette.surface_hover)),
            border: Border {
                color: palette.accent,
                width: 1.0,
                radius: 8.0.into(),
            },
            text_color: palette.accent,
            ..button::Style::default()
        },
        _ => button::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.3))),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
                width: 1.0,
                radius: 8.0.into(),
            },
            text_color: palette.text_secondary,
            ..button::Style::default()
        },
    }
}

pub fn ghost_button_style(palette: &Palette, _t: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(palette.surface_hover)),
            text_color: palette.text_primary,
            border: Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            ..button::Style::default()
        },
        _ => button::Style {
            background: None,
            text_color: palette.text_secondary,
            border: Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            ..button::Style::default()
        },
    }
}

pub fn orange_bar_style(palette: &Palette, _t: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1)),
        bar: Background::Color(palette.accent),
        border: Border {
            radius: 16.0.into(),
            ..Default::default()
        },
    }
}

pub fn primary_button_style(
    palette: &Palette,
    _t: &Theme,
    status: button::Status,
) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(palette.accent)),
        text_color: Color::BLACK,
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(Color::from_rgb(
                (&palette.accent.r + 0.1).min(1.0),
                (&palette.accent.g + 0.1).min(1.0),
                (&palette.accent.b + 0.1).min(1.0),
            ))),
            ..base
        },
        _ => base,
    }
}

pub fn success_button_style(
    palette: &Palette,
    _t: &Theme,
    status: button::Status,
) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(palette.success)),
        text_color: Color::BLACK,
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(Color::from_rgb(
                (&palette.success.r + 0.1).min(1.0),
                (&palette.success.g + 0.1).min(1.0),
                (&palette.success.b + 0.1).min(1.0),
            ))),
            ..base
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                palette.success.r,
                palette.success.g,
                palette.success.b,
                0.5,
            ))),
            text_color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            ..base
        },
        _ => base,
    }
}

pub fn active_tab_style(palette: &Palette, _t: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::from_rgba(
            palette.accent.r,
            palette.accent.g,
            palette.accent.b,
            0.15,
        ))),
        border: Border {
            color: palette.accent,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: palette.accent,
        ..Default::default()
    }
}

pub fn active_tab_container_style(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            palette.accent.r,
            palette.accent.g,
            palette.accent.b,
            0.15,
        ))),
        border: Border {
            color: palette.accent,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(palette.accent),
        ..Default::default()
    }
}

pub fn modal_container(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(palette.background)),
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
        text_color: Some(palette.text_primary),
        ..Default::default()
    }
}

pub fn sidebar_style(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.2))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.05),
            width: 0.0,
            radius: 0.0.into(),
        },
        text_color: Some(palette.text_primary),
        ..Default::default()
    }
}

pub fn footer_style(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.3))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.05),
            width: 1.0,
            radius: 0.0.into(),
        },
        text_color: Some(palette.text_primary),
        ..Default::default()
    }
}

pub fn text_input_style(
    palette: &Palette,
    _t: &Theme,
    status: text_input::Status,
) -> text_input::Style {
    let base = text_input::Style {
        background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.3)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 6.0.into(),
        },
        icon: palette.text_secondary,
        placeholder: palette.text_secondary,
        value: palette.text_primary,
        selection: Color::from_rgba(palette.accent.r, palette.accent.g, palette.accent.b, 0.3),
    };
    match status {
        text_input::Status::Focused { .. } => text_input::Style {
            border: Border {
                color: palette.accent,
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

pub fn slider_style(palette: &Palette, _t: &Theme, _status: slider::Status) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(palette.accent),
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
            background: Background::Color(palette.text_primary),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    }
}

pub fn checkbox_style(palette: &Palette, _t: &Theme, status: checkbox::Status) -> checkbox::Style {
    let base = checkbox::Style {
        background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.3)),
        icon_color: Color::BLACK,
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(palette.text_primary),
    };
    match status {
        checkbox::Status::Active { is_checked } | checkbox::Status::Hovered { is_checked } => {
            if is_checked {
                checkbox::Style {
                    background: Background::Color(palette.accent),
                    icon_color: Color::BLACK,
                    border: Border {
                        color: palette.accent,
                        ..base.border
                    },
                    ..base
                }
            } else {
                if matches!(status, checkbox::Status::Hovered { .. }) {
                    checkbox::Style {
                        border: Border {
                            color: palette.text_primary,
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

pub fn sub_bar_style(_palette: &Palette, _t: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(Color::TRANSPARENT),
        bar: Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.4)),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
    }
}

pub fn update_button_style(
    &palette: &Palette,
    _t: &Theme,
    status: button::Status,
) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(Color::from_rgba(0.035, 0.05, 0.1, 0.8))),
        border: Border {
            color: Color::from_rgba(0.27, 0.65, 1.0, 0.3),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: palette.text_primary,
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

pub fn dropdown_trigger_style(
    palette: &Palette,
    _t: &Theme,
    status: button::Status,
) -> button::Style {
    let base = button::Style {
        text_color: palette.text_primary,
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
                color: palette.accent,
                ..base.border
            },
            ..base
        },
        _ => base,
    }
}

pub fn pick_list_style(
    palette: &Palette,
    _t: &Theme,
    status: pick_list::Status,
) -> pick_list::Style {
    let base = pick_list::Style {
        text_color: palette.text_primary,
        placeholder_color: palette.text_secondary,
        handle_color: palette.text_secondary,
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
                color: palette.accent,
                ..base.border
            },
            handle_color: palette.accent,
            ..base
        },
        _ => base,
    }
}

pub fn menu_style(palette: &Palette, _t: &Theme) -> menu::Style {
    menu::Style {
        text_color: palette.text_primary,
        background: Background::Color(palette.background),
        border: Border {
            color: Color::from_rgba(palette.accent.r, palette.accent.g, palette.accent.b, 0.2),
            width: 1.0,
            radius: 8.0.into(),
        },
        selected_text_color: Color::BLACK,
        selected_background: Background::Color(palette.accent),
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 10.0,
        },
    }
}

pub fn svg_muted(
    _palette: &Palette,
    _t: &Theme,
    _status: iced::widget::svg::Status,
) -> iced::widget::svg::Style {
    iced::widget::svg::Style {
        color: Some(Color::from_rgb(0.5, 0.5, 0.5)),
    }
}

pub fn svg_accent(
    palette: &Palette,
    _t: &Theme,
    _status: iced::widget::svg::Status,
) -> iced::widget::svg::Style {
    iced::widget::svg::Style {
        color: Some(palette.accent),
    }
}

pub fn scrollable_style(
    palette: &Palette,
    _t: &Theme,
    status: scrollable::Status,
) -> scrollable::Style {
    let is_hovered = matches!(status, scrollable::Status::Hovered { .. });
    let rail = scrollable::Rail {
        background: Some(Background::Color(Color::TRANSPARENT)),
        border: Border {
            radius: 10.0.into(),
            ..Default::default()
        },
        scroller: scrollable::Scroller {
            background: Background::Color(if is_hovered {
                palette.accent
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
            icon: palette.text_primary,
        },
    }
}

pub fn container_style_transparent(_palette: &Palette, _t: &Theme) -> container::Style {
    container::Style::default()
}

#[derive(Debug, Clone, Copy)]
pub struct UIContext {
    pub palette: Palette,
    pub lsd_offset: (f32, f32),
    pub lsd_enabled: bool,
}

// --- LSD MODE WIDGETS ---

// Guardamos el último offset procesado para saber cuándo empieza un nuevo frame
static LAST_FRAME_OFFSET: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));
static LSD_COMPONENT_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Obtiene un índice estable para el componente actual dentro del frame actual.
/// Esto garantiza que el Componente A siempre se mueva igual, eliminando el parpadeo.
fn get_stable_disparity(current_offset: (f32, f32)) -> (f32, f32) {
    let mut last_offset = LAST_FRAME_OFFSET.lock().unwrap();

    // Si el offset global ha cambiado, es un nuevo frame -> reseteamos el contador de componentes
    if (current_offset.0 - last_offset.0).abs() > 0.0001
        || (current_offset.1 - last_offset.1).abs() > 0.0001
    {
        *last_offset = current_offset;
        LSD_COMPONENT_COUNTER.store(0, Ordering::SeqCst);
    }

    let index = LSD_COMPONENT_COUNTER.fetch_add(1, Ordering::Relaxed);

    // Usamos el Ángulo Áureo (~2.399 rad) para que la dispersión sea máxima y no se repita
    let angle = (index as f32) * 2.399;
    let s = angle.sin();
    let c = angle.cos();

    // Rotación suave del vector de movimiento original
    // Damping: Multiplicamos por 0.8 para evitar que el objeto se "separe" demasiado de su origen (evita duplicidad)
    let nx = (current_offset.0 * c - current_offset.1 * s) * 0.8;
    let ny = (current_offset.0 * s + current_offset.1 * c) * 0.8;

    (nx, ny)
}

/// Reemplazo para text() que aplica el efecto LSD si está activo
pub fn text<'a, M: 'a>(content: impl Into<Element<'a, M>>, ctx: UIContext) -> Element<'a, M> {
    let element = content.into();
    if ctx.lsd_enabled {
        let (vx, vy) = get_stable_disparity(ctx.lsd_offset);
        container(element)
            .padding(Padding {
                top: vy,
                left: vx,
                ..Default::default()
            })
            .into()
    } else {
        element
    }
}

pub fn svg<'a, M: 'a>(content: impl Into<Element<'a, M>>, ctx: UIContext) -> Element<'a, M> {
    let element = content.into();
    if ctx.lsd_enabled {
        // Los iconos tienen una entropía ligeramente desplazada para desincronizarlos de su texto
        let (vx, vy) = get_stable_disparity((ctx.lsd_offset.0 + 0.5, ctx.lsd_offset.1 + 0.5));
        container(element)
            .padding(Padding {
                top: vy * 1.3,
                left: vx * 1.3,
                ..Default::default()
            })
            .into()
    } else {
        element
    }
}
