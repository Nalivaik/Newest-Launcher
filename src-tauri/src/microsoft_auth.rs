//! Microsoft → Xbox Live → Minecraft Java authentication for the desktop launcher.
//!
//! The application is a public desktop client. It uses Microsoft's supported device
//! authorization flow, never receives a password or client secret, and keeps refresh and
//! Minecraft access tokens exclusively in the operating system credential store.

use keyring::Entry;
use newest_launcher_core::{LauncherCore, Profile, Snapshot};
use reqwest::{redirect::Policy, Client};
use serde::{Deserialize, Serialize};
use std::{sync::Mutex, time::{Duration, SystemTime, UNIX_EPOCH}};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use url::Url;

pub const MICROSOFT_CLIENT_ID: &str = "e3d677ff-6f37-4ebb-8893-faba1ae2b963";

// This Entra registration intentionally serves personal Microsoft accounts. Minecraft/Xbox
// accounts use that audience, and Entra rejects this app on the broader `/common` endpoint.
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBOX_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const CREDENTIAL_SERVICE: &str = "io.newest.launcher.minecraft";
const TOKEN_REFRESH_MARGIN_SECONDS: i64 = 90;

#[derive(Debug)]
pub struct MicrosoftAuth {
    client: Client,
    pending: Mutex<Option<PendingLogin>>,
}

#[derive(Debug)]
struct PendingLogin {
    id: String,
    device_code: String,
    interval_seconds: u64,
    expires_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftLoginChallenge {
    pub id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone)]
pub struct MinecraftSession {
    pub access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredTokens {
    refresh_token: String,
    minecraft_access_token: String,
    minecraft_expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    #[serde(default = "default_poll_interval")]
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct MicrosoftTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: String,
}

#[derive(Debug, Deserialize)]
struct XboxTokenResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XboxDisplayClaims,
}

#[derive(Debug, Deserialize)]
struct XboxDisplayClaims {
    xui: Vec<XboxUserHash>,
}

#[derive(Debug, Deserialize)]
struct XboxUserHash {
    uhs: String,
}

#[derive(Debug, Deserialize)]
struct MinecraftLoginResponse {
    access_token: String,
    expires_in: i64,
}

#[derive(Debug, Deserialize)]
struct EntitlementsResponse {
    items: Vec<Entitlement>,
}

#[derive(Debug, Deserialize)]
struct Entitlement {
    name: String,
}

#[derive(Debug, Deserialize)]
struct MinecraftProfileResponse {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<MinecraftSkin>,
}

#[derive(Debug, Deserialize)]
struct MinecraftSkin {
    #[serde(default)]
    state: String,
    url: String,
}

fn default_poll_interval() -> u64 { 5 }

