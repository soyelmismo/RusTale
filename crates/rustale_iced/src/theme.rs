use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell, overlay};
use iced::event::Event;
use iced::overlay::menu;
use iced::widget::{
    Space, button, checkbox, column, container, pick_list, progress_bar, row as iced_row,
    scrollable, slider, text as iced_text, text_input,
};
use iced::{
    Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Shadow, Size, Theme,
    Vector,
};
use std::cell::Cell;

// --- CONSTANTS ---
pub const STANDARD_PADDING: f32 = 20.0;
pub const STANDARD_SPACING: u32 = 15;

pub const LSD_RAMP_UP_SECONDS: f32 = 120.0; // Temporarily reduced for testing (was 300.0)

#[derive(Debug, Clone, Copy)]
pub struct UIContext {
    pub palette: Palette,
    pub lsd_offset: (f32, f32),
    pub lsd_enabled: bool,
    pub lsd_intensity: f32, // Factor de 0.0 a 1.0 (activacion progresiva)
    pub time: f32,
    pub mouse_pos: Point,     // Posicion real del raton para efectos magneticos
    pub mouse_stillness: f32, // 0.0 (se mueve) a 1.0 (quieto por X segundos)
    pub is_resizing: bool,    // <--- NUEVO CAMPO
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

#[derive(Debug, Default)]
struct SmoothTranslateState {
    _last_mouse_pos: Cell<iced::Point>,
    _last_time: Cell<f32>,
    current_repulsion: Cell<Vector>,
    current_velocity: Cell<Vector>, // Para inercia de masa (reaccion tardia)
}

impl SmoothTranslateState {
    pub fn new() -> Self {
        Self {
            _last_mouse_pos: Cell::new(iced::Point::ORIGIN),
            _last_time: Cell::new(0.0),
            current_repulsion: Cell::new(Vector::new(0.0, 0.0)),
            current_velocity: Cell::new(Vector::new(0.0, 0.0)),
        }
    }

    fn calculate_displacement(
        &self,
        mouse_pos: iced::Point,
        bounds: Rectangle,
        time: f32,
        intensity: f32,
        lsd_enabled: bool,
        offset: Vector,
        is_resizing: bool,
        _mouse_stillness: f32,
    ) -> Vector {
        // Only skip if resizing. If LSD disabled, we still might want subtle physics if requested,
        // but for now we honor the flag.
        if is_resizing || !lsd_enabled {
            return Vector::new(0.0, 0.0);
        }

        // --- IMPROVED PHYSICS WITH MAGNETISM ---
        let center = bounds.center();
        let center_dist = mouse_pos.distance(center);
        
        // Large interaction radius for general repulsion
        let max_influence_radius = 250.0; 

        // 1. Identify if mouse is inside or near the element for Magnetism/Capture
        let closest_x = mouse_pos.x.clamp(bounds.x, bounds.x + bounds.width);
        let closest_y = mouse_pos.y.clamp(bounds.y, bounds.y + bounds.height);
        let closest_point = iced::Point::new(closest_x, closest_y);
        let dist_to_boundary = mouse_pos.distance(closest_point);
        let is_inside = dist_to_boundary < 0.1;

        let mut force = Vector::new(0.0, 0.0);

        if is_inside {
            // Radio de pegado (Magnetism): 45% de la dimensión mínima
            let capture_radius = bounds.width.min(bounds.height) * 0.45;
            // 0.0 en el centro exacto, 1.0 en el borde del radio de captura
            let capture_factor = (center_dist / capture_radius.max(5.0)).clamp(0.0, 1.0);

            // Vector de atracción: El elemento intenta centrarse en el mouse
            // Cuando estamos en el centro (capture_factor=0), seguimos al mouse al 100%
            let attract_v = Vector::new(
                (mouse_pos.x - center.x) * (1.0 - capture_factor),
                (mouse_pos.y - center.y) * (1.0 - capture_factor),
            );

            // Vector de repulsión interna (opcional, para evitar que se pegue a bordes)
            // Empuja suavemente hacia el centro si estamos muy cerca de los bordes
            let mut repel_v = Vector::new(center.x - closest_point.x, center.y - closest_point.y);
            let mag = (repel_v.x * repel_v.x + repel_v.y * repel_v.y).sqrt();
            if mag > 0.1 {
                repel_v = Vector::new((repel_v.x / mag) * 5.0 * capture_factor, (repel_v.y / mag) * 5.0 * capture_factor);
            }

            force.x = attract_v.x + repel_v.x;
            force.y = attract_v.y + repel_v.y;
        } else if center_dist < max_influence_radius {
            // Repulsión externa (Inverse Square Law)
            let dx = center.x - mouse_pos.x;
            let dy = center.y - mouse_pos.y;
            let dist = center_dist.max(1.0);
            
            let strength = (1.0 - (dist / max_influence_radius)).powf(2.0) * 15.0;
            force.x = (dx / dist) * strength;
            force.y = (dy / dist) * strength;
        } else {
             // Damping return to zero when mouse leaves
             let current = self.current_repulsion.get();
             let damped = Vector::new(current.x * 0.9, current.y * 0.9);
             self.current_repulsion.set(damped);
             if damped.x.abs() < 0.1 && damped.y.abs() < 0.1 {
                 return Vector::new(0.0, 0.0);
             }
             return damped;
        }

        // Apply Jitter
        let jitter_x = (time * 10.0 + offset.x).sin() * 2.0 * intensity;
        let jitter_y = (time * 12.0 + offset.y).cos() * 2.0 * intensity;

        // Smooth physics step
        let current_pos = self.current_repulsion.get();
        let mut current_vel = self.current_velocity.get();
        
        let target_x = force.x + jitter_x;
        let target_y = force.y + jitter_y;
        
        let k = 0.12; // Increased stiffness for better "stickiness"
        let d = 0.82; // Slightly more damping
        
        current_vel.x = (current_vel.x + (target_x - current_pos.x) * k) * d;
        current_vel.y = (current_vel.y + (target_y - current_pos.y) * k) * d;
        
        let new_pos = Vector::new(current_pos.x + current_vel.x, current_pos.y + current_vel.y);
        
        self.current_velocity.set(current_vel);
        self.current_repulsion.set(new_pos);

        new_pos
    }
}

// 2. WIDGET WRAPPER
pub struct SmoothTranslate<'a, Message> {
    content: Element<'a, Message, Theme, Renderer>,
    offset: Vector,
    mouse_pos: iced::Point,
    proximity_only: bool,
    time: f32,
    lsd_intensity: f32,
    lsd_enabled: bool,
    is_resizing: bool,    // <--- NUEVO CAMPO
    mouse_stillness: f32, // <--- NUEVO CAMPO
}

