use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell, overlay};
use iced::event::Event;
use iced::overlay::menu;
use iced::widget::{
    Space, button, checkbox, column, container, pick_list, progress_bar, row as iced_row,
    scrollable, slider, svg as iced_svg, text as iced_text, text_input,
};
use iced::{
    Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Shadow, Size, Theme,
    Vector,
};
use std::cell::Cell;

// --- CONSTANTS ---
pub const STANDARD_PADDING: f32 = 20.0;
pub const STANDARD_SPACING: u32 = 15;

pub const LSD_RAMP_UP_SECONDS: f32 = 300.0;

#[derive(Debug, Clone, Copy)]
pub struct UIContext {
    pub palette: Palette,
    pub lsd_offset: (f32, f32),
    pub lsd_enabled: bool,
    pub lsd_intensity: f32, // Factor de 0.0 a 1.0 (activacion progresiva)
    pub time: f32,
    pub mouse_pos: Point,     // Posicion real del raton para efectos magneticos
    pub mouse_stillness: f32, // 0.0 (se mueve) a 1.0 (quieto por X segundos)
    pub is_resizing: bool,     // <--- NUEVO CAMPO
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
    // [FIX] Usar '_' para suprimir advertencias de campos calculados pero no leidos logicamente aun
    _smoothed_stillness: Cell<f32>,
    _last_mouse_pos: Cell<iced::Point>,
    _last_time: Cell<f32>,
    current_repulsion: Cell<Vector>,
    current_velocity: Cell<Vector>, // Para inercia de masa (reaccion tardia)
    intensity: Cell<f32>,           // Estado persistente de intensidad
}

