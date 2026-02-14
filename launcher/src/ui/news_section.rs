use crate::news::BlogPost;
use crate::util::image_cache::load_news_image;
use crate::{Message, theme, util};
use iced::widget::{
    ProgressBar, Space, button, column, container, image, row, scrollable, svg, Id,
};
use iced::{
    Alignment, Background, ContentFit, Element, Length, Renderer, Task, Theme,
};
use std::collections::HashMap;

const NEWS_SCROLL_ID: &str = "news_scroll";

#[derive(Debug, Clone)]
pub enum NewsMessage {
    LoadNews,
    NewsLoaded(Result<Vec<BlogPost>, String>),
    ImageLoaded(String, Result<image::Handle, String>),
    ReloadImages, // Nuevo mensaje para recargar imágenes después de liberar memoria
    OpenPost(String),
    OpenAllNews,
    ScrollOffsetChanged(f32), // Nuevo mensaje para tracking de scroll
    GetScrollOffset, // Nuevo mensaje para solicitar el offset actual
}

pub struct NewsSection {
    pub posts: Vec<BlogPost>,
    pub images: HashMap<String, image::Handle>,
    pub loading: bool,
    pub loaded_once: bool, // Nueva bandera para lazy loading
    pub error: Option<String>,
    pub scroll_offset: f32, // Posición actual del scroll
    pub viewport_height: f32, // Altura del viewport visible
}

impl NewsSection {
    pub fn new() -> Self {
        Self {
            posts: Vec::new(),
            images: HashMap::new(),
            loading: false, // Default: NO cargando
            loaded_once: false, // Default: NO ha cargado
            error: None,
            scroll_offset: 0.0,
            viewport_height: 600.0, // Valor por defecto, se actualizará dinámicamente
        }
    }

    // Nuevo método helper para saber si iniciar la carga
    pub fn should_load(&self) -> bool {
        // Cargar si nunca ha cargado, o si hay posts pero no imágenes (ej: después de liberar memoria)
        (!self.loaded_once && !self.loading) || 
        (!self.posts.is_empty() && self.images.is_empty() && !self.loading)
    }

