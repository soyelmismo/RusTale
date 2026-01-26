use iced::overlay::menu;
use iced::widget::{
    Space, button, checkbox, column, container, pick_list, progress_bar, row as iced_row,
    scrollable, slider, text as iced_text, text_input,
};
use iced::{
    Background, Border, Color, Element, Length, Rectangle, Renderer, Shadow, Size, Theme, Vector,
};

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::event::Event;

// --- CONSTANTS ---
pub const ACCENT_GREEN: Color = Color::from_rgb(0.2, 0.8, 0.2);
pub const STANDARD_PADDING: f32 = 20.0;
pub const STANDARD_SPACING: u32 = 15;

#[derive(Debug, Clone, Copy)]
pub struct UIContext {
    pub palette: Palette,
    pub lsd_offset: (f32, f32),
    pub lsd_enabled: bool,
    pub time: f32,
}

// --- PALETTE SYSTEM ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub accent: Color,
    pub background: Color,
    pub surface: Color,
    pub surface_hover: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_on_accent: Color,
    pub success: Color,
    pub danger: Color,
}

pub fn background_tint_color(palette: &Palette) -> Color {
    if palette.background.r > 0.5 {
        Color {
            a: 0.75,
            ..Color::WHITE
        }
    } else if palette.background.r > 0.1 {
        Color {
            a: 0.4,
            ..palette.background
        }
    } else {
        Color {
            a: 0.2,
            ..palette.background
        }
    }
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
    use crate::config::BaseThemeMode;
    let mut accent = hex_to_color(&config.accent_hex).unwrap_or(Color::from_rgb8(255, 168, 69));

    // 0. Aplicar saturación ANTES de los cálculos de modo
    if (config.saturation - 1.0).abs() > 0.01 {
        let gray = accent.r * 0.299 + accent.g * 0.587 + accent.b * 0.114;
        accent.r = (gray + (accent.r - gray) * config.saturation).clamp(0.0, 1.0);
        accent.g = (gray + (accent.g - gray) * config.saturation).clamp(0.0, 1.0);
        accent.b = (gray + (accent.b - gray) * config.saturation).clamp(0.0, 1.0);
    }

    // 1. Configurar colores base según el modo
    let (bg, surf, t_p, t_s) = match config.base_mode {
        BaseThemeMode::Black => (
            Color::from_rgb(0.01, 0.01, 0.02),
            Color::from_rgb(0.06, 0.06, 0.09),
            Color::WHITE,
            Color::from_rgba(1.0, 1.0, 1.0, 0.5),
        ),
        BaseThemeMode::Grey => (
            Color::from_rgb(0.12, 0.12, 0.14),
            Color::from_rgb(0.18, 0.18, 0.22),
            Color::WHITE,
            Color::from_rgba(1.0, 1.0, 1.0, 0.5),
        ),
        BaseThemeMode::Light => (
            Color::from_rgb(0.96, 0.97, 0.99),
            Color::from_rgb(1.0, 1.0, 1.0),
            Color::from_rgb(0.1, 0.1, 0.2),
            Color::from_rgba(0.2, 0.2, 0.3, 0.6),
        ),
    };

    // 2. Aplicar contraste y ajuste de intensidad según el modo
    if config.base_mode == BaseThemeMode::Light {
        accent.r = (accent.r * 0.7 * config.contrast).clamp(0.0, 1.0);
        accent.g = (accent.g * 0.7 * config.contrast).clamp(0.0, 1.0);
        accent.b = (accent.b * 0.7 * config.contrast).clamp(0.0, 1.0);
    } else {
        accent.r = (accent.r * config.contrast).clamp(0.0, 1.0);
        accent.g = (accent.g * config.contrast).clamp(0.0, 1.0);
        accent.b = (accent.b * config.contrast).clamp(0.0, 1.0);
    }

    // 3. CALCULAR TEXTO SOBRE ACENTO
    let luminance = 0.299 * accent.r + 0.587 * accent.g + 0.114 * accent.b;
    let text_on_accent = if luminance > 0.5 {
        Color::BLACK
    } else {
        Color::WHITE
    };

    Palette {
        accent,
        background: bg,
        surface: surf,
        surface_hover: Color::from_rgba(accent.r, accent.g, accent.b, 0.08),
        text_primary: t_p,
        text_secondary: t_s,
        text_on_accent,
        success: Color::from_rgb(0.1, 0.7, 0.3),
        danger: Color::from_rgb(0.8, 0.2, 0.2),
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
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.2),
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

/// Envuelve cualquier contenido en la "columna estándar" de RusTale
/// con el espaciado y padding que te gusta del menú de juego.
pub fn standard_column<'a, Message>(
    items: Vec<Element<'a, Message, Theme, Renderer>>,
) -> iced::widget::Column<'a, Message, Theme, Renderer> {
    iced::widget::column(items)
        .spacing(STANDARD_SPACING)
        .width(Length::Fill)
}