impl SmoothTranslateState {
    pub fn new() -> Self {
        Self {
            _smoothed_stillness: Cell::new(0.0),
            _last_mouse_pos: Cell::new(iced::Point::ORIGIN),
            _last_time: Cell::new(0.0),
            current_repulsion: Cell::new(Vector::new(0.0, 0.0)),
            current_velocity: Cell::new(Vector::new(0.0, 0.0)),
            intensity: Cell::new(0.0),
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
        is_resizing: bool, // <--- NUEVO PARÁMETRO
    ) -> Vector {
        // Si está desactivado O se está redimensionando la ventana
        if !lsd_enabled || is_resizing {
            // CRÍTICO: Reiniciar la velocidad y la repulsión a cero.
            // Esto evita que cuando termines de redimensionar, los objetos
            // salgan disparados por la inercia acumulada.
            self.current_velocity.set(Vector::new(0.0, 0.0));
            self.current_repulsion.set(Vector::new(0.0, 0.0));
            self.intensity.set(0.0);
            
            // Si quieres mantener el movimiento de "onda" suave (jitter) durante el resize,
            // puedes devolver solo la parte del jitter. 
            // Pero para máxima estabilidad, devolvemos 0.0.
            return Vector::new(0.0, 0.0);
        }

        // Actualizamos intensidad en el estado
        self.intensity.set(intensity);

        // --- 1. LoGICA DE REPULSIoN (Bordes) + ATRACCIoN (Centro) ---
        let center = bounds.center();
        let center_dist = mouse_pos.distance(center);

        let closest_x = mouse_pos.x.clamp(bounds.x, bounds.x + bounds.width);
        let closest_y = mouse_pos.y.clamp(bounds.y, bounds.y + bounds.height);
        let closest_point = iced::Point::new(closest_x, closest_y);

        let dist_to_boundary = mouse_pos.distance(closest_point);
        let is_inside = dist_to_boundary < 0.1;

        let radius = 100.0;
        let mut target_displacement = Vector::new(0.0, 0.0);

        if is_inside {
            // --- COMERZAR "CAPTURA" AL ACERCARSE AL CENTRO ---
            // Radio de pegado: 40% de la dimension minima
            let capture_radius = bounds.width.min(bounds.height) * 0.45;
            // 0.0 en el centro exacto, 1.0 en el borde del radio de captura
            let capture_factor = (center_dist / capture_radius.max(5.0)).clamp(0.0, 1.0);

            // Vector de atraccion (seguir al mouse) - reducido para contenedores grandes
            let element_size = bounds.width.min(bounds.height);
            let attraction_factor = if element_size > 150.0 { 0.2 } else { 1.0 }; // Contenedores grandes: 20%, elementos pequeños: 100%
            let attract_v = Vector::new(
                (mouse_pos.x - center.x) * attraction_factor,
                (mouse_pos.y - center.y) * attraction_factor,
            );

            // Vector de repulsion interna (empujar hacia el borde)
            let mut repel_v = Vector::new(closest_point.x - center.x, closest_point.y - center.y);
            let mag = (repel_v.x * repel_v.x + repel_v.y * repel_v.y).sqrt();
            if mag > 0.1 {
                // Reducido a 3.0 para que sea mucho más sutil
                repel_v = Vector::new((repel_v.x / mag) * 3.0, (repel_v.y / mag) * 3.0);
            }

            // Interpolamos: Centro (attract) -> Bordes (repel)
            // Cuando capt_factor es 0 (centro), seguimos al mouse al 100%
            target_displacement.x =
                attract_v.x * (1.0 - capture_factor) + repel_v.x * capture_factor;
            target_displacement.y =
                attract_v.y * (1.0 - capture_factor) + repel_v.y * capture_factor;
        } else if dist_to_boundary < radius {
            // --- REPULSIoN EXTERNA ---
            let dx = closest_point.x - mouse_pos.x;
            let dy = closest_point.y - mouse_pos.y;
            let mag = (dx * dx + dy * dy).sqrt();

            if mag > 0.1 {
                // Usamos un exponente mas alto (3.0) para que la fuerza caiga mucho mas rapido con la distancia
                let force = (1.0 - dist_to_boundary / radius).powf(3.0);
                // Reducido a 3.0 para mucho más sutileza
                target_displacement =
                    Vector::new((dx / mag) * force * 3.0, (dy / mag) * force * 3.0);
            }
        }

        // APLICAR INTENSIDAD PROGRESIVA A LA FUERZA
        let target_repulsion = Vector::new(
            target_displacement.x * intensity,
            target_displacement.y * intensity,
        );

        // --- 2. FiSICA "LENTA Y TONTA" (Aceleracion minima + Mucha viscosidad) ---
        let current_pos = self.current_repulsion.get();
        let mut current_vel = self.current_velocity.get();

        // Aceleracion bajisima (0.005): Tarda una eternidad en empezar a moverse
        let accel_x = (target_repulsion.x - current_pos.x) * 0.005;
        let accel_y = (target_repulsion.y - current_pos.y) * 0.005;

        current_vel.x += accel_x;
        current_vel.y += accel_y;

        // Friccion muy alta (0.94): Se siente como si estuviera en almibar, flota mucho
        current_vel.x *= 0.94;
        current_vel.y *= 0.94;

        let next_repulsion =
            Vector::new(current_pos.x + current_vel.x, current_pos.y + current_vel.y);

        self.current_velocity.set(current_vel);
        self.current_repulsion.set(next_repulsion);

        // --- 3. JITTER "CANSADO" (Frecuencia bajisima) ---
        let center_dist = mouse_pos.distance(bounds.center());
        let jitter_multiplier = (1.0 + (center_dist / 200.0)).min(2.5);

        let jitter = Vector::new(
            (time * 3.5 + offset.x * 0.1).sin() * 0.15 * jitter_multiplier,
            (time * 4.2 + offset.y * 0.13).cos() * 0.15 * jitter_multiplier,
        );

        Vector::new(
            next_repulsion.x + jitter.x + offset.x,
            next_repulsion.y + jitter.y + offset.y,
        )
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
    is_resizing: bool, // <--- NUEVO CAMPO
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
            let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, i + 10);

            let wrapped_item = Element::new(SmoothTranslate::new(
                item,
                (vx, vy),
                ctx.mouse_pos,
                false,
                ctx.lsd_intensity,
                ctx.lsd_enabled,
            ).resizing(ctx.is_resizing));

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
            let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, i + 12);

            let wrapped_item = Element::new(SmoothTranslate::new(
                item,
                (vx, vy),
                ctx.mouse_pos,
                false,
                ctx.lsd_intensity,
                ctx.lsd_enabled,
            ).resizing(ctx.is_resizing));

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
        // Onda mucho más sincronizada (0.08 en lugar de 0.5) para que las letras se muevan juntas
        let t = ctx.time * 1.8 + (i as f32 * 0.08);
        let off_x = (t * 1.2).sin() * 2.0;
        let off_y = (t * 0.8).cos() * 2.0;