    pub fn update(&mut self, message: NewsMessage, client: reqwest::Client) -> Task<Message> {
        match message {
            NewsMessage::LoadNews => {
                self.loading = true;
                self.loaded_once = true; // Marcar como intentado
                self.error = None;
                
                // Si ya hay posts, solo cargar imágenes faltantes (recuperación de memoria)
                if !self.posts.is_empty() {
                    let mut image_tasks = Vec::new();
                    for post in &self.posts {
                        if let Some(cover) = &post.cover_image {
                            let key = cover.s3_key.clone();
                            if !self.images.contains_key(&key) {
                                let c = client.clone();
                                image_tasks.push(Task::perform(
                                    async move {
                                        let res = load_news_image(&c, &key)
                                            .await
                                            .map_err(|e| e.to_string());
                                        (key, res)
                                    },
                                    |(key, res)| {
                                        Message::News(NewsMessage::ImageLoaded(key, res))
                                    },
                                ));
                            }
                        }
                    }
                    self.loading = false; // No estamos cargando posts, solo imágenes
                    if image_tasks.is_empty() {
                        Task::none()
                    } else {
                        Task::batch(image_tasks)
                    }
                } else {
                    // Carga completa de noticias y imágenes
                    self.images.clear();
                    Task::perform(
                        async move {
                            crate::news::fetch_news(&client)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        |res| Message::News(NewsMessage::NewsLoaded(res)),
                    )
                }
            }
            NewsMessage::NewsLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(posts) => {
                        self.posts = posts;
                        self.error = None;
                        let mut image_tasks = Vec::new();
                        for post in &self.posts {
                            if let Some(cover) = &post.cover_image {
                                let key = cover.s3_key.clone();
                                if !self.images.contains_key(&key) {
                                    let c = client.clone();
                                    image_tasks.push(Task::perform(
                                        async move {
                                            let res = load_news_image(&c, &key)
                                                .await
                                                .map_err(|e| e.to_string());
                                            (key, res)
                                        },
                                        |(key, res)| {
                                            Message::News(NewsMessage::ImageLoaded(key, res))
                                        },
                                    ));
                                }
                            }
                        }
                        if image_tasks.is_empty() {
                            Task::none()
                        } else {
                            Task::batch(image_tasks)
                        }
                    }
                    Err(e) => {
                        self.error = Some(e);
                        Task::none()
                    }
                }
            }
            NewsMessage::ImageLoaded(key, result) => {
                if let Ok(handle) = result {
                    self.images.insert(key, handle);
                }
                Task::none()
            }
            NewsMessage::ReloadImages => {
                // Recargar imágenes si hay posts pero no imágenes (después de liberar memoria)
                if !self.posts.is_empty() && self.images.is_empty() && !self.loading {
                    let mut image_tasks = Vec::new();
                    for post in &self.posts {
                        if let Some(cover) = &post.cover_image {
                            let key = cover.s3_key.clone();
                            let c = client.clone();
                            image_tasks.push(Task::perform(
                                async move {
                                    let res = load_news_image(&c, &key)
                                        .await
                                        .map_err(|e| e.to_string());
                                    (key, res)
                                },
                                |(key, res)| {
                                    Message::News(NewsMessage::ImageLoaded(key, res))
                                },
                            ));
                        }
                    }
                    if image_tasks.is_empty() {
                        Task::none()
                    } else {
                        Task::batch(image_tasks)
                    }
                } else {
                    Task::none()
                }
            }
            NewsMessage::OpenPost(url) => Task::perform(
                async move {
                    let _ = open::that(&url);
                },
                |_| Message::None,
            ),
            NewsMessage::OpenAllNews => Task::perform(
                async move {
                    let _ = open::that("https://hytale.com/news");
                },
                |_| Message::None,
            ),
            NewsMessage::ScrollOffsetChanged(delta) => {
                if delta == f32::MIN {
                    // Home key - resetear al inicio
                    self.scroll_offset = 0.0;
                } else if delta == f32::MAX {
                    // End key - scroll al final
                    let post_height_estimate = 120.0;
                    let post_spacing = 8.0;
                    let total_post_height = post_height_estimate + post_spacing;
                    let total_content_height = self.posts.len() as f32 * total_post_height;
                    self.scroll_offset = (total_content_height - self.viewport_height).max(0.0);
                } else {
                    // Scroll normal - acumular el delta
                    self.scroll_offset += delta;
                    // Asegurar que el offset no sea negativo
                    self.scroll_offset = self.scroll_offset.max(0.0);
                }
                Task::none()
            }
            NewsMessage::GetScrollOffset => {
                // Ya no necesitamos este mensaje con el enfoque de eventos
                Task::none()
            }
        }
    }

    pub fn update_viewport_height(&mut self, window_height: f32) {
        // El viewport de noticias es aproximadamente 70% de la altura de ventana
        // considerando el header, footer y otros elementos UI
        self.viewport_height = (window_height * 0.7).max(300.0); // Mínimo 300px
    }

    pub fn view<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
        is_disabled: bool,
        ctx: theme::UIContext,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let palette = ctx.palette;

        let main_area: Element<'a, NewsMessage, Theme, Renderer> = if self.loading {
            self.view_loading(localization, is_disabled, ctx)
        } else if let Some(error) = &self.error {
            self.view_error(error, localization, is_disabled, ctx)
        } else if self.posts.is_empty() {
            self.view_empty(localization, is_disabled, ctx)
        } else {
            self.view_posts(localization, is_disabled, ctx)
        };

        let inner_content = theme::magic_column(
            vec![
                header_section(self.loading, localization, is_disabled, ctx),
                main_area,
            ],
            ctx,
        );

        container(inner_content)
            .padding(theme::STANDARD_PADDING)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |t: &Theme| theme::news_panel_style(&palette, t))
            .into()
    }

    fn view_loading<'a>(
        &'a self,
        loc: &'a crate::lang::Localization,
        _is_disabled: bool,
        ctx: theme::UIContext,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let palette = ctx.palette;
        container(
            column![
                theme::text_body(loc.t("news.loading"), ctx),
                Space::new().height(20.0),
                container(
                    ProgressBar::new(0.0..=100.0, 50.0)
                        .style(move |t: &Theme| theme::orange_bar_style(&palette, t))
                )
                .width(200)
                .center_x(Length::Fill)
                .style(move |t| theme::container_style_transparent(&palette, t))
            ]
            .align_x(Alignment::Center)
            .spacing(10),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill)
        .style(move |t| theme::container_style_transparent(&palette, t))
        .into()
    }

    fn view_error<'a>(
        &self,
        err: &'a str,
        loc: &'a crate::lang::Localization,
        is_disabled: bool,
        ctx: theme::UIContext,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let palette = ctx.palette;
        container(
            column![
                theme::text_body(loc.t("news.failed"), ctx),
                theme::text_muted(err, ctx),
                {
                    let mut btn = button(theme::text_body(loc.t("news.retry").to_string(), ctx))
                        .style(move |t: &Theme, s| theme::primary_button_style(&palette, t, s))
                        .padding(8);
                    if !is_disabled && ctx.palette.text_primary.a > 0.05 {
                        btn = btn.on_press(NewsMessage::LoadNews);
                    }
                    theme::magic_button(btn.into(), ctx)
                }
            ]
            .spacing(10)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill)
        .style(move |t| theme::container_style_transparent(&palette, t))
        .into()
    }

    fn view_empty<'a>(
        &'a self,
        loc: &'a crate::lang::Localization,
        _is_disabled: bool,
        ctx: theme::UIContext,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let palette = ctx.palette;
        container(theme::text_body(loc.t("news.empty"), ctx))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill)
        .style(move |t| theme::container_style_transparent(&palette, t))
        .into()
    }

    fn view_posts<'a>(
        &'a self,
        loc: &'a crate::lang::Localization,
        is_disabled: bool,
        ctx: theme::UIContext,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let palette = ctx.palette;
        
        // [PRECISE VIEWPORT OPTIMIZATION]
        // Calcular qué posts son visibles basados en el scroll actual
        let post_height_estimate = 120.0; // Altura estimada por post (imagen + texto + padding)
        let post_spacing = 8.0;
        let total_post_height = post_height_estimate + post_spacing;
        
        // Calcular el rango de posts visibles
        let start_index = ((self.scroll_offset / total_post_height).floor() as usize).saturating_sub(1); // Uno extra como buffer
        let visible_count = ((self.viewport_height / total_post_height).ceil() as usize) + 2; // 2 extra como buffer
        
        let end_index = (start_index + visible_count).min(self.posts.len());
        let start_index = start_index.min(self.posts.len().saturating_sub(1));
        
        let posts_to_render: Vec<&BlogPost> = if start_index < self.posts.len() {
            self.posts[start_index..end_index].iter().collect()
        } else {
            Vec::new()
        };
        
        // Crear espacio vacío arriba para mantener la posición del scroll
        let top_space = Space::new().width(Length::Fill).height(Length::Fixed(start_index as f32 * total_post_height));
        
        // Crear espacio vacío abajo para permitir scroll completo
        let bottom_space = Space::new().width(Length::Fill).height(Length::Fixed(
            (self.posts.len().saturating_sub(end_index)) as f32 * total_post_height
        ));
        
        let posts_list = theme::magic_scrollable(
            scrollable(
                column![
                    top_space,
                    column(
                        posts_to_render
                            .iter()
                            .enumerate()
                            .map(|(i, post)| {
                                // Ajustar el índice real para mantener consistencia
                                let real_index = start_index + i;
                                self.view_post_with_index(post, real_index, loc, is_disabled, ctx)
                            })
                            .collect::<Vec<_>>(),
                    )
                    .spacing(post_spacing),
                    bottom_space,
                ]
                .spacing(0),
            )
            .id(Id::new(NEWS_SCROLL_ID))
            .height(Length::Fill)
            .style(move |t: &Theme, s| theme::scrollable_style(&palette, t, s))
            .into(),
            ctx,
        );
        
        if self.error.is_some() {
            column![
                posts_list,
                container({
                    let mut btn = button(theme::text_small(loc.t("news.retry"), ctx))
                        .style(move |t: &Theme, s| theme::secondary_button_style(&palette, t, s))
                        .padding(4);
                    if !is_disabled && ctx.palette.text_primary.a > 0.05 {
                        btn = btn.on_press(NewsMessage::LoadNews);
                    }
                    theme::magic_button(btn.into(), ctx)
                })
                .width(Length::Fill)
                .center_x(Length::Fill)
                .style(move |t| theme::container_style_transparent(&palette, t))
            ]
            .spacing(5)
            .into()
        } else {
            posts_list.into()
        }
    }

    fn view_post_with_index<'a>(
        &'a self,
        post: &'a BlogPost,
        _index: usize, // Índice real del post (para uso futuro si es necesario)
        loc: &'a crate::lang::Localization,
        is_disabled: bool,
        ctx: theme::UIContext,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        self.view_post(post, loc, is_disabled, ctx)
    }

    fn view_post<'a>(
        &'a self,
        post: &'a BlogPost,
        loc: &'a crate::lang::Localization,
        is_disabled: bool,
        ctx: theme::UIContext,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let palette = ctx.palette;
        let image_content: Element<'a, NewsMessage, Theme, Renderer> =
            if let Some(cover) = &post.cover_image {
                if let Some(handle) = self.images.get(&cover.s3_key) {
                    theme::magic_image(
                        image(handle.clone())
                            .width(100)
                            .height(56)
                            .border_radius(8)
                            .content_fit(ContentFit::Cover)
                            .opacity(ctx.palette.text_primary.a) // <--- Agrega esto para desvanecer imagenes
                            .into(),
                        ctx,
                    )
                } else {
                    placeholder_image()
                }
            } else {
                placeholder_image()
            };
        let mut btn = button(
            row![
                container(image_content).style(|_| container::Style {
                    border: crate::theme::image_border_container(),
                    ..Default::default()
                }),
                column![
                    row![
                        theme::svg(
                            svg(util::icons::icon(util::icons::CALENDAR))
                                .width(10)
                                .height(10)
                                .style(move |t: &Theme, s| theme::svg_accent(&palette, t, s))
                                .opacity(ctx.palette.text_primary.a),
                            ctx
                        ),
                        theme::text_micro(post.format_date(), ctx)
                    ]
                    .spacing(4),
                    theme::text_body(&post.title, ctx),
                    
                    // --- CAMBIO AQUi ---
                    // Antes usabas theme::text_caption, ahora usamos theme::text_paragraph
                    // Esto habilita el wrapping multilinea CON el efecto letra por letra
                    theme::text_paragraph(
                        post.body_excerpt
                            .as_deref()
                            .unwrap_or_else(|| loc.t("news.no_desc")),
                        ctx
                    )
                    // -------------------
                ]
                .spacing(2)
                .width(Length::Fill)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
        if !is_disabled && ctx.palette.text_primary.a > 0.05 {
            btn = btn.on_press(NewsMessage::OpenPost(post.get_post_url()));
        }
        theme::magic_button(
            btn.style(move |t: &Theme, s| theme::ghost_button_style(&palette, t, s))
                .width(Length::Fill)
                .padding(8)
                .into(),
            ctx,
        )
    }
}