/// Contenedor base para páginas dentro de modales (Settings, Mods, etc.)
pub fn page_container<'a, Message>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> iced::widget::Container<'a, Message, Theme, Renderer> {
    iced::widget::container(content)
        .padding(STANDARD_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
}

/// Crea el marco estandarizado para cualquier modal (Settings, Mods, etc.)
pub fn modal_shell<'a, Message>(
    title: &str,
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    footer: Option<Element<'a, Message, Theme, Renderer>>,
    on_close: Message,
    ctx: UIContext,
) -> iced::widget::Container<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
{
    let palette = ctx.palette;

    let header = iced_row![
        text_title(title, ctx),
        Space::new().width(Length::Fill),
        button(text(iced_text("✕").size(16), ctx))
            .on_press(on_close)
            .style(move |t, s| icon_button_style(&palette, t, s))
    ]
    .align_y(iced::Alignment::Center)
    .padding(20);

    let mut col = column![Element::from(header), content.into()];

    if let Some(f) = footer {
        col = col.push(
            container(f)
                .padding(15)
                .style(move |t| footer_style(&palette, t)),
        );
    }

    container(col).style(move |t| modal_container(&palette, t))
}

pub fn text_title<'a, M: 'a>(content: &str, ctx: UIContext) -> Element<'a, M, Theme, Renderer> {
    text(
        iced_text(content.to_string())
            .size(18)
            .color(ctx.palette.accent)
            .font(iced::font::Font::MONOSPACE),
        ctx,
    )
}

pub fn lsd_magic_text<'a, M: 'a>(
    label: &'a str,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    // 1. Definir valores estables
    let (color, vib_x, vib_y, alpha) = if ctx.lsd_enabled {
        let t = ctx.time * 1.0;
        let r = (t.sin() * 0.5 + 0.5).clamp(0.0, 1.0);
        let g = ((t + 2.09).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
        let b = ((t + 4.18).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
        (
            Color::from_rgb(r, g, b),
            (t * 1.5).cos() * 2.0,
            (t * 1.2).sin() * 2.0,
            0.3,
        )
    } else {
        (ctx.palette.text_primary, 0.0, 0.0, 0.0) // Estático y color normal
    };

    let content = iced::widget::stack![
        iced_text(label).size(14).color(Color { a: alpha, ..color }),
        iced_text(label)
            .size(14)
            .color(color)
            .font(iced::font::Font::MONOSPACE)
    ];

    // USAR SIEMPRE SmoothTranslate para que el tipo de widget no cambie al hacer hover
    Element::new(SmoothTranslate::new(content.into(), (vib_x, vib_y)))
}

pub fn text_body<'a, M: 'a>(content: &str, ctx: UIContext) -> Element<'a, M, Theme, Renderer> {
    text(
        iced_text(content.to_string())
            .size(14)
            .color(ctx.palette.text_primary),
        ctx,
    )
}

pub fn text_caption<'a, M: 'a>(content: &str, ctx: UIContext) -> Element<'a, M, Theme, Renderer> {
    text(
        iced_text(content.to_string())
            .size(11)
            .color(ctx.palette.text_secondary),
        ctx,
    )
}

/// Una fila estandarizada para listas (usada en Mods, Settings > Storage, etc.)
pub fn list_item_row<'a, Message: 'a>(
    label: Element<'a, Message, Theme, Renderer>,
    actions: Vec<Element<'a, Message, Theme, Renderer>>,
    ctx: UIContext,
) -> Element<'a, Message, Theme, Renderer> {
    container(
        iced_row![
            label,
            Space::new().width(Length::Fill),
            iced_row(actions)
                .spacing(10)
                .align_y(iced::Alignment::Center)
        ]
        .align_y(iced::Alignment::Center)
        .padding(12),
    )
    .style(move |t| card_style(&ctx.palette, t))
    .into()
}

