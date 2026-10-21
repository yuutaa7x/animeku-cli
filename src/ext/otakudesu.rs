use base64::prelude::*;
use std::collections::HashMap;
use std::time::Duration;
use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;

use crate::{
    ext::Ext,
    models::{Episode, Meta, Movie, Stream},
};

pub const OTAKUDESU_BASE_URL: &str = "https://otakudesu.blog";
pub const OTAKUDESU_REFERER: &str = "https://desustream.net/";
pub const OTAKUDESU_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/110.0.0.0 Safari/537.36";

pub fn otakudesu_base_url() -> String {
    std::env::var("ANIMEKU_OTAKUDESU_BASE_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| OTAKUDESU_BASE_URL.to_string())
}

pub struct Otakudesu {
    client: Client,
    base_url: String,
}

impl Otakudesu {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent(OTAKUDESU_USER_AGENT)
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap(),
            base_url: otakudesu_base_url(),
        }
    }

    async fn resolve_desustream(&self, url: &str) -> String {
        if !url.contains("desustream") {
            return clean_url(url);
        }
        if let Ok(resp) = self.client.get(url).send().await {
            if let Ok(text) = resp.text().await {
                if let Some(extracted) = extract_desustream_url(&text) {
                    return extracted;
                }
            }
        }
        clean_url(url)
    }
}

/// Decode HTML entities that leak from `src="..."` attributes. Google video
/// URLs contain many `&amp;` separators; leaving them encoded breaks the
/// signature and results in HTTP 403.
fn clean_url(raw: &str) -> String {
    html_escape::decode_html_entities(raw.trim()).to_string()
}

fn page_number_from_href(href: &str) -> u64 {
    let re = regex::Regex::new(r"(?:/page/|[?&]paged=)(\d+)").unwrap();
    re.captures(href)
        .and_then(|caps| caps[1].parse::<u64>().ok())
        .unwrap_or(0)
}

fn extract_between<'a>(text: &'a str, start_delim: &str, end_delim: &str) -> Option<&'a str> {
    let start_idx = text.find(start_delim)?;
    let rest = &text[start_idx + start_delim.len()..];
    let end_idx = rest.find(end_delim)?;
    Some(&rest[..end_idx])
}

fn extract_desustream_url(text: &str) -> Option<String> {
    extract_between(text, "<source src=\"", "\"")
        .or_else(|| extract_between(text, "otakudesu('{\"file\":\"", "\""))
        .or_else(|| extract_between(text, "\"file\":\"", "\""))
        .map(clean_url)
}

#[async_trait]
impl Ext for Otakudesu {
    async fn search(&mut self, title: String, page: usize) -> anyhow::Result<(Vec<Movie>, u64)> {
        let page = page.max(1);
        let url = if page == 1 {
            format!(
                "{}/?s={}&post_type=anime",
                self.base_url,
                urlencoding::encode(&title)
            )
        } else {
            format!(
                "{}/page/{}/?s={}&post_type=anime",
                self.base_url,
                page,
                urlencoding::encode(&title)
            )
        };
        let html_text = self.client.get(&url).send().await?.text().await?;
        let document = Html::parse_document(&html_text);
        let selector = Selector::parse("ul.chivsrc li").unwrap();
        let a_selector = Selector::parse("h2 a").unwrap();

        let mut movies = vec![];
        for element in document.select(&selector) {
            if let Some(a_el) = element.select(&a_selector).next() {
                let text = crate::models::clean_movie_title(&a_el.inner_html());
                let href = a_el.value().attr("href").unwrap_or("").to_string();
                movies.push(Movie {
                    id: href,
                    title: text,
                    total_episodes: None,
                });
            }
        }

        // Only expose a next page when the search page actually links to one.
        // If there is no pagination markup, this stays at the current page.
        let mut total_pages = page as u64;
        for pagination_selector in [".pagination a", ".pagenavi a", ".hpage a", "a.page-numbers"] {
            if let Ok(selector) = Selector::parse(pagination_selector) {
                for link in document.select(&selector) {
                    if let Some(href) = link.value().attr("href") {
                        total_pages = total_pages.max(page_number_from_href(href));
                    }
                    if let Ok(number) = link.text().collect::<String>().trim().parse::<u64>() {
                        total_pages = total_pages.max(number);
                    }
                }
            }
        }

        Ok((movies, total_pages))
    }

