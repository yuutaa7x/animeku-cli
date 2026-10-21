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

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> reqwest::Result<reqwest::Response> {
        crate::ext::wait_for_provider(crate::ext::PROVIDER_OTAKUDESU).await;
        request.send().await
    }

    async fn resolve_desustream(&self, url: &str) -> String {
        if !url.contains("desustream") {
            return clean_url(url);
        }

        let Ok(resp) = self.send(self.client.get(url)).await else {
            return clean_url(url);
        };
        let Ok(text) = resp.text().await else {
            return clean_url(url);
        };

        let embed = extract_desustream_url(&text).or_else(|| extract_iframe_src(&text));
        let Some(embed) = embed else {
            return clean_url(url);
        };

        // Newer desustream pages wrap a Blogger video player instead of
        // exposing the MP4 directly. Resolve that player to its googlevideo
        // source so ffmpeg can download it.
        if embed.contains("blogger.com/video.g") {
            if let Ok(resp) = self
                .send(
                    self.client
                        .get(&embed)
                        .header("Referer", OTAKUDESU_REFERER),
                )
                .await
            {
                if let Ok(text) = resp.text().await {
                    if let Some(video) = extract_blogger_video_url(&text) {
                        return video;
                    }
                }
            }
        }

        clean_url(&embed)
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

fn extract_iframe_src(text: &str) -> Option<String> {
    let re = regex::Regex::new(r#"(?is)<iframe\b[^>]*\bsrc\s*=\s*["']([^"']+)["']"#).unwrap();
    let mut url = clean_url(re.captures(text)?.get(1)?.as_str());
    if url.starts_with("//") {
        url = format!("https:{}", url);
    }
    Some(url)
}

fn unescape_js_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('u') => {
                let hex = chars.by_ref().take(4).collect::<String>();
                if hex.len() == 4 {
                    if let Some(decoded) =
                        u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                    {
                        out.push(decoded);
                        continue;
                    }
                }
                out.push('\\');
                out.push('u');
                out.push_str(&hex);
            }
            Some('x') => {
                let hex = chars.by_ref().take(2).collect::<String>();
                if hex.len() == 2 {
                    if let Some(decoded) =
                        u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                    {
                        out.push(decoded);
                        continue;
                    }
                }
                out.push('\\');
                out.push('x');
                out.push_str(&hex);
            }
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn extract_blogger_video_url(text: &str) -> Option<String> {
    let decoded = html_escape::decode_html_entities(text);
    let re =
        regex::Regex::new(r#"https://[^"<>\s]*googlevideo\.com/videoplayback[^"<>\s]*"#).unwrap();
    re.find(&decoded)
        .map(|found| clean_url(&unescape_js_string(found.as_str())))
}

fn parse_search_page(html_text: &str, page: usize) -> (Vec<Movie>, u64) {
    let document = Html::parse_document(html_text);
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

    (movies, total_pages)
}

fn parse_episode_page(html_text: &str) -> (Vec<Episode>, Meta) {
    let document = Html::parse_document(html_text);

    let ep_list_selector = Selector::parse(".episodelist ul li").unwrap();
    let a_selector = Selector::parse("span a").unwrap();
    let mut episodes = vec![];

    for element in document.select(&ep_list_selector) {
        if let Some(a_el) = element.select(&a_selector).next() {
            let raw_text = a_el.inner_html();
            let href = a_el.value().attr("href").unwrap_or("").to_string();
            if raw_text.to_lowercase().contains("batch")
                || href.contains("/lengkap/")
                || href.contains("/batch/")
            {
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
            if lower.contains("tonton juga")
                || lower.contains("kelanjutannya")
                || lower.contains("batch")
            {
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

    (episodes, meta)
}

fn parse_stream_page(html_text: &str) -> (Option<String>, Vec<(String, String)>) {
    let document = Html::parse_document(html_text);
    let mut default_url = None;

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

    let mut mirrors = Vec::new();
    let mirror_sel = Selector::parse(".mirrorstream ul li a").unwrap();
    for a_el in document.select(&mirror_sel) {
        let text = a_el.inner_html().trim().to_string();
        let data_content = a_el.value().attr("data-content").unwrap_or("").to_string();
        mirrors.push((text, data_content));
    }

    (default_url, mirrors)
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
        let html_text = self.send(self.client.get(&url)).await?.text().await?;
        Ok(parse_search_page(&html_text, page))
    }

    async fn get_episodes(&self, movie: Movie) -> anyhow::Result<(Vec<Episode>, Meta)> {
        let html_text = self.send(self.client.get(&movie.id)).await?.text().await?;
        Ok(parse_episode_page(&html_text))
    }

    async fn get_stream_urls(&self, episode: Episode) -> anyhow::Result<Vec<Stream>> {
        let ep_url = episode.id.clone();
        let html_text = self.send(self.client.get(&ep_url)).await?.text().await?;
        
        let mut streams = vec![];
        let (default_url, mirrors) = parse_stream_page(&html_text);

        if let Some(mut url) = default_url {
            url = self.resolve_desustream(&url).await;
            streams.push(Stream {
                url,
                title: "Default".to_string(),
            });
        }

        let params = [("action", "aa1208d27f29ca340c92c66d1926f13f")];
        let ajax_url = format!("{}/wp-admin/admin-ajax.php", self.base_url);
        let res = self
            .send(
                self.client
                    .post(&ajax_url)
                    .header("Referer", &ep_url)
                    .form(&params),
            )
            .await?;
            
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
            
            if let Ok(m_resp) = self
                .send(
                    self.client
                        .post(&ajax_url)
                        .header("Referer", &ep_url)
                        .form(&form),
                )
                .await
            {
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
    fn fixture_search_parses_movies_and_pagination() {
        let (movies, total_pages) =
            parse_search_page(include_str!("fixtures/otakudesu_search.html"), 1);

        assert_eq!(movies.len(), 2);
        assert_eq!(movies[0].title, "Sousou no Frieren");
        assert_eq!(
            movies[0].id,
            "https://otakudesu.blog/anime/sousou-no-frieren-sub-indo/"
        );
        assert_eq!(movies[1].title, "Frieren: Beyond Journey's End");
        assert_eq!(total_pages, 3);
    }

    #[test]
    fn fixture_episode_page_parses_episodes_and_metadata() {
        let (episodes, meta) = parse_episode_page(include_str!("fixtures/otakudesu_episode.html"));

        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[0].title, "Episode 11");
        assert_eq!(episodes[1].title, "Episode 12");
        assert!(meta
            .thumb_url
            .as_deref()
            .is_some_and(|url| url.ends_with("frieren.jpg")));
        assert!(meta
            .data
            .iter()
            .any(|(key, value)| key == "Judul" && value == "Sousou no Frieren"));
        assert!(meta
            .data
            .iter()
            .any(|(key, value)| key == "Status" && value == "Ongoing"));
        assert!(meta
            .data
            .iter()
            .any(|(key, value)| key == "Sinopsis" && value.contains("penyihir elf")));
        assert!(!meta
            .data
            .iter()
            .any(|(_, value)| value.contains("Tonton juga")));
    }

    #[test]
    fn fixture_stream_page_parses_default_and_mirrors() {
        let (default_url, mirrors) = parse_stream_page(include_str!("fixtures/otakudesu_stream.html"));

        assert_eq!(
            default_url.as_deref(),
            Some("https://desustream.net/embed/frieren-12")
        );
        assert_eq!(mirrors.len(), 1);
        assert_eq!(mirrors[0].0, "Mirror 1");
        assert_eq!(mirrors[0].1, "eyJpZCI6Im1pcnJvci0xIn0=");
    }

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
    fn test_extract_iframe_src() {
        let html = r#"<iframe id="myIframe" src="https://www.blogger.com/video.g?token=abc" allowfullscreen></iframe>"#;
        assert_eq!(
            extract_iframe_src(html),
            Some("https://www.blogger.com/video.g?token=abc".to_string())
        );
    }

    #[test]
    fn test_extract_blogger_video_url_unescapes_js_string() {
        let html = r#"[[&quot;https://rr1---sn.example.googlevideo.com/videoplayback?expire\u003d1\u0026sig\u003dabc&quot;,[18]]]"#;
        assert_eq!(
            extract_blogger_video_url(html),
            Some(
                "https://rr1---sn.example.googlevideo.com/videoplayback?expire=1&sig=abc"
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
