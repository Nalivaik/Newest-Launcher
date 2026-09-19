//! Official Forge and NeoForge installer pipelines.
//!
//! This module deliberately delegates installation to the upstream installer JARs. It does
//! not reconstruct installation profiles or invoke their processors itself. The Minecraft
//! runtime prepares an isolated target directory; this module downloads, runs, and verifies
//! the official installer result inside that directory.

use reqwest::Client;
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::{
    cmp::Ordering,
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;
const MAX_INSTALLER_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

const FORGE_METADATA: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
const NEOFORGE_METADATA: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

#[derive(Debug, Clone)]
pub(crate) struct AvailableLoaderVersion {
    pub artifact_version: String,
    pub display_version: String,
}

#[derive(Debug, Clone)]
pub(crate) struct InstalledLoader {
    pub loader: &'static str,
    pub loader_version: String,
    pub version_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct VerifiedLoader {
    pub loader_version: String,
    pub version_id: String,
    pub version_json: PathBuf,
    pub library_paths: Vec<PathBuf>,
}

/// Common contract for upstream-owned loaders. The async methods are intentionally used via
/// their concrete types: the two installers have different metadata URLs and command flags,
/// while sharing the same lifecycle and verification boundary.
pub(crate) trait ModLoaderInstaller {
    fn loader(&self) -> &'static str;
    async fn available_versions(&self, client: &Client, minecraft_version: &str) -> Result<Vec<AvailableLoaderVersion>, String>;
    async fn install(&self, request: &InstallerRequest<'_>) -> Result<InstalledLoader, String>;
    async fn verify(&self, minecraft_root: &Path, minecraft_version: &str, expected_artifact: &str) -> Result<VerifiedLoader, String>;
}

pub(crate) struct InstallerRequest<'a> {
    pub client: &'a Client,
    pub cache_root: &'a Path,
    pub minecraft_root: &'a Path,
    pub java: &'a Path,
    pub minecraft_version: &'a str,
    pub requested_version: Option<&'a str>,
    pub log_path: &'a Path,
    pub progress: &'a (dyn Fn(&str) + Send + Sync),
}

pub(crate) struct ForgeInstaller;
pub(crate) struct NeoForgeInstaller;

impl ModLoaderInstaller for ForgeInstaller {
    fn loader(&self) -> &'static str { "forge" }

    async fn available_versions(&self, client: &Client, minecraft_version: &str) -> Result<Vec<AvailableLoaderVersion>, String> {
        let xml = fetch_metadata(client, FORGE_METADATA).await?;
        let prefix = format!("{minecraft_version}-");
        let mut versions = xml_versions(&xml).into_iter()
            .filter_map(|artifact_version| {
                let display_version = artifact_version.strip_prefix(&prefix)?.to_owned();
                Some(AvailableLoaderVersion { artifact_version, display_version })
            })
            .collect::<Vec<_>>();
        sort_versions(&mut versions);
        if versions.is_empty() { return Err(format!("Официальный Forge Maven не содержит версий для Minecraft {minecraft_version}")); }
        Ok(versions)
    }

    async fn install(&self, request: &InstallerRequest<'_>) -> Result<InstalledLoader, String> {
        install_official(self, request, "--installClient", |artifact| {
            format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{artifact}/forge-{artifact}-installer.jar")
        }).await
    }

    async fn verify(&self, minecraft_root: &Path, minecraft_version: &str, expected_artifact: &str) -> Result<VerifiedLoader, String> {
        verify_official("forge", minecraft_root, minecraft_version, expected_artifact)
    }
}

impl ModLoaderInstaller for NeoForgeInstaller {
    fn loader(&self) -> &'static str { "neoforge" }

    async fn available_versions(&self, client: &Client, minecraft_version: &str) -> Result<Vec<AvailableLoaderVersion>, String> {
        let xml = fetch_metadata(client, NEOFORGE_METADATA).await?;
        // NeoForge's artifact prefix is the Minecraft version without the historical `1.`.
        // Modern Minecraft versions (26.x and later) are already in this form.
        let normalized = minecraft_version.strip_prefix("1.").unwrap_or(minecraft_version);
        let prefix = format!("{normalized}.");
        let mut versions = xml_versions(&xml).into_iter()
            .filter(|artifact_version| artifact_version.starts_with(&prefix))
            .map(|artifact_version| AvailableLoaderVersion { display_version: artifact_version.clone(), artifact_version })
            .collect::<Vec<_>>();
        sort_versions(&mut versions);
        if versions.is_empty() { return Err(format!("Официальный NeoForge Maven не содержит версий для Minecraft {minecraft_version}")); }
        Ok(versions)
    }

    async fn install(&self, request: &InstallerRequest<'_>) -> Result<InstalledLoader, String> {
        install_official(self, request, "--install-client", |artifact| {
            format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{artifact}/neoforge-{artifact}-installer.jar")
        }).await
    }

    async fn verify(&self, minecraft_root: &Path, minecraft_version: &str, expected_artifact: &str) -> Result<VerifiedLoader, String> {
        verify_official("neoforge", minecraft_root, minecraft_version, expected_artifact)
    }
}

pub(crate) async fn install(request: &InstallerRequest<'_>, loader: &str) -> Result<InstalledLoader, String> {
    match loader {
        "forge" => ForgeInstaller.install(request).await,
        "neoforge" => NeoForgeInstaller.install(request).await,
        _ => Err("Неизвестный официальный загрузчик".into()),
    }
}

pub(crate) async fn verify(loader: &str, minecraft_root: &Path, minecraft_version: &str, expected_artifact: &str) -> Result<VerifiedLoader, String> {
    match loader {
        "forge" => ForgeInstaller.verify(minecraft_root, minecraft_version, expected_artifact).await,
        "neoforge" => NeoForgeInstaller.verify(minecraft_root, minecraft_version, expected_artifact).await,
        _ => Err("Неизвестный официальный загрузчик".into()),
    }
}

async fn install_official<I: ModLoaderInstaller>(installer: &I, request: &InstallerRequest<'_>, flag: &str, installer_url: impl Fn(&str) -> String) -> Result<InstalledLoader, String> {
    (request.progress)("Получение версий");
    let available = installer.available_versions(request.client, request.minecraft_version).await?;
    let selected = select_version(installer.loader(), request.minecraft_version, request.requested_version, &available)?;
    let url = installer_url(&selected.artifact_version);
    validate_source_url(&url, installer.loader())?;
    let installer_cache = request.cache_root.join("loader-installers").join(installer.loader());
    ensure_directory(&installer_cache)?;
    let archive = installer_cache.join(format!("{}.jar", safe_component(&selected.artifact_version)?));

    (request.progress)("Скачивание installer");
    append_install_log(request.log_path, &format!("loader={} minecraft={} loader_version={} installer_url={}", installer.loader(), request.minecraft_version, selected.display_version, url))?;
    download_installer(request.client, &url, &archive).await?;

    (request.progress)("Запуск installer");
    append_install_log(request.log_path, &format!("java={} target={}", request.java.display(), request.minecraft_root.display()))?;
    let output = run_installer(request.java, &archive, flag, request.minecraft_root).await?;
    append_install_log(request.log_path, &format!("exit_code={}", output.exit_code))?;
    append_install_output(request.log_path, "stdout", &output.stdout)?;
    append_install_output(request.log_path, "stderr", &output.stderr)?;
    if output.exit_code != 0 {
        return Err(format!("{} installer завершился с кодом {}. Полный stdout/stderr сохранён в {}", installer.loader(), output.exit_code, request.log_path.display()));
    }

    (request.progress)("Установка libraries");
    (request.progress)("Проверка результата");
    let verified = installer.verify(request.minecraft_root, request.minecraft_version, &selected.artifact_version).await?;
    Ok(InstalledLoader {
        loader: installer.loader(), loader_version: verified.loader_version,
        version_id: verified.version_id,
    })
}

fn select_version(loader: &str, minecraft_version: &str, requested: Option<&str>, available: &[AvailableLoaderVersion]) -> Result<AvailableLoaderVersion, String> {
    let Some(requested) = requested.filter(|value| !value.is_empty()) else {
        return available.last().cloned().ok_or_else(|| "Для выбранной версии Minecraft нет совместимого загрузчика".to_owned());
    };
    let forge_artifact = (loader == "forge").then(|| format!("{minecraft_version}-{requested}"));
    available.iter().find(|candidate| candidate.artifact_version == requested || candidate.display_version == requested
        || forge_artifact.as_deref() == Some(candidate.artifact_version.as_str()))
        .cloned()
        .ok_or_else(|| format!("{} {} несовместим с Minecraft {} или отсутствует в официальном Maven", loader, requested, minecraft_version))
}

fn sort_versions(versions: &mut [AvailableLoaderVersion]) {
    versions.sort_by(|left, right| natural_version_compare(&left.artifact_version, &right.artifact_version));
}

fn natural_version_compare(left: &str, right: &str) -> Ordering {
    let left_parts = left.split(|character: char| !character.is_ascii_alphanumeric()).collect::<Vec<_>>();
    let right_parts = right.split(|character: char| !character.is_ascii_alphanumeric()).collect::<Vec<_>>();
    for (left, right) in left_parts.iter().zip(right_parts.iter()) {
        let order = match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            _ => left.cmp(right),
        };
        if order != Ordering::Equal { return order; }
    }
    left_parts.len().cmp(&right_parts.len()).then_with(|| left.cmp(right))
}

async fn fetch_metadata(client: &Client, url: &str) -> Result<String, String> {
    validate_metadata_url(url)?;
    let mut response = client.get(url).send().await.map_err(network_error)?;
    if !response.status().is_success() { return Err(format!("Официальный Maven ответил HTTP {}", response.status())); }
    if response.content_length().is_some_and(|size| size as usize > MAX_METADATA_BYTES) { return Err("Metadata загрузчика превышает безопасный лимит".into()); }
    let mut content = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if content.len().saturating_add(chunk.len()) > MAX_METADATA_BYTES { return Err("Metadata загрузчика превышает безопасный лимит".into()); }
        content.extend_from_slice(&chunk);
    }
    String::from_utf8(content).map_err(|_| "Официальный Maven вернул некорректный XML".into())
}

