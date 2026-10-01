use async_trait::async_trait;
use colored::Colorize;
use serde_json::Value;
use std::fs;
use std::io::{stdout, Write};
use std::process::Command;
use std::time::Duration;

use crate::{
    ext::Ext,
    models::{Episode, Meta, Movie, Stream},
};

const CHROME_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
fn cookie_cache_path() -> String { crate::util::temp_file("animeku_idlix_cookies.txt") }
fn audio_playlist_path() -> String { crate::util::temp_file("animeku_idlix_audio.m3u8") }
fn subtitle_file_path() -> String {
    if std::env::var("TERMUX_VERSION").is_ok() {
        let sdcard = "/sdcard/Download/animeku_idlix.vtt";
        if std::fs::write(sdcard, "").is_ok() {
            return sdcard.to_string();
        }
    }
    crate::util::temp_file("animeku_idlix.vtt")
}
const DEFAULT_REDEEM_URL: &str = "https://e2e.majorplay.net/api/play";
const BANDWIDTH_1080P: u64 = 2_000_000;
const BANDWIDTH_720P: u64 = 800_000;
const BANDWIDTH_480P: u64 = 450_000;

fn check_curl_available() -> anyhow::Result<()> {
    // Try running `curl --version` to confirm the binary is reachable
    let ok = Command::new("curl")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        anyhow::bail!(
            "curl tidak ditemukan di sistem.\n\
             Streaming Idlix memerlukan curl. Silahkan install:\n\
             - Linux  : sudo apt install curl  /  sudo pacman -S curl  /  pkg install curl (Termux)\n\
             - macOS  : brew install curl\n\
             - Windows: curl sudah tersedia di Windows 10 1803+. Pastikan Windows kamu sudah diperbarui.\n\
             Setelah install, jalankan ulang animeku-cli."
        );
    }
    Ok(())
}