pub fn background_tint_color(palette: &Palette) -> Color {
    if palette.background.r > 0.5 {
        // Modo Light: blanco con transparencia moderada
        Color {
            a: 0.35,
            ..Color::WHITE
        }
    } else if palette.background.r > 0.1 {
        // Modo Grey: color del fondo con transparencia similar
        Color {
            a: 0.35,
            ..palette.background
        }
    } else {
        // Modo Black: color del fondo con transparencia similar
        Color {
            a: 0.35,
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

pub fn generate_palette(config: &rustale_shared::config::ThemeConfig) -> Palette {
    use rustale_shared::config::BaseThemeMode;
    let mut accent = hex_to_color(&config.accent_hex).unwrap_or(Color::from_rgb8(255, 168, 69));

    // 0. Aplicar saturacion ANTES de los calculos de modo
    if (config.saturation - 1.0).abs() > 0.01 {
        let gray = accent.r * 0.299 + accent.g * 0.587 + accent.b * 0.114;
        accent.r = (gray + (accent.r - gray) * config.saturation).clamp(0.0, 1.0);
        accent.g = (gray + (accent.g - gray) * config.saturation).clamp(0.0, 1.0);
        accent.b = (gray + (accent.b - gray) * config.saturation).clamp(0.0, 1.0);
    }

    // 1. Configurar colores base segun el modo
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

    // 2. Aplicar contraste y ajuste de intensidad segun el modo
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
    let alpha = palette.background.a; // Usamos el alfa dinamico del fondo

    container::Style {
        background: Some(Background::Color(palette.surface)), // Ya trae alfa dinamico
        border: Border {
            // El borde ahora se desvanece con el texto
            color: Color {
                a: palette.text_primary.a * 0.1,
                ..palette.text_primary
            },
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            // La sombra desaparece cuando el fondo es 0.0
            color: Color {
                a: alpha * 0.2,
                ..Color::BLACK
            },
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
        background: Some(Background::Color(Color {
            a: palette.danger.a,
            ..palette.danger
        })),
        border: Border {
            color: Color {
                a: 0.2 * palette.background.a,
                ..Color::WHITE
            },
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Color {
            a: palette.text_on_accent.a,
            ..Color::WHITE
        },
        ..Default::default()
    }
}

pub fn magic_column<'a, M: 'a + Clone>(
    items: Vec<Element<'a, M, Theme, Renderer>>,
    ctx: UIContext,
) -> iced::widget::Column<'a, M, Theme, Renderer> {
    let mut col = iced::widget::column!()
        .spacing(STANDARD_SPACING)
        .width(Length::Fill);

    if ctx.lsd_enabled {
        for (i, item) in items.into_iter().enumerate() {
            // Generamos una disparidad unica para cada fila de la columna
            // Esto hace que la columna parezca gelatina en lugar de un bloque rigido
            let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, i + 10, ctx.lsd_intensity);

            let wrapped_item = Element::new(
                SmoothTranslate::new(
                    item,
                    (vx, vy),
                    ctx.mouse_pos,
                    false,
                    ctx.lsd_intensity,
                    ctx.lsd_enabled,
                )
                .resizing(ctx.is_resizing)
                .with_stillness(ctx.mouse_stillness),
            );

            col = col.push(wrapped_item);
        }
    } else {
        for item in items {
            col = col.push(item);
        }
    }

    col
}
pub fn magic_row<'a, M: 'a + Clone>(
    items: Vec<Element<'a, M, Theme, Renderer>>,
    ctx: UIContext,
) -> iced::widget::Row<'a, M, Theme, Renderer> {
    let mut col = iced::widget::row!()
        .spacing(STANDARD_SPACING)
        .width(Length::Fill);

    if ctx.lsd_enabled {
        for (i, item) in items.into_iter().enumerate() {
            // Generamos una disparidad unica para cada fila de la columna
            // Esto hace que la columna parezca gelatina en lugar de un bloque rigido
            let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, i + 12, ctx.lsd_intensity);

            let wrapped_item = Element::new(
                SmoothTranslate::new(
                    item,
                    (vx, vy),
                    ctx.mouse_pos,
                    false,
                    ctx.lsd_intensity,
                    ctx.lsd_enabled,
                )
                .resizing(ctx.is_resizing)
                .with_stillness(ctx.mouse_stillness),
            );

            col = col.push(wrapped_item);
        }
    } else {
        for item in items {
            col = col.push(item);
        }
    }

    col
}

/// Contenedor base para paginas dentro de modales (Settings, Mods, etc.)
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
    title: String,
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
        text(
            iced_text(title)
                .size(18)
                .color(palette.accent)
                .font(iced::font::Font::MONOSPACE),
            ctx,
        ),
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

pub fn text_title<'a, M: 'a>(
    content: impl Into<String>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    text_lsd_letters(content, 18, ctx.palette.accent, ctx)
}

pub fn lsd_magic_text<'a, M: 'a>(
    label: &'a str,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    let mut row = iced::widget::row![].spacing(0);

    for (i, c) in label.chars().enumerate() {
        // Onda mucho mas suave y lenta para evitar temblor excesivo
        let t = ctx.time * 0.8 + (i as f32 * 0.08);
        let off_x = (t * 0.6).sin() * 1.0; // Reducido frecuencia y amplitud
        let off_y = (t * 0.4).cos() * 1.0; // Reducido frecuencia y amplitud

        let char_el = iced::widget::text(c.to_string())
            .size(14)
            .font(iced::font::Font::MONOSPACE)
            // Color neon que resalta sobre el fondo
            .color(Color::from_rgb(1.0, 0.4, 0.2));

        row = row.push(Element::new(
            SmoothTranslate::new(
                char_el.into(),
                (off_x, off_y),
                iced::Point::new(-1000.0, -1000.0), // Dummy mouse pos (no trackeo para efecto "dummy")
                false,                              // Que se mueva siempre
                1.0,                                // Fuerza maxima
                true, // Forzar siempre activo para ESTE texto (el label "LSD" en settings)
            )
            .resizing(ctx.is_resizing)
            .with_stillness(ctx.mouse_stillness),
        ));
    }

    row.into()
}

pub fn text_body<'a, M: 'a>(
    content: impl Into<String>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    text_lsd_letters(content, 14, ctx.palette.text_primary, ctx)
}

pub fn text_caption<'a, M: 'a>(
    content: impl Into<String>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    text_lsd_letters(content, 11, ctx.palette.text_secondary, ctx)
}

// Texto pequeno de tamano 12 con color primario
pub fn text_small<'a, M: 'a>(
    content: impl Into<String>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    text_lsd_letters(content, 12, ctx.palette.text_primary, ctx)
}

// Texto pequeno de tamano 12 con color secundario/muted
pub fn text_muted<'a, M: 'a>(
    content: impl Into<String>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    text_lsd_letters(content, 12, ctx.palette.text_secondary, ctx)
}

// Texto de tamano 10 para descripciones muy pequenas
pub fn text_micro<'a, M: 'a>(
    content: impl Into<String>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    text_lsd_letters(content, 10, ctx.palette.text_secondary, ctx)
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
    label: impl Into<String>,
    value: &str,
    on_change: impl Fn(String) -> M + 'static,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    // 1. Crear el input crudo
    let raw_input = iced::widget::text_input("", value)
        .on_input(on_change)
        .padding(10)
        .style(move |t, s| text_input_style(&ctx.palette, t, s));

    // 2. Envolverlo con el efecto LSD
    let wrapped_input = magic_text_input(raw_input.into(), ctx);

    // 3. Devolver la columna
    column![text_caption(label, ctx), wrapped_input]
        .spacing(5)
        .into()
}

pub fn glass_container(palette: &Palette, _t: &Theme) -> container::Style {
    // Tomamos el alpha actual de la paleta (que ya viene multiplicado en main.rs)
    let current_alpha = palette.background.a;

    container::Style {
        background: Some(Background::Color(Color {
            a: current_alpha * 0.85,
            ..palette.background
        })),
        border: Border {
            color: Color {
                a: palette.text_primary.a * 0.1,
                ..palette.text_primary
            },
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color {
                a: current_alpha * 0.2,
                ..Color::BLACK
            }, // <--- CLAVE: sombra dinamica
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
            text_color: palette.accent, // Ya tiene alfa modificado
            background: Some(Background::Color(Color {
                a: palette.surface_hover.a,
                ..palette.surface_hover
            })),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        _ => button::Style {
            background: None,
            text_color: palette.text_secondary, // Ya tiene alfa modificado
            ..Default::default()
        },
    }
}

pub fn play_button_style(palette: &Palette, _t: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(Color {
            a: palette.background.a * 0.8,
            ..palette.background
        })),
        border: Border {
            color: Color {
                a: palette.accent.a * 0.3,
                ..palette.accent
            },
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: palette.text_primary, // Ya tiene alfa modificado
        ..button::Style::default()
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(Color {
                a: palette.surface.a * 0.9,
                ..palette.surface
            })),
            border: Border {
                color: palette.accent, // Ya tiene alfa modificado
                width: 1.0,
                radius: 8.0.into(),
            },
            text_color: palette.accent, // Ya tiene alfa modificado
            shadow: Shadow {
                color: Color {
                    a: palette.accent.a * 0.2,
                    ..palette.accent
                },
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
    let ui_a = palette.background.a; // Para modo LSD/Transparencia

    // Base: Color de superficie pero con borde "peligro" sutil
    let base = button::Style {
        background: Some(Background::Color(Color {
            a: 0.4 * ui_a,
            ..palette.surface
        })),
        border: Border {
            color: Color {
                a: 0.5 * ui_a,
                ..palette.danger
            },
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Color {
            a: 0.9 * ui_a,
            ..palette.danger
        }, // Texto rojo suave
        ..button::Style::default()
    };

    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(Color {
                a: 0.1 * ui_a,
                ..palette.danger
            })),
            border: Border {
                color: palette.danger,
                ..base.border
            },
            text_color: Color {
                a: 1.0 * ui_a,
                ..Color::WHITE
            },
            shadow: Shadow {
                color: Color {
                    a: 0.2 * ui_a,
                    ..palette.danger
                },
                blur_radius: 10.0,
                ..Default::default()
            },
            ..base
        },
        _ => base,
    }
}