fn xml_versions(xml: &str) -> Vec<String> {
    let mut versions = Vec::new();
    let mut remaining = xml;
    while let Some(start) = remaining.find("<version>") {
        let value = &remaining[start + "<version>".len()..];
        let Some(end) = value.find("</version>") else { break; };
        let version = value[..end].trim();
        if version.len() <= 100 && !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-+".contains(&byte)) {
            versions.push(version.to_owned());
        }
        remaining = &value[end + "</version>".len()..];
    }
    versions
}

async fn download_installer(client: &Client, url: &str, target: &Path) -> Result<(), String> {
    let checksum = fetch_optional_sha1(client, &format!("{url}.sha1")).await?;
    if let Some(expected) = checksum.as_deref() {
        if target.exists() && verify_sha1(target, expected).await? { return Ok(()); }
    }
    ensure_parent(target)?;
    reject_symlink(target)?;
    let mut response = client.get(url).send().await.map_err(network_error)?;
    if !response.status().is_success() { return Err(format!("Не удалось скачать официальный installer: HTTP {}", response.status())); }
    if response.content_length().is_some_and(|size| size > MAX_INSTALLER_BYTES) { return Err("Installer превышает безопасный лимит размера".into()); }
    let temporary = target.with_extension(format!("part-{}", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&temporary).await.map_err(io_error)?;
    let mut sha1 = Sha1::new();
    let mut received = 0_u64;
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        received = received.checked_add(chunk.len() as u64).ok_or_else(|| "Размер installer переполнен".to_owned())?;
        if received > MAX_INSTALLER_BYTES {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err("Installer превышает безопасный лимит размера".into());
        }
        sha1.update(&chunk);
        file.write_all(&chunk).await.map_err(io_error)?;
    }
    file.flush().await.map_err(io_error)?;
    file.sync_all().await.map_err(io_error)?;
    drop(file);
    if let Some(expected) = checksum {
        if format!("{:x}", sha1.finalize()) != expected {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err("Контрольная сумма официального installer не совпала".into());
        }
    }
    tokio::fs::rename(&temporary, target).await.map_err(io_error)?;
    Ok(())
}

