use iced::overlay::menu;
use iced::widget::{
    button, checkbox, column, container, pick_list, progress_bar, row as iced_row,
    scrollable, slider, text as iced_text, text_input, Space, 
};
use iced::{
    Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Shadow, Size, Theme,
    Vector,
};

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::event::Event;
use std::cell::Cell;

// --- CONSTANTS ---
pub const ACCENT_GREEN: Color = Color::from_rgb(0.2, 0.8, 0.2);
pub const STANDARD_PADDING: f32 = 20.0;
pub const STANDARD_SPACING: u32 = 15;

pub const LSD_RAMP_UP_SECONDS: f32 = 5.0;

#[derive(Debug, Clone, Copy)]
pub struct UIContext {
    pub palette: Palette,
    pub lsd_offset: (f32, f32),
    pub lsd_enabled: bool,
    pub lsd_intensity: f32, // Factor de 0.0 a 1.0 (activación progresiva)
    pub time: f32,
    pub mouse_pos: Point,     // Posicion real del raton para efectos magneticos
    pub mouse_stillness: f32, // 0.0 (se mueve) a 1.0 (quieto por X segundos)
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

// --- WIDGET EFFECT SYSTEM ---

#[derive(Debug, Default)]
struct WidgetEffectState {
    smoothed_stillness: Cell<f32>,
    last_mouse_pos: Cell<iced::Point>,
    last_time: Cell<f32>,
    current_repulsion: Cell<Vector>,
    current_velocity: Cell<Vector>, // Para inercia de masa (reaccion tardia)
    intensity: Cell<f32>,           // Estado persistente de intensidad
}

impl WidgetEffectState {
    pub fn new() -> Self {
        Self {
            smoothed_stillness: Cell::new(0.0),
            last_mouse_pos: Cell::new(iced::Point::ORIGIN),
            last_time: Cell::new(0.0),
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
    ) -> Vector {
        if !lsd_enabled {
            return Vector::new(0.0, 0.0);
        }

        // Actualizamos intensidad en el estado
        self.intensity.set(intensity);

        // --- 1. LOGICA DE REPULSION (Bordes) + ATRACCION (Centro) ---
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
            // --- COMENZAR "CAPTURA" AL ACERCARSE AL CENTRO ---
            // Radio de pegado: 45% de la dimension minima
            let capture_radius = bounds.width.min(bounds.height) * 0.45;
            // 0.0 en el centro exacto, 1.0 en el borde del radio de captura
            let capture_factor = (center_dist / capture_radius.max(5.0)).clamp(0.0, 1.0);

            // Vector de atraccion (seguir al mouse)
            let attract_v = Vector::new(mouse_pos.x - center.x, mouse_pos.y - center.y);

            // Vector de repulsion interna (empujar hacia el borde)
            let mut repel_v = Vector::new(closest_point.x - center.x, closest_point.y - center.y);
            let mag = (repel_v.x * repel_v.x + repel_v.y * repel_v.y).sqrt();
            if mag > 0.1 {
                // Reducido a 8.0 para que sea un empujoncito leve
                repel_v = Vector::new((repel_v.x / mag) * 8.0, (repel_v.y / mag) * 8.0);
            }

            // Interpolamos: Centro (attract) -> Bordes (repel)
            // Cuando capt_factor es 0 (centro), seguimos al mouse al 100%
            target_displacement.x =
                attract_v.x * (1.0 - capture_factor) + repel_v.x * capture_factor;
            target_displacement.y =
                attract_v.y * (1.0 - capture_factor) + repel_v.y * capture_factor;
        } else if dist_to_boundary < radius {
            // --- REPULSION EXTERNA ---
            let dx = closest_point.x - mouse_pos.x;
            let dy = closest_point.y - mouse_pos.y;
            let mag = (dx * dx + dy * dy).sqrt();

            if mag > 0.1 {
                // Usamos un exponente mas alto (3.0) para que la fuerza caiga mucho mas rapido con la distancia
                let force = (1.0 - dist_to_boundary / radius).powf(3.0);
                // Reducimos el multiplicador de 30.0 a 12.0
                target_displacement =
                    Vector::new((dx / mag) * force * 12.0, (dy / mag) * force * 12.0);
            }
        }

        // APLICAR INTENSIDAD PROGRESIVA A LA FUERZA
        let target_repulsion = Vector::new(
            target_displacement.x * intensity,
            target_displacement.y * intensity,
        );

        // --- 2. FISICA "LENTA Y TONTA" (Aceleracion minima + Mucha viscosidad) ---
        let current_pos = self.current_repulsion.get();
        let mut current_vel = self.current_velocity.get();

        // Aceleracion bajisima (0.005): Tarda una eternidad en empezar a moverse
        let accel_x = (target_repulsion.x - current_pos.x) * 0.005;
        let accel_y = (target_repulsion.y - current_pos.y) * 0.005;

        current_vel.x += accel_x;
        current_vel.y += accel_y;

        // Friccion muy alta (0.94): Se siente como si estuviera en almiar, flota mucho
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
            (time * 0.4).sin() * 0.1 * jitter_multiplier,
            (time * 0.3).cos() * 0.1 * jitter_multiplier,
        );