fn placeholder_image<'a>() -> Element<'a, NewsMessage, Theme, Renderer> {
    container(Space::new().width(100.0).height(56.0))
        .style(|_| crate::theme::image_placeholder_container(&iced::Theme::Dark))
        .into()
}

fn header_section<'a>(
    loading: bool,
    loc: &'a crate::lang::Localization,
    is_disabled: bool,
    ctx: theme::UIContext,
) -> Element<'a, NewsMessage, Theme, Renderer> {
    let palette = ctx.palette;
    row![
        row![
            container(Space::new())
                .width(6)
                .height(6)
                .style(move |_| container::Style {
                    background: Some(Background::Color(palette.accent)),
                    border: crate::theme::small_progress_bar(&palette),
                    ..Default::default()
                }),
            theme::text_micro(if loading {
                loc.t("news.feed_loading")
            } else {
                loc.t("news.feed_latest")
            }, ctx)
        ]
        .spacing(8)
        .padding(5)
        .align_y(Alignment::Center),
        Space::new().width(Length::Fill),
        {
            let mut btn = button(
                row![
                    theme::text_micro(loc.t("news.browse_all"), ctx),
                    theme::svg(
                        svg(util::icons::icon(util::icons::CHEVRON_RIGHT))
                            .width(10)
                            .height(10)
                            .style(move |t: &Theme, s| theme::svg_muted(&palette, t, s))
                            .opacity(ctx.palette.text_primary.a),
                        ctx
                    )
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            );
            if !is_disabled && ctx.palette.text_primary.a > 0.05 {
                btn = btn.on_press(NewsMessage::OpenAllNews);
            }
            theme::magic_button(
                btn.style(move |t: &Theme, s| theme::ghost_button_style(&palette, t, s))
                    .padding(4)
                    .into(),
                ctx,
            )
        }
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}
