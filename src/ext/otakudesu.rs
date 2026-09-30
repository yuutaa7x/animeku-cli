use base64::prelude::*;
use std::collections::HashMap;
use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;

use crate::{
    ext::Ext,
    models::{Episode, Meta, Movie, Stream},
};

pub struct Otakudesu {
    client: Client,
    base_url: String,
}

impl Otakudesu {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/110.0.0.0 Safari/537.36")
                .build()
                .unwrap(),
            base_url: "https://otakudesu.blog".to_string(),
        }
    }
}

#[async_trait]
impl Ext for Otakudesu {
    async fn search(&mut self, title: String, _page: usize) -> anyhow::Result<(Vec<Movie>, u64)> {
        let url = format!("{}/?s={}&post_type=anime", self.base_url, urlencoding::encode(&title));
        let html_text = self.client.get(&url).send().await?.text().await?;
        let document = Html::parse_document(&html_text);
        let selector = Selector::parse("ul.chivsrc li").unwrap();
        let a_selector = Selector::parse("h2 a").unwrap();
        
        let mut movies = vec![];
        for element in document.select(&selector) {
            if let Some(a_el) = element.select(&a_selector).next() {
                let text = a_el.inner_html().trim().to_string();
                let href = a_el.value().attr("href").unwrap_or("").to_string();
                movies.push(Movie {
                    id: href,
                    title: text,
                    total_episodes: None,
                });
            }
        }
        
        // Pagination logic not fully implemented for otakudesu, returning hardcoded total 1 for now
        Ok((movies, 1))
    }

    async fn get_episodes(&self, movie: Movie) -> anyhow::Result<(Vec<Episode>, Meta)> {
        let html_text = self.client.get(&movie.id).send().await?.text().await?;
        let document = Html::parse_document(&html_text);
        
        let ep_list_selector = Selector::parse(".episodelist ul li").unwrap();
        let a_selector = Selector::parse("span a").unwrap();
        let mut episodes = vec![];
        

        for element in document.select(&ep_list_selector) {
            if let Some(a_el) = element.select(&a_selector).next() {
                let text = a_el.inner_html().trim().to_string();
                let href = a_el.value().attr("href").unwrap_or("").to_string();
                if text.to_lowercase().contains("batch") || href.contains("/lengkap/") || href.contains("/batch/") {
                    continue;
                }
                episodes.push(Episode {
                    id: href,
                    title: text,
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
                if let Some(idx) = clean.find(":") {
                    meta.data.push((clean[..idx].trim().to_string(), clean[idx+1..].trim().to_string()));
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
                    sinopsis_text.push_str(" ");
                }
                sinopsis_text.push_str(&clean);
            }
            if sinopsis_text.len() > 320 {
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
                    let mut url = src.to_string();
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
            
                                        if url.contains("desustream") {
                                            if let Ok(resp) = self.client.get(&url).send().await {
                                                if let Ok(text) = resp.text().await {
                                                    if let Some(src_start) = text.find("<source src=\"") {
                                                        let rest = &text[src_start + 13..];
                                                        if let Some(src_end) = rest.find("\"") {
                                                            url = rest[..src_end].to_string();
                                                        }
                                                    } else if let Some(file_start) = text.find("otakudesu('{\"file\":\"") {
                                                        let rest = &text[file_start + 20..];
                                                        if let Some(file_end) = rest.find("\"") {
                                                            url = rest[..file_end].to_string();
                                                        }
                                                    } else if let Some(file_start) = text.find("\"file\":\"") {
                                                        let rest = &text[file_start + 8..];
                                                        if let Some(file_end) = rest.find("\"") {
                                                            url = rest[..file_end].to_string();
                                                        }
                                                    }
                                                }
                                            }
                                        }
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
                                if let Some(src_start) = html_str.find("src=\"") {
                                    let rest = &html_str[src_start + 5..];
                                    if let Some(src_end) = rest.find("\"") {
                                        let mut url = rest[..src_end].to_string();
                                        
                                        if url.starts_with("//") {
                                            url = format!("https:{}", url);
                                        }
                                        
                                        if url.contains("desustream") {
                                            if let Ok(resp) = self.client.get(&url).send().await {
                                                if let Ok(text) = resp.text().await {
                                                    if let Some(src_start) = text.find("<source src=\"") {
                                                        let rest = &text[src_start + 13..];
                                                        if let Some(src_end) = rest.find("\"") {
                                                            url = rest[..src_end].to_string();
                                                        }
                                                    } else if let Some(file_start) = text.find("otakudesu('{\"file\":\"") {
                                                        let rest = &text[file_start + 20..];
                                                        if let Some(file_end) = rest.find("\"") {
                                                            url = rest[..file_end].to_string();
                                                        }
                                                    } else if let Some(file_start) = text.find("\"file\":\"") {
                                                        let rest = &text[file_start + 8..];
                                                        if let Some(file_end) = rest.find("\"") {
                                                            url = rest[..file_end].to_string();
                                                        }
                                                    }
                                                }
                                            }
                                        }
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
        }

        Ok(streams)
    }
}