        let char_el = iced::widget::text(c.to_string())
            .size(14)
            .font(iced::font::Font::MONOSPACE)
            // Color neón que resalta sobre el fondo
            .color(Color::from_rgb(1.0, 0.4, 0.2));

        row = row.push(Element::new(SmoothTranslate::new(
            char_el.into(),
            (off_x, off_y),
            ctx.mouse_pos,
            false, // Que se mueva siempre
            1.0,   // Fuerza máxima
            true,  // Ignorar modo global OFF
        ).resizing(ctx.is_resizing)));
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

// Texto pequeño de tamaño 12 con color primario
pub fn text_small<'a, M: 'a>(
    content: impl Into<String>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    text_lsd_letters(content, 12, ctx.palette.text_primary, ctx)
}

// Texto pequeño de tamaño 12 con color secundario/muted
pub fn text_muted<'a, M: 'a>(
    content: impl Into<String>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    text_lsd_letters(content, 12, ctx.palette.text_secondary, ctx)
}

// Texto de tamaño 10 para descripciones muy pequeñas
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
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 10.0,
        },
    }
}

pub fn magic_menu_style<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if ctx.lsd_enabled {
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 9);
        Element::new(SmoothTranslate::new(
            element,
            (vx * ctx.lsd_intensity, vy * ctx.lsd_intensity),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
}

/// Un Dropdown totalmente personalizado que aplica efectos LSD a las letras del menu.
/// Retorna: (Boton Activador, Opcional[El Menu Flotante ya configurado])
pub fn custom_dropdown<'a, M>(
    current_label: String,
    is_open: bool,
    on_toggle: M,              // El mensaje al hacer click en el activador
    options: Vec<(String, M)>, // Lista de tuplas: (Texto a mostrar, Mensaje al clickear)
    is_compact: bool,
    ctx: UIContext,
) -> (
    Element<'a, M, Theme, Renderer>,
    Option<Element<'a, M, Theme, Renderer>>,
)
where
    M: 'a + Clone,
{
    let palette = ctx.palette;

    // 1. Crear el boton Activador (Trigger) con letras LSD
    let trigger_content = iced::widget::row![
        // Usamos text_body (que ya tiene text_lsd_letters)
        container(text_body(current_label, ctx)).width(Length::Fill),
        iced_svg(iced::widget::svg::Handle::from_memory(
            if is_open {
                crate::util::icons::X
            } else {
                crate::util::icons::CHEVRON_RIGHT
            }
            .as_bytes()
        ))
        .width(12)
        .height(12)
        .style(move |_, _| iced::widget::svg::Style {
            color: Some(palette.text_secondary)
        })
    ]
    .align_y(iced::Alignment::Center);

    let trigger = magic_button(
        button(trigger_content)
            .on_press(on_toggle)
            .width(if is_compact {
                Length::Fill
            } else {
                Length::Fixed(180.0)
            })
            .padding(10)
            .style(move |t, s| dropdown_trigger_style(&palette, t, s))
            .into(),
        ctx,
    )
    .into();

    // 2. Si esta cerrado, devolver None en la segunda parte
    if !is_open {
        return (trigger, None);
    }

    // 3. Construir el contenido del menu (La lista de opciones)
    let mut options_col = column![].spacing(2);

    for (label, msg) in options {
        // Aqui esta la MAGIA: Usamos `text_body` (LSD) dentro de los botones del menu
        let btn = button(
            container(text_body(label, ctx))
                .width(Length::Fill)
                .padding(5),
        )
        .on_press(msg)
        .width(Length::Fill)
        .style(move |t, s| ghost_button_style(&palette, t, s)); // Estilo sutil

        options_col = options_col.push(magic_button(btn.into(), ctx));
    }

    // Calcular altura aproximada
    let count = options_col.children().len();
    let calculated_height = (count as f32 * 35.0 + 10.0).clamp(40.0, 300.0);

    let menu_container = container(magic_container(
        magic_scrollable(scrollable(options_col).into(), ctx).into(),
        ctx,
    ))
    .width(if is_compact {
        Length::Fill
    } else {
        Length::Fixed(180.0)
    })
    .height(Length::Fixed(calculated_height))
    .padding(5)
    // CAMBIO AQUi: Usamos un nuevo estilo con acento
    .style(move |t| dropdown_menu_style(&palette, t));

    (trigger, Some(menu_container.into()))
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
            is_resizing: false, // Valor por defecto, se sobreescribe en magic_*
        }
    }
    
    // Método helper para encadenar
    pub fn resizing(mut self, is_resizing: bool) -> Self {
        self.is_resizing = is_resizing;
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
        let bounds = layout.bounds();

        // [OPTIMIZATION] SOPHISTICATED CULLING (Culling con Margen)
        // 1. Margen de seguridad: El efecto LSD mueve los objetos visualmente fuera de sus bounds reales.
        //    Si hacemos un culling estricto, los objetos parpadearán al entrar en pantalla.
        //    Añadimos 100px extra (suficiente para cubrir repulsión + jitter).
        let safe_margin = 100.0; 
        
        let visible_area = Rectangle {
            x: viewport.x - safe_margin,
            y: viewport.y - safe_margin,
            width: viewport.width + (safe_margin * 2.0),
            height: viewport.height + (safe_margin * 2.0),
        };

        // 2. Comprobación rápida AABB (Axis-Aligned Bounding Box)
        // Si no está en el área expandida, abortamos inmediatamente.
        // Esto ahorra:
        // - El cálculo pesado de físicas en calculate_displacement (CPU)
        // - El envío de vértices a WGPU (GPU)
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
            self.is_resizing, // <--- PASAR AQUÍ
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
            self.is_resizing, // <--- PASAR AQUÍ
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
            self.is_resizing, // <--- PASAR AQUÍ
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

fn get_seeded_disparity(offset: (f32, f32), seed: usize) -> (f32, f32) {
    let s = seed as f32;

    // 1. Ruido pseudo-aleatorio estático para dispersión (fijo por seed)
    // Reducción drástica (0.4) para que las letras no se desordenen
    let scatter_x = (s * 12.9898).sin().fract() * 0.4 - 0.2;
    let scatter_y = (s * 78.233).sin().fract() * 0.4 - 0.2;

    // 2. Modulacion caotica del offset dinamico
    // Reducimos la influencia caótica (cos/sin del offset) para un movimiento más de "bloque"
    let phase = (s * 0.15).fract() * 6.28; // Fase con menos variación entre letras
    let chaotic_offset_x = (offset.0 * (phase + offset.1 * 0.02).cos()
        - offset.1 * (phase + offset.0 * 0.02).sin())
        * 0.85;
    let chaotic_offset_y = (offset.0 * (phase + offset.1 * 0.02).sin()
        + offset.1 * (phase + offset.0 * 0.02).cos())
        * 0.85;

    (scatter_x + chaotic_offset_x, scatter_y + chaotic_offset_y)
}

pub fn text<'a, M: 'a>(
    content: iced::widget::Text<'a, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    let element: Element<'a, M, Theme, Renderer> = content.into();
    if ctx.lsd_enabled {
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 1);
        let vx = vx * ctx.lsd_intensity;
        let vy = vy * ctx.lsd_intensity;

        Element::new(SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
}

// NUEVA FUNCIoN: Aplica efecto LSD letra por letra a un string
// Acepta cualquier tipo de string (String, &str, etc.) para evitar problemas de lifetime
pub fn text_lsd_letters<'a, M: 'a>(
    text_str: impl Into<String>,
    size: impl Into<iced::Pixels>,
    color: Color,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer> {
    let text_owned = text_str.into();
    let size_px = size.into();

    if !ctx.lsd_enabled {
        return iced_text(text_owned).size(size_px).color(color).into();
    }

    let lsd_intensity = ctx.lsd_intensity;
    let lsd_enabled = ctx.lsd_enabled;
    let mouse_pos = ctx.mouse_pos;

    // EFECTO LETRA POR LETRA
    let mut letter_row = iced::widget::row!().spacing(0);

    // Iterar sobre cada caracter y aplicar el efecto LSD individualmente
    for (i, ch) in text_owned.chars().enumerate() {
        // Crear un texto individual para este caracter
        let char_text = iced_text(ch.to_string()).size(size_px).color(color);

        let char_element: Element<'a, M, Theme, Renderer> = char_text.into();

        // Calcular offset unico para cada letra usando el indice como seed
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 100 + i);
        let vx = vx * ctx.lsd_intensity;
        let vy = vy * ctx.lsd_intensity;

        // Envolver cada letra en su propio SmoothTranslate
        let magic_char = Element::new(SmoothTranslate::new(
            char_element,
            (vx, vy),
            mouse_pos,
            false,
            lsd_intensity,
            lsd_enabled,
        ).resizing(ctx.is_resizing));

        letter_row = letter_row.push(magic_char);
    }

    letter_row.into()
}

pub fn svg<'a, M: 'a>(content: impl Into<Element<'a, M>>, ctx: UIContext) -> Element<'a, M> {
    let element = content.into();
    if ctx.lsd_enabled {
        let (vx, vy) = get_seeded_disparity((ctx.lsd_offset.0 + 0.5, ctx.lsd_offset.1 + 0.5), 15);
        let vx = vx * ctx.lsd_intensity;
        let vy = vy * ctx.lsd_intensity;

        Element::new(SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
}

pub fn magic_pick_list_with_menu<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if ctx.lsd_enabled {
        // Usamos una semilla unica (2) e intensidad plena
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 2);
        let vx = vx * ctx.lsd_intensity;
        let vy = vy * ctx.lsd_intensity;

        Element::new(SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
}
pub fn magic_text_input<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if ctx.lsd_enabled {
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 3);
        let vx = vx * ctx.lsd_intensity;
        let vy = vy * ctx.lsd_intensity;

        Element::new(SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
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
            // CORRECCIÓN E0533: Focused tiene campos internos
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
        // CORRECCIÓN E0560: Eliminados campos 'icon' y 'cursor' que no existen en el struct Style
        value: palette.text_primary,
        placeholder: palette.text_secondary,
        selection: Color::from_rgba(palette.accent.r, palette.accent.g, palette.accent.b, 0.2),
    }
}

// 2. Actualizar la función para que se comporte como un área real
pub fn magic_text_area<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if ctx.lsd_enabled {
        // Usamos una semilla diferente (11) para que el área de texto se mueva
        // de forma distinta a los inputs pequeños
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 11);

        Element::new(SmoothTranslate::new(
            container(element)
                .width(Length::Fill)
                .height(Length::Fixed(150.0)) // Altura de área de texto real
                .padding(2)
                .into(),
            (vx * ctx.lsd_intensity, vy * ctx.lsd_intensity),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ))
    } else {
        container(element)
            .height(Length::Fixed(150.0)) // Mantener altura incluso sin LSD
            .into()
    }
}
pub fn magic_button<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if ctx.lsd_enabled {
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 4);
        Element::new(SmoothTranslate::new(
            element,
            (vx * ctx.lsd_intensity, vy * ctx.lsd_intensity),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
}

pub fn magic_checkbox<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if ctx.lsd_enabled {
        // Semilla 5 para checkboxes
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 5);
        Element::new(SmoothTranslate::new(
            element,
            (vx * ctx.lsd_intensity, vy * ctx.lsd_intensity),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
}

pub fn magic_image<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if ctx.lsd_enabled {
        // Semilla 1.5 para imagenes (similar a svg)
        let (vx, vy) = get_seeded_disparity((ctx.lsd_offset.0 + 0.3, ctx.lsd_offset.1 + 0.3), 1);
        Element::new(SmoothTranslate::new(
            element,
            (vx * ctx.lsd_intensity, vy * ctx.lsd_intensity),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
}

pub fn magic_container<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if ctx.lsd_enabled {
        // Semilla 6 para contenedores (mas sutil)
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 6);
        let vx = vx * ctx.lsd_intensity;
        let vy = vy * ctx.lsd_intensity;

        Element::new(SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
}

pub fn magic_scrollable<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    // SIEMPRE envolvemos en SmoothTranslate para mantener el árbol de widgets estable.
    // Si lsd_enabled es false, SmoothTranslate simplemente no aplicará efectos,
    // pero el widget "padre" seguirá siendo el mismo para el motor de Iced.
    let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 5);
    Element::new(SmoothTranslate::new(
        element,
        (vx * ctx.lsd_intensity, vy * ctx.lsd_intensity),
        ctx.mouse_pos,
        !ctx.lsd_enabled, // proximity_only si está desactivado
        ctx.lsd_intensity,
        ctx.lsd_enabled,
    ))
}

pub fn magic_slider<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if ctx.lsd_enabled {
        // Semilla 7 para sliders (vibrante)
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 7);
        Element::new(SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
}

pub fn magic_tooltip<'a, M>(
    element: Element<'a, M, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, M, Theme, Renderer>
where
    M: 'a + Clone,
{
    if ctx.lsd_enabled {
        // Semilla 8 para tooltips
        let (vx, vy) = get_seeded_disparity(ctx.lsd_offset, 8);
        Element::new(SmoothTranslate::new(
            element,
            (vx, vy),
            ctx.mouse_pos,
            false,
            ctx.lsd_intensity,
            ctx.lsd_enabled,
        ).resizing(ctx.is_resizing))
    } else {
        element
    }
}

pub fn dropdown_menu_style(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        // Fondo muy oscuro (casi negro) para que resalte sobre el modal gris
        background: Some(Background::Color(Color {
            r: 0.05,
            g: 0.05,
            b: 0.07,
            a: 1.0,
        })),
        // BORDE DEL COLOR DE ACENTO (verde/naranja/etc)
        border: Border {
            color: palette.accent, // <--- Aqui esta el cambio visual clave
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow {
            color: Color::BLACK,
            offset: Vector::new(0.0, 5.0),
            blur_radius: 15.0,
        },
        text_color: Some(palette.text_primary),
        ..Default::default()
    }
}

// --- NUEVOS ESTILOS PARA WINDOW FRAME & TITLE BAR ---

pub fn window_frame_style(_palette: &Palette, _t: &Theme, is_maximized: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::TRANSPARENT)), // El fondo lo maneja el contenido interno
        border: Border {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            width: if is_maximized { 0.0 } else { 1.0 }, // Sin borde si está maximizado
            radius: if is_maximized { 0.0.into() } else { 10.0.into() }, // Redondeado solo si es ventana
        },
        // OPTIMIZACIÓN: Reducir blur_radius de 15.0 a 5.0 para mejorar rendimiento
        shadow: if is_maximized {
            Shadow::default()
        } else {
            Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.4),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 5.0, // Reducido para rendimiento
            }
        },
        ..Default::default()
    }
}

