use rustale_engine::news::BlogPost;
use crate::util::image_cache::load_news_image_bytes;
use crate::{Message, theme, util};
use iced::widget::{
    Id, ProgressBar, Space, button, column, container, image, row, scrollable, svg,
};
use iced::{Alignment, Background, ContentFit, Element, Length, Renderer, Task, Theme};


const NEWS_SCROLL_ID: &str = "news_scroll";

#[derive(Debug, Clone)]
pub enum NewsMessage {
    LoadNews,
    NewsLoaded(Result<Vec<BlogPost>, String>),
    ImageLoaded(String, Result<image::Handle, String>),
    ReloadImages, 
    OpenPost(String),
    OpenAllNews,
    ScrollOffsetChanged(f32), 
    ScrollDelta(f32),         
    GetScrollOffset,          
}

#[derive(Debug, Clone)]
pub struct NewsSection {
    pub posts: Vec<BlogPost>,
    pub loading: bool,
    pub loaded_once: bool, 
    pub error: Option<String>,
    pub scroll_offset: f32,   
    pub viewport_height: f32, 
}

impl NewsSection {
    pub fn new() -> Self {
        Self {
            posts: Vec::new(),
            loading: false,     
            loaded_once: false, 
            error: None,
            scroll_offset: 0.0,
            viewport_height: 600.0, 
        }
    }

    pub fn should_load(&self, resources: &crate::ui::resources::UiResources) -> bool {
        (!self.loaded_once && !self.loading)
            || (!self.posts.is_empty() && resources.global_thumbnails.len() == 0 && !self.loading)
    }