        Vector::new(next_repulsion.x + jitter.x, next_repulsion.y + jitter.y)
    }
}

// Widget wrapper for applying effects
pub struct WidgetEffect<'a, Message> {
    content: Element<'a, Message, Theme, Renderer>,
    offset_seed: (f32, f32),      // To make each widget unique
    mouse_pos: iced::Point,       // Current mouse position
    proximity_only: bool,         // Only apply effect when mouse is near
    time: f32,                    // Global time from main.rs
    effect_type: EffectType,      // Type of effect to apply
}

#[derive(Debug, Clone, Copy)]
pub enum EffectType {
    Translate,      // Smooth translation effect
    Scale,          // Scale effect
    Rotate,         // Rotation effect
    Glow,           // Glow/hover effect
    Combined,       // Combination of effects
}

impl<'a, Message> WidgetEffect<'a, Message> {
    pub fn new(
        content: Element<'a, Message, Theme, Renderer>,
        offset_seed: (f32, f32),
        mouse_pos: iced::Point,
        proximity_only: bool,
        time: f32,
        effect_type: EffectType,
    ) -> Self {
        Self {
            content,
            offset_seed,
            mouse_pos,
            proximity_only,
            time,
            effect_type,
        }
    }
}

impl<'a, Message> Widget<Message, Theme, Renderer> for WidgetEffect<'a, Message> {
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
        widget::tree::Tag::of::<WidgetEffectState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(WidgetEffectState::new())
    }

    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut widget::Tree) {
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
        let state = tree.state.downcast_ref::<WidgetEffectState>();
        let bounds = layout.bounds();

        // Apply effect only if enabled and not in proximity-only mode
        if self.proximity_only && (!self.lsd_enabled || self.lsd_intensity < 0.1) {
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

        // Calculate displacement based on effect type
        let displacement = state.calculate_displacement(
            self.mouse_pos,
            bounds,
            self.time,
            self.lsd_intensity,
            true,
        );

        // Apply transformation based on effect type
        match self.effect_type {
            EffectType::Translate => {
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
            EffectType::Scale => {
                // Calculate scale factor based on distance from mouse
                let distance = self.mouse_pos.distance(bounds.center());
                let scale_factor = 1.0 + (0.1 * self.lsd_intensity * (1.0 - (distance / 200.0).min(1.0)));
                
                renderer.with_layer(bounds, |r| {
                    r.with_scale(scale_factor, |scaled_renderer| {
                        self.content.as_widget().draw(
                            &tree.children[0],
                            scaled_renderer,
                            theme,
                            style,
                            layout,
                            cursor,
                            viewport,
                        );
                    });
                });
            }
            EffectType::Rotate => {
                // Calculate rotation based on time and mouse position
                let rotation_angle = (self.time * 0.5 + self.mouse_pos.x * 0.01) * self.lsd_intensity;
                let cos = rotation_angle.cos();
                let sin = rotation_angle.sin();
                let matrix = [
                    [cos, -sin, 0.0],
                    [sin, cos, 0.0],
                    [0.0, 0.0, 1.0],
                ];
                
                renderer.with_layer(bounds, |r| {
                    r.with_transform(matrix, |transformed_renderer| {
                        self.content.as_widget().draw(
                            &tree.children[0],
                            transformed_renderer,
                            theme,
                            style,
                            layout,
                            cursor,
                            viewport,
                        );
                    });
                });
            }
            EffectType::Glow => {
                // Draw content normally, but potentially with glow effects
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    layout,
                    cursor,
                    viewport,
                );
            }
            EffectType::Combined => {
                // Apply combined effects
                renderer.with_translation(displacement, |r| {
                    let distance = self.mouse_pos.distance(bounds.center());
                    let scale_factor = 1.0 + (0.05 * self.lsd_intensity * (1.0 - (distance / 200.0).min(1.0)));
                    
                    r.with_scale(scale_factor, |scaled_renderer| {
                        let rotation_angle = (self.time * 0.3 + self.mouse_pos.x * 0.005) * self.lsd_intensity;
                        let cos = rotation_angle.cos();
                        let sin = rotation_angle.sin();
                        let matrix = [
                            [cos, -sin, 0.0],
                            [sin, cos, 0.0],
                            [0.0, 0.0, 1.0],
                        ];
                        
                        scaled_renderer.with_transform(matrix, |transformed_renderer| {
                            self.content.as_widget().draw(
                                &tree.children[0],
                                transformed_renderer,
                                theme,
                                style,
                                layout,
                                cursor,
                                viewport,
                            );
                        });
                    });
                });
            }
        }
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
        let state = tree.state.downcast_ref::<WidgetEffectState>();
        let bounds = layout.bounds();

        // Calculate the displacement for interaction alignment
        let displacement = state.calculate_displacement(
            self.mouse_pos,
            bounds,
            self.time,
            self.lsd_intensity,
            !self.proximity_only,
        );

        // Adjust cursor position for interaction with transformed content
        let offset_cursor = match cursor.position() {
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
            offset_cursor,
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
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
    ) -> Option<iced::overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer)
    }
}