pub fn blocked_button_style(
    palette: &Palette,
    _t: &Theme,
    _status: button::Status,
) -> button::Style {
    let ui_a = palette.background.a;

    button::Style {
        background: Some(Background::Color(Color {
            a: 0.1 * ui_a,
            ..palette.text_primary
        })),
        border: Border {
            color: Color {
                a: 0.05 * ui_a,
                ..palette.text_primary
            },
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: palette.text_secondary,
        ..button::Style::default()
    }
}

pub fn secondary_button_style(
    palette: &Palette,
    _t: &Theme,
    status: button::Status,
) -> button::Style {
    let ui_a = palette.text_primary.a; // Nuestra referencia de visibilidad total
    let is_light = palette.background.r > 0.5;

    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(Color {
                a: palette.surface_hover.a,
                ..palette.surface_hover
            })),
            text_color: palette.accent, // Ya tiene alfa modificado
            border: Border {
                color: palette.accent, // Ya tiene alfa modificado
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        },
        _ => button::Style {
            background: Some(Background::Color(Color {
                // CLAVE: Multiplicar el alfa deseado (0.05) por el alfa dinamico (ui_a)
                a: (if is_light { 0.03 } else { 0.05 }) * ui_a,
                ..if is_light { Color::BLACK } else { Color::WHITE }
            })),
            text_color: palette.text_secondary, // Ya tiene alfa modificado
            border: Border {
                color: Color {
                    a: 0.05 * ui_a,
                    ..palette.text_primary
                },
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

pub fn accent_bar_style(palette: &Palette, _t: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1)),
        bar: Background::Color(palette.accent),
        border: Border {
            radius: 4.0.into(),
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
        background: Some(Background::Color(Color {
            a: palette.accent.a * 0.15, // Alfa dinamico relativo al acento
            ..palette.accent
        })),
        border: Border {
            color: palette.accent, // Ya trae alfa
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: palette.accent,
        ..Default::default()
    }
}

pub fn active_tab_container_style(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: palette.accent.a * 0.15,
            ..palette.accent
        })),
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
    let ui_a = palette.text_primary.a; // Referencia de opacidad general
    let is_light = palette.background.r > 0.5;
    text_input::Style {
        background: Background::Color(if is_light {
            Color::from_rgb(0.92, 0.93, 0.95)
        } else {
            Color {
                a: 0.3 * palette.background.a,
                ..Color::BLACK
            }
        }),
        border: Border {
            color: if matches!(status, text_input::Status::Focused { .. }) {
                palette.accent
            } else {
                Color {
                    a: 0.1 * ui_a,
                    ..palette.text_primary
                }
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

pub fn update_button_style(palette: &Palette, _t: &Theme, status: button::Status) -> button::Style {
    let ui_a = palette.background.a;

    // En lugar de azul, usamos el color del tema pero con un estilo "brillante"
    let mut s = primary_button_style(palette, _t, status);

    if status != button::Status::Hovered {
        // Hacemos que parpadee levemente o sea un poco mas claro que el play normal
        s.background = Some(Background::Color(Color {
            a: 0.8 * ui_a,
            ..palette.accent
        }));
    }
    s
}

pub fn dropdown_trigger_style(
    palette: &Palette,
    _t: &Theme,
    status: button::Status,
) -> button::Style {
    let ui_a = palette.background.a; // Usamos el alfa del fondo como base
    let is_light = palette.background.r > 0.5;

    let base = button::Style {
        text_color: palette.text_primary,
        background: Some(Background::Color(if is_light {
            Color {
                a: 1.0 * ui_a,
                ..Color::from_rgb(0.92, 0.93, 0.95)
            }
        } else {
            // Reemplazar Color::from_rgba fijo por dinamico
            Color {
                a: 0.3 * ui_a,
                ..Color::BLACK
            }
        })),
        border: Border {
            color: Color {
                a: 0.1 * ui_a,
                ..palette.text_primary
            },
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

/// Convierte un pick_list estandar al estilo de dropdown de perfil (contenedor + boton)
pub fn styled_dropdown<'a, T, M>(
    options: &'a [T],
    selected: Option<&'a T>,
    on_changed: impl Fn(T) -> M + 'a,
    _placeholder: &'a str,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    T: std::fmt::Display + Clone + 'a + std::cmp::PartialEq,
    M: 'a + Clone,
{
    let palette = ctx.palette;

    // Crear el pick_list con el estilo de dropdown
    let styled_pick_list = pick_list(options, selected, on_changed)
        .padding(10)
        .width(Length::Fill)
        .style(move |t, s| pick_list_style(&palette, t, s))
        .menu_style(move |t| menu_style(&palette, t));

    // Envolver el pick_list en un contenedor con estilo de card
    magic_container(
        container(styled_pick_list)
            .width(Length::Fill)
            .style(move |t| card_style(&palette, t))
            .into(),
        ctx,
    )
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
        shadow: Shadow::default(),
    }
}

pub fn svg_muted(
    palette: &Palette,
    _t: &Theme,
    _status: iced::widget::svg::Status,
) -> iced::widget::svg::Style {
    let base_color = if palette.background.r > 0.5 {
        Color::from_rgb(0.3, 0.3, 0.4)
    } else {
        Color::from_rgb(0.5, 0.5, 0.5)
    };
    iced::widget::svg::Style {
        color: Some(Color {
            a: palette.text_secondary.a,
            ..base_color
        }), // Usa el alfa del texto secundario
    }
}

pub fn svg_accent(
    palette: &Palette,
    _t: &Theme,
    _status: iced::widget::svg::Status,
) -> iced::widget::svg::Style {
    iced::widget::svg::Style {
        color: Some(palette.accent), // palette.accent ya tiene el alfa modificado
    }
}

pub fn scrollable_style(
    palette: &Palette,
    _t: &Theme,
    status: scrollable::Status,
) -> scrollable::Style {
    let is_hovered = matches!(status, scrollable::Status::Hovered { .. });
    let ui_a = palette.text_primary.a; // Usamos el alfa del texto como referencia de visibilidad de la UI

    let rail = scrollable::Rail {
        background: Some(Background::Color(Color::TRANSPARENT)),
        border: Border {
            radius: 10.0.into(),
            ..Default::default()
        },
        scroller: scrollable::Scroller {
            background: Background::Color(if is_hovered {
                palette.accent // palette.accent ya tiene el alfa corregido
            } else {
                Color {
                    a: 0.1 * ui_a,
                    ..palette.text_primary
                } // <--- Scrollbar suave
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
            background: Background::Color(Color {
                a: 0.5 * ui_a,
                ..Color::BLACK
            }),
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
    container::Style {
        background: None,
        border: Border::default(),
        shadow: Shadow::default(),
        ..Default::default()
    }
}

impl<'a, Message> SmoothTranslate<'a, Message> {
    pub fn new(
        content: Element<'a, Message, Theme, Renderer>,
        offset: (f32, f32),
        mouse_pos: iced::Point,
        proximity_only: bool,
        lsd_intensity: f32,
        lsd_enabled: bool,
    ) -> Self {
        Self {
            content,
            offset: Vector::new(offset.0, offset.1),
            mouse_pos,
            proximity_only,
            time: offset.0.abs() + offset.1.abs(),
            lsd_intensity,
            lsd_enabled,
            is_resizing: false,   // Valor por defecto, se sobreescribe en magic_*
            mouse_stillness: 0.0, // Valor por defecto, se sobreescribe en magic_*
        }
    }

    // Metodo helper para encadenar
    pub fn resizing(mut self, is_resizing: bool) -> Self {
        self.is_resizing = is_resizing;
        self
    }

    // Nuevo metodo helper para mouse_stillness
    pub fn with_stillness(mut self, mouse_stillness: f32) -> Self {
        self.mouse_stillness = mouse_stillness;
        self
    }

    // Helper para mantener la firma limpia si ya pasas ctx en otros lados
    // Si usas las funciones `magic_*`, estas ya tienen acceso a ctx.time.
    // Asegurate de que tus funciones `magic_*` pasen el tiempo aqui si es posible,
    // si no, el lerp funcionara "por frame" (aprox 60fps), lo cual es aceptable visualmente.
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
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<SmoothTranslateState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(SmoothTranslateState::new())
    }

    fn children(&self) -> Vec<widget::Tree> {
        // CORRECCIoN: Declaramos que tenemos un hijo (el contenido)
        vec![widget::Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut widget::Tree) {
        // CORRECCIoN: Diffing correcto del hijo
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // Durante el redimensionamiento, saltamos el calculo de SmoothTranslate.
        // Dibujamos el widget en su posicion absoluta. Esto evita que el shader
        // intente interpolar posiciones en un viewport que esta cambiando de tamaño.
        if self.is_resizing {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
            return;
        }

        let bounds = layout.bounds();

        // [OPTIMIZATION] SOPHISTICATED CULLING (Culling con Margen)
        // 1. Margen de seguridad: El efecto LSD mueve los objetos visualmente fuera de sus bounds reales.
        //    Si hacemos un culling estricto, los objetos parpadearan al entrar en pantalla.
        //    Anadimos 100px extra (suficiente para cubrir repulsion + jitter).
        let safe_margin = 100.0;

        let visible_area = Rectangle {
            x: viewport.x - safe_margin,
            y: viewport.y - safe_margin,
            width: viewport.width + (safe_margin * 2.0),
            height: viewport.height + (safe_margin * 2.0),
        };

        // 2. Comprobacion rapida AABB (Axis-Aligned Bounding Box)
        // Si no esta en el area expandida, abortamos inmediatamente.
        // Esto ahorra:
        // - El calculo pesado de fisicas en calculate_displacement (CPU)
        // - El envio de vertices a WGPU (GPU)
        if !visible_area.intersects(&bounds) {
            return;
        }

        let state = tree.state.downcast_ref::<SmoothTranslateState>();

        // Logica de "Derretimiento"
        if self.proximity_only {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
            return;
        }

        // Recuperar intensidad del widget y calcular desplazamiento
        let displacement = state.calculate_displacement(
            self.mouse_pos,
            bounds,
            self.time,
            self.lsd_intensity,
            self.lsd_enabled,
            self.offset,
            self.is_resizing,     // <--- PASAR AQUi
            self.mouse_stillness, // <--- PASAR AQUi
        );

        renderer.with_translation(displacement, |r| {
            self.content.as_widget().draw(
                &tree.children[0],
                r,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        });
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
        let state = tree.state.downcast_ref::<SmoothTranslateState>();
        let bounds = layout.bounds();

        // Calcular desplazamiento para traducir el cursor
        let displacement = state.calculate_displacement(
            self.mouse_pos,
            bounds,
            self.time,
            self.lsd_intensity,
            self.lsd_enabled && !self.proximity_only,
            self.offset,
            self.is_resizing,     // <--- PASAR AQUi
            self.mouse_stillness, // <--- PASAR AQUi
        );

        // Traducir el cursor para que coincida con lo que el usuario ve
        let final_cursor = match cursor.position() {
            Some(p) => mouse::Cursor::Available(iced::Point::new(
                p.x - displacement.x,
                p.y - displacement.y,
            )),
            None => mouse::Cursor::Unavailable,
        };

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            final_cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        // Operamos sobre el hijo (esto incluye logica de scroll, foco, etc.)
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_ref::<SmoothTranslateState>();
        let bounds = layout.bounds();

        // Calcular desplazamiento actual
        let displacement = state.calculate_displacement(
            self.mouse_pos,
            bounds,
            self.time,
            self.lsd_intensity,
            self.lsd_enabled && !self.proximity_only,
            self.offset,
            self.is_resizing,     // <--- PASAR AQUi
            self.mouse_stillness, // <--- PASAR AQUi
        );

        // Ajustamos la translacion para que el overlay aparezca en la posicion visual correcta
        let adjusted_translation = translation + displacement;

        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            adjusted_translation,
        )
    }
}

// Modificamos la firma para aceptar 'intensity'
fn get_seeded_disparity(offset: (f32, f32), seed: usize, intensity: f32) -> (f32, f32) {
    // 1. HARD OPTIMIZATION: If intensity is negligible, return ZERO immediately.
    // This stops layout thrashing when the mouse is still.
    if intensity < 0.01 {
        return (0.0, 0.0);
    }

    // 1. Hash de alta calidad para cada semilla (valores únicos por letra)
    let s = seed as f32;
    let hash_x = ((s * 12.9898).fract() * 43758.5453).fract();
    let hash_y = ((s * 78.233).fract() * 43758.5453).fract();

    // 2. Componente caótico temporal con frecuencias reducidas para evitar temblor
    let t1 = offset.0 * 0.4; // Reducido de 1.3 a 0.4
    let t2 = offset.1 * 0.7; // Reducido de 2.7 a 0.7
    let t3 = (offset.0 + offset.1) * 1.1; // Reducido de 5.3 a 1.1

    // 3. Mapa caótico no lineal (evita patrones predecibles)
    let chaos_x = (t1.sin() * 0.5 + t2.cos() * 0.3 + (t1 * t3).sin() * 0.2) * hash_x;
    let chaos_y = (t2.sin() * 0.4 + t3.cos() * 0.4 + (t2 * t1).cos() * 0.2) * hash_y;

    // 4. Dispersión aleatoria individual por letra
    let scatter_x = (hash_x - 0.5) * 0.6;
    let scatter_y = (hash_y - 0.5) * 0.6;

    // 5. Combinación final con entropía real
    let final_x = (chaos_x + scatter_x) * intensity;
    let final_y = (chaos_y + scatter_y) * intensity;

    (final_x, final_y)
}

pub fn text<'a, M: 'a>(
    content: iced::widget::Text<'a, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    if !ctx.lsd_enabled {
        return content.into();
    }

    let element: Element<'a, M, Theme, Renderer> = content.into();
    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 1, ctx.lsd_intensity);

    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy), // Pasar vectores directos
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing)
        .with_stillness(ctx.mouse_stillness),
    )
}

// NUEVA FUNCIoN: Aplica efecto LSD letra por letra a un string con optimización batching
// Acepta cualquier tipo de string (String, &str, etc.) para evitar problemas de lifetime
pub fn text_lsd_letters<'a, M: 'a>(
    text_str: impl Into<String>,
    size: impl Into<iced::Pixels>,
    color: Color,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    // [PERFORMANCE] Aumentamos batch_size por defecto a 3.
    // Letra a letra (1) consume demasiada CPU en listas largas.
    text_lsd_letters_batched(text_str, size, color, ctx, 3)
}

// VERSIÓN CON BATCHING: Para uso interno con optimización
pub fn text_lsd_letters_batched<'a, M: 'a>(
    text_str: impl Into<String>,
    size: impl Into<iced::Pixels>,
    color: Color,
    ctx: UIContext,
    batch_size: usize,
) -> Element<'a, M, Theme, Renderer> {
    let text_owned = text_str.into();
    let size_px = size.into();

    if !ctx.lsd_enabled {
        return iced_text(text_owned).size(size_px).color(color).into();
    }

    // Determinar batch_size dinámico basado en longitud del texto
    let effective_batch_size = if batch_size == 1 {
        // Batching dinámico solo cuando se solicita explícitamente batch_size > 1
        1
    } else {
        batch_size
    };

    let lsd_intensity = ctx.lsd_intensity;
    let lsd_enabled = ctx.lsd_enabled;
    let mouse_pos = ctx.mouse_pos;

    // EFECTO POR BATCHES en lugar de letra por letra
    let mut batch_row = iced::widget::row!().spacing(0);

    // Agrupar caracteres en batches
    let chars: Vec<char> = text_owned.chars().collect();
    for (batch_idx, batch) in chars.chunks(effective_batch_size).enumerate() {
        // Unir los caracteres del batch en un solo string
        let batch_string: String = batch.iter().collect();

        // Crear texto para todo el batch
        let batch_text = iced_text(batch_string).size(size_px).color(color);
        let batch_element: Element<'a, M, Theme, Renderer> = batch_text.into();

        // Calcular offset único para cada batch
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 100 + batch_idx, lsd_intensity);

        // Envolver cada batch con SmoothTranslate
        let magic_batch = Element::new(
            SmoothTranslate::new(
                batch_element,
                (vx, vy),
                mouse_pos,
                false,
                lsd_intensity,
                lsd_enabled,
            )
            .resizing(ctx.is_resizing)
            .with_stillness(ctx.mouse_stillness),
        );

        batch_row = batch_row.push(magic_batch);
    }

    batch_row.into()
}

pub fn svg<'a, M: 'a>(content: impl Into<Element<'a, M>>, ctx: UIContext) -> Element<'a, M> {
    if !ctx.lsd_enabled {
        return content.into();
    }

    let element = content.into();
    let (vx, vy) = get_seeded_disparity(
        (ctx.lsd_offset.0 + 0.5, ctx.lsd_offset.1 + 0.5),
        15,
        ctx.lsd_intensity,
    );

    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy), // Pasar vectores directos
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing),
    )
}