async fn fetch_optional_sha1(client: &Client, url: &str) -> Result<Option<String>, String> {
    validate_checksum_url(url)?;
    let response = client.get(url).send().await.map_err(network_error)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND { return Ok(None); }
    if !response.status().is_success() { return Err(format!("Официальный Maven не отдал checksum installer: HTTP {}", response.status())); }
    let value = response.text().await.map_err(network_error)?;
    let hash = value.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    validate_sha1(&hash)?;
    Ok(Some(hash))
}

async fn verify_sha1(path: &Path, expected: &str) -> Result<bool, String> {
    let path = path.to_owned();
    let expected = expected.to_owned();
    tokio::task::spawn_blocking(move || {
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_INSTALLER_BYTES { return Ok(false); }
        let mut file = fs::File::open(path).map_err(io_error)?;
        let mut sha1 = Sha1::new();
        let mut buffer = [0_u8; 1024 * 128];
        loop {
            let read = std::io::Read::read(&mut file, &mut buffer).map_err(io_error)?;
            if read == 0 { break; }
            sha1.update(&buffer[..read]);
        }
        Ok(format!("{:x}", sha1.finalize()) == expected)
    }).await.map_err(|_| "Проверка installer была прервана")?
}

struct InstallerOutput { exit_code: i32, stdout: Vec<u8>, stderr: Vec<u8> }