// --- MAGIC WRAPPERS ---

/// Magic button with animated effects
pub fn magic_button<'a, Message: Clone + 'a>(
    element: Element<'a, Message, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, Message, Theme, Renderer> {
    if ctx.lsd_enabled {
        let seed = get_seeded_disparity(ctx.lsd_offset, 1);
        Element::new(WidgetEffect::new(
            element,
            seed,
            ctx.mouse_pos,
            false,
            ctx.time,
            EffectType::Combined,
        ))
    } else {
        element
    }
}

/// Magic container with animated effects
pub fn magic_container<'a, Message: Clone + 'a>(
    element: Element<'a, Message, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, Message, Theme, Renderer> {
    if ctx.lsd_enabled {
        let seed = get_seeded_disparity(ctx.lsd_offset, 2);
        Element::new(WidgetEffect::new(
            element,
            seed,
            ctx.mouse_pos,
            false,
            ctx.time,
            EffectType::Translate,
        ))
    } else {
        element
    }
}

/// Magic column with animated effects for each child
pub fn magic_column<'a, Message: Clone + 'a>(
    items: Vec<Element<'a, Message, Theme, Renderer>>,
    ctx: UIContext,
) -> iced::widget::Column<'a, Message, Theme, Renderer> {
    let mut col = iced::widget::column!()
        .spacing(STANDARD_SPACING)
        .width(Length::Fill);

    if ctx.lsd_enabled {
        for (i, item) in items.into_iter().enumerate() {
            let seed = get_seeded_disparity(ctx.lsd_offset, i + 10);
            
            let wrapped_item = Element::new(WidgetEffect::new(
                item,
                seed,
                ctx.mouse_pos,
                false,
                ctx.time,
                EffectType::Translate,
            ));

            col = col.push(wrapped_item);
        }
    } else {
        for item in items {
            col = col.push(item);
        }
    }

    col
}

/// Magic row with animated effects for each child
pub fn magic_row<'a, Message: Clone + 'a>(
    items: Vec<Element<'a, Message, Theme, Renderer>>,
    ctx: UIContext,
) -> iced::widget::Row<'a, Message, Theme, Renderer> {
    let mut row = iced::widget::row!()
        .spacing(STANDARD_SPACING)
        .width(Length::Fill);

    if ctx.lsd_enabled {
        for (i, item) in items.into_iter().enumerate() {
            let seed = get_seeded_disparity(ctx.lsd_offset, i + 12);
            
            let wrapped_item = Element::new(WidgetEffect::new(
                item,
                seed,
                ctx.mouse_pos,
                false,
                ctx.time,
                EffectType::Translate,
            ));

            row = row.push(wrapped_item);
        }
    } else {
        for item in items {
            row = row.push(item);
        }
    }

    row
}