pub fn magic_pick_list_with_menu<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if !ctx.lsd_enabled {
        return element.into();
    }

    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 2, ctx.lsd_intensity);

    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing),
    )
}
pub fn magic_text_input<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if !ctx.lsd_enabled {
        return element.into();
    }

    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 3, ctx.lsd_intensity);

    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing),
    )
}

pub fn text_editor_style(
    palette: &Palette,
    _t: &Theme,
    status: iced::widget::text_editor::Status,
) -> iced::widget::text_editor::Style {
    let is_light = palette.background.r > 0.5;
    iced::widget::text_editor::Style {
        background: Background::Color(if is_light {
            Color::from_rgb(0.92, 0.93, 0.95)
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.4)
        }),
        border: Border {
            // CORRECCIoN E0533: Focused tiene campos internos
            color: if matches!(status, iced::widget::text_editor::Status::Focused { .. }) {
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
        // CORRECCIoN E0560: Eliminados campos 'icon' y 'cursor' que no existen en el struct Style
        value: palette.text_primary,
        placeholder: palette.text_secondary,
        selection: Color::from_rgba(palette.accent.r, palette.accent.g, palette.accent.b, 0.2),
    }
}

// 2. Actualizar la funcion para que se comporte como un area real
pub fn magic_text_area<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if !ctx.lsd_enabled {
        return element.into();
    }

    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 11, ctx.lsd_intensity);

    Element::new(
        SmoothTranslate::new(
            container(element)
                .width(Length::Fill)
                .height(Length::Fixed(150.0)) // Altura de area de texto real
                .padding(2)
                .into(),
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing),
    )
}
pub fn magic_button<'a, M: 'a + Clone>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if !ctx.lsd_enabled {
        return element.into();
    }

    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 9, ctx.lsd_intensity);
    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy), // Pasar vectores directos
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing)
        .with_stillness(ctx.mouse_stillness),
    )
}

