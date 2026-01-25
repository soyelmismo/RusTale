use crate::news::BlogPost;
use crate::util::image_cache::load_news_image;
use crate::{Message, theme, util};
use iced::widget::{
    ProgressBar, Space, button, column, container, image, row, scrollable, svg, text,
};
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Renderer, Task, Theme,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum NewsMessage {
    LoadNews,
    NewsLoaded(Result<Vec<BlogPost>, String>),
    ImageLoaded(String, Result<image::Handle, String>),
    OpenPost(String),
    OpenAllNews,
}

pub struct NewsSection {
    pub posts: Vec<BlogPost>,
    pub images: HashMap<String, image::Handle>,
    pub loading: bool,
    pub error: Option<String>,
}

impl NewsSection {
    pub fn new() -> Self {
        Self {
            posts: Vec::new(),
            images: HashMap::new(),
            loading: true,
            error: None,
        }
    }

    pub fn update(&mut self, message: NewsMessage, client: reqwest::Client) -> Task<Message> {
        match message {
            NewsMessage::LoadNews => {
                self.loading = true;
                self.error = None;
                self.images.clear();
                Task::none()
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
        }
    }

    pub fn view<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
        is_disabled: bool,
        palette: &'a theme::Palette,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let content: Element<'a, NewsMessage, Theme, Renderer> = if self.loading {
            self.view_loading(localization, is_disabled, palette)
        } else if let Some(error) = &self.error {
            self.view_error(error, localization, is_disabled, palette)
        } else if self.posts.is_empty() {
            self.view_empty(localization, is_disabled, palette)
        } else {
            self.view_posts(localization, is_disabled, palette)
        };

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |t: &Theme| theme::news_panel_style(palette, t))
            .into()
    }

    fn view_loading<'a>(
        &'a self,
        loc: &'a crate::lang::Localization,
        is_disabled: bool,
        palette: &'a theme::Palette,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        column![
            header_section(true, loc, is_disabled, palette),
            container(
                column![
                    text(loc.t("news.loading"))
                        .size(14)
                        .color(palette.text_secondary),
                    Space::new().height(20.0),
                    container(
                        ProgressBar::new(0.0..=100.0, 50.0)
                            .style(move |t: &Theme| theme::orange_bar_style(palette, t))
                    )
                    .width(200)
                    .center_x(Length::Fill)
                ]
                .align_x(Alignment::Center)
                .spacing(10)
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
        ]
        .spacing(15)
        .into()
    }

    fn view_error<'a>(
        &self,
        err: &'a str,
        loc: &'a crate::lang::Localization,
        is_disabled: bool,
        palette: &'a theme::Palette,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        column![
            header_section(false, loc, is_disabled, palette),
            container(
                column![
                    text(loc.t("news.failed"))
                        .size(16)
                        .color(Color::from_rgb(1.0, 0.5, 0.5)),
                    text(err).size(12).color(palette.text_secondary),
                    {
                        let mut btn = button(text(loc.t("news.retry")).size(14))
                            .style(move |t: &Theme, s| theme::primary_button_style(palette, t, s))
                            .padding(8);
                        if !is_disabled {
                            btn = btn.on_press(NewsMessage::LoadNews);
                        }
                        btn
                    }
                ]
                .spacing(10)
                .align_x(Alignment::Center)
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
        ]
        .spacing(15)
        .into()
    }

    fn view_empty<'a>(
        &'a self,
        loc: &'a crate::lang::Localization,
        is_disabled: bool,
        palette: &'a theme::Palette,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        column![
            header_section(false, loc, is_disabled, palette),
            container(
                text(loc.t("news.empty"))
                    .size(14)
                    .color(palette.text_secondary)
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
        ]
        .spacing(15)
        .into()
    }

    fn view_posts<'a>(
        &'a self,
        loc: &'a crate::lang::Localization,
        is_disabled: bool,
        palette: &'a theme::Palette,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let posts_view = column![
            header_section(false, loc, is_disabled, palette),
            scrollable(
                column(
                    self.posts
                        .iter()
                        .map(|post| self.view_post(post, loc, is_disabled, palette))
                        .collect::<Vec<_>>()
                )
                .spacing(8)
            )
            .height(Length::Fill)
            .style(move |t: &Theme, s| theme::scrollable_style(palette, t, s))
        ]
        .spacing(10);
        if self.error.is_some() {
            column![
                posts_view,
                container({
                    let mut btn = button(text(loc.t("news.retry")).size(12))
                        .style(move |t: &Theme, s| theme::secondary_button_style(palette, t, s))
                        .padding(4);
                    if !is_disabled {
                        btn = btn.on_press(NewsMessage::LoadNews);
                    }
                    btn
                })
                .width(Length::Fill)
                .center_x(Length::Fill)
            ]
            .spacing(5)
            .into()
        } else {
            posts_view.into()
        }
    }

    fn view_post<'a>(
        &'a self,
        post: &'a BlogPost,
        loc: &'a crate::lang::Localization,
        is_disabled: bool,
        palette: &'a theme::Palette,
    ) -> Element<'a, NewsMessage, Theme, Renderer> {
        let image_content: Element<'a, NewsMessage, Theme, Renderer> =
            if let Some(cover) = &post.cover_image {
                if let Some(handle) = self.images.get(&cover.s3_key) {
                    image(handle.clone())
                        .width(100)
                        .height(56)
                        .content_fit(ContentFit::Cover)
                        .into()
                } else {
                    placeholder_image()
                }
            } else {
                placeholder_image()
            };
        let mut btn = button(
            row![
                container(image_content).style(|_| container::Style {
                    border: Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                column![
                    row![
                        svg(util::icons::icon(util::icons::CALENDAR))
                            .width(10)
                            .height(10)
                            .style(move |t: &Theme, s| theme::svg_accent(palette, t, s)),
                        text(post.format_date())
                            .size(9)
                            .color(palette.text_secondary)
                    ]
                    .spacing(4),
                    text(&post.title).size(14).color(palette.text_primary),
                    text(
                        post.body_excerpt
                            .as_deref()
                            .unwrap_or_else(|| loc.t("news.no_desc"))
                    )
                    .size(11)
                    .color(palette.text_secondary)
                ]
                .spacing(2)
                .width(Length::Fill)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
        if !is_disabled {
            btn = btn.on_press(NewsMessage::OpenPost(post.get_post_url()));
        }
        btn.style(move |t: &Theme, s| theme::ghost_button_style(palette, t, s))
            .width(Length::Fill)
            .padding(8)
            .into()
    }
}

fn placeholder_image<'a>() -> Element<'a, NewsMessage, Theme, Renderer> {
    container(Space::new().width(100.0).height(56.0))
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.1))),
            ..Default::default()
        })
        .into()
}

