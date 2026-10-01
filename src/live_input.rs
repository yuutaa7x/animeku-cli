use std::time::Duration;
use crossterm::{
    cursor,
    event::{Event, EventStream, KeyCode, KeyModifiers},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
};
use futures::StreamExt;
use std::io::{stdout, Write};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum FetchResult {
    Success(String, Vec<String>),
    Error(String),
}

async fn fetch_suggestions(query: String, tx: mpsc::Sender<FetchResult>) {
    if query.trim().is_empty() {
        let _ = tx.send(FetchResult::Success(query, vec![])).await;
        return;
    }
    
    // Create client with timeout and User-Agent (some APIs block default agents or hang)
    let client = match reqwest::Client::builder()
        .user_agent("animeku-cli/0.2.0")
        .timeout(Duration::from_secs(4))
        .build() 
    {
        Ok(c) => c,
        Err(_) => {
            let _ = tx.send(FetchResult::Error(query)).await;
            return;
        }
    };

    let query_payload = serde_json::json!({
        "query": "query ($search: String) { Page(page: 1, perPage: 4) { media(search: $search, type: ANIME, sort: POPULARITY_DESC) { title { romaji english } } } }",
        "variables": { "search": &query }
    });

    match client.post("https://graphql.anilist.co").json(&query_payload).send().await {
        Ok(res) => {
            if res.status().is_success() {
                if let Ok(json) = res.json::<serde_json::Value>().await {
                    let mut titles = vec![];
                    if let Some(media) = json.pointer("/data/Page/media").and_then(|m| m.as_array()) {
                        for item in media {
                            let romaji = item.pointer("/title/romaji").and_then(|t| t.as_str()).unwrap_or("");
                            let english = item.pointer("/title/english").and_then(|t| t.as_str()).unwrap_or("");
                            
                            if !romaji.is_empty() && !titles.contains(&romaji.to_string()) {
                                titles.push(romaji.to_string());
                            }
                            if !english.is_empty() && english.to_lowercase() != romaji.to_lowercase() && !titles.contains(&english.to_string()) {
                                titles.push(english.to_string());
                            }
                        }
                    }
                    let _ = tx.send(FetchResult::Success(query, titles)).await;
                    return;
                }
            }
            // If not success or failed to parse
            let _ = tx.send(FetchResult::Error(query)).await;
        }
        Err(_) => {
            let _ = tx.send(FetchResult::Error(query)).await;
        }
    }
}