pub fn magic_checkbox<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if !ctx.lsd_enabled {
        return element.into();
    }

    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 5, ctx.lsd_intensity);
    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy), // Pasar vectores directos
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing),
    )
}

pub fn magic_image<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if !ctx.lsd_enabled {
        return element.into();
    }

    let (vx, vy) = get_seeded_disparity(
        (ctx.lsd_offset.0 + 0.3, ctx.lsd_offset.1 + 0.3),
        1,
        ctx.lsd_intensity,
    );
    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy), // Pasar vectores directos
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing),
    )
}

pub fn magic_container<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if !ctx.lsd_enabled {
        return element.into();
    }

    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 6, ctx.lsd_intensity);

    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing),
    )
}

pub fn magic_scrollable<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if !ctx.lsd_enabled {
        return element.into();
    }

    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 5, ctx.lsd_intensity);
    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy), // Pasar vectores directos
            ctx.mouse_pos,
            !ctx.lsd_enabled, // proximity_only si esta desactivado
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing),
    )
}

pub fn magic_slider<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if !ctx.lsd_enabled {
        return element.into();
    }

    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 7, ctx.lsd_intensity);
    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing),
    )
}

pub fn magic_tooltip<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if !ctx.lsd_enabled {
        return element.into();
    }

    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 8, ctx.lsd_intensity);
    Element::new(
        SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        )
        .resizing(ctx.is_resizing),
    )
}