    async fn get_episodes(&self, movie: Movie) -> anyhow::Result<(Vec<Episode>, Meta)> {
        let html_text = self.client.get(&movie.id).send().await?.text().await?;
        let document = Html::parse_document(&html_text);
        
        let ep_list_selector = Selector::parse(".episodelist ul li").unwrap();
        let a_selector = Selector::parse("span a").unwrap();
        let mut episodes = vec![];
        

        for element in document.select(&ep_list_selector) {
            if let Some(a_el) = element.select(&a_selector).next() {
                let raw_text = a_el.inner_html();
                let href = a_el.value().attr("href").unwrap_or("").to_string();
                if raw_text.to_lowercase().contains("batch") || href.contains("/lengkap/") || href.contains("/batch/") {
                    continue;
                }
                episodes.push(Episode {
                    id: href,
                    title: crate::models::clean_episode_title(&raw_text),
                    is_series: true,
                });
            }
        }
        episodes.reverse();
        
        // Meta (optional)
        let mut meta = Meta {
            thumb_url: None,
            data: vec![],
        };

        if let Ok(img_sel) = Selector::parse("img.attachment-post-thumbnail") {
            if let Some(img) = document.select(&img_sel).next() {
                if let Some(src) = img.value().attr("src") {
                    meta.thumb_url = Some(src.to_string());
                }
            }
        }
        
        if let Ok(info_sel) = Selector::parse(".infozingle p") {
            for p in document.select(&info_sel) {
                let clean = p.text().collect::<Vec<_>>().join(" ");
                if let Some((k, v)) = clean.split_once(':') {
                    meta.data.push((k.trim().to_string(), v.trim().to_string()));
                } else {
                    meta.data.push(("".to_string(), clean));
                }
            }
        }

        if let Ok(sinop_sel) = Selector::parse(".sinopc p") {
            let mut sinopsis_text = String::new();
            for p in document.select(&sinop_sel) {
                let clean = p.text().collect::<Vec<_>>().join(" ");
                let lower = clean.to_lowercase();
                if lower.contains("tonton juga") || lower.contains("kelanjutannya") || lower.contains("batch") {
                    continue;
                }
                if !sinopsis_text.is_empty() {
                    sinopsis_text.push(' ');
                }
                sinopsis_text.push_str(&clean);
            }
            if sinopsis_text.chars().count() > 320 {
                let mut truncated = sinopsis_text.chars().take(320).collect::<String>();
                truncated.push_str("...");
                meta.data.push(("Sinopsis".to_string(), truncated));
            } else if !sinopsis_text.is_empty() {
                meta.data.push(("Sinopsis".to_string(), sinopsis_text));
            }
        }

        Ok((episodes, meta))
    }

