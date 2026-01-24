use crate::news::BlogPost;
use crate::util::image_cache::load_news_image;
use crate::{Message, theme, util};
use iced::widget::{
    ProgressBar, Space, button, column, container, image, row, scrollable, svg, text,
};
use iced::{Alignment, Background, Border, Color, ContentFit, Element, Length, Task, Theme};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum NewsMessage {
    LoadNews,
    NewsLoaded(Result<Vec<BlogPost>, String>),
    // Nuevo mensaje para cuando una imagen se carga individualmente
    ImageLoaded(String, Result<image::Handle, String>),
    OpenPost(String),
    OpenAllNews,
}

pub struct NewsSection {
    pub posts: Vec<BlogPost>,
    // Almacenamos las imágenes cargadas aquí: s3_key -> Handle
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
                // Limpiamos imágenes viejas si recargamos
                self.images.clear();
                Task::none()
            }
            NewsMessage::NewsLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(posts) => {
                        self.posts = posts;
                        self.error = None;

                        // Creamos tareas para descargar las imágenes de cada post en paralelo
                        let mut image_tasks = Vec::new();
                        for post in &self.posts {
                            if let Some(cover) = &post.cover_image {
                                let key = cover.s3_key.clone();
                                // Solo cargamos si no la tenemos ya (por si acaso)
                                if !self.images.contains_key(&key) {
                                    let c = client.clone();
                                    image_tasks.push(Task::perform(
                                        async move {
                                            let res = load_news_image(&c, &key).await;
                                            (key, res.map_err(|e| e.to_string()))
                                        },
                                        |(key, res)| {
                                            Message::News(NewsMessage::ImageLoaded(key, res))
                                        },
                                    ));
                                }
                            }
                        }

                        // Devolvemos el lote de tareas
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
                // Si falla, simplemente no mostramos imagen o dejamos el placeholder, no es crítico.
                Task::none()
            }
            NewsMessage::OpenPost(url) => Task::perform(
                async move {
                    if let Err(e) = open::that(&url) {
                        eprintln!("Failed to open URL: {}", e);
                    }
                },
                |_| Message::None,
            ),
            NewsMessage::OpenAllNews => Task::perform(
                async move {
                    if let Err(e) = open::that("https://hytale.com/news") {
                        eprintln!("Failed to open all news: {}", e);
                    }
                },
                |_| Message::None,
            ),
        }
    }

    pub fn view<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
    ) -> Element<'a, NewsMessage> {
        let content: Element<'a, NewsMessage> = if self.loading {
            self.view_loading(localization)
        } else if let Some(error) = &self.error {
            self.view_error(error, localization)
        } else if self.posts.is_empty() {
            self.view_empty(localization)
        } else {
            self.view_posts(localization)
        };

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::news_panel_style)
            .into()
    }

    fn view_loading<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
    ) -> Element<'a, NewsMessage> {
        column![
            header_section(true, localization),
            container(
                column![
                    text(localization.t("news.loading"))
                        .size(14)
                        .color(Color::from_rgb(0.7, 0.7, 0.7)),
                    Space::new().height(20.0),
                    container(ProgressBar::new(0.0..=100.0, 50.0).style(theme::orange_bar_style))
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
        error: &'a str,
        localization: &'a crate::lang::Localization,
    ) -> Element<'a, NewsMessage> {
        column![
            header_section(false, localization),
            container(
                column![
                    text(localization.t("news.failed"))
                        .size(16)
                        .color(Color::from_rgb(1.0, 0.5, 0.5)),
                    text(error).size(12).color(Color::from_rgb(0.7, 0.7, 0.7)),
                    button(text(localization.t("news.retry")).size(14))
                        .on_press(NewsMessage::LoadNews)
                        .padding(8)
                        .style(theme::primary_button_style)
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
        localization: &'a crate::lang::Localization,
    ) -> Element<'a, NewsMessage> {
        column![
            header_section(false, localization),
            container(
                text(localization.t("news.empty"))
                    .size(14)
                    .color(Color::from_rgb(0.7, 0.7, 0.7))
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
        localization: &'a crate::lang::Localization,
    ) -> Element<'a, NewsMessage> {
        let posts_view = column![
            header_section(false, localization),
            scrollable(
                column(
                    self.posts
                        .iter()
                        .map(|post| self.view_post(post, localization))
                        .collect::<Vec<_>>()
                )
                .spacing(8)
            )
            .height(Length::Fill)
            .style(theme::scrollable_style)
        ]
        .spacing(10);

        if self.error.is_some() {
            column![
                posts_view,
                container(
                    button(text(localization.t("news.retry")).size(12))
                        .on_press(NewsMessage::LoadNews)
                        .padding(4)
                        .style(theme::secondary_button_style)
                )
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
        localization: &'a crate::lang::Localization,
    ) -> Element<'a, NewsMessage> {
        // Obtenemos la imagen del HashMap en lugar de bloquear el hilo
        let image_content: Element<'a, NewsMessage> = if let Some(cover) = &post.cover_image {
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

        let body_text = post
            .body_excerpt
            .as_deref()
            .unwrap_or_else(|| localization.t("news.no_desc"));

        button(
            row![
                container(image_content).style(|_: &Theme| container::Style {
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
                            .style(theme::svg_accent),
                        text(post.format_date())
                            .size(9)
                            .color(Color::from_rgb(0.7, 0.7, 0.7)),
                    ]
                    .spacing(4),
                    text(&post.title).size(14).color(Color::WHITE),
                    text(body_text)
                        .size(11)
                        .color(Color::from_rgb(0.7, 0.7, 0.7)),
                ]
                .spacing(2)
                .width(Length::Fill)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .on_press(NewsMessage::OpenPost(post.get_post_url()))
        .style(theme::ghost_button_style)
        .width(Length::Fill)
        .padding(8)
        .into()
    }
}

fn placeholder_image<'a>() -> Element<'a, NewsMessage> {
    container(Space::new().width(100.0).height(56.0))
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.1))),
            ..Default::default()
        })
        .into()
}

fn header_section<'a>(
    loading: bool,
    localization: &'a crate::lang::Localization,
) -> Element<'a, NewsMessage> {
    row![
        row![
            container(Space::new())
                .width(6)
                .height(6)
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(theme::ACCENT_ORANGE)),
                    border: Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            text(if loading {
                localization.t("news.feed_loading")
            } else {
                localization.t("news.feed_latest")
            })
            .size(10)
            .color(Color::from_rgb(0.5, 0.5, 0.5)),
        ]
        .spacing(8)
        .padding(5)
        .align_y(Alignment::Center),
        Space::new().width(Length::Fill),
        button(
            row![
                text(localization.t("news.browse_all"))
                    .size(10)
                    .color(Color::from_rgb(0.5, 0.5, 0.5)),
                svg(util::icons::icon(util::icons::CHEVRON_RIGHT))
                    .width(10)
                    .height(10)
                    .style(theme::svg_muted),
            ]
            .spacing(2)
            .align_y(Alignment::Center)
        )
        .on_press(NewsMessage::OpenAllNews)
        .style(theme::ghost_button_style)
        .padding(4),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}