impl MicrosoftAuth {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder().redirect(Policy::none()).user_agent("Newest-Launcher/0.1 MicrosoftAuth").build()
            .map_err(|_| "Не удалось подготовить Microsoft authentication client".to_owned())?;
        Ok(Self { client, pending: Mutex::new(None) })
    }

    pub async fn begin_sign_in(&self, app: &AppHandle) -> Result<MicrosoftLoginChallenge, String> {
        let response = self.client.post(DEVICE_CODE_URL)
            .form(&[("client_id", MICROSOFT_CLIENT_ID), ("scope", "XboxLive.signin offline_access")])
            .send().await.map_err(microsoft_network_error)?;
        if !response.status().is_success() { return Err("Microsoft не смог начать вход. Проверьте настройки Application (Client) ID.".into()); }
        let body: DeviceCodeResponse = response.json().await.map_err(|_| "Microsoft вернул некорректный ответ для входа".to_owned())?;
        validate_device_code_response(&body)?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = unix_now()?;
        let verification_uri = body.verification_uri_complete.unwrap_or(body.verification_uri);
        let challenge = MicrosoftLoginChallenge { id: id.clone(), user_code: body.user_code, verification_uri: verification_uri.clone(), expires_in: body.expires_in };
        let mut pending = self.pending.lock().map_err(|_| "Microsoft authentication недоступен".to_owned())?;
        *pending = Some(PendingLogin { id, device_code: body.device_code, interval_seconds: body.interval.clamp(1, 15), expires_at: now.saturating_add(body.expires_in.min(900) as i64) });
        drop(pending);
        app.opener().open_url(verification_uri, None::<&str>).map_err(|_| "Не удалось открыть браузер для Microsoft входа".to_owned())?;
        Ok(challenge)
    }

    pub async fn finish_sign_in(&self, core: &LauncherCore, challenge_id: &str) -> Result<Snapshot, String> {
        let pending = {
            let mut guard = self.pending.lock().map_err(|_| "Microsoft authentication недоступен".to_owned())?;
            let pending = guard.take().ok_or_else(|| "Сначала начните вход Microsoft".to_owned())?;
            if pending.id != challenge_id { return Err("Этот запрос Microsoft входа больше не активен".into()); }
            pending
        };
        if unix_now()? >= pending.expires_at { return Err("Код Microsoft входа истёк. Начните вход заново.".into()); }
        let microsoft = self.poll_device_token(&pending).await?;
        let tokens = self.minecraft_tokens(&microsoft.access_token, microsoft.refresh_token.as_deref()).await?;
        let profile = self.verify_entitlement_and_profile(&tokens.minecraft_access_token).await?;
        // The verified Minecraft UUID is stable and non-secret, so it is a safe credential
        // key even before the persistent profile receives its launcher-local random ID.
        save_tokens(&profile.uuid, &tokens).await?;
        core.upsert_microsoft_profile(profile.username, profile.uuid, profile.skin_url).map_err(|error| error.to_string())
    }

    pub async fn session_for_profile(&self, profile: &Profile) -> Result<MinecraftSession, String> {
        if profile.kind != "microsoft" { return Err("Для online-сессии нужен Microsoft-профиль".into()); }
        let tokens = load_tokens(&profile.uuid).await?;
        if tokens.minecraft_expires_at.saturating_sub(unix_now()?) > TOKEN_REFRESH_MARGIN_SECONDS {
            return Ok(MinecraftSession { access_token: tokens.minecraft_access_token });
        }
        let microsoft = refresh_microsoft_token(&self.client, &tokens.refresh_token).await?;
        let refreshed = self.minecraft_tokens(&microsoft.access_token, microsoft.refresh_token.as_deref().or(Some(&tokens.refresh_token))).await?;
        save_tokens(&profile.uuid, &refreshed).await?;
        Ok(MinecraftSession { access_token: refreshed.minecraft_access_token })
    }

    async fn poll_device_token(&self, pending: &PendingLogin) -> Result<MicrosoftTokenResponse, String> {
        let mut interval = pending.interval_seconds;
        loop {
            if unix_now()? >= pending.expires_at { return Err("Время Microsoft входа истекло. Начните заново.".into()); }
            tokio::time::sleep(Duration::from_secs(interval)).await;
            let response = self.client.post(TOKEN_URL).form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", MICROSOFT_CLIENT_ID), ("device_code", pending.device_code.as_str()),
            ]).send().await.map_err(microsoft_network_error)?;
            if response.status().is_success() {
                let token: MicrosoftTokenResponse = response.json().await.map_err(|_| "Microsoft вернул некорректный access token".to_owned())?;
                validate_microsoft_token(&token)?;
                return Ok(token);
            }
            let error = response.json::<OAuthErrorResponse>().await.ok().map(|value| value.error).unwrap_or_default();
            match error.as_str() {
                "authorization_pending" => continue,
                "slow_down" => { interval = (interval + 5).min(30); continue; }
                "authorization_declined" => return Err("Microsoft вход был отменён в браузере".into()),
                "expired_token" => return Err("Код Microsoft входа истёк. Начните вход заново.".into()),
                _ => return Err("Microsoft не подтвердил вход. Попробуйте заново.".into()),
            }
        }
    }

    async fn minecraft_tokens(&self, microsoft_access_token: &str, refresh_token: Option<&str>) -> Result<StoredTokens, String> {
        let xbox: XboxTokenResponse = post_json(&self.client, XBOX_AUTH_URL, &serde_json::json!({
            "Properties": {"AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={microsoft_access_token}")},
            "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT"
        })).await?;
        let uhs = xbox.display_claims.xui.first().map(|value| value.uhs.as_str()).filter(|value| !value.is_empty())
            .ok_or_else(|| "Xbox Live не вернул идентификатор пользователя".to_owned())?;
        let xsts: XboxTokenResponse = post_json(&self.client, XSTS_URL, &serde_json::json!({
            "Properties": {"SandboxId": "RETAIL", "UserTokens": [xbox.token]},
            "RelyingParty": "rp://api.minecraftservices.com/", "TokenType": "JWT"
        })).await?;
        let login: MinecraftLoginResponse = post_json(&self.client, MINECRAFT_LOGIN_URL, &serde_json::json!({
            "identityToken": format!("XBL3.0 x={uhs};{}", xsts.token)
        })).await?;
        if login.access_token.len() < 20 || login.expires_in <= 0 || login.expires_in > 172_800 { return Err("Minecraft Services вернул некорректную online-сессию".into()); }
        let refresh_token = refresh_token.filter(|value| !value.is_empty()).ok_or_else(|| "Microsoft не выдал refresh token. Проверьте public client flow и войдите заново.".to_owned())?;
        if refresh_token.len() > 16_384 { return Err("Microsoft вернул слишком большой refresh token".into()); }
        Ok(StoredTokens { refresh_token: refresh_token.to_owned(), minecraft_access_token: login.access_token, minecraft_expires_at: unix_now()?.saturating_add(login.expires_in) })
    }

    async fn verify_entitlement_and_profile(&self, access_token: &str) -> Result<VerifiedMicrosoftProfile, String> {
        let entitlements: EntitlementsResponse = get_minecraft(&self.client, ENTITLEMENTS_URL, access_token).await?;
        if !entitlements.items.iter().any(|item| matches!(item.name.as_str(), "product_minecraft" | "game_minecraft")) {
            return Err("На этом Microsoft-аккаунте не найдено владение Minecraft Java Edition".into());
        }
        let profile: MinecraftProfileResponse = get_minecraft(&self.client, PROFILE_URL, access_token).await?;
        if !(3..=16).contains(&profile.name.len()) || !profile.name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
            return Err("Minecraft Services вернул некорректное имя профиля".into());
        }
        let uuid = minecraft_uuid(&profile.id)?;
        let skin_url = profile.skins.iter().find(|skin| skin.state.eq_ignore_ascii_case("ACTIVE")).or_else(|| profile.skins.first())
            .map(|skin| validate_skin_url(&skin.url)).transpose()?;
        Ok(VerifiedMicrosoftProfile { username: profile.name, uuid, skin_url })
    }
}

