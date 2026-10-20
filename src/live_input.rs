use std::time::Duration;
use crossterm::{
    cursor,
    event::{
        DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyModifiers,
    },
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
};
use dialoguer::console::measure_text_width;
use futures::StreamExt;
use std::io::{stdout, Write};
use tokio::sync::mpsc;

fn prev_boundary(text: &str, mut index: usize) -> usize {
    if index == 0 {
        return 0;
    }
    index -= 1;
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn next_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    index += 1;
    while !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn local_suggestions(history: &[String], query: &str) -> Vec<String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return vec![];
    }
    history
        .iter()
        .filter(|title| title.to_lowercase().contains(&query))
        .take(4)
        .cloned()
        .collect()
}

fn merge_suggestions(local: Vec<String>, remote: Vec<String>) -> Vec<String> {
    let mut merged = local;
    for title in remote {
        if !merged.iter().any(|existing| existing.eq_ignore_ascii_case(&title)) {
            merged.push(title);
        }
    }
    merged.truncate(6);
    merged
}

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

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> std::io::Result<Self> {
        enable_raw_mode()?;
        let _ = execute!(stdout(), EnableBracketedPaste);
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = execute!(stdout(), DisableBracketedPaste, cursor::Show);
        let _ = disable_raw_mode();
    }
}