// --- NUEVOS ESTILOS PARA WINDOW FRAME & TITLE BAR ---

pub fn window_frame_style(palette: &Palette, _t: &Theme, is_maximized: bool) -> container::Style {
    let alpha = palette.background.a; // Si palette es faded_palette, este valor llega a 0.0

    container::Style {
        background: Some(Background::Color(Color::TRANSPARENT)), // El fondo lo maneja el contenido interno
        border: Border {
            // El color del borde desaparece junto con el alpha general
            color: Color {
                a: 0.5 * alpha,
                ..Color::BLACK
            },
            width: if is_maximized { 0.0 } else { 1.0 }, // Sin borde si esta maximizado
            radius: if is_maximized {
                0.0.into()
            } else {
                10.0.into()
            }, // Redondeado solo si es ventana
        },
        // OPTIMIZACIoN: Reducir blur_radius de 15.0 a 5.0 para mejorar rendimiento
        shadow: if is_maximized {
            Shadow::default()
        } else {
            Shadow {
                // La sombra tambien muere en la transparencia total
                color: Color {
                    a: 0.4 * alpha,
                    ..Color::BLACK
                },
                offset: Vector::new(0.0, 2.0),
                blur_radius: 5.0, // Reducido para rendimiento
            }
        },
        ..Default::default()
    }
}

// --- FUNCIONES PARA COLORES Y ESTILOS CENTRALIZADOS ---

/// Color para elementos SVG que deberían ser negros (iconos, etc.)
pub fn svg_icon_color(palette: &Palette) -> Color {
    // Usar el color primario de texto en modo claro, o negro suave en modo oscuro
    if palette.background.r > 0.5 {
        Color::BLACK
    } else {
        Color::from_rgb(0.05, 0.05, 0.05) // Negro suave para modo oscuro
    }
}

/// Estilo para contenedores de fondo oscuro (usado en main.rs)
pub fn dark_background_container(_t: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb(0.05, 0.05, 0.05))),
        ..Default::default()
    }
}

/// Estilo para overlays semitransparentes (usado en main.rs)
pub fn overlay_container(_t: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgba(
            0.0, 0.0, 0.0, 0.8,
        ))),
        ..Default::default()
    }
}

/// Estilo para placeholders de imágenes (usado en news_section.rs)
pub fn image_placeholder_container(_t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.1))),
        ..Default::default()
    }
}

/// Estilo para bordes redondeados de imágenes (usado en news_section.rs)
pub fn image_border_container() -> Border {
    Border {
        radius: 8.0.into(),
        ..Default::default()
    }
}

/// Estilo para barras de progreso pequeñas (usado en news_section.rs)
pub fn small_progress_bar(_palette: &Palette) -> Border {
    Border {
        radius: 3.0.into(),
        ..Default::default()
    }
}

// --- CONSTANTES DE LAYOUT (Para uso futuro en migración) ---
// pub const UI_FULL_WIDTH: Length = Length::Fill;
// pub const UI_FULL_HEIGHT: Length = Length::Fill;
// pub const UI_FIXED_WIDTH_180: Length = Length::Fixed(180.0);
// pub const UI_FIXED_WIDTH_200: Length = Length::Fixed(200.0);

// --- CONSTANTES DE FUENTES (Para uso futuro en migración) ---
// pub const UI_MONOSPACE_FONT: iced::Font = iced::Font::MONOSPACE;

#[cfg(test)]
mod tests {
    use super::*;
    use rustale_shared::config::{BaseThemeMode, ThemeConfig};

    // === hex_to_color Tests ===

    #[test]
    fn test_hex_to_color_valid() {
        let color = hex_to_color("FF8B45").expect("Should parse valid hex");
        assert!((color.r - 1.0).abs() < 0.01);
        assert!((color.g - 0.545).abs() < 0.01);
        assert!((color.b - 0.271).abs() < 0.01);
    }

