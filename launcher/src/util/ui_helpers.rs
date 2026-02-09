//! UI Helpers to reduce boilerplate in UI components
//! Provides common styling patterns and container configurations

use crate::theme::{UIContext, Palette, STANDARD_PADDING};
use iced::widget::{container, Container};
use iced::{Element, Length, Theme, Renderer};

/// Creates a standard card container with consistent styling
pub fn card_container<'a, Message>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    ctx: UIContext,
) -> Container<'a, Message, Theme, Renderer> {
    container(content)
        .style(move |t| crate::theme::card_style(&ctx.palette, t))
        .padding(STANDARD_PADDING)
}

/// Creates a transparent container for layout purposes
pub fn transparent_container<'a, Message>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    ctx: UIContext,
) -> Container<'a, Message, Theme, Renderer> {
    container(content)
        .style(move |t| crate::theme::container_style_transparent(&ctx.palette, t))
}

/// Creates a centered container that fills available space
pub fn centered_container<'a, Message>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    ctx: UIContext,
) -> Container<'a, Message, Theme, Renderer> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |t| crate::theme::container_style_transparent(&ctx.palette, t))
}

/// Creates a full-width container with standard padding
pub fn full_width_container<'a, Message>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    ctx: UIContext,
) -> Container<'a, Message, Theme, Renderer> {
    container(content)
        .width(Length::Fill)
        .padding(STANDARD_PADDING)
        .style(move |t| crate::theme::container_style_transparent(&ctx.palette, t))
}

/// Creates a loading container with centered content
pub fn loading_container<'a, Message: 'a>(
    loading_text: &str,
    ctx: UIContext,
) -> Container<'a, Message, Theme, Renderer> {
    centered_container(
        crate::theme::text_body(loading_text, ctx),
        ctx
    )
}

/// Creates an error container with centered content
pub fn error_container<'a, Message: 'a>(
    error_text: &str,
    ctx: UIContext,
) -> Container<'a, Message, Theme, Renderer> {
    centered_container(
        crate::theme::text_body(error_text, ctx),
        ctx
    )
}

/// Creates a button container with transparent background
pub fn button_container<'a, Message>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    ctx: UIContext,
) -> Container<'a, Message, Theme, Renderer> {
    container(content)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .style(move |t| crate::theme::container_style_transparent(&ctx.palette, t))
}