pub fn labeled_input<'a, M: 'a + Clone>(
    label: &str,
    value: &str,
    on_change: impl Fn(String) -> M + 'static,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    column![
        text_caption(label, ctx),
        iced::widget::text_input("", value)
            .on_input(on_change)
            .padding(10)
            .style(move |t, s| text_input_style(&ctx.palette, t, s))
    ]
    .spacing(5)
    .into()
}

pub fn glass_container(palette: &Palette, _t: &Theme) -> container::Style {
    let alpha = if palette.background.r > 0.5 {
        0.6
    } else {
        0.85
    }; // Menos opaco en modo claro
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            palette.background.r,
            palette.background.g,
            palette.background.b,
            alpha,
        ))),
        border: Border {
            color: Color::from_rgba(
                palette.text_primary.r,
                palette.text_primary.g,
                palette.text_primary.b,
                0.1,
            ),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.2),
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
            text_color: palette.text_on_accent,
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
    let is_light = palette.background.r > 0.5;
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(palette.surface_hover)),
            text_color: palette.accent,
            border: Border {
                color: palette.accent,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        },
        _ => button::Style {
            background: Some(Background::Color(if is_light {
                Color::from_rgba(0.0, 0.0, 0.0, 0.03)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.05)
            })),
            text_color: palette.text_secondary,
            border: Border {
                color: Color::from_rgba(
                    palette.text_primary.r,
                    palette.text_primary.g,
                    palette.text_primary.b,
                    0.05,
                ),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
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
    let mut style = button::Style {
        background: Some(Background::Color(palette.accent)),
        text_color: palette.text_on_accent,
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    if status == button::Status::Hovered {
        style.shadow = Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.2),
            blur_radius: 5.0,
            ..Default::default()
        };
    }
    style
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
    let is_light = palette.background.r > 0.5;
    container::Style {
        background: Some(Background::Color(palette.background)),
        border: Border {
            color: Color::from_rgba(0.0, 0.0, 0.0, if is_light { 0.05 } else { 0.2 }),
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, if is_light { 0.1 } else { 0.5 }),
            offset: Vector::new(0.0, 10.0),
            blur_radius: 25.0,
        },
        text_color: Some(palette.text_primary),
        ..Default::default()
    }
}

pub fn sidebar_style(palette: &Palette, _t: &Theme) -> container::Style {
    let is_light = palette.background.r > 0.5;
    container::Style {
        background: Some(Background::Color(if is_light {
            Color::from_rgba(0.0, 0.0, 0.0, 0.03)
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.2)
        })),
        border: Border {
            color: Color::from_rgba(
                palette.text_primary.r,
                palette.text_primary.g,
                palette.text_primary.b,
                0.05,
            ),
            width: 0.0,
            radius: 0.0.into(),
        },
        text_color: Some(palette.text_primary),
        ..Default::default()
    }
}

pub fn footer_style(palette: &Palette, _t: &Theme) -> container::Style {
    let is_light = palette.background.r > 0.5;
    container::Style {
        background: Some(Background::Color(if is_light {
            Color::from_rgba(0.0, 0.0, 0.0, 0.05)
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.3)
        })),
        border: Border {
            color: Color::from_rgba(
                palette.text_primary.r,
                palette.text_primary.g,
                palette.text_primary.b,
                0.05,
            ),
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
    let is_light = palette.background.r > 0.5;
    text_input::Style {
        background: Background::Color(if is_light {
            Color::from_rgb(0.92, 0.93, 0.95)
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.4)
        }),
        border: Border {
            color: if matches!(status, text_input::Status::Focused { .. }) {
                palette.accent
            } else {
                Color::from_rgba(
                    palette.text_primary.r,
                    palette.text_primary.g,
                    palette.text_primary.b,
                    0.1,
                )
            },
            width: 1.0,
            radius: 6.0.into(),
        },
        icon: palette.text_secondary,
        placeholder: palette.text_secondary,
        value: palette.text_primary,
        selection: Color::from_rgba(palette.accent.r, palette.accent.g, palette.accent.b, 0.2),
    }
}