async fn run_installer(java: &Path, installer: &Path, flag: &str, target: &Path) -> Result<InstallerOutput, String> {
    ensure_directory(target)?;
    let home = target.join(".newest-installer-home");
    ensure_directory(&home)?;
    let java = console_java(java);
    let mut command = Command::new(&java);
    command.arg(format!("-Duser.home={}", home.display())).arg("-jar").arg(installer).arg(flag).arg(target)
        .current_dir(target).env("HOME", &home).env("USERPROFILE", &home).env("APPDATA", &home)
        .stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| format!("Не удалось запустить официальный installer: {error}"))?;
    let stdout = child.stdout.take().ok_or_else(|| "Installer не предоставил stdout".to_owned())?;
    let stderr = child.stderr.take().ok_or_else(|| "Installer не предоставил stderr".to_owned())?;
    let stdout_task = tokio::spawn(read_capped(stdout));
    let stderr_task = tokio::spawn(read_capped(stderr));
    let status = child.wait().await.map_err(|error| format!("Не удалось дождаться installer: {error}"))?;
    let stdout = stdout_task.await.map_err(|_| "Чтение stdout installer было прервано")?;
    let stderr = stderr_task.await.map_err(|_| "Чтение stderr installer было прервано")?;
    Ok(InstallerOutput { exit_code: status.code().unwrap_or(-1), stdout, stderr })
}

async fn read_capped<R: AsyncRead + Unpin>(mut stream: R) -> Vec<u8> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let Ok(read) = stream.read(&mut buffer).await else { break; };
        if read == 0 { break; }
        let remaining = MAX_INSTALLER_OUTPUT_BYTES.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    output
}

fn console_java(java: &Path) -> PathBuf {
    if cfg!(windows) && java.file_name().is_some_and(|name| name.eq_ignore_ascii_case("javaw.exe")) {
        let console = java.with_file_name("java.exe");
        if console.is_file() { return console; }
    }
    java.to_owned()
}

fn verify_official(loader: &str, minecraft_root: &Path, minecraft_version: &str, expected_artifact: &str) -> Result<VerifiedLoader, String> {
    let versions = minecraft_root.join("versions");
    reject_symlink(&versions)?;
    let mut found = Vec::new();
    for entry in fs::read_dir(&versions).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let directory = entry.path();
        let metadata = fs::symlink_metadata(&directory).map_err(io_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() { continue; }
        for file in fs::read_dir(&directory).map_err(io_error)? {
            let file = file.map_err(io_error)?;
            let path = file.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") { continue; }
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_METADATA_BYTES as u64 { continue; }
            let content = fs::read(&path).map_err(io_error)?;
            let Ok(json) = serde_json::from_slice::<Value>(&content) else { continue; };
            let id = json.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
            let parent = json.get("inheritsFrom").and_then(Value::as_str).unwrap_or_default().to_owned();
            if parent == minecraft_version && loader_matches(loader, &id) && profile_mentions_artifact(loader, &id, expected_artifact) {
                found.push((path, json, id));
            }
        }
    }
    if found.len() != 1 {
        return Err(format!("После установки {loader} не найден ровно один version JSON для Minecraft {minecraft_version}"));
    }
    let (version_json, json, version_id) = found.remove(0);
    if json.get("mainClass").and_then(Value::as_str).filter(|value| !value.is_empty()).is_none() {
        return Err(format!("Созданный {loader} version JSON не содержит mainClass"));
    }
    let libraries = verify_libraries(minecraft_root, &json)?;
    if libraries.is_empty() { return Err(format!("Созданный {loader} version JSON не содержит проверяемых libraries")); }
    let loader_version = loader_version_from_id(loader, &version_id, expected_artifact)?;
    Ok(VerifiedLoader { loader_version, version_id, version_json, library_paths: libraries })
}