pub fn title_bar_style(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        // Fondo semitransparente oscuro para que el texto se lea bien sobre imágenes claras
        background: Some(Background::Color(Color::from_rgba(0.05, 0.05, 0.08, 0.8))), 
        text_color: Some(palette.text_primary),
        ..Default::default()
    }
}

// Botón de control de ventana (Min/Max/Close)
pub fn window_control_button_style(
    palette: &Palette,
    _t: &Theme,
    status: button::Status,
    is_close: bool,
) -> button::Style {
    let base = button::Style {
        background: None,
        text_color: palette.text_secondary,
        ..Default::default()
    };

    match status {
        button::Status::Hovered | button::Status::Pressed => {
            if is_close {
                button::Style {
                    background: Some(Background::Color(Color::from_rgb(0.9, 0.2, 0.2))), // Rojo al hover
                    text_color: Color::WHITE,
                    ..base
                }
            } else {
                button::Style {
                    background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1))),
                    text_color: palette.text_primary,
                    ..base
                }
            }
        },
        _ => base,
    }
}

// Helper widget para botones de ventana
pub fn window_control_button<'a, Message: Clone + 'a>(
    icon_svg: &'static str,
    msg: Message,
    is_close: bool,
    palette: &'a Palette,
    ctx: UIContext
) -> Element<'a, Message, Theme, Renderer> {
    let btn = button(
        container(
            iced_svg(crate::util::icons::icon(icon_svg))
                .width(12)
                .height(12)
                .style(move |_t, _s| iced::widget::svg::Style {
                    color: Some(if is_close { 
                        Color::WHITE // Icono blanco para el rojo
                    } else {
                        palette.text_primary
                    }), 
                })
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
    )
    .on_press(msg)
    .width(45) // Ancho estándar de botón de ventana
    .height(32) // Altura completa de la barra
    .style(move |t, s| window_control_button_style(palette, t, s, is_close));

    // Aplicar LSD sutil (solo movimiento, no "derretimiento" excesivo para que sean clickeables)
    if ctx.lsd_enabled && !is_close {
        magic_button(btn.into(), ctx)
    } else {
        btn.into()
    }
}
