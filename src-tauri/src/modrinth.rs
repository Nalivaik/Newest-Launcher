//! Verified, instance-local downloads from the official Modrinth API.
//!
//! The webview supplies only a Modrinth project id and a content category. This module resolves
//! the compatible version itself, accepts only Modrinth's CDN, verifies SHA-512 while streaming,
//! and never accepts a destination path or a download URL from the frontend.

use newest_launcher_core::{ContentRecord, Instance, LauncherCore, Snapshot};
use reqwest::{Client, Url};
use serde::Deserialize;
use sha2::{Digest, Sha512};
use std::{fs, path::Path, time::Duration};
use tokio::io::AsyncWriteExt;

const API: &str = "https://api.modrinth.com/v2";
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
const MAX_CONTENT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub struct ModrinthInstaller { client: Client }

impl ModrinthInstaller {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: Client::builder()
                .user_agent(concat!("NewestLauncher/", env!("CARGO_PKG_VERSION"), " (desktop; verified Modrinth installer)"))
                .connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(60))
                .redirect(reqwest::redirect::Policy::none()).build()?,
        })
    }

    pub async fn install(&self, core: &LauncherCore, instance_id: String, project_id: String, requested_type: String) -> Result<Snapshot, String> {
        let content_type = ContentType::parse(&requested_type)?;
        validate_catalog_id(&project_id)?;
        let instance = core.instance(&instance_id).map_err(core_error)?;
        if content_type == ContentType::Mod && instance.loader == "vanilla" {
            return Err("Для модов выберите instance с Fabric, Forge, NeoForge или Quilt".into());
        }

        let version = self.compatible_version(&project_id, &instance, content_type).await?;
        let file = select_file(&version, content_type)?;
        validate_cdn_url(&file.url)?;
        validate_sha512(&file.sha512)?;
        if file.size == 0 || file.size > MAX_CONTENT_BYTES {
            return Err("Файл Modrinth имеет недопустимый размер".into());
        }

        let directory = core.folder_path(content_type.directory(), Some(&instance_id)).map_err(core_error)?;
        let filename = format!("modrinth-{}.{}", project_id, content_type.extension());
        let target = directory.join(&filename);
        reject_symlink(&target)?;
        let record = ContentRecord::new(
            project_id.clone(), version.id, version.name, version.version_number, filename,
            file.sha512.to_ascii_lowercase(),
        );
        let existing = content_records(&instance, content_type).iter().find(|item| item.project_id == project_id).cloned();

        if target.exists() && verify_sha512(&target, &record.sha512).await? {
            return core.record_content_installation(&instance_id, content_type.as_str(), record).map_err(core_error);
        }
        if target.exists() {
            let Some(previous) = existing.as_ref() else {
                return Err(format!("Файл {} уже существует и не принадлежит сохранённой записи Modrinth", target.display()));
            };
            if previous.filename != record.filename || !verify_sha512(&target, &previous.sha512).await? {
                return Err(format!("Файл {} был изменён вне лаунчера. Удалите или переименуйте его перед обновлением", target.display()));
            }
        }

        let temporary = directory.join(format!(".newest-modrinth-{}.part", uuid::Uuid::new_v4()));
        let downloaded = self.download_file(&file, &temporary).await;
        if let Err(error) = downloaded {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error);
        }

        let backup = directory.join(format!(".newest-modrinth-backup-{}.part", uuid::Uuid::new_v4()));
        let had_previous = target.exists();
        if had_previous { fs::rename(&target, &backup).map_err(io_error)?; }
        if let Err(error) = tokio::fs::rename(&temporary, &target).await {
            if had_previous { let _ = fs::rename(&backup, &target); }
            return Err(io_error(error));
        }
        match core.record_content_installation(&instance_id, content_type.as_str(), record) {
            Ok(snapshot) => {
                if had_previous { let _ = fs::remove_file(&backup); }
                Ok(snapshot)
            }
            Err(error) => {
                let _ = fs::remove_file(&target);
                if had_previous { let _ = fs::rename(&backup, &target); }
                Err(core_error(error))
            }
        }
    }

    async fn compatible_version(&self, project_id: &str, instance: &Instance, content_type: ContentType) -> Result<ModrinthVersion, String> {
        let mut url = Url::parse(&format!("{API}/project/{project_id}/version")).map_err(|_| "Некорректный адрес Modrinth".to_owned())?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("game_versions", &serde_json::to_string(&[&instance.minecraft_version]).map_err(|_| "Не удалось сформировать запрос Modrinth")?);
            if content_type == ContentType::Mod {
                query.append_pair("loaders", &serde_json::to_string(&[&instance.loader]).map_err(|_| "Не удалось сформировать запрос Modrinth")?);
            }
        }
        let mut response = self.client.get(url).send().await.map_err(network_error)?;
        if !response.status().is_success() {
            return Err(format!("Modrinth не отдал совместимую версию: HTTP {}", response.status().as_u16()));
        }
        if response.content_length().is_some_and(|size| size as usize > MAX_METADATA_BYTES) {
            return Err("Ответ Modrinth с версиями слишком большой".into());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if body.len().saturating_add(chunk.len()) > MAX_METADATA_BYTES { return Err("Ответ Modrinth с версиями слишком большой".into()); }
            body.extend_from_slice(&chunk);
        }
        let versions: Vec<ModrinthVersion> = serde_json::from_slice(&body).map_err(|_| "Modrinth вернул некорректные metadata версии")?;
        let version = versions.into_iter().find(|version| {
            version.game_versions.iter().any(|value| value == &instance.minecraft_version)
                && (content_type != ContentType::Mod || version.loaders.iter().any(|value| value == &instance.loader))
        }).ok_or_else(|| format!("В Modrinth нет {} для Minecraft {}{}", content_type.label(), instance.minecraft_version,
            if content_type == ContentType::Mod { format!(" и {}", instance.loader) } else { String::new() }))
        ?;
        if version.project_id != project_id { return Err("Modrinth вернул версию другого проекта".into()); }
        Ok(version)
    }

    async fn download_file(&self, file: &SelectedFile, temporary: &Path) -> Result<(), String> {
        reject_symlink(temporary)?;
        let mut response = self.client.get(&file.url).send().await.map_err(network_error)?;
        if !response.status().is_success() { return Err(format!("Не удалось скачать файл Modrinth: HTTP {}", response.status().as_u16())); }
        if response.content_length().is_some_and(|size| size != file.size || size > MAX_CONTENT_BYTES) {
            return Err("Размер файла Modrinth не соответствует metadata".into());
        }
        let mut output = tokio::fs::OpenOptions::new().create_new(true).write(true).open(temporary).await.map_err(io_error)?;
        let mut hash = Sha512::new();
        let mut received = 0_u64;
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            received = received.checked_add(chunk.len() as u64).ok_or_else(|| "Размер файла Modrinth переполнен".to_owned())?;
            if received > file.size || received > MAX_CONTENT_BYTES { return Err("Файл Modrinth превышает заявленный размер".into()); }
            hash.update(&chunk);
            output.write_all(&chunk).await.map_err(io_error)?;
        }
        output.flush().await.map_err(io_error)?;
        output.sync_all().await.map_err(io_error)?;
        if received != file.size || format!("{:x}", hash.finalize()) != file.sha512.to_ascii_lowercase() {
            return Err("SHA-512 файла Modrinth не совпала с metadata".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentType { Mod, ResourcePack, Shader }

impl ContentType {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "mod" => Ok(Self::Mod), "resourcepack" => Ok(Self::ResourcePack), "shader" => Ok(Self::Shader),
            _ => Err("Поддерживаются только моды, ресурспаки и шейдеры Modrinth".into()),
        }
    }
    fn as_str(self) -> &'static str { match self { Self::Mod => "mod", Self::ResourcePack => "resourcepack", Self::Shader => "shader" } }
    fn directory(self) -> &'static str { match self { Self::Mod => "mods", Self::ResourcePack => "resourcepacks", Self::Shader => "shaderpacks" } }
    fn extension(self) -> &'static str { match self { Self::Mod => "jar", Self::ResourcePack | Self::Shader => "zip" } }
    fn label(self) -> &'static str { match self { Self::Mod => "мода", Self::ResourcePack => "ресурспака", Self::Shader => "шейдера" } }
}