struct VerifiedMicrosoftProfile {
    username: String,
    uuid: String,
    skin_url: Option<String>,
}

async fn refresh_microsoft_token(client: &Client, refresh_token: &str) -> Result<MicrosoftTokenResponse, String> {
    let response = client.post(TOKEN_URL).form(&[
        ("grant_type", "refresh_token"), ("client_id", MICROSOFT_CLIENT_ID), ("refresh_token", refresh_token),
        ("scope", "XboxLive.signin offline_access"),
    ]).send().await.map_err(microsoft_network_error)?;
    if !response.status().is_success() { return Err("Microsoft-сессия истекла. Выполните вход заново.".into()); }
    let token: MicrosoftTokenResponse = response.json().await.map_err(|_| "Microsoft вернул некорректный refresh token".to_owned())?;
    validate_microsoft_token(&token)?;
    Ok(token)
}

async fn post_json<T: for<'de> Deserialize<'de>>(client: &Client, url: &str, body: &serde_json::Value) -> Result<T, String> {
    let response = client.post(url).json(body).send().await.map_err(|_| "Сервис Microsoft/Xbox недоступен".to_owned())?;
    if !response.status().is_success() { return Err("Microsoft/Xbox не подтвердил online-сессию".into()); }
    response.json().await.map_err(|_| "Microsoft/Xbox вернул некорректный ответ".into())
}

async fn get_minecraft<T: for<'de> Deserialize<'de>>(client: &Client, url: &str, access_token: &str) -> Result<T, String> {
    let response = client.get(url).bearer_auth(access_token).send().await.map_err(|_| "Minecraft Services недоступен".to_owned())?;
    if !response.status().is_success() { return Err("Minecraft Services не подтвердил online-сессию".into()); }
    response.json().await.map_err(|_| "Minecraft Services вернул некорректный ответ".into())
}