fn header_section<'a>(
    loading: bool,
    loc: &'a crate::lang::Localization,
    is_disabled: bool,
    palette: &'a theme::Palette,
) -> Element<'a, NewsMessage, Theme, Renderer> {
    row![
        row![
            container(Space::new())
                .width(6)
                .height(6)
                .style(move |_| container::Style {
                    background: Some(Background::Color(palette.accent)),
                    border: Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            text(if loading {
                loc.t("news.feed_loading")
            } else {
                loc.t("news.feed_latest")
            })
            .size(10)
            .color(palette.text_secondary)
        ]
        .spacing(8)
        .padding(5)
        .align_y(Alignment::Center),
        Space::new().width(Length::Fill),
        {
            let mut btn = button(
                row![
                    text(loc.t("news.browse_all"))
                        .size(10)
                        .color(palette.text_secondary),
                    svg(util::icons::icon(util::icons::CHEVRON_RIGHT))
                        .width(10)
                        .height(10)
                        .style(move |t: &Theme, s| theme::svg_muted(palette, t, s))
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            );
            if !is_disabled {
                btn = btn.on_press(NewsMessage::OpenAllNews);
            }
            btn.style(move |t: &Theme, s| theme::ghost_button_style(palette, t, s))
                .padding(4)
        }
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}
