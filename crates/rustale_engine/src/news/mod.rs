pub mod news;

use anyhow::{Context, Result};
use feed_rs::parser;
use tokio::task::JoinSet;

pub use self::news::BlogPost;

pub async fn fetch_news() -> Result<Vec<BlogPost>> {
    let response = rustale_shared::HTTP_CLIENT
        .get("https://hytale.com/rss.xml")
        .send()
        .await
        .context("Failed to fetch RSS feed")?;

    if !response.status().is_success() {
        anyhow::bail!("RSS feed error: HTTP {}", response.status());
    }

    let rss_content = response.text().await.context("Failed to read RSS content")?;
    
    let feed = parser::parse(rss_content.as_bytes())
        .context("Failed to parse RSS feed")?;

    let mut posts: Vec<BlogPost> = feed.entries
        .into_iter()
        .map(|entry| BlogPost::from_rss_entry(&entry))
        .collect();

    // Ordenar por fecha (más reciente primero) ANTES de cargar imágenes
    posts.sort_by(|a, b| b.published_at.cmp(&a.published_at));

    // Limitar a las 10 noticias más recientes
    posts.truncate(10);

    // Cargar imágenes asíncronamente para las noticias
    let mut image_tasks = JoinSet::new();
    
    for (i, post) in posts.iter().enumerate() {
        let post_url = post.get_post_url();
        image_tasks.spawn(async move {
            let image = news::extract_image_from_page(&post_url).await;
            (i, image)
        });
    }
    
    // Esperar a que se carguen las imágenes
    while let Some(result) = image_tasks.join_next().await {
        if let Ok((index, image)) = result {
            if let Some(img) = image {
                posts[index].cover_image = Some(img);
            }
        }
    }

    Ok(posts)
}