pub fn slider_style(palette: &Palette, _t: &Theme, _status: slider::Status) -> slider::Style {
    let is_light = palette.background.r > 0.5;
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(palette.accent),
                Background::Color(if is_light {
                    Color::from_rgba(0.0, 0.0, 0.0, 0.1)
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.1)
                }),
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
            border_width: 1.0,
            border_color: if is_light {
                Color::from_rgba(0.0, 0.0, 0.0, 0.1)
            } else {
                Color::TRANSPARENT
            },
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
    let is_light = palette.background.r > 0.5;
    let base = button::Style {
        text_color: palette.text_primary,
        background: Some(Background::Color(if is_light {
            Color::from_rgb(0.92, 0.93, 0.95)
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.3)
        })),
        border: Border {
            color: Color::from_rgba(
                palette.text_primary.r,
                palette.text_primary.g,
                palette.text_primary.b,
                0.1,
            ),
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
    let is_light = palette.background.r > 0.5;
    let base = pick_list::Style {
        text_color: palette.text_primary,
        placeholder_color: palette.text_secondary,
        handle_color: palette.text_secondary,
        background: Background::Color(if is_light {
            Color::from_rgb(0.92, 0.93, 0.95)
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.3)
        }),
        border: Border {
            color: Color::from_rgba(
                palette.text_primary.r,
                palette.text_primary.g,
                palette.text_primary.b,
                0.1,
            ),
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
        selected_text_color: palette.text_on_accent,
        selected_background: Background::Color(palette.accent),
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 10.0,
        },
    }
}

pub fn svg_muted(
    palette: &Palette,
    _t: &Theme,
    _status: iced::widget::svg::Status,
) -> iced::widget::svg::Style {
    iced::widget::svg::Style {
        color: Some(if palette.background.r > 0.5 {
            Color::from_rgb(0.3, 0.3, 0.4)
        } else {
            Color::from_rgb(0.5, 0.5, 0.5)
        }),
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

// --- LSD MODE WIDGETS ---

pub struct SmoothTranslate<'a, Message> {
    content: Element<'a, Message, Theme, Renderer>,
    offset: Vector,
}

impl<'a, Message> SmoothTranslate<'a, Message> {
    pub fn new(content: Element<'a, Message, Theme, Renderer>, offset: (f32, f32)) -> Self {
        Self {
            content,
            offset: Vector::new(offset.0, offset.1),
        }
    }
}

impl<'a, Message> Widget<Message, Theme, Renderer> for SmoothTranslate<'a, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // IMPORTANTE: Pasamos un cursor falso (fuera de pantalla) al contenido hijo
        // para que nada dentro de la vibración crea que tiene el ratón encima.
        let fake_cursor = mouse::Cursor::Unavailable;

        renderer.with_translation(self.offset, |renderer| {
            self.content.as_widget().draw(
                tree,
                renderer,
                theme,
                style,
                layout,
                fake_cursor,
                viewport,
            );
        });
    }

    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<widget::Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut widget::Tree) {
        self.content.as_widget().diff(tree);
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        // El widget visual NO debe tener interacción propia,
        // dejamos que el mouse_area superior se encargue.
        mouse::Interaction::None
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }
}

fn get_seeded_disparity(offset: (f32, f32), seed: usize) -> (f32, f32) {
    let s = (seed as f32 * 0.618033).fract() * 6.28;

    let nx = offset.0 * s.cos() - offset.1 * s.sin();
    let ny = offset.0 * s.sin() + offset.1 * s.cos();

    (nx * 0.8, ny * 0.8)
}

pub fn text<'a, M: 'a>(
    content: iced::widget::Text<'a, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    let element: Element<'a, M, Theme, Renderer> = content.into();
    if ctx.lsd_enabled {
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 1);
        Element::new(SmoothTranslate::new(element, (vx, vy)))
    } else {
        element
    }
}

pub fn svg<'a, M: 'a>(content: impl Into<Element<'a, M>>, ctx: UIContext) -> Element<'a, M> {
    let element = content.into();
    if ctx.lsd_enabled {
        let (vx, vy) = get_seeded_disparity((ctx.lsd_offset.0 + 0.5, ctx.lsd_offset.1 + 0.5), 1);
        Element::new(SmoothTranslate::new(element, (vx * 1.3, vy * 1.3)))
    } else {
        element
    }
}