/// Magic scrollable with animated effects
pub fn magic_scrollable<'a, Message: Clone + 'a>(
    content: iced::widget::Column<'a, Message, Theme, Renderer>,
    ctx: UIContext,
) -> Element<'a, Message, Theme, Renderer> {
    let scrollable_element = scrollable(content);
    if ctx.lsd_enabled {
        let seed = get_seeded_disparity(ctx.lsd_offset, 3);
        Element::new(WidgetEffect::new(
            scrollable_element.into(),
            seed,
            ctx.mouse_pos,
            false,
            ctx.time,
            EffectType::Translate,
        ))
    } else {
        scrollable_element.into()
    }
}

// --- HELPER FUNCTIONS ---

/// Generate a seeded disparity value for animation uniqueness
pub fn get_seeded_disparity(seed: (f32, f32), index: usize) -> (f32, f32) {
    let x = (seed.0 + index as f32 * 0.618) % 1.0;
    let y = (seed.1 + index as f32 * 0.382) % 1.0;
    (x * 2.0 - 1.0, y * 2.0 - 1.0) // Convert to range [-1, 1]
}

/// Apply smooth interpolation (lerp) between two values
pub fn lerp(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t.clamp(0.0, 1.0)
}

// --- TEXT STYLING FUNCTIONS ---

/// Text with applied styling
pub fn text<'a, Message: Clone + 'a>(
    content: iced::widget::Text<'a, Theme>,
    ctx: UIContext,
) -> Element<'a, Message, Theme, Renderer> {
    if ctx.lsd_enabled {
        let seed = get_seeded_disparity(ctx.lsd_offset, 100);
        let wrapped_content = Element::from(content);
        Element::new(WidgetEffect::new(
            wrapped_content,
            seed,
            ctx.mouse_pos,
            true, // proximity_only for text
            ctx.time,
            EffectType::Glow,
        ))
    } else {
        Element::from(content)
    }
}

/// SVG with applied styling
pub fn svg<'a, Message: Clone + 'a>(
    content: iced::widget::Svg<Renderer>,
    ctx: UIContext,
) -> Element<'a, Message, Theme, Renderer> {
    if ctx.lsd_enabled {
        let seed = get_seeded_disparity(ctx.lsd_offset, 101);
        let wrapped_content = Element::from(content);
        Element::new(WidgetEffect::new(
            wrapped_content,
            seed,
            ctx.mouse_pos,
            true, // proximity_only for icons
            ctx.time,
            EffectType::Glow,
        ))
    } else {
        Element::from(content)
    }
}

// --- CONTAINER STYLES ---

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

pub fn container_style_transparent(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        background: None,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
        text_color: Some(palette.text_primary),
        ..Default::default()
    }
}

pub fn play_button_style(
    palette: &Palette,
    _t: &Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(palette.accent)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: palette.text_on_accent,
        ..Default::default()
    }
}

pub fn play_button_style_active(
    palette: &Palette,
    _t: &Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color {
            r: palette.accent.r * 0.8,
            g: palette.accent.g * 0.8,
            b: palette.accent.b * 0.8,
            a: palette.accent.a,
        })),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: palette.text_on_accent,
        ..Default::default()
    }
}

pub fn update_button_style(
    palette: &Palette,
    _t: &Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::from_rgb(0.1, 0.6, 0.9))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: palette.text_on_accent,
        ..Default::default()
    }
}

pub fn secondary_button_style(
    palette: &Palette,
    _t: &Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(palette.surface)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: palette.text_primary,
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

pub fn icon_button_style(
    palette: &Palette,
    _t: &Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        text_color: palette.accent,
        ..Default::default()
    }
}

pub fn checkbox_style(
    palette: &Palette,
    _t: &Theme,
    _status: iced::widget::checkbox::Status,
) -> iced::widget::checkbox::Style {
    iced::widget::checkbox::Style {
        background: Background::Color(palette.surface),
        border: Border {
            color: palette.text_secondary,
            width: 1.0,
            radius: 4.0.into(),
        },
        icon_color: palette.accent,
        text_color: Some(palette.text_primary),
    }
}