fn loader_matches(loader: &str, id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    match loader {
        "forge" => id.contains("forge") && !id.contains("neoforge"),
        "neoforge" => id.contains("neoforge"),
        _ => false,
    }
}

fn profile_mentions_artifact(loader: &str, id: &str, artifact: &str) -> bool {
    let display = if loader == "forge" { artifact.rsplit_once('-').map(|(_, value)| value).unwrap_or(artifact) } else { artifact };
    id.contains(artifact) || id.contains(display)
}

fn loader_version_from_id(loader: &str, id: &str, artifact: &str) -> Result<String, String> {
    let value = match loader {
        "forge" => id.rsplit_once("-forge-").map(|(_, value)| value).unwrap_or_else(|| artifact.rsplit_once('-').map(|(_, value)| value).unwrap_or(artifact)),
        "neoforge" => id.rsplit_once("neoforge-").map(|(_, value)| value).unwrap_or(artifact),
        _ => return Err("Неизвестный официальный загрузчик".into()),
    };
    if value.is_empty() || value.len() > 100 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-+".contains(&byte)) {
        return Err("Installer создал небезопасную версию загрузчика".into());
    }
    Ok(value.to_owned())
}

fn verify_libraries(root: &Path, json: &Value) -> Result<Vec<PathBuf>, String> {
    let libraries = json.get("libraries").and_then(Value::as_array).ok_or_else(|| "Version JSON не содержит libraries".to_owned())?;
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for library in libraries {
        if !library_rules_apply(library) { continue; }
        let name = library.get("name").and_then(Value::as_str).ok_or_else(|| "Version JSON содержит library без Maven имени".to_owned())?;
        let relative = maven_library_path(name)?;
        let path = root.join("libraries").join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
            return Err(format!("Installer не установил library {}", path.display()));
        }
        if seen.insert(path.clone()) { paths.push(path); }
    }
    Ok(paths)
}

fn library_rules_apply(library: &Value) -> bool {
    let Some(rules) = library.get("rules").and_then(Value::as_array) else { return true; };
    let mut allowed = false;
    for rule in rules {
        let Some(rule) = rule.as_object() else { continue; };
        let os_matches = match rule.get("os").and_then(Value::as_object) {
            None => true,
            Some(os) => os.get("name").and_then(Value::as_str).map_or(true, |name| name == minecraft_os())
                && os.get("arch").and_then(Value::as_str).map_or(true, |arch| arch == std::env::consts::ARCH || (arch == "x86" && cfg!(target_pointer_width = "32"))),
        };
        if os_matches { allowed = rule.get("action").and_then(Value::as_str).unwrap_or("allow") == "allow"; }
    }
    allowed
}

fn maven_library_path(name: &str) -> Result<PathBuf, String> {
    let (coordinate, extension) = name.split_once('@').unwrap_or((name, "jar"));
    let parts = coordinate.split(':').collect::<Vec<_>>();
    if !(3..=4).contains(&parts.len()) || parts.iter().any(|part| part.is_empty() || part.len() > 200
        || !part.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte))) {
        return Err("Version JSON содержит небезопасную Maven-координату".into());
    }
    if extension.is_empty() || extension.len() > 12 || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err("Version JSON содержит небезопасное расширение Maven library".into());
    }
    let classifier = if parts.len() == 4 { format!("-{}", parts[3]) } else { String::new() };
    Ok(PathBuf::from(parts[0].replace('.', "/")).join(parts[1]).join(parts[2])
        .join(format!("{}-{}{}.{}", parts[1], parts[2], classifier, extension)))
}

