use serde_json::Value;
use std::{collections::HashMap, time::{Duration, Instant}};
use tauri::State;
use tokio::sync::Mutex;

const API: &str = "https://api.modrinth.com/v2";
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
const CACHE_ENTRIES: usize = 64;
const CACHE_TTL: Duration = Duration::from_secs(300);

pub struct MetadataClient {
    client: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
}

impl MetadataClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("NewestLauncher/", env!("CARGO_PKG_VERSION"), " (desktop; Modrinth catalogue)"))
                .connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(25))
                .redirect(reqwest::redirect::Policy::none()).build()?,
            cache: Mutex::new(HashMap::new()),
        })
    }

    async fn get(&self, path: String) -> Result<Value, String> {
        if !path.starts_with("/search?") && path != "/tag/game_version" {
            return Err("Неизвестный запрос каталога".into());
        }
        if path.len() > 4096 || path.contains('#') || path.contains('\\') {
            return Err("Некорректный запрос каталога".into());
        }
        // Serialize metadata requests; bounded cache + frontend debounce avoid API bursts.
        let mut cache = self.cache.lock().await;
        if let Some((created, value)) = cache.get(&path) {
            if created.elapsed() < CACHE_TTL { return Ok(value.clone()); }
        }
        for attempt in 0..3_u32 {
            let result = self.client.get(format!("{API}{path}")).send().await;
            let mut response = match result {
                Ok(response) => response,
                Err(_) if attempt < 2 => { tokio::time::sleep(Duration::from_millis(500 << attempt)).await; continue; }
                Err(_) => return Err("Modrinth недоступен: проверьте подключение к сети".into()),
            };
            if response.status().as_u16() == 429 || response.status().is_server_error() {
                if attempt < 2 {
                    let delay = response.headers().get("retry-after").and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok()).unwrap_or(1 << attempt).min(30);
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    continue;
                }
            }
            if !response.status().is_success() {
                return Err(format!("Modrinth ответил HTTP {}. Повторите запрос позже.", response.status().as_u16()));
            }
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| "Соединение с Modrinth прервано")? {
                if body.len() + chunk.len() > MAX_METADATA_BYTES { return Err("Ответ каталога слишком большой".into()); }
                body.extend_from_slice(&chunk);
            }
            let value: Value = serde_json::from_slice(&body).map_err(|_| "Некорректный ответ Modrinth")?;
            if cache.len() >= CACHE_ENTRIES { cache.clear(); }
            cache.insert(path, (Instant::now(), value.clone()));
            return Ok(value);
        }
        Err("Каталог временно недоступен".into())
    }
}

#[tauri::command]
pub async fn modrinth_metadata(client: State<'_, MetadataClient>, path: String) -> Result<Value, String> {
    client.get(path).await
}