#[derive(Deserialize)]
struct ModrinthVersion {
    id: String,
    project_id: String,
    name: String,
    version_number: String,
    #[serde(default)] game_versions: Vec<String>,
    #[serde(default)] loaders: Vec<String>,
    files: Vec<ModrinthFile>,
}

#[derive(Clone, Deserialize)]
struct ModrinthFile {
    hashes: std::collections::HashMap<String, String>,
    url: String,
    filename: String,
    primary: bool,
    size: u64,
}

impl ModrinthFile {
    fn sha512(&self) -> Option<&str> { self.hashes.get("sha512").map(String::as_str) }
}

fn select_file(version: &ModrinthVersion, content_type: ContentType) -> Result<SelectedFile, String> {
    validate_catalog_id(&version.id)?;
    validate_catalog_id(&version.project_id)?;
    if version.name.trim().is_empty() || version.name.chars().count() > 160 || version.name.chars().any(char::is_control)
        || version.version_number.trim().is_empty() || version.version_number.chars().count() > 160 || version.version_number.chars().any(char::is_control) {
        return Err("Modrinth вернул небезопасные metadata версии".into());
    }
    let allowed = |file: &&ModrinthFile| file.filename.to_ascii_lowercase().ends_with(&format!(".{}", content_type.extension()));
    let file = version.files.iter().find(|file| file.primary && allowed(file))
        .or_else(|| version.files.iter().find(allowed))
        .ok_or_else(|| format!("Совместимая версия Modrinth не содержит .{} файла", content_type.extension()))?;
    let sha512 = file.sha512().ok_or_else(|| "Modrinth не предоставил SHA-512 файла".to_owned())?.to_owned();
    Ok(SelectedFile { url: file.url.clone(), sha512, size: file.size })
}