fn curl_get(url: &str) -> anyhow::Result<String> {
    check_curl_available()?;
    let cookie_path = cookie_cache_path();
    let output = Command::new("curl")
        .args([
            "-s",
            "-A", CHROME_USER_AGENT,
            "-H", "Accept: application/json",
            "-H", "Origin: https://z2.idlixku.com",
            "-H", "Referer: https://z2.idlixku.com/",
            "-b", &cookie_path,
            "-c", &cookie_path,
            url,
        ])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("Curl request failed with status: {}", output.status);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn curl_post(url: &str, body: &Value) -> anyhow::Result<String> {
    check_curl_available()?;
    let cookie_path = cookie_cache_path();
    let output = Command::new("curl")
        .args([
            "-s",
            "-X", "POST",
            "-A", CHROME_USER_AGENT,
            "-H", "Accept: application/json",
            "-H", "Content-Type: application/json",
            "-H", "Origin: https://z2.idlixku.com",
            "-H", "Referer: https://z2.idlixku.com/",
            "-b", &cookie_path,
            "-c", &cookie_path,
            "-d", &body.to_string(),
            url,
        ])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("Curl POST failed with status: {}", output.status);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub struct Idlix {
    base_url: String,
}

impl Idlix {
    pub fn new() -> Self {
        Self {
            base_url: "https://z2.idlixku.com".to_string(),
        }
    }
}

#[async_trait]
impl Ext for Idlix {
    async fn search(&mut self, title: String, _page: usize) -> anyhow::Result<(Vec<Movie>, u64)> {
        let url = format!("{}/api/search?q={}", self.base_url, urlencoding::encode(&title));
        let text = curl_get(&url)?;

        let json: Value = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Gagal parse respon pencarian Idlix: {} | mentah: {}", e, text.chars().take(200).collect::<String>()))?;

        let mut movies = vec![];

        if let Some(results) = json["results"].as_array() {
            for item in results {
                let content_type = item["contentType"].as_str().unwrap_or("movie");
                let slug = item["slug"].as_str().unwrap_or("");
                let title_str = item["title"].as_str().unwrap_or("Unknown").to_string();

                let type_tag = if content_type == "tv_series" {
                    "Series"
                } else {
                    "Movie"
                };

                let total_eps = if content_type == "tv_series" {
                    let seasons = item["numberOfSeasons"].as_u64().unwrap_or(1);
                    Some(format!("{} Season", seasons))
                } else {
                    item["quality"].as_str().map(|q| q.to_string())
                };

                movies.push(Movie {
                    id: format!("{}|{}", content_type, slug),
                    title: format!("[{}] {}", type_tag, title_str),
                    total_episodes: total_eps,
                });
            }
        }

        Ok((movies, 1))
    }

    async fn get_episodes(&self, movie: Movie) -> anyhow::Result<(Vec<Episode>, Meta)> {
        let parts: Vec<&str> = movie.id.split('|').collect();
        if parts.len() < 2 {
            anyhow::bail!("Format ID movie tidak valid");
        }

        let content_type = parts[0];
        let slug = parts[1];

        if content_type == "movie" {
            let url = format!("{}/api/movies/{}", self.base_url, slug);
            let text = curl_get(&url)?;
            let json: Value = serde_json::from_str(&text)?;

            let movie_obj = if json.get("movie").is_some() {
                &json["movie"]
            } else {
                &json
            };

            let movie_id = movie_obj["id"].as_str().unwrap_or("").to_string();
            let poster_path = movie_obj["posterPath"].as_str().unwrap_or("");
            let vote_avg = movie_obj["voteAverage"]
                .as_str()
                .or_else(|| movie_obj["voteAverage"].as_f64().map(|_| ""))
                .unwrap_or("N/A");
            let overview = movie_obj["overview"].as_str().unwrap_or("-");
            let release = movie_obj["releaseDate"].as_str().unwrap_or("-");

            let thumb_url = if !poster_path.is_empty() {
                Some(format!("https://image.tmdb.org/t/p/w500{}", poster_path))
            } else {
                None
            };

            let meta = Meta {
                thumb_url,
                data: vec![
                    ("Tipe".to_string(), "Movie".to_string()),
                    ("Rilis".to_string(), release.to_string()),
                    ("Rating".to_string(), vote_avg.to_string()),
                    ("Sinopsis".to_string(), overview.to_string()),
                ],
            };

            let episodes = vec![Episode {
                id: format!("movie|{}", movie_id),
                title: "Putar Film (Full Movie)".to_string(),
                is_series: false,
            }];

            Ok((episodes, meta))
        } else {
            // TV Series
            let url = format!("{}/api/series/{}", self.base_url, slug);
            let text = curl_get(&url)?;
            let json: Value = serde_json::from_str(&text)?;

            let series_obj = if json.get("series").is_some() {
                &json["series"]
            } else {
                &json
            };

            let num_seasons = series_obj["numberOfSeasons"].as_u64().unwrap_or(1);
            let poster_path = series_obj["posterPath"].as_str().unwrap_or("");
            let overview = series_obj["overview"].as_str().unwrap_or("-");
            let vote_avg = series_obj["voteAverage"].as_str().unwrap_or("N/A");
            let status = series_obj["status"].as_str().unwrap_or("-");

            let thumb_url = if !poster_path.is_empty() {
                Some(format!("https://image.tmdb.org/t/p/w500{}", poster_path))
            } else {
                None
            };

            let meta = Meta {
                thumb_url,
                data: vec![
                    ("Tipe".to_string(), "TV Series".to_string()),
                    ("Status".to_string(), status.to_string()),
                    ("Rating".to_string(), vote_avg.to_string()),
                    ("Total Season".to_string(), num_seasons.to_string()),
                    ("Sinopsis".to_string(), overview.to_string()),
                ],
            };

            let mut episodes = vec![];
            for s in 1..=num_seasons {
                let season_url = format!("{}/api/series/{}/season/{}", self.base_url, slug, s);
                if let Ok(s_text) = curl_get(&season_url) {
                    if let Ok(s_json) = serde_json::from_str::<Value>(&s_text) {
                        if let Some(ep_list) = s_json["season"]["episodes"].as_array() {
                            for ep in ep_list {
                                let ep_id = ep["id"].as_str().unwrap_or("");
                                let ep_num = ep["episodeNumber"].as_u64().unwrap_or(0);
                                let ep_name = ep["name"].as_str().unwrap_or("");

                                episodes.push(Episode {
                                    id: format!("episode|{}", ep_id),
                                    title: format!("S{}E{:02} - {}", s, ep_num, ep_name),
                                    is_series: true,
                                });
                            }
                        }
                    }
                }
            }

            Ok((episodes, meta))
        }
    }

    async fn get_stream_urls(&self, episode: Episode) -> anyhow::Result<Vec<Stream>> {
        let parts: Vec<&str> = episode.id.split('|').collect();
        if parts.len() < 2 {
            anyhow::bail!("Format ID episode tidak valid");
        }

        let content_type = parts[0];
        let content_id = parts[1];

        print!("{} Menghubungkan ke Idlix Gateway...", "◆".blue());
        stdout().flush()?;

        let info_url = format!(
            "{}/api/watch/play-info/{}/{}",
            self.base_url, content_type, content_id
        );

        let info_text = curl_get(&info_url)?;
        let info_json: Value = serde_json::from_str(&info_text)
            .map_err(|e| anyhow::anyhow!("Gagal parse play-info: {} | {}", e, info_text.chars().take(200).collect::<String>()))?;

        let gate_token = info_json["gateToken"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Gate token tidak ditemukan: {:?}", info_json))?;

        let server_now = info_json["serverNow"].as_i64().unwrap_or(0);
        let unlock_at = info_json["unlockAt"].as_i64().unwrap_or(0);

        let wait_ms = if unlock_at > server_now {
            unlock_at - server_now
        } else {
            0
        };

        let wait_secs = (wait_ms as f64 / 1000.0).ceil() as u64;

        if wait_secs > 0 {
            println!();
            for s in (1..=wait_secs).rev() {
                print!("\r{} Menunggu proteksi stream terbuka... ({}s) ", "◆".yellow(), s);
                stdout().flush()?;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            println!();
        }

        // Claim session
        print!("{} Mengklaim sesi pemutaran...", "◆".blue());
        stdout().flush()?;

        let claim_url = format!("{}/api/watch/session/claim", self.base_url);
        let claim_payload = serde_json::json!({
            "gateToken": gate_token
        });

        let claim_text = curl_post(&claim_url, &claim_payload)?;
        let claim_json: Value = serde_json::from_str(&claim_text)
            .map_err(|e| anyhow::anyhow!("Klaim gagal: {} | {}", e, claim_text.chars().take(200).collect::<String>()))?;

        let claim = claim_json["claim"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Token claim kosong: {:?}", claim_json))?;
        let redeem_url = claim_json["redeemUrl"]
            .as_str()
            .unwrap_or(DEFAULT_REDEEM_URL);

        // Redeem stream URL
        let redeem_payload = serde_json::json!({
            "claim": claim,
            "mode": "browser"
        });

        let redeem_text = curl_post(redeem_url, &redeem_payload)?;
        let redeem_json: Value = serde_json::from_str(&redeem_text)
            .map_err(|e| anyhow::anyhow!("Redeem gagal: {} | {}", e, redeem_text.chars().take(200).collect::<String>()))?;

        let master_url = redeem_json["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("URL master m3u8 tidak ditemukan: {:?}", redeem_json))?;

        // Extract token parameter from master URL (e.g. ?t=...&pm=browser)
        let token_suffix = if let Some(q_pos) = master_url.find('?') {
            &master_url[q_pos..]
        } else {
            ""
        };

        // Download subtitle if available
        let sub_file = subtitle_file_path();
        let _ = fs::remove_file(&sub_file);
        if let Some(subs) = redeem_json["subtitles"].as_array() {
            let id_sub = subs.iter().find(|s| {
                let lang = s["lang"].as_str().unwrap_or("").to_lowercase();
                let label = s["label"].as_str().unwrap_or("").to_lowercase();
                lang == "id" || lang == "ind" || label.contains("indo")
            });

            if let Some(sub) = id_sub {
                if let Some(sub_path) = sub["path"].as_str() {
                    if let Ok(vtt_text) = curl_get(sub_path) {
                        let _ = fs::write(&sub_file, vtt_text);
                        println!(" {} Subtitle Indonesia berhasil dimuat.", "✓".green());
                    }
                }
            } else if let Some(first_sub) = subs.first() {
                let label = first_sub["label"].as_str().unwrap_or("English");
                if let Some(sub_path) = first_sub["path"].as_str() {
                    if let Ok(vtt_text) = curl_get(sub_path) {
                        let _ = fs::write(&sub_file, vtt_text);
                        println!(" {} Subtitle Indo tidak ada di server Idlix, memuat [{}] (tekan 'v' di MPV untuk hide).", "ℹ".yellow(), label);
                    }
                }
            }
        }

        // Fetch master m3u8 playlist
        let master_text = curl_get(master_url)?;

        let lines: Vec<&str> = master_text.lines().collect();

        // 1. Check for separate audio stream
        let audio_file = audio_playlist_path();
        let _ = fs::remove_file(&audio_file);
        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with("#EXT-X-MEDIA:TYPE=AUDIO") {
                if let Some(uri_pos) = trimmed.find("URI=\"") {
                    let after = &trimmed[uri_pos + 5..];
                    if let Some(uri_end) = after.find('"') {
                        let audio_path = &after[..uri_end];
                        let audio_full_url = resolve_url(master_url, audio_path, token_suffix);
                        if let Ok(audio_raw) = curl_get(&audio_full_url) {
                            if audio_raw.trim().starts_with("#EXTM3U") {
                                let audio_patched = patch_playlist(&audio_raw, &audio_full_url, token_suffix);
                                let _ = fs::write(&audio_file, audio_patched);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // 2. Parse video variants
        let mut variants = Vec::new();
        let mut pending_inf: Option<(String, u32, u64)> = None;

        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with("#EXT-X-STREAM-INF:") {
                pending_inf = Some(parse_stream_inf(trimmed));
            } else if !trimmed.starts_with('#') && !trimmed.is_empty() {
                if let Some((quality, height, bandwidth)) = pending_inf.take() {
                    let child_url = resolve_url(master_url, trimmed, token_suffix);
                    variants.push(StreamVariant {
                        quality,
                        height,
                        bandwidth,
                        child_url,
                    });
                }
            }
        }

        // Sort descending: highest resolution & bandwidth first
        variants.sort_by(|a, b| {
            b.height
                .cmp(&a.height)
                .then_with(|| b.bandwidth.cmp(&a.bandwidth))
        });

        // Track quality counts for disambiguation
        let mut quality_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for variant in &variants {
            *quality_counts.entry(variant.quality.clone()).or_insert(0) += 1;
        }

        let mut seen_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut streams = Vec::new();

        for (idx, variant) in variants.into_iter().enumerate() {
            let child_raw = match curl_get(&variant.child_url) {
                Ok(raw) if raw.trim().starts_with("#EXTM3U") => raw,
                Ok(_) => {
                    eprintln!(" {} Respon stream [{}] tidak valid (bukan m3u8).", "■".red(), variant.quality);
                    continue;
                }
                Err(e) => {
                    eprintln!(" {} Gagal mengunduh stream [{}]: {}", "■".red(), variant.quality, e);
                    continue;
                }
            };

            let child_patched = patch_playlist(&child_raw, &variant.child_url, token_suffix);
            let video_out_path = crate::util::temp_file(&format!("animeku_idlix_video_{}_{}.m3u8", idx + 1, variant.quality));
            if let Err(e) = fs::write(&video_out_path, child_patched) {
                eprintln!(" {} Gagal menulis file playlist [{}]: {}", "■".red(), variant.quality, e);
                continue;
            }

            let title = if *quality_counts.get(&variant.quality).unwrap_or(&0) > 1 {
                let count = seen_counts.entry(variant.quality.clone()).or_insert(0);
                *count += 1;
                format!("Idlix [{} #{}]", variant.quality, count)
            } else {
                format!("Idlix [{}]", variant.quality)
            };

            streams.push(Stream {
                url: video_out_path,
                title,
            });
        }

        streams.push(Stream {
            url: master_url.to_string(),
            title: "Idlix [Auto / Android Native]".to_string(),
        });

        println!(" {}", "✓ Berhasil!".green());
        Ok(streams)
    }
}

struct StreamVariant {
    quality: String,
    height: u32,
    bandwidth: u64,
    child_url: String,
}

fn parse_stream_inf(line: &str) -> (String, u32, u64) {
    let mut height = 0;
    let mut res_label = String::new();

    if let Some(res_pos) = line.find("RESOLUTION=") {
        let rest = &line[res_pos + 11..];
        let end = rest.find(|c: char| c == ',' || c.is_whitespace()).unwrap_or(rest.len());
        let res_str = &rest[..end];

        if let Some(x_pos) = res_str.find('x') {
            if let Ok(h) = res_str[x_pos + 1..].parse::<u32>() {
                height = h;
            }
        }

        if res_str.contains("1080") {
            res_label = "1080p".to_string();
        } else if res_str.contains("720") {
            res_label = "720p".to_string();
        } else if res_str.contains("480") || res_str.contains("854") {
            res_label = "480p".to_string();
        } else if res_str.contains("360") || res_str.contains("640") {
            res_label = "360p".to_string();
        } else {
            res_label = res_str.to_string();
        }
    }

    let mut bandwidth = 0;
    if let Some(bw_pos) = line.find("BANDWIDTH=") {
        let rest = &line[bw_pos + 10..];
        let end = rest.find(|c: char| c == ',' || c.is_whitespace()).unwrap_or(rest.len());
        if let Ok(bw) = rest[..end].parse::<u64>() {
            bandwidth = bw;
        }
    }

    if res_label.is_empty() {
        if bandwidth >= BANDWIDTH_1080P {
            res_label = "1080p".to_string();
            height = 1080;
        } else if bandwidth >= BANDWIDTH_720P {
            res_label = "720p".to_string();
            height = 720;
        } else if bandwidth >= BANDWIDTH_480P {
            res_label = "480p".to_string();
            height = 480;
        } else if bandwidth > 0 {
            res_label = "360p".to_string();
            height = 360;
        }
    }

    if res_label.is_empty() {
        if let Some(name_pos) = line.find("NAME=\"") {
            let rest = &line[name_pos + 6..];
            if let Some(end_quote) = rest.find('"') {
                res_label = rest[..end_quote].to_string();
            }
        }
    }

    if res_label.is_empty() {
        res_label = "Default".to_string();
    }

    (res_label, height, bandwidth)
}

fn resolve_url(base: &str, path: &str, token: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        format!("{}{}", path, token)
    } else if path.starts_with('/') {
        let origin = if let Some(idx) = base.find("://") {
            let rest = &base[idx + 3..];
            if let Some(slash_idx) = rest.find('/') {
                &base[..idx + 3 + slash_idx]
            } else {
                base
            }
        } else {
            "https://e2e.majorplay.net"
        };
        format!("{}{}{}", origin, path, token)
    } else {
        let base_path = if let Some(query_idx) = base.find('?') {
            &base[..query_idx]
        } else {
            base
        };
        let dir = if let Some(last_slash) = base_path.rfind('/') {
            &base_path[..last_slash]
        } else {
            base_path
        };
        format!("{}/{}{}", dir, path, token)
    }
}

fn patch_playlist(raw: &str, base_url: &str, token: &str) -> String {
    let mut out = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#EXT-X-MAP:") {
            if let Some(uri_pos) = trimmed.find("URI=\"") {
                let after = &trimmed[uri_pos + 5..];
                if let Some(uri_end) = after.find('"') {
                    let map_uri = &after[..uri_end];
                    let resolved_map = resolve_url(base_url, map_uri, token);
                    let patched_line = format!(
                        "{}URI=\"{}\"{}",
                        &trimmed[..uri_pos],
                        resolved_map,
                        &after[uri_end + 1..]
                    );
                    out.push(patched_line);
                    continue;
                }
            }
            out.push(trimmed.to_string());
        } else if !trimmed.starts_with('#') && !trimmed.is_empty() {
            out.push(resolve_url(base_url, trimmed, token));
        } else {
            out.push(trimmed.to_string());
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idlix_search() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut idlix = Idlix::new();
            let res = idlix.search("A New Dawn".to_string(), 1).await;
            assert!(res.is_ok());
            let (movies, _) = res.unwrap();
            assert!(!movies.is_empty());
        });
    }

    #[test]
    #[ignore]
    fn test_idlix_stream() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut idlix = Idlix::new();
            let res = idlix.search("A New Dawn".to_string(), 1).await.unwrap();
            let movie = res.0.into_iter().find(|m| m.id.contains("a-new-dawn")).unwrap();
            let (episodes, _meta) = idlix.get_episodes(movie).await.unwrap();
            let ep = episodes.into_iter().next().unwrap();
            let streams = idlix.get_stream_urls(ep).await.unwrap();
            assert!(!streams.is_empty());
        });
    }

    #[test]
    #[ignore]
    fn test_idlix_weathering() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut idlix = Idlix::new();
            let res = idlix.search("Weathering".to_string(), 1).await.unwrap();
            let movie = res.0.into_iter().find(|m| m.title.contains("Weathering")).unwrap();
            let (episodes, _meta) = idlix.get_episodes(movie).await.unwrap();
            let ep = episodes.into_iter().next().unwrap();
            let streams = idlix.get_stream_urls(ep).await.unwrap();
            println!("\nStreams found for Weathering: {:#?}", streams);
            assert!(!streams.is_empty());
            for s in &streams {
                assert!(!s.title.contains("Default"), "Title should not be Default: {}", s.title);
            }
        });
    }

    #[test]
    fn test_parse_stream_inf_variants() {
        let (label1, h1, bw1) = parse_stream_inf("#EXT-X-STREAM-INF:BANDWIDTH=266465");
        assert_eq!(label1, "360p");
        assert_eq!(h1, 360);
        assert_eq!(bw1, 266465);

        let (label2, h2, bw2) = parse_stream_inf("#EXT-X-STREAM-INF:BANDWIDTH=1153667");
        assert_eq!(label2, "720p");
        assert_eq!(h2, 720);
        assert_eq!(bw2, 1153667);

        let (label3, h3, bw3) = parse_stream_inf("#EXT-X-STREAM-INF:BANDWIDTH=2405564");
        assert_eq!(label3, "1080p");
        assert_eq!(h3, 1080);
        assert_eq!(bw3, 2405564);

        let (label4, h4, bw4) = parse_stream_inf("#EXT-X-STREAM-INF:BANDWIDTH=5763366,RESOLUTION=1920x1080,AUDIO=\"audio\"");
        assert_eq!(label4, "1080p");
        assert_eq!(h4, 1080);
        assert_eq!(bw4, 5763366);
    }

    #[test]
    fn test_resolve_url_query_handling() {
        let base = "https://e2e.majorplay.net/v/d1fe173d/config-247532.json?t=c1_/v/d1fe173d/.1790777240-wjJDXb3GCKRy811aMowFCi7mh5umdN41Uy6aaD7XUKk&pm=browser";
        let path = "data-414111.json";
        let token = "?t=c1_/v/d1fe173d/.1790777240-wjJDXb3GCKRy811aMowFCi7mh5umdN41Uy6aaD7XUKk&pm=browser";
        let resolved = resolve_url(base, path, token);
        assert_eq!(
            resolved,
            "https://e2e.majorplay.net/v/d1fe173d/data-414111.json?t=c1_/v/d1fe173d/.1790777240-wjJDXb3GCKRy811aMowFCi7mh5umdN41Uy6aaD7XUKk&pm=browser"
        );
    }

    #[test]
    fn test_patch_playlist() {
        let raw = "#EXTM3U\n#EXT-X-VERSION:3\n#EXTINF:3.12,\nhttps://cdn.example.com/vec-0.svg\n#EXTINF:4.00,\nfiles/style-1.css";
        let base = "https://e2e.majorplay.net/v/123/data.json?t=token123";
        let token = "?t=token123";
        let patched = patch_playlist(raw, base, token);
        assert!(patched.contains("https://cdn.example.com/vec-0.svg?t=token123"));
        assert!(patched.contains("https://e2e.majorplay.net/v/123/files/style-1.css?t=token123"));
    }
}