pub async fn get_live_input(prompt: &str) -> anyhow::Result<Option<(String, bool)>> {
    let _raw_guard = RawModeGuard::new()?;
    let mut stdout_handle = stdout();
    
    let mut input = String::new();
    let mut cursor_index = 0usize;
    let local_history = crate::input::load_history();
    let mut suggestions: Vec<String> = vec![];
    let mut selected_index: Option<usize> = None;
    let mut is_loading = false;
    let mut last_fetched_query = String::new();

    let mut reader = EventStream::new();
    let (tx, mut rx) = mpsc::channel(10);
    
    // Render closure
    let mut render = |input: &str, cursor_index: usize, suggestions: &[String], selected: Option<usize>, loading: bool| -> anyhow::Result<()> {
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
        
        let prompt_len = 2 + measure_text_width(prompt) + 3; // "? " + prompt + " > "
        let typed_width = measure_text_width(&input[..cursor_index]);
        queue!(
            stdout_handle,
            cursor::MoveToColumn((prompt_len + typed_width) as u16),
            cursor::Show
        )?;
        stdout_handle.flush()?;
        Ok(())
    };

    render(&input, cursor_index, &suggestions, selected_index, is_loading)?;

    let debounce_timer = tokio::time::sleep(Duration::MAX);
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
                    // Show local history matches immediately; the network
                    // request below will refine the list when it arrives.
                    suggestions = local_suggestions(&local_history, &input);
                    render(&input, cursor_index, &suggestions, selected_index, is_loading)?;
                    
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
                        FetchResult::Success(q, remote) => {
                            if q == input {
                                let local = local_suggestions(&local_history, &q);
                                suggestions = merge_suggestions(local, remote);
                                selected_index = None;
                                last_fetched_query = q;
                            }
                        }
                        FetchResult::Error(q) => {
                            if q == input {
                                // Keep local history matches when AniList is
                                // unreachable, instead of clearing the list.
                                suggestions = local_suggestions(&local_history, &q);
                                selected_index = None;
                                last_fetched_query = q;
                            }
                        }
                    }
                    render(&input, cursor_index, &suggestions, selected_index, is_loading)?;
                }
            },
            
            event = reader.next() => {
                match event {
                    Some(Ok(Event::Key(key_event))) => {
                        // FIX WINDOWS DOUBLE TYPING BUG:
                        // Ignore key release events, only process press or repeat events
                        if key_event.kind == crossterm::event::KeyEventKind::Release {
                            continue;
                        }

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
                                if cursor_index > 0 {
                                    let prev = prev_boundary(&input, cursor_index);
                                    input.remove(prev);
                                    cursor_index = prev;
                                }
                                if input.trim().is_empty() {
                                    suggestions.clear();
                                    is_loading = false;
                                    last_fetched_query.clear();
                                    if let Some(t) = fetch_task.take() {
                                        t.abort();
                                    }
                                } else {
                                    suggestions = local_suggestions(&local_history, &input);
                                    selected_index = None;
                                    debounce_timer.as_mut().reset(
                                        tokio::time::Instant::now() + Duration::from_millis(500),
                                    );
                                }
                            }
                            KeyCode::Delete => {
                                if cursor_index < input.len() {
                                    let next = next_boundary(&input, cursor_index);
                                    input.drain(cursor_index..next);
                                    suggestions = local_suggestions(&local_history, &input);
                                    selected_index = None;
                                    debounce_timer.as_mut().reset(
                                        tokio::time::Instant::now() + Duration::from_millis(500),
                                    );
                                }
                            }
                            KeyCode::Left => {
                                cursor_index = prev_boundary(&input, cursor_index);
                            }
                            KeyCode::Right => {
                                cursor_index = next_boundary(&input, cursor_index);
                            }
                            KeyCode::Home => cursor_index = 0,
                            KeyCode::End => cursor_index = input.len(),
                            KeyCode::Tab if !suggestions.is_empty() => {
                                let index = selected_index.unwrap_or(0);
                                if let Some(suggestion) = suggestions.get(index) {
                                    input = suggestion.clone();
                                    cursor_index = input.len();
                                    selected_index = None;
                                    suggestions =
                                        local_suggestions(&local_history, &input);
                                }
                            }
                            KeyCode::Char(c) => {
                                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                                    match c.to_ascii_lowercase() {
                                        'c' => {
                                            final_result = None;
                                            break;
                                        }
                                        'u' => {
                                            input.drain(..cursor_index);
                                            cursor_index = 0;
                                        }
                                        'w' => {
                                            let mut start = cursor_index;
                                            while start > 0 {
                                                let prev = prev_boundary(&input, start);
                                                if input[prev..start]
                                                    .chars()
                                                    .all(char::is_whitespace)
                                                {
                                                    start = prev;
                                                } else {
                                                    break;
                                                }
                                            }
                                            while start > 0 {
                                                let prev = prev_boundary(&input, start);
                                                if input[prev..start]
                                                    .chars()
                                                    .any(char::is_whitespace)
                                                {
                                                    break;
                                                }
                                                start = prev;
                                            }
                                            input.drain(start..cursor_index);
                                            cursor_index = start;
                                        }
                                        _ => {}
                                    }
                                } else if !c.is_control() {
                                    input.insert(cursor_index, c);
                                    cursor_index += c.len_utf8();
                                }
                                if input.trim().is_empty() {
                                    suggestions.clear();
                                    is_loading = false;
                                    last_fetched_query.clear();
                                    if let Some(t) = fetch_task.take() {
                                        t.abort();
                                    }
                                } else {
                                    suggestions = local_suggestions(&local_history, &input);
                                    selected_index = None;
                                }
                                debounce_timer.as_mut().reset(
                                    tokio::time::Instant::now() + Duration::from_millis(500),
                                );
                            }
                            KeyCode::Down if !suggestions.is_empty() => {
                                selected_index = match selected_index {
                                    None => Some(0),
                                    Some(idx) => Some(std::cmp::min(idx + 1, suggestions.len() - 1)),
                                };
                            }
                            KeyCode::Up if !suggestions.is_empty() => {
                                selected_index = match selected_index {
                                    None => None,
                                    Some(0) => None,
                                    Some(idx) => Some(idx - 1),
                                };
                            }
                            _ => {}
                        }
                        render(&input, cursor_index, &suggestions, selected_index, is_loading)?;
                    }
                    Some(Ok(Event::Paste(text))) => {
                        let sanitized = text
                            .chars()
                            .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
                            .collect::<String>();
                        input.insert_str(cursor_index, &sanitized);
                        cursor_index += sanitized.len();
                        suggestions = local_suggestions(&local_history, &input);
                        selected_index = None;
                        debounce_timer.as_mut().reset(
                            tokio::time::Instant::now() + Duration::from_millis(500),
                        );
                        render(&input, cursor_index, &suggestions, selected_index, is_loading)?;
                    }
                    Some(Err(_)) => break,
                    None => break,
                    _ => {}
                }
            }
        }
    }

    if let Some(t) = fetch_task.take() {
        t.abort();
    }

    // Cleanup screen to avoid leaving artifacts
    let lines_down = if is_loading { 1 } else { suggestions.len() as u16 };
    let _ = queue!(
        stdout(),
        cursor::MoveToColumn(0),
        Clear(ClearType::FromCursorDown)
    );
    if lines_down > 0 {
        for _ in 0..lines_down {
            let _ = queue!(stdout(), Print("\n"), Clear(ClearType::CurrentLine));
        }
        let _ = queue!(stdout(), cursor::MoveUp(lines_down));
    }
    let _ = stdout().flush();
    
    println!(); // Move to next line

    Ok(final_result)
}