struct SelectedFile { url: String, sha512: String, size: u64 }

fn content_records(instance: &Instance, content_type: ContentType) -> &[ContentRecord] {
    match content_type { ContentType::Mod => &instance.mods, ContentType::ResourcePack => &instance.resource_packs, ContentType::Shader => &instance.shader_packs }
}

async fn verify_sha512(path: &Path, expected: &str) -> Result<bool, String> {
    let path = path.to_owned();
    let expected = expected.to_owned();
    tokio::task::spawn_blocking(move || {
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_CONTENT_BYTES { return Ok(false); }
        let mut file = std::fs::File::open(&path).map_err(io_error)?;
        let mut hash = Sha512::new();
        let mut buffer = [0_u8; 128 * 1024];
        loop {
            let read = std::io::Read::read(&mut file, &mut buffer).map_err(io_error)?;
            if read == 0 { break; }
            hash.update(&buffer[..read]);
        }
        Ok(format!("{:x}", hash.finalize()) == expected.to_ascii_lowercase())
    }).await.map_err(|_| "Проверка файла Modrinth была прервана")?
}

fn validate_cdn_url(value: &str) -> Result<(), String> {
    let url = Url::parse(value).map_err(|_| "Modrinth вернул некорректный URL файла")?;
    if url.scheme() != "https" || url.host_str() != Some("cdn.modrinth.com") || url.port().is_some() || !url.username().is_empty() || url.password().is_some() {
        return Err("Файл разрешено скачивать только с официального CDN Modrinth".into());
    }
    Ok(())
}