    pub fn update(
        &mut self, 
        message: NewsMessage, 
        resources: &mut crate::ui::resources::UiResources
    ) -> Task<Message> {
        match message {
            NewsMessage::LoadNews => {
                self.loading = true;
                self.loaded_once = true; 
                self.error = None;

                if !self.posts.is_empty() {
                    let mut image_tasks = Vec::new();
                    for post in &self.posts {
                        if let Some(cover) = &post.cover_image {
                            let key = cover.s3_key.clone();
                            if resources.global_thumbnails.get(&key).is_none() {
                                        image_tasks.push(Task::perform(
                                            async move {
                                                let res = load_news_image_bytes(&key).await;
                                                (
                                                    key,
                                                    res.map(image::Handle::from_bytes)
                                                       .map_err(|e: anyhow::Error| e.to_string()),
                                                )
                                            },
                                            |(key, res)| Message::News(NewsMessage::ImageLoaded(key, res)),
                                        ));
                            }
                        }
                    }
                    self.loading = false; 
                    if image_tasks.is_empty() {
                        Task::none()
                    } else {
                        Task::batch(image_tasks)
                    }
                } else {
                    Task::perform(
                        async move {
                            rustale_engine::news::fetch_news()
                                .await
                                .map_err(|e| e.to_string())
                        },
                        |res: Result<Vec<BlogPost>, String>| Message::News(NewsMessage::NewsLoaded(res)),
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
                                if resources.global_thumbnails.get(&key).is_none() {
                                    image_tasks.push(Task::perform(
                                        async move {
                                            let res = load_news_image_bytes(&key).await;
                                            (
                                                key,
                                                res.map(image::Handle::from_bytes)
                                                   .map_err(|e: anyhow::Error| e.to_string()),
                                            )
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
                    resources.global_thumbnails.insert(key, handle);
                }
                Task::none()
            }
            NewsMessage::ReloadImages => {
                let cache_empty = resources.global_thumbnails.len() == 0;
                if !self.posts.is_empty() && cache_empty && !self.loading {
                    let mut image_tasks = Vec::new();
                    for post in &self.posts {
                        if let Some(cover) = &post.cover_image {
                            let key = cover.s3_key.clone();
                            image_tasks.push(Task::perform(
                                async move {
                                    let res = load_news_image_bytes(&key).await;
                                    (
                                        key,
                                        res.map(image::Handle::from_bytes)
                                           .map_err(|e: anyhow::Error| e.to_string()),
                                    )
                                },
                                |(key, res)| Message::News(NewsMessage::ImageLoaded(key, res)),
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
            NewsMessage::ScrollOffsetChanged(offset) => {
                if (self.scroll_offset - offset).abs() > 1.0 {
                    self.scroll_offset = offset;
                }
                Task::none()
            }
            NewsMessage::ScrollDelta(delta) => {
                self.scroll_offset = (self.scroll_offset - delta).max(0.0);
                Task::none()
            }
            NewsMessage::GetScrollOffset => {
                Task::none()
            }
        }
    }

    pub fn update_viewport_height(&mut self, window_height: f32) {
        self.viewport_height = (window_height * 0.7).max(300.0); 
    }

    pub fn view<'a>(
        &'a self,
        localization: &'a rustale_shared::lang::Localization,
        is_disabled: bool,
        resources: &'a crate::ui::resources::UiResources,
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
            self.view_posts(localization, is_disabled, resources, ctx)
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
        loc: &'a rustale_shared::lang::Localization,
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
        loc: &'a rustale_shared::lang::Localization,
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
        loc: &'a rustale_shared::lang::Localization,
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
        loc: &'a rustale_shared::lang::Localization,
        is_disabled: bool,
        resources: &'a crate::ui::resources::UiResources,
        ctx: theme::UIContext,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let palette = ctx.palette;

        let post_height_estimate = 120.0; 
        let post_spacing = 8.0;
        let total_post_height = post_height_estimate + post_spacing;

        let start_index =
            ((self.scroll_offset / total_post_height).floor() as usize).saturating_sub(1); 
        let visible_count = ((self.viewport_height / total_post_height).ceil() as usize) + 2; 

        let end_index = (start_index + visible_count).min(self.posts.len());
        let start_index = start_index.min(self.posts.len().saturating_sub(1));

        let posts_to_render: Vec<&BlogPost> = if start_index < self.posts.len() {
            self.posts[start_index..end_index].iter().collect()
        } else {
            Vec::new()
        };

        let top_space = Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(start_index as f32 * total_post_height));

        let bottom_space = Space::new().width(Length::Fill).height(Length::Fixed(
            (self.posts.len().saturating_sub(end_index)) as f32 * total_post_height,
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
                                let real_index = start_index + i;
                                self.view_post_with_index(post, real_index, loc, is_disabled, resources, ctx)
                            })
                            .collect::<Vec<_>>(),
                    )
                    .spacing(post_spacing),
                    bottom_space,
                ]
                .spacing(0),
            )
            .id(Id::new(NEWS_SCROLL_ID))
            .on_scroll(|viewport| NewsMessage::ScrollOffsetChanged(viewport.absolute_offset().y))
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
        _index: usize, 
        loc: &'a rustale_shared::lang::Localization,
        is_disabled: bool,
        resources: &'a crate::ui::resources::UiResources,
        ctx: theme::UIContext,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        self.view_post(post, loc, is_disabled, resources, ctx)
    }

    fn view_post<'a>(
        &'a self,
        post: &'a BlogPost,
        loc: &'a rustale_shared::lang::Localization,
        is_disabled: bool,
        resources: &'a crate::ui::resources::UiResources,
        ctx: theme::UIContext,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let palette = ctx.palette;
        let image_content: Element<'a, NewsMessage, Theme, Renderer> =
            if let Some(cover) = &post.cover_image {
                if let Some(handle) = resources.global_thumbnails.peek(&cover.s3_key) {
                    theme::magic_image(
                        image(handle.clone())
                            .width(100)
                            .height(56)
                            .border_radius(8)
                            .content_fit(ContentFit::Cover)
                            .opacity(ctx.palette.text_primary.a) 
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
                            svg(util::svg_handle(util::icons::CALENDAR))
                                .width(10)
                                .height(10)
                                .style(move |t: &Theme, s| theme::svg_accent(&palette, t, s))
                                .opacity(ctx.palette.text_primary.a),
                            ctx
                        ),
                        theme::text(
                            iced::widget::text(post.format_date())
                                .size(10)
                                .color(palette.text_secondary),
                            ctx
                        )
                    ]
                    .spacing(4),
                    theme::text(
                        iced::widget::text(&post.title)
                            .size(18)
                            .color(palette.accent),
                        ctx
                    ),
                    theme::text(
                        iced::widget::text(
                            post.body_excerpt
                                .as_deref()
                                .unwrap_or_else(|| loc.t("news.no_desc"))
                        )
                        .size(12)
                        .color(palette.text_secondary),
                        ctx
                    )
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
    loc: &'a rustale_shared::lang::Localization,
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
            theme::text_micro(
                if loading {
                    loc.t("news.feed_loading")
                } else {
                    loc.t("news.feed_latest")
                },
                ctx
            )
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
                        svg(util::svg_handle(util::icons::CHEVRON_RIGHT))
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
