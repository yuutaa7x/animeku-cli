use tokio::net::{TcpListener, TcpStream};
use tokio::io::AsyncWriteExt;

pub async fn start_proxy(provider_id: usize) -> anyhow::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let _ = handle_connection(&mut socket, port, provider_id).await;
                });
            }
        }
    });

    Ok(port)
}

fn wrap_url(base_url: &str, target: &str, port: u16) -> String {
    let absolute_url = if target.starts_with("http") {
        target.to_string()
    } else {
        match reqwest::Url::parse(base_url) {
            Ok(base) => base.join(target).map(|u| u.to_string()).unwrap_or_else(|_| target.to_string()),
            Err(_) => target.to_string(),
        }
    };
    let encoded = urlencoding::encode(&absolute_url);
    format!("http://127.0.0.1:{}/?url={}", port, encoded)
}

fn rewrite_m3u8(text: &str, target_url: &str, port: u16) -> String {
    let mut rewritten = String::new();
    
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        
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

async fn handle_connection(socket: &mut TcpStream, port: u16, provider_id: usize) -> anyhow::Result<()> {
    let mut buf = [0; 4096];
    let n = tokio::io::AsyncReadExt::read(socket, &mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);
    
    let mut lines = request.lines();
    let first_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "GET" {
        return Ok(());
    }

    let path = parts[1];
    if !path.starts_with("/?url=") {
        return Ok(());
    }

    let encoded_url = &path[6..];
    let target_url = urlencoding::decode(encoded_url)?.into_owned();

    let is_m3u8 = target_url.contains(".m3u8");
    let content_type = if is_m3u8 {
        "application/vnd.apple.mpegurl"
    } else if target_url.contains(".ts") {
        "video/MP2T"
    } else {
        "application/octet-stream"
    };

    let mut cmd = tokio::process::Command::new("curl");
    cmd.args(&[
        "-s",
        "-L",
        "-A", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    ]);

    if provider_id == crate::PROVIDER_OTAKUDESU {
        cmd.args(&["-e", "https://desustream.net/"]);
    } else {
        cmd.args(&["-e", "https://idlixku.com/"]);
        let cookie_path = crate::util::temp_file("animeku_idlix_cookies.txt");
        cmd.args(&["-b", &cookie_path]);
    }
    
    cmd.arg(&target_url);
    cmd.stdout(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let mut stdout = child.stdout.take().unwrap();

    let header = format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\nConnection: close\r\n\r\n", content_type);
    socket.write_all(header.as_bytes()).await?;

    if is_m3u8 {
        let mut text = String::new();
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut stdout, &mut text).await;
        let rewritten = rewrite_m3u8(&text, &target_url, port);
        socket.write_all(rewritten.as_bytes()).await?;
    } else {
        let _ = tokio::io::copy(&mut stdout, socket).await;
    }
    let _ = child.wait().await;

    Ok(())
}