fn validate_metadata_url(input: &str) -> Result<(), String> {
    let url = url::Url::parse(input).map_err(|_| "Некорректный URL metadata загрузчика".to_owned())?;
    let allowed = ["maven.minecraftforge.net", "maven.neoforged.net"];
    if url.scheme() != "https" || !allowed.contains(&url.host_str().unwrap_or_default()) || !url.username().is_empty() || url.password().is_some() {
        return Err("Metadata загрузчика содержит неразрешённый адрес".into());
    }
    Ok(())
}

fn validate_source_url(input: &str, loader: &str) -> Result<(), String> {
    let url = url::Url::parse(input).map_err(|_| "Некорректный URL installer".to_owned())?;
    let host = match loader { "forge" => "maven.minecraftforge.net", "neoforge" => "maven.neoforged.net", _ => return Err("Неизвестный официальный загрузчик".into()) };
    if url.scheme() != "https" || url.host_str() != Some(host) || !url.username().is_empty() || url.password().is_some() {
        return Err("Installer загружается только из официального Maven загрузчика".into());
    }
    Ok(())
}

fn validate_checksum_url(input: &str) -> Result<(), String> {
    let url = url::Url::parse(input).map_err(|_| "Некорректный URL checksum installer".to_owned())?;
    let allowed = ["maven.minecraftforge.net", "maven.neoforged.net"];
    if url.scheme() != "https" || !allowed.contains(&url.host_str().unwrap_or_default()) || !url.username().is_empty() || url.password().is_some() || !url.path().ends_with(".sha1") {
        return Err("Checksum installer содержит неразрешённый адрес".into());
    }
    Ok(())
}

fn validate_sha1(value: &str) -> Result<(), String> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err("Официальный Maven вернул некорректный SHA-1 installer".into()); }
    Ok(())
}

fn safe_component(value: &str) -> Result<&str, String> {
    if value.is_empty() || value.len() > 120 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-+".contains(&byte)) {
        return Err("Некорректный идентификатор installer".into());
    }
    Ok(value)
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    if path.exists() { reject_symlink(path)?; }
    fs::create_dir_all(path).map_err(io_error)
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "Некорректный путь installer".to_owned())?;
    ensure_directory(parent)
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) => Err("Небезопасный путь installer".into()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn append_install_log(path: &Path, line: &str) -> Result<(), String> {
    ensure_parent(path)?;
    reject_symlink(path)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(io_error)?;
    writeln!(file, "[Newest Launcher][loader-install] {line}").map_err(io_error)
}

fn append_install_output(path: &Path, stream: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() { return Ok(()); }
    let text = String::from_utf8_lossy(bytes);
    append_install_log(path, &format!("{stream}:\n{text}"))
}

fn minecraft_os() -> &'static str {
    if cfg!(target_os = "windows") { "windows" } else if cfg!(target_os = "macos") { "osx" } else { "linux" }
}

fn io_error(error: std::io::Error) -> String { format!("Ошибка файловой системы loader installer: {error}") }
fn network_error(error: reqwest::Error) -> String { format!("Официальный loader Maven недоступен: {error}") }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_and_neoforge_versions_are_filtered_and_sorted() {
        let forge = xml_versions("<version>1.20.1-47.4.9</version><version>1.20.1-47.4.10</version><version>1.21-51.0.1</version>");
        let mut forge = forge.into_iter().filter_map(|value| {
            let display_version = value.strip_prefix("1.20.1-")?.to_owned();
            Some(AvailableLoaderVersion { artifact_version: value, display_version })
        }).collect::<Vec<_>>();
        sort_versions(&mut forge);
        assert_eq!(forge.last().unwrap().display_version, "47.4.10");
        assert_eq!(maven_library_path("net.minecraftforge:forge:1.20.1-47.4.10:client").unwrap(), PathBuf::from("net/minecraftforge/forge/1.20.1-47.4.10/forge-1.20.1-47.4.10-client.jar"));
    }

    #[test]
    fn rejects_unofficial_installer_sources() {
        assert!(validate_source_url("https://maven.minecraftforge.net/net/minecraftforge/forge/a.jar", "forge").is_ok());
        assert!(validate_source_url("https://example.invalid/installer.jar", "forge").is_err());
        assert!(validate_checksum_url("https://maven.neoforged.net/releases/a.jar.sha1").is_ok());
    }
}