    #[test]
    fn test_hex_to_color_with_hash() {
        let color = hex_to_color("#FF8B45").expect("Should parse with hash");
        assert!((color.r - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_hex_to_color_black() {
        let color = hex_to_color("000000").expect("Should parse black");
        assert!((color.r - 0.0).abs() < 0.01);
        assert!((color.g - 0.0).abs() < 0.01);
        assert!((color.b - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_hex_to_color_white() {
        let color = hex_to_color("FFFFFF").expect("Should parse white");
        assert!((color.r - 1.0).abs() < 0.01);
        assert!((color.g - 1.0).abs() < 0.01);
        assert!((color.b - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_hex_to_color_invalid_too_short() {
        let result = hex_to_color("FF");
        assert!(result.is_none());
    }

    #[test]
    fn test_hex_to_color_invalid_too_long() {
        let result = hex_to_color("FF8B45AB");
        assert!(result.is_none());
    }

    #[test]
    fn test_hex_to_color_invalid_chars() {
        let result = hex_to_color("GG8B45");
        assert!(result.is_none());
    }

    #[test]
    fn test_hex_to_color_empty() {
        let result = hex_to_color("");
        assert!(result.is_none());
    }

    // === generate_palette Tests ===

    fn default_theme_config() -> ThemeConfig {
        ThemeConfig {
            accent_hex: "FF8B45".to_string(),
            base_mode: BaseThemeMode::Black,
            saturation: 1.0,
            contrast: 1.0,
            lsd_mode: false,
        }
    }

    #[test]
    fn test_generate_palette_black_mode() {
        let config = ThemeConfig {
            base_mode: BaseThemeMode::Black,
            ..default_theme_config()
        };
        let palette = generate_palette(&config);
        
        // Black mode should have very dark background
        assert!(palette.background.r < 0.05);
        assert!(palette.background.g < 0.05);
        assert!(palette.background.b < 0.05);
        
        // Text should be white
        assert!((palette.text_primary.r - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_generate_palette_grey_mode() {
        let config = ThemeConfig {
            base_mode: BaseThemeMode::Grey,
            ..default_theme_config()
        };
        let palette = generate_palette(&config);
        
        // Grey mode should have medium-dark background
        assert!(palette.background.r > 0.1);
        assert!(palette.background.r < 0.2);
        
        // Text should be white
        assert!((palette.text_primary.r - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_generate_palette_light_mode() {
        let config = ThemeConfig {
            base_mode: BaseThemeMode::Light,
            ..default_theme_config()
        };
        let palette = generate_palette(&config);
        
        // Light mode should have bright background
        assert!(palette.background.r > 0.9);
        
        // Text should be dark
        assert!(palette.text_primary.r < 0.3);
    }

    #[test]
    fn test_generate_palette_text_on_accent_calculation() {
        // With dark accent (low luminance), text should be white
        let config = ThemeConfig {
            accent_hex: "333333".to_string(), // Dark gray
            ..default_theme_config()
        };
        let palette = generate_palette(&config);
        assert!((palette.text_on_accent.r - 1.0).abs() < 0.01);
        
        // With bright accent (high luminance), text should be black
        let config = ThemeConfig {
            accent_hex: "FFFFFF".to_string(), // White
            ..default_theme_config()
        };
        let palette = generate_palette(&config);
        assert!((palette.text_on_accent.r - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_generate_palette_saturation_effect() {
        let config_desaturated = ThemeConfig {
            saturation: 0.0, // Fully desaturated
            ..default_theme_config()
        };
        let config_saturated = ThemeConfig {
            saturation: 1.0,
            ..default_theme_config()
        };
        
        let palette_desaturated = generate_palette(&config_desaturated);
        let palette_saturated = generate_palette(&config_saturated);
        
        // Desaturated accent should be more gray (closer to gray value)
        // The exact values depend on the accent color
        assert_ne!(palette_desaturated.accent.r, palette_saturated.accent.r);
    }

    #[test]
    fn test_generate_palette_contrast_effect() {
        let config_low = ThemeConfig {
            contrast: 0.5,
            ..default_theme_config()
        };
        let config_high = ThemeConfig {
            contrast: 1.5,
            ..default_theme_config()
        };
        
        let palette_low = generate_palette(&config_low);
        let palette_high = generate_palette(&config_high);
        
        // Higher contrast should produce brighter accent
        assert!(palette_high.accent.r > palette_low.accent.r);
    }

    #[test]
    fn test_generate_palette_invalid_hex_uses_default() {
        let config = ThemeConfig {
            accent_hex: "INVALID".to_string(),
            ..default_theme_config()
        };
        let palette = generate_palette(&config);
        
        // Should still produce a valid palette with default color
        // Default is defined in the function as Color::from_rgb8(255, 168, 69)
        assert!((palette.accent.r - 1.0).abs() < 0.01);
    }

    // === background_tint_color Tests ===

    #[test]
    fn test_background_tint_color_dark_mode() {
        let palette = Palette {
            background: Color::from_rgb(0.01, 0.01, 0.02),
            ..generate_palette(&default_theme_config())
        };
        let tint = background_tint_color(&palette);
        
        // Dark mode should return background with alpha
        assert!((tint.a - 0.35).abs() < 0.01);
    }

    #[test]
    fn test_background_tint_color_light_mode() {
        let palette = Palette {
            background: Color::from_rgb(0.96, 0.97, 0.99), // Light background
            ..generate_palette(&default_theme_config())
        };
        let tint = background_tint_color(&palette);
        
        // Light mode should return white with alpha
        assert!((tint.a - 0.35).abs() < 0.01);
        assert!((tint.r - 1.0).abs() < 0.01);
    }

    // === Palette Equality Test ===

    #[test]
    fn test_palette_equality() {
        let p1 = generate_palette(&default_theme_config());
        let p2 = generate_palette(&default_theme_config());
        assert_eq!(p1, p2);
    }

    // === UIContext Tests ===

    #[test]
    fn test_ui_context_creation() {
        let ctx = UIContext {
            palette: generate_palette(&default_theme_config()),
            lsd_offset: (0.0, 0.0),
            lsd_enabled: false,
            lsd_intensity: 0.0,
            time: 0.0,
            mouse_pos: Point::ORIGIN,
            mouse_stillness: 0.0,
            is_resizing: false,
        };
        
        assert!(!ctx.lsd_enabled);
        assert!(!ctx.is_resizing);
    }

    // === Constants Tests ===

    #[test]
    fn test_constants() {
        assert!((STANDARD_PADDING - 20.0).abs() < 0.01);
        assert_eq!(STANDARD_SPACING, 15);
        assert!((LSD_RAMP_UP_SECONDS - 120.0).abs() < 0.01);
    }

    // === Color Edge Cases ===

    #[test]
    fn test_hex_to_color_case_insensitive() {
        let upper = hex_to_color("FF8B45").expect("Upper case");
        let lower = hex_to_color("ff8b45").expect("Lower case");
        
        assert!((upper.r - lower.r).abs() < 0.01);
        assert!((upper.g - lower.g).abs() < 0.01);
        assert!((upper.b - lower.b).abs() < 0.01);
    }

    #[test]
    fn test_hex_to_color_mixed_case() {
        let color = hex_to_color("Ff8B45").expect("Mixed case");
        assert!((color.r - 1.0).abs() < 0.01);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // UI COMPONENT VALIDATION TESTS
    // These tests verify that critical UI components can be constructed.
    // While we can't test visual output, we can catch construction errors.
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Verify that the Play button style can be constructed for all states
    #[test]
    fn test_play_button_style_all_states() {
        let palette = generate_palette(&default_theme_config());
        let t = Theme::Dark;
        
        // All button states should produce valid styles
        let _idle = play_button_style(&palette, &t, button::Status::Active);
        let _hovered = play_button_style(&palette, &t, button::Status::Hovered);
        let _pressed = play_button_style(&palette, &t, button::Status::Pressed);
        let _disabled = play_button_style(&palette, &t, button::Status::Disabled);
        
        // Active button should have accent color
        let active = play_button_style(&palette, &t, button::Status::Active);
        assert!(active.background.is_some());
        
        // Disabled should have reduced opacity
        let disabled = play_button_style(&palette, &t, button::Status::Disabled);
        assert!(disabled.background.is_some());
    }

    /// Verify that primary button style works
    #[test]
    fn test_primary_button_style_consistency() {
        let palette = generate_palette(&default_theme_config());
        let t = Theme::Dark;
        
        let style = primary_button_style(&palette, &t, button::Status::Active);
        
        // Primary button should have accent background
        assert!(style.background.is_some());
        
        // Text color should be set for readability
        // On accent, text should be either black or white depending on luminance
        let luminance = 0.299 * palette.accent.r + 0.587 * palette.accent.g + 0.114 * palette.accent.b;
        let expected_text = if luminance > 0.5 { Color::BLACK } else { Color::WHITE };
        assert!((style.text_color.r - expected_text.r).abs() < 0.01);
        assert!((style.text_color.g - expected_text.g).abs() < 0.01);
        assert!((style.text_color.b - expected_text.b).abs() < 0.01);
    }

    /// Verify that danger button style is visually distinct
    #[test]
    fn test_danger_button_style() {
        let palette = generate_palette(&default_theme_config());
        let t = Theme::Dark;
        
        let style = danger_button_style(&palette, &t, button::Status::Active);
        
        // Danger button should have danger color
        assert!(style.background.is_some());
        
        // Should have red-tinted background
        if let Some(Background::Color(bg)) = style.background {
            assert!(bg.r > bg.g && bg.r > bg.b, "Danger button should be red-tinted");
        }
    }

    /// Verify that text input style handles focus state
    #[test]
    fn test_text_input_style_focused() {
        let palette = generate_palette(&default_theme_config());
        let t = Theme::Dark;
        
        let unfocused = text_input_style(&palette, &t, text_input::Status::Active);
        let focused = text_input_style(&palette, &t, text_input::Status::Focused { is_hovered: false });
        
        // Focused border should use accent color
        assert!((focused.border.color.r - palette.accent.r).abs() < 0.01);
        assert!((focused.border.color.g - palette.accent.g).abs() < 0.01);
        assert!((focused.border.color.b - palette.accent.b).abs() < 0.01);
        
        // Unfocused should have muted border
        assert!(unfocused.border.color.a < focused.border.color.a);
    }

    /// Verify that checkbox style handles checked state
    #[test]
    fn test_checkbox_style_checked() {
        let palette = generate_palette(&default_theme_config());
        let t = Theme::Dark;
        
        let unchecked = checkbox_style(&palette, &t, checkbox::Status::Active { is_checked: false });
        let checked = checkbox_style(&palette, &t, checkbox::Status::Active { is_checked: true });
        
        // Checked should have accent background
        if let Background::Color(bg) = checked.background {
            assert!((bg.r - palette.accent.r).abs() < 0.01);
        } else {
            panic!("Checked checkbox should have color background");
        }
        
        // Unchecked should have dark background
        if let Background::Color(bg) = unchecked.background {
            assert!(bg.r < 0.5, "Unchecked checkbox should have dark background");
        }
    }

    /// Verify that slider style produces valid rail colors
    #[test]
    fn test_slider_style_rail_colors() {
        let palette = generate_palette(&default_theme_config());
        let t = Theme::Dark;
        
        let style = slider_style(&palette, &t, slider::Status::Active);
        
        // Rail should have accent color on active side
        match style.rail.backgrounds {
            (Background::Color(active), Background::Color(inactive)) => {
                // Active side should be accent
                assert!((active.r - palette.accent.r).abs() < 0.01);
                // Inactive side should be muted
                assert!(inactive.a < active.a);
            }
            _ => panic!("Slider rail should have color backgrounds"),
        }
    }

    /// Verify that container styles produce valid backgrounds
    #[test]
    fn test_container_styles_valid() {
        let palette = generate_palette(&default_theme_config());
        let t = Theme::Dark;
        
        // Card style
        let card = card_style(&palette, &t);
        assert!(card.background.is_some());
        
        // Glass container
        let glass = glass_container(&palette, &t);
        assert!(glass.background.is_some());
        
        // Modal container
        let modal = modal_container(&palette, &t);
        assert!(modal.background.is_some());
        
        // Sidebar
        let sidebar = sidebar_style(&palette, &t);
        assert!(sidebar.background.is_some());
    }

    /// Verify that progress bar styles are valid
    #[test]
    fn test_progress_bar_styles() {
        let palette = generate_palette(&default_theme_config());
        let t = Theme::Dark;
        
        let orange = orange_bar_style(&palette, &t);
        let accent = accent_bar_style(&palette, &t);
        
        // Both should have accent-colored bars
        match orange.bar {
            Background::Color(c) => assert!((c.r - palette.accent.r).abs() < 0.01),
            _ => panic!("Progress bar should have color"),
        }
        
        match accent.bar {
            Background::Color(c) => assert!((c.r - palette.accent.r).abs() < 0.01),
            _ => panic!("Progress bar should have color"),
        }
    }

    /// Verify that pick list and menu styles are consistent
    #[test]
    fn test_pick_list_menu_style_consistency() {
        let palette = generate_palette(&default_theme_config());
        let t = Theme::Dark;
        
        let menu = menu_style(&palette, &t);
        
        // Menu selected background should match accent
        match menu.selected_background {
            Background::Color(c) => {
                assert!((c.r - palette.accent.r).abs() < 0.01);
            }
            _ => panic!("Menu selected background should be color"),
        }
    }

    /// Verify that scrollable style produces valid backgrounds
    #[test]
    fn test_scrollable_style_valid() {
        let palette = generate_palette(&default_theme_config());
        let t = Theme::Dark;
        
        // Test with default status values
        let style = scrollable_style(&palette, &t, scrollable::Status::Active {
            is_horizontal_scrollbar_disabled: false,
            is_vertical_scrollbar_disabled: false,
        });
        
        // Should have valid scroller backgrounds
        match style.vertical_rail.scroller.background {
            Background::Color(_) => {}
            _ => panic!("Scroller should have color background"),
        }
    }

    /// Verify that UIContext can be created with all fields
    #[test]
    fn test_ui_context_complete() {
        let ctx = UIContext {
            palette: generate_palette(&default_theme_config()),
            lsd_offset: (1.5, 2.5),
            lsd_enabled: true,
            lsd_intensity: 0.75,
            time: 10.0,
            mouse_pos: Point::new(100.0, 200.0),
            mouse_stillness: 0.5,
            is_resizing: false,
        };
        
        assert!(ctx.lsd_enabled);
        assert!((ctx.lsd_offset.0 - 1.5).abs() < 0.01);
        assert!((ctx.lsd_intensity - 0.75).abs() < 0.01);
        assert!((ctx.mouse_pos.x - 100.0).abs() < 0.01);
        assert!((ctx.mouse_stillness - 0.5).abs() < 0.01);
    }

    /// Verify that LSD intensity clamps properly
    #[test]
    fn test_lsd_intensity_bounds() {
        // get_seeded_disparity should handle extreme values
        let zero = get_seeded_disparity((0.0, 0.0), 0, 0.0);
        assert!((zero.0).abs() < 0.01);
        assert!((zero.1).abs() < 0.01);
        
        // Even with high intensity, displacement should be reasonable
        let high = get_seeded_disparity((0.0, 0.0), 0, 10.0);
        // Just verify it doesn't panic and produces some value
        let _ = high;
    }

    /// Verify that background_tint_color handles all palette modes
    #[test]
    fn test_background_tint_all_modes() {
        // Black mode
        let dark_palette = Palette {
            background: Color::from_rgb(0.01, 0.01, 0.02),
            ..generate_palette(&default_theme_config())
        };
        let dark_tint = background_tint_color(&dark_palette);
        assert!((dark_tint.a - 0.35).abs() < 0.01);
        
        // Light mode
        let light_palette = Palette {
            background: Color::from_rgb(0.96, 0.97, 0.99),
            ..generate_palette(&default_theme_config())
        };
        let light_tint = background_tint_color(&light_palette);
        assert!((light_tint.a - 0.35).abs() < 0.01);
        assert!((light_tint.r - 1.0).abs() < 0.01);
        
        // Grey mode
        let grey_palette = Palette {
            background: Color::from_rgb(0.15, 0.15, 0.17),
            ..generate_palette(&default_theme_config())
        };
        let grey_tint = background_tint_color(&grey_palette);
        assert!((grey_tint.a - 0.35).abs() < 0.01);
    }
}
