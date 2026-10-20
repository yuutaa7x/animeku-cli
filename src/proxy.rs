use std::sync::Arc;

use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

use crate::ext::{idlix, otakudesu, CHROME_USER_AGENT};

/// Maximum size of an HTTP request head accepted by the proxy.
const MAX_HEADER_BYTES: usize = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Handle to a running local proxy.
///
/// The listener is stopped automatically when this value is dropped, so each
/// playback session leaves no orphaned proxy behind.
pub struct ProxyHandle {
    pub port: u16,
    task: JoinHandle<()>,
}

impl Drop for ProxyHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn start_proxy(provider_id: usize) -> anyhow::Result<ProxyHandle> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let client = Arc::new(
        reqwest::Client::builder()
            .user_agent(CHROME_USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            .build()?,
    );
    let include_subtitles = provider_id == crate::PROVIDER_IDLIX;

    let task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((mut socket, _)) => {
                    let client = Arc::clone(&client);
                    tokio::spawn(async move {
                        let _ = handle_connection(
                            &mut socket,
                            port,
                            provider_id,
                            include_subtitles,
                            &client,
                        )
                        .await;
                    });
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    });

    Ok(ProxyHandle { port, task })
}

fn wrap_url(base_url: &str, target: &str, port: u16) -> String {
    let mut absolute_url = if target.starts_with("http") {
        target.to_string()
    } else {
        match reqwest::Url::parse(base_url) {
            Ok(base) => base
                .join(target)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| target.to_string()),
            Err(_) => target.to_string(),
        }
    };

    if let Ok(base_parsed) = reqwest::Url::parse(base_url) {
        if let Some(query) = base_parsed.query() {
            if let Ok(mut abs_parsed) = reqwest::Url::parse(&absolute_url) {
                if abs_parsed.query().is_none() {
                    abs_parsed.set_query(Some(query));
                    absolute_url = abs_parsed.to_string();
                }
            }
        }
    }

    let encoded = urlencoding::encode(&absolute_url);
    format!("http://127.0.0.1:{}/?url={}", port, encoded)
}

fn rewrite_m3u8(text: &str, target_url: &str, port: u16, include_subtitles: bool) -> String {
    let mut rewritten = String::new();
    let is_master = text.contains("#EXT-X-STREAM-INF");
    let sub_uri = format!("http://127.0.0.1:{}/?sub=1", port);
    let sub_media = format!(
        "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"Indonesia\",LANGUAGE=\"id\",DEFAULT=YES,AUTOSELECT=YES,URI=\"{}\"\n",
        sub_uri
    );

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed == "#EXTM3U" && is_master {
            rewritten.push_str(trimmed);
            rewritten.push('\n');
            if include_subtitles {
                rewritten.push_str(&sub_media);
            }
            continue;
        }

        if trimmed.starts_with("#EXT-X-STREAM-INF:") {
            rewritten.push_str(trimmed);
            if include_subtitles && !trimmed.contains("SUBTITLES=") {
                rewritten.push_str(",SUBTITLES=\"subs\"");
            }
            rewritten.push('\n');
            continue;
        }

        if trimmed.starts_with("#EXT-X-MAP:") || trimmed.starts_with("#EXT-X-MEDIA:") {
            if let Some(uri_pos) = trimmed.find("URI=\"") {
                let after = &trimmed[uri_pos + 5..];
                if let Some(uri_end) = after.find('"') {
                    let map_uri = &after[..uri_end];
                    let new_uri = wrap_url(target_url, map_uri, port);
                    rewritten.push_str(&trimmed[..uri_pos]);
                    rewritten.push_str("URI=\"");
                    rewritten.push_str(&new_uri);
                    rewritten.push('"');
                    rewritten.push_str(&after[uri_end + 1..]);
                    rewritten.push('\n');
                    continue;
                }
            }
            rewritten.push_str(trimmed);
            rewritten.push('\n');
        } else if trimmed.starts_with('#') {
            rewritten.push_str(trimmed);
            rewritten.push('\n');
        } else {
            rewritten.push_str(&wrap_url(target_url, trimmed, port));
            rewritten.push('\n');
        }
    }
    rewritten
}