    async fn get_stream_urls(&self, episode: Episode) -> anyhow::Result<Vec<Stream>> {
        let ep_url = episode.id.clone();
        let html_text = self.client.get(&ep_url).send().await?.text().await?;
        
        let mut streams = vec![];
        let mut mirrors = vec![];
        let mut default_url = None;

        {
            let document = Html::parse_document(&html_text);
            let iframe_sel = Selector::parse("#pembed iframe, .player-embed iframe").unwrap();
            if let Some(iframe) = document.select(&iframe_sel).next() {
                if let Some(src) = iframe.value().attr("src") {
                    let mut url = clean_url(src);
                    if url.starts_with("//") {
                        url = format!("https:{}", url);
                    }
                    default_url = Some(url);
                }
            }

            let mirror_sel = Selector::parse(".mirrorstream ul li a").unwrap();
            for a_el in document.select(&mirror_sel) {
                let text = a_el.inner_html().trim().to_string();
                let data_content = a_el.value().attr("data-content").unwrap_or("").to_string();
                mirrors.push((text, data_content));
            }
        }

        if let Some(mut url) = default_url {
            url = self.resolve_desustream(&url).await;
            streams.push(Stream {
                url,
                title: "Default".to_string(),
            });
        }

        let params = [("action", "aa1208d27f29ca340c92c66d1926f13f")];
        let ajax_url = format!("{}/wp-admin/admin-ajax.php", self.base_url);
        let res = self.client.post(&ajax_url)
            .header("Referer", &ep_url)
            .form(&params)
            .send().await?;
            
        let json_val: Value = res.json().await.unwrap_or(Value::Null);
        let nonce = if let Some(n) = json_val.get("data").and_then(|v| v.as_str()) {
            n.to_string()
        } else {
            return Ok(streams);
        };

        for (name, content) in mirrors {
            let decoded_bytes = match base64::prelude::BASE64_STANDARD.decode(&content) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let decoded_str = match String::from_utf8(decoded_bytes) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let payload: HashMap<String, String> = match serde_json::from_str(&decoded_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let mut form = HashMap::new();
            form.insert("action".to_string(), "2a3505c93b0035d3f455df82bf976b84".to_string());
            form.insert("nonce".to_string(), nonce.clone());
            for (k, v) in payload {
                form.insert(k, v);
            }
            
            if let Ok(m_resp) = self.client.post(&ajax_url).header("Referer", &ep_url).form(&form).send().await {
                if let Ok(m_json) = m_resp.json::<Value>().await {
                    if let Some(data) = m_json.get("data").and_then(|v| v.as_str()) {
                        if let Ok(html_bytes) = base64::prelude::BASE64_STANDARD.decode(data) {
                            if let Ok(html_str) = String::from_utf8(html_bytes) {
                                if let Some(src) = extract_between(&html_str, "src=\"", "\"") {
                                    let mut url = clean_url(src);
                                    
                                    if url.starts_with("//") {
                                        url = format!("https:{}", url);
                                    }
                                    
                                    url = self.resolve_desustream(&url).await;
                                    streams.push(Stream {
                                        url,
                                        title: format!("Mirror - {}", name),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(streams)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_between() {
        let sample = r#"<div class="test"><source src="https://example.com/video.mp4" type="video/mp4"></div>"#;
        assert_eq!(
            extract_between(sample, "<source src=\"", "\""),
            Some("https://example.com/video.mp4")
        );
        assert_eq!(extract_between(sample, "nonexistent", "\""), None);
    }

    #[test]
    fn test_extract_desustream_url() {
        let html1 = r#"<source src="https://stream.desu/vid.mp4">"#;
        assert_eq!(extract_desustream_url(html1), Some("https://stream.desu/vid.mp4".to_string()));

        let html2 = r#"otakudesu('{"file":"https://stream.desu/otaku.mp4"}"#;
        assert_eq!(extract_desustream_url(html2), Some("https://stream.desu/otaku.mp4".to_string()));

        let html3 = r#"{"other":1,"file":"https://stream.desu/fallback.mp4"}"#;
        assert_eq!(extract_desustream_url(html3), Some("https://stream.desu/fallback.mp4".to_string()));
    }

    #[test]
    fn test_extract_desustream_url_decodes_html_entities() {
        let html = r#"<source src="https://rr3.googlevideo.com/videoplayback?expire=1&amp;sig=abc&amp;lsig=def">"#;
        assert_eq!(
            extract_desustream_url(html),
            Some(
                "https://rr3.googlevideo.com/videoplayback?expire=1&sig=abc&lsig=def"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_clean_url_trims_and_decodes() {
        assert_eq!(
            clean_url("  https://example.com/a?x=1&amp;y=2  "),
            "https://example.com/a?x=1&y=2"
        );
    }

    #[test]
    fn test_page_number_from_href() {
        assert_eq!(
            page_number_from_href("https://otakudesu.blog/page/2/?s=naruto&post_type=anime"),
            2
        );
        assert_eq!(
            page_number_from_href("https://otakudesu.blog/?s=naruto&paged=3"),
            3
        );
        assert_eq!(
            page_number_from_href("https://otakudesu.blog/anime/example/"),
            0
        );
    }
}
