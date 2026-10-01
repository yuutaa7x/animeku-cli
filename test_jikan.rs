use std::time::Duration;

#[tokio::main]
async fn main() {
    let url = "https://api.jikan.moe/v4/anime?q=chain&limit=5";
    let client = reqwest::Client::builder()
        .user_agent("animeku-cli/0.2.0")
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    println!("Fetching...");
    match client.get(url).send().await {
        Ok(res) => {
            println!("Status: {}", res.status());
            if let Ok(text) = res.text().await {
                println!("Body: {}", &text[0..std::cmp::min(100, text.len())]);
            }
        }
        Err(e) => println!("Error: {}", e),
    }
}