/// Parse a Netscape cookie jar.
///
/// curl writes HttpOnly cookies with a `#HttpOnly_` prefix. Those lines must
/// not be skipped together with comments, otherwise session/Cloudflare cookies
/// are lost.
fn parse_cookie_jar(content: &str) -> Option<String> {
    let mut pairs = Vec::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 7 {
            pairs.push(format!("{}={}", parts[5], parts[6]));
        }
    }

    if pairs.is_empty() {
        None
    } else {
        Some(pairs.join("; "))
    }
}

fn read_cookies_from_jar(path: &str) -> Option<String> {
    parse_cookie_jar(&std::fs::read_to_string(path).ok()?)
}

async fn read_request(socket: &mut TcpStream) -> anyhow::Result<Option<Vec<u8>>> {
    let mut buf = Vec::with_capacity(8192);

    loop {
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() >= MAX_HEADER_BYTES {
            return Ok(None);
        }

        let mut chunk = [0u8; 4096];
        let read = match timeout(REQUEST_TIMEOUT, socket.read(&mut chunk)).await {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e.into()),
        };
        buf.extend_from_slice(&chunk[..read]);
    }

    if buf.is_empty() {
        Ok(None)
    } else {
        Ok(Some(buf))
    }
}

fn status_reason(status: u16) -> &'static str {
    reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("Unknown")
}

fn response_head(
    status: u16,
    content_type: &str,
    content_length: Option<u64>,
    content_range: Option<&str>,
) -> String {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n",
        status,
        status_reason(status),
        content_type
    );
    if let Some(len) = content_length {
        head.push_str(&format!("Content-Length: {}\r\n", len));
    }
    if let Some(range) = content_range {
        head.push_str(&format!("Content-Range: {}\r\n", range));
    }
    head.push_str("\r\n");
    head
}

async fn respond(
    socket: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    head_only: bool,
    content_range: Option<&str>,
) -> anyhow::Result<()> {
    let head = response_head(status, content_type, Some(body.len() as u64), content_range);
    socket.write_all(head.as_bytes()).await?;
    if !head_only {
        socket.write_all(body).await?;
    }
    Ok(())
}