async fn save_tokens(profile_id: &str, tokens: &StoredTokens) -> Result<(), String> {
    let profile_id = profile_id.to_owned();
    let encoded = serde_json::to_string(tokens).map_err(|_| "Не удалось подготовить Microsoft-сессию".to_owned())?;
    tauri::async_runtime::spawn_blocking(move || {
        Entry::new(CREDENTIAL_SERVICE, &profile_id).map_err(|_| "Системное хранилище учётных данных недоступно".to_owned())?
            .set_password(&encoded).map_err(|_| "Не удалось безопасно сохранить Microsoft-сессию".to_owned())
    }).await.map_err(|_| "Сохранение Microsoft-сессии прервано".to_owned())?
}

async fn load_tokens(profile_id: &str) -> Result<StoredTokens, String> {
    let profile_id = profile_id.to_owned();
    let encoded = tauri::async_runtime::spawn_blocking(move || {
        Entry::new(CREDENTIAL_SERVICE, &profile_id).map_err(|_| "Системное хранилище учётных данных недоступно".to_owned())?
            .get_password().map_err(|_| "Microsoft-сессия не найдена в системном хранилище. Войдите заново.".to_owned())
    }).await.map_err(|_| "Чтение Microsoft-сессии прервано".to_owned())??;
    let tokens: StoredTokens = serde_json::from_str(&encoded).map_err(|_| "Microsoft-сессия в системном хранилище повреждена. Войдите заново.".to_owned())?;
    if tokens.refresh_token.is_empty() || tokens.minecraft_access_token.is_empty() { return Err("Microsoft-сессия повреждена. Войдите заново.".into()); }
    Ok(tokens)
}

fn validate_device_code_response(response: &DeviceCodeResponse) -> Result<(), String> {
    if response.device_code.is_empty() || response.device_code.len() > 16_384 || response.user_code.is_empty() || response.user_code.len() > 128
        || response.expires_in == 0 || response.expires_in > 900 || !is_https_url(&response.verification_uri) {
        return Err("Microsoft вернул некорректный код входа".into());
    }
    if response.verification_uri_complete.as_ref().is_some_and(|url| !is_https_url(url)) { return Err("Microsoft вернул небезопасный URL входа".into()); }
    Ok(())
}

fn validate_microsoft_token(token: &MicrosoftTokenResponse) -> Result<(), String> {
    if token.access_token.len() < 20 || token.access_token.len() > 16_384 { return Err("Microsoft вернул некорректный access token".into()); }
    Ok(())
}

fn minecraft_uuid(value: &str) -> Result<String, String> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err("Minecraft Services вернул некорректный UUID".into()); }
    let formatted = format!("{}-{}-{}-{}-{}", &value[..8], &value[8..12], &value[12..16], &value[16..20], &value[20..]);
    uuid::Uuid::parse_str(&formatted).map_err(|_| "Minecraft Services вернул некорректный UUID".to_owned())?;
    Ok(formatted.to_ascii_lowercase())
}

fn validate_skin_url(value: &str) -> Result<String, String> {
    let url = Url::parse(value).map_err(|_| "Minecraft Services вернул некорректный skin URL".to_owned())?;
    if url.scheme() != "https" || url.username() != "" || url.password().is_some() || url.host_str() != Some("textures.minecraft.net") {
        return Err("Minecraft Services вернул неподдерживаемый skin URL".into());
    }
    Ok(url.to_string())
}

fn is_https_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some() && url.username().is_empty() && url.password().is_none())
}

fn unix_now() -> Result<i64, String> {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_secs().min(i64::MAX as u64) as i64)
        .map_err(|_| "Системное время недоступно для Microsoft входа".to_owned())
}

fn microsoft_network_error(_: reqwest::Error) -> String { "Microsoft authentication недоступен. Проверьте подключение к интернету.".into() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_minecraft_uuid_and_rejects_bad_values() {
        assert_eq!(minecraft_uuid("069a79f444e94726a5befca90e38aaf5").unwrap(), "069a79f4-44e9-4726-a5be-fca90e38aaf5");
        assert!(minecraft_uuid("not-a-minecraft-uuid").is_err());
    }

    #[test]
    fn accepts_only_safe_minecraft_skin_urls() {
        assert!(validate_skin_url("https://textures.minecraft.net/texture/abc").is_ok());
        assert!(validate_skin_url("http://textures.minecraft.net/texture/abc").is_err());
        assert!(validate_skin_url("https://example.invalid/texture/abc").is_err());
    }
}
