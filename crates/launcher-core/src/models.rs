use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) fn now() -> String { chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true) }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
}
impl Default for Resolution {
    fn default() -> Self { Self { width: 1280, height: 720, fullscreen: false } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInput {
    pub name: String,
    pub minecraft_version: String,
    pub loader: String,
    #[serde(default)]
    pub loader_version: Option<String>,
    #[serde(default)]
    pub java_path: Option<String>,
    pub ram_mb: u32,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default)]
    pub resolution: Resolution,
}

impl InstanceInput {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        validate_version(&self.minecraft_version)?;
        if !["vanilla", "fabric", "forge", "neoforge", "quilt"].contains(&self.loader.as_str()) {
            return Err(CoreError::Invalid("unknown loader"));
        }
        if let Some(version) = &self.loader_version { validate_version(version)?; }
        if let Some(path) = &self.java_path {
            if path.len() > 4096 || path.chars().any(char::is_control) || !Path::new(path).is_absolute() {
                return Err(CoreError::Invalid("Java path must be absolute"));
            }
        }
        if !(256..=131072).contains(&self.ram_mb) { return Err(CoreError::Invalid("RAM must be 256–131072 MB")); }
        if self.jvm_args.len() > 64 || self.jvm_args.iter().any(|arg| arg.len() > 2048 || arg.contains('\0') || arg.contains('\n') || arg.contains('\r')) {
            return Err(CoreError::Invalid("invalid JVM arguments"));
        }
        if !(320..=16384).contains(&self.resolution.width) || !(240..=16384).contains(&self.resolution.height) {
            return Err(CoreError::Invalid("invalid window resolution"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub minecraft_version: String,
    pub loader: String,
    pub loader_version: Option<String>,
    pub game_directory: String,
    pub icon: Option<String>,
    pub created_at: String,
    pub last_played: Option<String>,
    pub total_playtime_seconds: u64,
    pub java_path: Option<String>,
    pub ram_mb: u32,
    pub jvm_args: Vec<String>,
    pub resolution: Resolution,
    pub installation_state: String,
    #[serde(default)]
    pub mods: Vec<ContentRecord>,
    #[serde(default)]
    pub resource_packs: Vec<ContentRecord>,
    #[serde(default)]
    pub shader_packs: Vec<ContentRecord>,
}

/// A locally installed Modrinth file. It contains public package metadata only; download
/// credentials and arbitrary URLs are deliberately never stored in the launcher state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentRecord {
    pub project_id: String,
    pub version_id: String,
    pub title: String,
    pub version_number: String,
    pub filename: String,
    pub sha512: String,
    pub installed_at: String,
}

impl ContentRecord {
    pub fn new(project_id: String, version_id: String, title: String, version_number: String, filename: String, sha512: String) -> Self {
        Self { project_id, version_id, title, version_number, filename, sha512, installed_at: now() }
    }

    pub fn validate(&self) -> Result<()> {
        validate_catalog_id(&self.project_id)?;
        validate_catalog_id(&self.version_id)?;
        if self.title.trim().is_empty() || self.title.chars().count() > 160 || self.title.chars().any(char::is_control) {
            return Err(CoreError::Invalid("invalid content title"));
        }
        if self.version_number.trim().is_empty() || self.version_number.chars().count() > 160 || self.version_number.chars().any(char::is_control) {
            return Err(CoreError::Invalid("invalid content version"));
        }
        validate_content_filename(&self.filename)?;
        if self.sha512.len() != 128 || !self.sha512.bytes().all(|value| value.is_ascii_hexdigit()) {
            return Err(CoreError::Invalid("invalid content SHA-512"));
        }
        if chrono::DateTime::parse_from_rfc3339(&self.installed_at).is_err() {
            return Err(CoreError::Invalid("invalid content timestamp"));
        }
        Ok(())
    }
}

impl Instance {
    pub(crate) fn new(root: &Path, id: String, input: InstanceInput) -> Self {
        let mut instance = Self {
            game_directory: root.join("instances").join(&id).join("game").to_string_lossy().into_owned(),
            id, name: String::new(), minecraft_version: String::new(), loader: String::new(), loader_version: None,
            icon: None, created_at: now(), last_played: None, total_playtime_seconds: 0, java_path: None,
            ram_mb: 4096, jvm_args: vec![], resolution: Resolution::default(), installation_state: "not_installed".into(),
            mods: vec![], resource_packs: vec![], shader_packs: vec![],
        };
        instance.apply(input);
        instance
    }

    pub(crate) fn apply(&mut self, input: InstanceInput) {
        self.name = input.name.trim().to_owned();
        self.minecraft_version = input.minecraft_version;
        self.loader = input.loader;
        self.loader_version = if self.loader == "vanilla" { None } else { input.loader_version };
        self.java_path = input.java_path;
        self.ram_mb = input.ram_mb;
        self.jvm_args = input.jvm_args;
        self.resolution = input.resolution;
    }

    pub(crate) fn input(&self) -> InstanceInput {
        InstanceInput { name: self.name.clone(), minecraft_version: self.minecraft_version.clone(), loader: self.loader.clone(),
            loader_version: self.loader_version.clone(), java_path: self.java_path.clone(), ram_mb: self.ram_mb,
            jvm_args: self.jvm_args.clone(), resolution: self.resolution.clone() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub animations: bool,
    pub transparency: bool,
    pub ui_scale: f64,
    pub default_ram_mb: u32,
    pub concurrent_downloads: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self { language: "ru".into(), theme: "dark".into(), animations: true, transparency: true,
            ui_scale: 1.0, default_ram_mb: 4096, concurrent_downloads: 4 }
    }
}
impl Settings {
    pub(crate) fn validate(&self) -> Result<()> {
        if !["ru", "en", "uk"].contains(&self.language.as_str()) { return Err(CoreError::Invalid("unsupported language")); }
        if !["dark", "light", "system"].contains(&self.theme.as_str()) { return Err(CoreError::Invalid("unsupported theme")); }
        if !self.ui_scale.is_finite() || !(0.75..=2.0).contains(&self.ui_scale) { return Err(CoreError::Invalid("invalid UI scale")); }
        if !(256..=131072).contains(&self.default_ram_mb) { return Err(CoreError::Invalid("invalid default RAM")); }
        if !(1..=16).contains(&self.concurrent_downloads) { return Err(CoreError::Invalid("invalid download concurrency")); }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub schema_version: u32,
    pub instances: Vec<Instance>,
    pub trash: Vec<Instance>,
    pub active_instance_id: Option<String>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub active_profile_id: Option<String>,
    pub settings: Settings,
    pub data_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    /// "offline" profiles are always marked locally; Microsoft profiles will be created only
    /// after a successful entitlement check and never contain access tokens here.
    pub kind: String,
    pub username: String,
    pub uuid: String,
    pub skin_url: Option<String>,
    pub created_at: String,
    pub last_used: Option<String>,
}

impl Profile {
    pub(crate) fn new_offline(username: String) -> Self {
        let now = now();
        Self {
            id: uuid::Uuid::new_v4().to_string(), kind: "offline".into(),
            username, uuid: uuid::Uuid::new_v4().to_string(), skin_url: None,
            created_at: now.clone(), last_used: Some(now),
        }
    }

    pub(crate) fn new_microsoft(username: String, uuid: String, skin_url: Option<String>) -> Self {
        let now = now();
        Self {
            id: uuid::Uuid::new_v4().to_string(), kind: "microsoft".into(), username, uuid, skin_url,
            created_at: now.clone(), last_used: Some(now),
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        if !["offline", "microsoft"].contains(&self.kind.as_str()) { return Err(CoreError::Invalid("unknown profile kind")); }
        validate_username(&self.username)?;
        validate_id(&self.uuid)?;
        if chrono::DateTime::parse_from_rfc3339(&self.created_at).is_err()
            || self.last_used.as_ref().is_some_and(|date| chrono::DateTime::parse_from_rfc3339(date).is_err()) {
            return Err(CoreError::Invalid("invalid profile timestamp"));
        }
        if self.kind == "offline" && self.skin_url.is_some() { return Err(CoreError::Invalid("offline profile cannot claim a skin")); }
        if self.skin_url.as_ref().is_some_and(|url| !url.starts_with("https://")) { return Err(CoreError::Invalid("invalid skin URL")); }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub instances_bytes: u64,
    pub cache_bytes: u64,
    pub trash_bytes: u64,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry { pub timestamp: String, pub level: String, pub event: String, pub message: String }

pub(crate) fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(CoreError::Invalid("name must contain 1–80 printable characters"));
    }
    Ok(())
}
pub(crate) fn validate_username(username: &str) -> Result<()> {
    if !(3..=16).contains(&username.len()) || !username.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return Err(CoreError::Invalid("username must contain 3–16 letters, digits, or underscores"));
    }
    Ok(())
}
pub(crate) fn validate_version(version: &str) -> Result<()> {
    if version.is_empty() || version.len() > 100 || !version.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-+".contains(&c)) {
        return Err(CoreError::Invalid("invalid version identifier"));
    }
    Ok(())
}

pub(crate) fn validate_catalog_id(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 64 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        return Err(CoreError::Invalid("invalid catalogue identifier"));
    }
    Ok(())
}

pub(crate) fn validate_content_filename(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty() || value.len() > 240 || value.chars().any(char::is_control)
        || path.file_name().and_then(|name| name.to_str()) != Some(value) {
        return Err(CoreError::Invalid("invalid content filename"));
    }
    Ok(())
}
pub(crate) fn validate_id(id: &str) -> Result<()> {
    let uuid = uuid::Uuid::parse_str(id).map_err(|_| CoreError::UnsafePath)?;
    if uuid.hyphenated().to_string() != id { return Err(CoreError::UnsafePath); }
    Ok(())
}