pub async fn get_live_input(prompt: &str) -> anyhow::Result<Option<(String, bool)>> {
    let mut stdout_handle = stdout();
    enable_raw_mode()?;
    
    let mut input = String::new();
    let mut suggestions: Vec<String> = vec![];
    let mut selected_index: Option<usize> = None;
    let mut is_loading = false;
    let mut last_fetched_query = String::new();

    let mut reader = EventStream::new();
    let (tx, mut rx) = mpsc::channel(10);
    
    // Render closure
    let mut render = |input: &str, suggestions: &[String], selected: Option<usize>, loading: bool| -> anyhow::Result<()> {
        queue!(
            stdout_handle,
            cursor::Hide,
            cursor::MoveToColumn(0),
            Clear(ClearType::FromCursorDown),
            SetForegroundColor(Color::Cyan),
            Print("? "),
            ResetColor,
            Print(prompt),
            Print(" > "),
            Print(input)
        )?;

        // Print loading or suggestions
        if loading {
            queue!(
                stdout_handle,
                Print("\r\n  "),
                SetForegroundColor(Color::DarkGrey),
                Print("Memuat suggestion..."),
                ResetColor
            )?;
        } else if !suggestions.is_empty() {
            for (i, s) in suggestions.iter().enumerate() {
                queue!(stdout_handle, Print("\r\n"))?;
                if Some(i) == selected {
                    queue!(
                        stdout_handle,
                        SetForegroundColor(Color::Green),
                        Print("  > "),
                        Print(s),
                        ResetColor
                    )?;
                } else {
                    queue!(
                        stdout_handle,
                        Print("    "),
                        Print(s)
                    )?;
                }
            }
        }
        
        // Move cursor back to input line
        let lines_down = if loading { 1 } else { suggestions.len() as u16 };
        if lines_down > 0 {
            queue!(stdout_handle, cursor::MoveUp(lines_down))?;
        }
        
        let prompt_len = 2 + prompt.len() + 3; // "? " + prompt + " > "
        queue!(
            stdout_handle,
            cursor::MoveToColumn((prompt_len + input.len()) as u16),
            cursor::Show
        )?;
        stdout_handle.flush()?;
        Ok(())
    };

    render(&input, &suggestions, selected_index, is_loading)?;

    let mut debounce_timer = tokio::time::sleep(Duration::MAX);
    tokio::pin!(debounce_timer);
    
    let mut fetch_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut final_result: Option<(String, bool)> = None;

    loop {
        tokio::select! {
            _ = &mut debounce_timer => {
                // Timer fired, let's fetch!
                if input != last_fetched_query && !input.trim().is_empty() {
                    is_loading = true;
                    selected_index = None;
                    render(&input, &suggestions, selected_index, is_loading)?;
                    
                    if let Some(t) = fetch_task.take() {
                        t.abort();
                    }
                    let query = input.clone();
                    let tx_clone = tx.clone();
                    fetch_task = Some(tokio::spawn(async move {
                        fetch_suggestions(query, tx_clone).await;
                    }));
                }
                // Reset timer so it doesn't fire immediately again
                debounce_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(86400));
            },
            
            fetch_res = rx.recv() => {
                if let Some(res) = fetch_res {
                    is_loading = false;
                    match res {
                        FetchResult::Success(q, s) => {
                            if q == input {
                                suggestions = s;
                                selected_index = None;
                                last_fetched_query = q;
                            }
                        }
                        FetchResult::Error(q) => {
                            if q == input {
                                suggestions.clear();
                                selected_index = None;
                                last_fetched_query = q;
                            }
                        }
                    }
                    render(&input, &suggestions, selected_index, is_loading)?;
                }
            },
            
            event = reader.next() => {
                match event {
                    Some(Ok(Event::Key(key_event))) => {
                        match key_event.code {
                            KeyCode::Esc => {
                                final_result = None;
                                break;
                            }
                            KeyCode::Enter => {
                                if let Some(idx) = selected_index {
                                    if idx < suggestions.len() {
                                        final_result = Some((suggestions[idx].clone(), true));
                                        break;
                                    }
                                }
                                if !input.trim().is_empty() {
                                    final_result = Some((input.clone(), false));
                                    break;
                                }
                            }
                            KeyCode::Backspace => {
                                if !input.is_empty() {
                                    input.pop();
                                    debounce_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(500));
                                }
                            }
                            KeyCode::Char(c) => {
                                if key_event.modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                                    final_result = None;
                                    break;
                                }
                                input.push(c);
                                debounce_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(500));
                            }
                            KeyCode::Down => {
                                if !suggestions.is_empty() {
                                    selected_index = match selected_index {
                                        None => Some(0),
                                        Some(idx) => Some(std::cmp::min(idx + 1, suggestions.len() - 1)),
                                    };
                                }
                            }
                            KeyCode::Up => {
                                if !suggestions.is_empty() {
                                    selected_index = match selected_index {
                                        None => None,
                                        Some(0) => None,
                                        Some(idx) => Some(idx - 1),
                                    };
                                }
                            }
                            _ => {}
                        }
                        render(&input, &suggestions, selected_index, is_loading)?;
                    }
                    Some(Err(_)) => break,
                    None => break,
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    // Cleanup screen to avoid leaving artifacts
    let lines_down = if is_loading { 1 } else { suggestions.len() as u16 };
    queue!(
        stdout(),
        cursor::MoveToColumn(0),
        Clear(ClearType::FromCursorDown)
    )?;
    if lines_down > 0 {
        for _ in 0..lines_down {
            queue!(stdout(), Print("\n"), Clear(ClearType::CurrentLine))?;
        }
        queue!(stdout(), cursor::MoveUp(lines_down))?;
    }
    stdout().flush()?;
    
    println!(); // Move to next line

    Ok(final_result)
}