pub fn scrollable_style(palette: &Palette, _t: &Theme) -> scrollable::Style {
    scrollable::Style {
        container: container::Style {
            background: None,
            ..Default::default()
        },
        vertical_rail: iced::scrollable::Rail {
            background: Some(Background::Color(Color::from_rgba(0.5, 0.5, 0.5, 0.2))),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            scroller: iced::scrollable::Scroller {
                color: palette.text_secondary,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 4.0.into(),
                },
            },
        },
        horizontal_rail: iced::scrollable::Rail {
            background: Some(Background::Color(Color::from_rgba(0.5, 0.5, 0.5, 0.2))),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            scroller: iced::scrollable::Scroller {
                color: palette.text_secondary,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 4.0.into(),
                },
            },
        },
        gap: None,
    }
}

pub fn slider_style(palette: &Palette, _t: &Theme) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            colors: (palette.surface, palette.accent),
            width: 4.0.into(),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 10.0 },
            color: palette.accent,
            border_width: 1.0,
            border_color: palette.surface,
        },
    }
}

pub fn pick_list_style(
    palette: &Palette,
    _t: &Theme,
    _status: pick_list::Status,
) -> pick_list::Style {
    pick_list::Style {
        text_color: palette.text_primary,
        background: Background::Color(palette.surface),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            width: 1.0,
            radius: 6.0.into(),
        },
        handle: menu::Handle::default(),
        placeholder: palette.text_secondary,
    }
}

pub fn text_input_style(palette: &Palette, _t: &Theme) -> text_input::Style {
    text_input::Style {
        background: Background::Color(palette.surface),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            width: 1.0,
            radius: 6.0.into(),
        },
        icon: palette.text_secondary,
        placeholder: palette.text_secondary,
        value: palette.text_primary,
        selection: Background::Color(Color::from_rgba(0.5, 0.5, 1.0, 0.3)),
    }
}

pub fn progress_bar_style(palette: &Palette, _t: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(palette.surface),
        bar: Background::Color(palette.accent),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
    }
}

pub fn orange_bar_style(palette: &Palette, _t: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(palette.surface),
        bar: Background::Color(Color::from_rgb(1.0, 0.6, 0.2)),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
    }
}

pub fn sub_bar_style(palette: &Palette, _t: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(Color::from_rgba(0.5, 0.5, 0.5, 0.2)),
        bar: Background::Color(Color::from_rgba(0.7, 0.7, 0.7, 0.5)),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
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

pub fn modal_container(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(palette.background)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 20.0,
        },
        ..Default::default()
    }
}

pub fn footer_style(palette: &Palette, _t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(palette.surface)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

// --- UTILITY FUNCTIONS ---

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

pub fn text_title<'a, Message: 'a>(content: &str, ctx: UIContext) -> Element<'a, Message, Theme, Renderer> {
    text(
        iced_text(content.to_string())
            .size(18)
            .color(ctx.palette.accent)
            .font(iced::font::Font::MONOSPACE),
        ctx,
    )
}

pub fn text_body<'a, Message: 'a>(content: &str, ctx: UIContext) -> Element<'a, Message, Theme, Renderer> {
    text(
        iced_text(content.to_string())
            .size(14)
            .color(ctx.palette.text_primary),
        ctx,
    )
}

pub fn text_caption<'a, Message: 'a>(content: &str, ctx: UIContext) -> Element<'a, Message, Theme, Renderer> {
    text(
        iced_text(content.to_string())
            .size(11)
            .color(ctx.palette.text_secondary),
        ctx,
    )
}

/// Una fila estandarizada para listas (usada en Mods, Settings > Storage, etc.)
pub fn list_item<'a, Message: Clone + 'a>(
    left_label: &str,
    right_value: &str,
    ctx: UIContext,
) -> Element<'a, Message, Theme, Renderer> {
    iced_row![
        text_body(left_label, ctx),
        Space::new().width(Length::Fill),
        text_body(right_value, ctx)
    ]
    .align_y(iced::Alignment::Center)
    .padding([10, 15])
    .into()
}