fn validate_catalog_id(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 64 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        return Err("Некорректный идентификатор проекта Modrinth".into());
    }
    Ok(())
}

fn validate_sha512(value: &str) -> Result<(), String> {
    if value.len() != 128 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Modrinth вернул некорректную SHA-512 файла".into());
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) => Err("Небезопасный путь файла Modrinth".into()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn network_error(_: reqwest::Error) -> String { "Не удалось связаться с Modrinth. Проверьте подключение к сети".into() }
fn io_error(error: std::io::Error) -> String { format!("Ошибка файловой операции: {error}") }
fn core_error(error: newest_launcher_core::CoreError) -> String { error.to_string() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_official_cdn_urls() {
        assert!(validate_cdn_url("https://cdn.modrinth.com/data/project/file.jar").is_ok());
        assert!(validate_cdn_url("https://example.com/file.jar").is_err());
        assert!(validate_cdn_url("https://cdn.modrinth.com.evil.example/file.jar").is_err());
    }

    #[test]
    fn chooses_primary_file_with_required_extension() {
        let version = ModrinthVersion {
            id: "version".into(), project_id: "project".into(), name: "Example".into(), version_number: "1.0.0".into(),
            game_versions: vec!["1.21.1".into()], loaders: vec!["fabric".into()],
            files: vec![
                ModrinthFile { hashes: [("sha512".into(), "a".repeat(128))].into(), url: "https://cdn.modrinth.com/data/project/readme.txt".into(), filename: "readme.txt".into(), primary: true, size: 1 },
                ModrinthFile { hashes: [("sha512".into(), "b".repeat(128))].into(), url: "https://cdn.modrinth.com/data/project/mod.jar".into(), filename: "mod.jar".into(), primary: false, size: 2 },
            ],
        };
        let file = select_file(&version, ContentType::Mod).unwrap();
        assert_eq!(file.size, 2);
        assert!(file.url.ends_with("mod.jar"));
    }

    #[tokio::test]
    #[ignore = "downloads real, compatibility-filtered content from the official Modrinth CDN"]
    async fn installs_real_modrinth_content_into_an_isolated_instance() {
        let root = std::env::temp_dir().join(format!("newest-launcher-modrinth-{}", uuid::Uuid::new_v4()));
        let core = LauncherCore::open(root.clone()).unwrap();
        let input = newest_launcher_core::InstanceInput {
            name: "Modrinth test".into(), minecraft_version: "1.21.1".into(), loader: "fabric".into(),
            loader_version: Some("0.16.9".into()), java_path: None, ram_mb: 4096, jvm_args: vec![],
            resolution: newest_launcher_core::Resolution::default(),
        };
        let id = core.create_instance(input).unwrap().active_instance_id.unwrap();
        let installer = ModrinthInstaller::new().unwrap();
        installer.install(&core, id.clone(), "AANobbMI".into(), "mod".into()).await.unwrap();
        installer.install(&core, id.clone(), "50dA9Sha".into(), "resourcepack".into()).await.unwrap();
        let snapshot = installer.install(&core, id.clone(), "HVnmMxH1".into(), "shader".into()).await.unwrap();
        let instance = snapshot.instances.iter().find(|item| item.id == id).unwrap();
        assert_eq!(instance.mods.len(), 1);
        assert_eq!(instance.resource_packs.len(), 1);
        assert_eq!(instance.shader_packs.len(), 1);
        for (folder, record) in [
            ("mods", &instance.mods[0]), ("resourcepacks", &instance.resource_packs[0]), ("shaderpacks", &instance.shader_packs[0]),
        ] {
            let path = core.folder_path(folder, Some(&id)).unwrap().join(&record.filename);
            assert!(path.is_file());
            assert!(verify_sha512(&path, &record.sha512).await.unwrap());
        }
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }
}