async fn handle_connection(
    socket: &mut TcpStream,
    port: u16,
    provider_id: usize,
    include_subtitles: bool,
    client: &reqwest::Client,
) -> anyhow::Result<()> {
    let Some(request_bytes) = read_request(socket).await? else {
        return Ok(());
    };
    let request = String::from_utf8_lossy(&request_bytes);

    let mut lines = request.lines();
    let first_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 || (parts[0] != "GET" && parts[0] != "HEAD") {
        return Ok(());
    }
    let head_only = parts[0] == "HEAD";
    let path = parts[1];

    // Extract Range header if present
    let mut range_header = None;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("range:") {
            range_header = Some(value.trim().to_string());
            break;
        }
    }

    if path.starts_with("/?sub=1") {
        if !include_subtitles {
            respond(socket, 404, "text/plain", b"no subtitles", head_only, None).await?;
            return Ok(());
        }

        let active_path = crate::util::get_subtitle_path();

        let mut duration = 1440.0;
        if let Ok(content) = tokio::fs::read_to_string(&active_path).await {
            if let Some(last_arrow) = content.rfind("-->") {
                let after_arrow = &content[last_arrow + 3..];
                let end_time_str = after_arrow.split_whitespace().next().unwrap_or("");
                let parts: Vec<&str> = end_time_str.split(':').collect();
                if parts.len() == 3 {
                    let h: f64 = parts[0].parse().unwrap_or(0.0);
                    let m: f64 = parts[1].parse().unwrap_or(0.0);
                    let s_parts: Vec<&str> = parts[2].split('.').collect();
                    let s: f64 = s_parts[0].parse().unwrap_or(0.0);
                    let ms: f64 = if s_parts.len() > 1 {
                        s_parts[1].parse().unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    let calc = h * 3600.0 + m * 60.0 + s + (ms / 1000.0);
                    if calc > 0.0 {
                        duration = calc;
                    }
                }
            }
        }

        let m3u8_content = format!(
            "#EXTM3U\n#EXT-X-TARGETDURATION:{}\n#EXT-X-VERSION:3\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:{:.3},\nhttp://127.0.0.1:{}/?vtt=1\n#EXT-X-ENDLIST\n",
            duration.ceil() as u64,
            duration,
            port
        );
        respond(
            socket,
            200,
            "application/vnd.apple.mpegurl",
            m3u8_content.as_bytes(),
            head_only,
            None,
        )
        .await?;
        return Ok(());
    }

    if path.starts_with("/?vtt=1") {
        if !include_subtitles {
            respond(socket, 404, "text/plain", b"no subtitles", head_only, None).await?;
            return Ok(());
        }

        let active_path = crate::util::get_subtitle_path();
        match tokio::fs::read_to_string(&active_path).await {
            Ok(sub_content) => {
                respond(
                    socket,
                    200,
                    "text/vtt",
                    sub_content.as_bytes(),
                    head_only,
                    None,
                )
                .await?;
            }
            Err(_) => {
                respond(socket, 404, "text/plain", b"not found", head_only, None).await?;
            }
        }
        return Ok(());
    }

    if !path.starts_with("/?url=") {
        return Ok(());
    }

    let encoded_url = &path[6..];
    let target_url = urlencoding::decode(encoded_url)?.into_owned();

    if !target_url.starts_with("http://") && !target_url.starts_with("https://") {
        respond(socket, 400, "text/plain", b"invalid url", head_only, None).await?;
        return Ok(());
    }

    let mut req = if head_only {
        client.head(&target_url)
    } else {
        client.get(&target_url)
    };

    if provider_id == crate::PROVIDER_OTAKUDESU {
        req = req.header("Referer", otakudesu::OTAKUDESU_REFERER);
    } else {
        req = req.header("Referer", idlix::idlix_referer());
        let cookie_path = crate::util::temp_file("animeku_idlix_cookies.txt");
        if let Some(cookie_hdr) = read_cookies_from_jar(&cookie_path) {
            req = req.header("Cookie", cookie_hdr);
        }
    }

    if let Some(r) = &range_header {
        req = req.header("Range", r);
    }

    let mut resp = match req.send().await {
        Ok(r) => r,
        Err(_) => {
            respond(
                socket,
                502,
                "text/plain",
                b"upstream error",
                head_only,
                None,
            )
            .await?;
            return Ok(());
        }
    };

    let status_code = resp.status().as_u16();
    let content_range = resp
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let content_length = resp
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let upstream_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if std::env::var("ANIMEKU_DEBUG").is_ok() {
        let log_msg = format!(
            "URL: {}\nStatus: {}\nContent-Type: {}\nContent-Length: {:?}\nRange: {:?}\n\n",
            target_url, status_code, upstream_type, content_length, range_header
        );
        let debug_path = crate::util::temp_file("animeku_proxy_debug.txt");
        let _ = crate::util::ensure_private_file(&debug_path);
        if let Ok(mut f) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_path)
            .await
        {
            let _ = f.write_all(log_msg.as_bytes()).await;
        }
    }

    if head_only {
        let head = response_head(
            status_code,
            "application/octet-stream",
            content_length,
            content_range.as_deref(),
        );
        socket.write_all(head.as_bytes()).await?;
        return Ok(());
    }

    // Peek at the first chunk: playlists (including hidden Idlix endpoints
    // whose URL/content-type do not look like m3u8) must be buffered and
    // rewritten, while media segments stream straight through.
    match resp.chunk().await {
        Ok(Some(first)) if first.starts_with(b"#EXTM3U") => {
            let mut body = first.to_vec();
            loop {
                match resp.chunk().await {
                    Ok(Some(bytes)) => body.extend_from_slice(&bytes),
                    Ok(None) => break,
                    Err(_) => break,
                }
            }

            let text = String::from_utf8_lossy(&body);
            let rewritten = rewrite_m3u8(&text, &target_url, port, include_subtitles);
            respond(
                socket,
                200,
                "application/vnd.apple.mpegurl",
                rewritten.as_bytes(),
                false,
                None,
            )
            .await?;
        }
        Ok(Some(first)) => {
            // Regular segment: forward the head, the peeked chunk, then stream
            // the remainder without buffering the whole segment.
            let head = response_head(
                status_code,
                "application/octet-stream",
                content_length,
                content_range.as_deref(),
            );
            socket.write_all(head.as_bytes()).await?;
            socket.write_all(&first).await?;

            loop {
                match resp.chunk().await {
                    Ok(Some(bytes)) => socket.write_all(&bytes).await?,
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }
        Ok(None) => {
            let head = response_head(
                status_code,
                "application/octet-stream",
                Some(0),
                content_range.as_deref(),
            );
            socket.write_all(head.as_bytes()).await?;
        }
        Err(_) => {
            respond(socket, 502, "text/plain", b"upstream error", false, None).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_url() {
        let base = "https://example.com/stream/master.m3u8";
        let target = "seg1.ts";
        let wrapped = wrap_url(base, target, 8080);
        assert_eq!(
            wrapped,
            "http://127.0.0.1:8080/?url=https%3A%2F%2Fexample.com%2Fstream%2Fseg1.ts"
        );
    }

    #[test]
    fn test_parse_cookie_jar_keeps_httponly_cookies() {
        let jar = "# Netscape HTTP Cookie File\n\
                   .example.com\tTRUE\t/\tTRUE\t0\tsession\tabc123\n\
                   #HttpOnly_.example.com\tTRUE\t/\tTRUE\t0\tcf_clearance\tsecret\n\
                   # a comment\n";
        let parsed = parse_cookie_jar(jar).unwrap();
        assert!(parsed.contains("session=abc123"));
        assert!(parsed.contains("cf_clearance=secret"));
    }

    #[test]
    fn test_rewrite_m3u8_without_subtitles() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000000\nvideo/720p.m3u8\n";
        let out = rewrite_m3u8(master, "https://example.com/master.m3u8?t=abc", 8080, false);
        assert!(!out.contains("EXT-X-MEDIA"));
        assert!(!out.contains("SUBTITLES"));
        assert!(out.contains("http://127.0.0.1:8080/?url="));
    }

    #[test]
    fn test_rewrite_m3u8_with_subtitles() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000000\nvideo/720p.m3u8\n";
        let out = rewrite_m3u8(master, "https://example.com/master.m3u8?t=abc", 8080, true);
        assert!(out.contains("TYPE=SUBTITLES"));
        assert!(out.contains("SUBTITLES=\"subs\""));
    }

    #[tokio::test]
    async fn test_proxy_rewrites_hidden_playlist() {
        // Upstream endpoint that returns an m3u8 body without an `.m3u8` URL
        // or an mpegurl content type, mimicking Idlix's hidden playlists.
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = upstream.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let _ = socket.read(&mut buf).await;
                    let body = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nchild.m3u8\n";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });

        let handle = start_proxy(crate::PROVIDER_OTAKUDESU).await.unwrap();
        let target = format!("http://127.0.0.1:{}/hidden-endpoint.json", upstream_port);
        let proxy_url = format!(
            "http://127.0.0.1:{}/?url={}",
            handle.port,
            urlencoding::encode(&target)
        );

        let client = reqwest::Client::new();
        let response = client.get(proxy_url).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let text = response.text().await.unwrap();
        assert!(text.starts_with("#EXTM3U"));
        assert!(text.contains("child.m3u8"));
        assert!(text.contains(&format!("http://127.0.0.1:{}", handle.port)));
    }

    #[tokio::test]
    async fn test_proxy_passes_through_partial_content() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = upstream.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let _ = socket.read(&mut buf).await;
                    let body = b"test";
                    let response = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Type: video/mp4\r\nContent-Range: bytes 0-3/100\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.write_all(body).await;
                });
            }
        });

        let handle = start_proxy(crate::PROVIDER_OTAKUDESU).await.unwrap();
        let target = format!("http://127.0.0.1:{}/segment.m4s", upstream_port);
        let proxy_url = format!(
            "http://127.0.0.1:{}/?url={}",
            handle.port,
            urlencoding::encode(&target)
        );

        let client = reqwest::Client::new();
        let response = client
            .get(proxy_url)
            .header("Range", "bytes=0-3")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 206);
        assert_eq!(
            response
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok()),
            Some("bytes 0-3/100")
        );
        assert_eq!(response.bytes().await.unwrap().as_ref(), b"test");
    }
}
