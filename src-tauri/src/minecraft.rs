//! Official Minecraft metadata, verified downloads, and the Vanilla launch runtime.
//!
//! This module is deliberately owned by the Tauri backend. The webview receives only an
//! instance UUID and state snapshots; it never supplies a download URL, filesystem path, Java
//! argument list, or authentication token.

use crate::loaders::{self, InstallerRequest};
use newest_launcher_core::{Instance, LauncherCore, Profile, Snapshot};
use flate2::read::GzDecoder;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;

const VERSION_MANIFEST: &str = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ASSETS: usize = 200_000;
const MAX_NATIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_JAVA_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_JAVA_EXTRACTED_BYTES: u64 = 3 * 1024 * 1024 * 1024;
const ADOPTIUM_ASSETS_API: &str = "https://api.adoptium.net/v3/assets/latest";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    pub instance_id: Option<String>,
    pub phase: String,
    pub message: String,
    pub completed_files: u64,
    pub total_files: u64,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub pid: Option<u32>,
    pub last_error: Option<String>,
}

impl Default for GameStatus {
    fn default() -> Self {
        Self {
            instance_id: None, phase: "idle".into(), message: "Готово к установке Minecraft".into(),
            completed_files: 0, total_files: 0, completed_bytes: 0, total_bytes: 0,
            pid: None, last_error: None,
        }
    }
}

struct ActiveGame {
    instance_id: String,
    child: Arc<Mutex<Child>>,
}

#[derive(Default)]
struct Runtime {
    operation: Option<String>,
    active: Option<ActiveGame>,
    status: GameStatus,
}

pub struct MinecraftService {
    client: reqwest::Client,
    runtime: Arc<Mutex<Runtime>>,
}

impl MinecraftService {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("NewestLauncher/", env!("CARGO_PKG_VERSION"), " (desktop; Minecraft installer)"))
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(90))
                .redirect(Policy::none())
                .build()?,
            runtime: Arc::new(Mutex::new(Runtime::default())),
        })
    }

    pub fn status(&self) -> Result<GameStatus, String> {
        Ok(self.runtime.lock().map_err(|_| "Состояние Minecraft недоступно")?.status.clone())
    }

    pub async fn install(&self, core: &LauncherCore, instance_id: String) -> Result<Snapshot, String> {
        let official_loader = core.instance(&instance_id).map_err(core_error)?.loader;
        self.begin(&instance_id, "installing", "Получаем официальный manifest Minecraft")?;
        let outcome = async {
            if is_official_loader(&official_loader) {
                let prepared = self.prepare_official_loader(core, &instance_id).await?;
                core.complete_loader_installation(&instance_id, &official_loader, prepared.loader_version).map_err(core_error)
            } else {
                self.install_inner(core, &instance_id).await?;
                core.set_installation_state(&instance_id, "installed").map_err(core_error)
            }
        }.await;
        match outcome {
            Ok(snapshot) => {
                self.complete(if is_official_loader(&official_loader) { "Готово: официальный loader установлен и проверен" } else { "Minecraft установлен и проверен" });
                Ok(snapshot)
            }
            Err(error) => {
                if is_official_loader(&official_loader) { let _ = core.set_installation_state(&instance_id, "not_installed"); }
                self.fail(&error);
                Err(error)
            }
        }
    }

    pub async fn launch(&self, core: Arc<LauncherCore>, instance_id: String) -> Result<GameStatus, String> {
        self.begin(&instance_id, "installing", "Проверяем файлы Minecraft")?;
        let outcome = async {
            let profile = core.active_profile().map_err(core_error)?
                .ok_or_else(|| "Создайте или выберите Local / Offline профиль перед запуском".to_owned())?;
            if profile.kind != "offline" {
                return Err("Microsoft-профиль нельзя запускать до завершения безопасной авторизации и проверки владения Minecraft".into());
            }
            let loader = core.instance(&instance_id).map_err(core_error)?.loader;
            let plan = if is_official_loader(&loader) {
                let prepared = self.prepare_official_loader(&core, &instance_id).await?;
                if prepared.newly_installed {
                    core.complete_loader_installation(&instance_id, &loader, prepared.loader_version).map_err(core_error)?;
                }
                prepared.plan
            } else {
                let plan = self.install_inner(&core, &instance_id).await?;
                core.set_installation_state(&instance_id, "installed").map_err(core_error)?;
                plan
            };
            self.set_phase("launching", "Подбираем совместимую Java");
            let required_java = plan.version.java_version.as_ref().map(|value| value.major_version).unwrap_or(8);
            let java = self.resolve_or_download_java(&plan.instance, &core.data_directory(), required_java).await?;
            let command = build_command(&plan, &profile, java)?;
            self.spawn_game(core.clone(), command, plan.instance.id.clone(), plan.instance.game_directory.clone())
        }.await;
        match outcome {
            Ok(status) => Ok(status),
            Err(error) => {
                self.fail(&error);
                Err(error)
            }
        }
    }

    pub fn stop(&self) -> Result<GameStatus, String> {
        let (child, instance_id) = {
            let mut runtime = self.runtime.lock().map_err(|_| "Состояние Minecraft недоступно")?;
            let (child, instance_id) = runtime.active.as_ref().map(|active| (active.child.clone(), active.instance_id.clone()))
                .ok_or_else(|| "Minecraft сейчас не запущен".to_owned())?;
            runtime.status.phase = "stopping".into();
            runtime.status.message = "Останавливаем Minecraft…".into();
            runtime.status.last_error = None;
            (child, instance_id)
        };
        child.lock().map_err(|_| "Процесс Minecraft недоступен")?.kill()
            .map_err(|error| format!("Не удалось остановить Minecraft: {error}"))?;
        let mut runtime = self.runtime.lock().map_err(|_| "Состояние Minecraft недоступно")?;
        runtime.status.instance_id = Some(instance_id);
        Ok(runtime.status.clone())
    }

    fn begin(&self, instance_id: &str, phase: &str, message: &str) -> Result<(), String> {
        let mut runtime = self.runtime.lock().map_err(|_| "Состояние Minecraft недоступно")?;
        if runtime.operation.is_some() { return Err("Уже выполняется другая операция Minecraft".into()); }
        if runtime.active.is_some() { return Err("Minecraft уже запущен. Сначала остановите игру.".into()); }
        runtime.operation = Some(instance_id.to_owned());
        runtime.status = GameStatus {
            instance_id: Some(instance_id.to_owned()), phase: phase.into(), message: message.into(),
            completed_files: 0, total_files: 0, completed_bytes: 0, total_bytes: 0, pid: None, last_error: None,
        };
        Ok(())
    }

    fn complete(&self, message: &str) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.operation = None;
            runtime.status.phase = "installed".into();
            runtime.status.message = message.into();
            runtime.status.last_error = None;
        }
    }

    fn fail(&self, error: &str) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.operation = None;
            runtime.status.phase = "failed".into();
            runtime.status.message = "Операция Minecraft не завершена".into();
            runtime.status.last_error = Some(error.to_owned());
            runtime.status.pid = None;
        }
    }

    fn set_phase(&self, phase: &str, message: &str) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.status.phase = phase.into();
            runtime.status.message = message.into();
        }
    }

    fn set_totals(&self, files: u64, bytes: u64) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.status.total_files = files;
            runtime.status.total_bytes = bytes;
        }
    }

    fn increment_progress(&self, bytes: u64) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.status.completed_files = runtime.status.completed_files.saturating_add(1);
            runtime.status.completed_bytes = runtime.status.completed_bytes.saturating_add(bytes);
        }
    }

    async fn install_inner(&self, core: &LauncherCore, instance_id: &str) -> Result<PreparedVersion, String> {
        let instance = core.instance(instance_id).map_err(core_error)?;
        if !["vanilla", "fabric", "quilt"].contains(&instance.loader.as_str()) {
            return Err(format!("{} использует {}. Для него требуется официальный installer pipeline.", instance.name, instance.loader));
        }
        self.prepare_vanilla_inner(core, instance_id, true).await
    }

    /// Prepares only the Mojang base runtime. Forge and NeoForge use this before delegating
    /// the loader part to their official installer JARs in `loaders.rs`.
    async fn prepare_base_vanilla(&self, core: &LauncherCore, instance_id: &str) -> Result<PreparedVersion, String> {
        self.prepare_vanilla_inner(core, instance_id, false).await
    }

    async fn prepare_vanilla_inner(&self, core: &LauncherCore, instance_id: &str, include_lightweight_loader: bool) -> Result<PreparedVersion, String> {
        let instance = core.instance(instance_id).map_err(core_error)?;
        let root = core.data_directory();
        let cache = root.join("cache").join("minecraft");
        ensure_directory(&cache)?;
        let manifest: VersionManifest = self.fetch_json(VERSION_MANIFEST).await?;
        let entry = manifest.versions.into_iter().find(|entry| entry.id == instance.minecraft_version)
            .ok_or_else(|| format!("Версия Minecraft {} отсутствует в официальном manifest", instance.minecraft_version))?;
        let version_path = cache.join("versions").join(safe_component(&entry.id)?).join("version.json");
        self.download_verified(&entry.url, &version_path, &entry.sha1, None).await?;
        let mut version: VersionMeta = read_json(&version_path).await?;
        if version.id != instance.minecraft_version { return Err("Официальный version JSON не соответствует выбранной версии".into()); }
        if version.inherits_from.is_some() { return Err("Эта версия требует наследуемый version JSON, который ещё не поддержан установщиком Vanilla".into()); }
        let loader_libraries = if include_lightweight_loader {
            self.resolve_lightweight_loader(&instance, &mut version, &cache).await?
        } else { vec![] };
        let asset_index_path = cache.join("assets").join("indexes").join(format!("{}.json", safe_component(&version.asset_index.id)?));
        self.download_verified(&version.asset_index.url, &asset_index_path, &version.asset_index.sha1, Some(version.asset_index.size)).await?;
        let asset_index: AssetIndex = read_json(&asset_index_path).await?;
        if asset_index.objects.len() > MAX_ASSETS { return Err("Asset index содержит слишком много файлов".into()); }
        let libraries = select_libraries(&version.libraries, &cache)?;
        let client_path = cache.join("versions").join(safe_component(&version.id)?).join("client.jar");
        let mut downloads = Vec::new();
        downloads.push(PlannedDownload { target: client_path.clone(), source: version.downloads.client.clone() });
        let mut logging_argument = None;
        let mut logging_path = None;
        if let Some(logging) = &version.logging.as_ref().and_then(|logging| logging.client.as_ref()) {
            let logging_id = logging.file.id.as_deref().ok_or_else(|| "В logging configuration отсутствует ID файла".to_owned())?;
            let target = cache.join("assets").join("log_configs").join(safe_component(logging_id)?);
            logging_argument = Some(logging.argument.clone());
            logging_path = Some(target.clone());
            downloads.push(PlannedDownload { target, source: logging.file.clone() });
        }
        for library in &libraries.classpath { downloads.push(PlannedDownload { target: library.path.clone(), source: library.download.clone() }); }
        for library in &libraries.natives { downloads.push(PlannedDownload { target: library.path.clone(), source: library.download.clone() }); }
        for library in &loader_libraries { downloads.push(PlannedDownload { target: library.path.clone(), source: library.download.clone() }); }
        for asset in asset_index.objects.values() {
            validate_sha1(&asset.hash)?;
            let prefix = asset.hash.get(..2).ok_or_else(|| "Некорректный hash asset".to_owned())?;
            let target = cache.join("assets").join("objects").join(prefix).join(&asset.hash);
            downloads.push(PlannedDownload {
                target,
                source: Download { url: format!("https://resources.download.minecraft.net/{prefix}/{}", asset.hash), sha1: asset.hash.clone(), size: asset.size, path: None, id: None },
            });
        }
        let total_files = downloads.len() as u64;
        let total_bytes = downloads.iter().fold(0_u64, |total, item| total.saturating_add(item.source.size));
        self.set_phase("installing", "Скачиваем и проверяем файлы Minecraft");
        self.set_totals(total_files, total_bytes);
        for item in downloads {
            let expected_size = (item.source.size != 0).then_some(item.source.size);
            let size = self.download_verified(&item.source.url, &item.target, &item.source.sha1, expected_size).await?;
            self.increment_progress(size);
        }
        let native_directory = PathBuf::from(&instance.game_directory).join(".newest").join("natives").join(safe_component(&version.id)?);
        extract_natives(libraries.natives.iter().map(|item| item.path.clone()).collect(), native_directory.clone()).await?;
        let assets_root = cache.join("assets");
        let library_directory = cache.join("libraries");
        Ok(PreparedVersion {
            instance, version, cache, client_path, classpath: libraries.classpath.into_iter().map(|item| item.path)
                .chain(loader_libraries.into_iter().map(|item| item.path)).collect(), native_directory,
            assets_root, library_directory, logging_argument, logging_path,
        })
    }

    /// Fabric and Quilt publish launcher profiles which inherit from Mojang's version JSON.
    /// Their libraries are downloaded into the same verified shared cache as Vanilla libraries.
    async fn resolve_lightweight_loader(&self, instance: &Instance, version: &mut VersionMeta, cache: &Path) -> Result<Vec<CachedLibrary>, String> {
        if instance.loader == "vanilla" { return Ok(vec![]); }
        let loader_version = match &instance.loader_version {
            Some(version) => version.clone(),
            None => self.latest_loader_version(&instance.loader, &instance.minecraft_version).await?,
        };
        let profile_url = match instance.loader.as_str() {
            "fabric" => format!("https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json", instance.minecraft_version, loader_version),
            "quilt" => format!("https://meta.quiltmc.org/v3/versions/loader/{}/{}/profile/json", instance.minecraft_version, loader_version),
            _ => return Ok(vec![]),
        };
        let profile: LoaderProfile = self.fetch_json(&profile_url).await?;
        if profile.inherits_from.as_deref() != Some(instance.minecraft_version.as_str()) {
            return Err("Профиль загрузчика не соответствует выбранной версии Minecraft".into());
        }
        if profile.main_class.is_empty() { return Err("Официальный профиль загрузчика не содержит main class".into()); }
        version.main_class = profile.main_class;
        version.arguments = merge_arguments(&version.arguments, &profile.arguments);
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        for library in profile.libraries {
            let (path, url) = loader_maven_location(cache, &library)?;
            if !seen.insert(path.clone()) { continue; }
            let sha1 = match library.sha1 {
                Some(hash) => hash,
                None => self.fetch_sha1_sidecar(&url).await?,
            };
            result.push(CachedLibrary { path, download: Download { url, sha1, size: library.size.unwrap_or(0), path: None, id: None } });
        }
        Ok(result)
    }

    async fn latest_loader_version(&self, loader: &str, minecraft_version: &str) -> Result<String, String> {
        let url = match loader {
            "fabric" => format!("https://meta.fabricmc.net/v2/versions/loader/{minecraft_version}"),
            "quilt" => format!("https://meta.quiltmc.org/v3/versions/loader/{minecraft_version}"),
            _ => return Err("Неизвестный загрузчик".into()),
        };
        let versions: Vec<LoaderListing> = self.fetch_json(&url).await?;
        versions.iter().find(|item| item.loader.stable.unwrap_or(true)).or_else(|| versions.first())
            .map(|item| item.loader.version.clone()).ok_or_else(|| "Для этой версии Minecraft нет совместимого загрузчика".into())
    }

    async fn fetch_sha1_sidecar(&self, url: &str) -> Result<String, String> {
        let checksum_url = format!("{url}.sha1");
        validate_download_url(&checksum_url)?;
        let response = self.client.get(&checksum_url).send().await.map_err(network_error)?;
        if !response.status().is_success() { return Err("Репозиторий загрузчика не предоставил SHA-1 для библиотеки".into()); }
        let text = response.text().await.map_err(network_error)?;
        let hash = text.split_whitespace().next().ok_or_else(|| "Репозиторий загрузчика вернул пустую SHA-1 сумму".to_owned())?.to_ascii_lowercase();
        validate_sha1(&hash)?;
        Ok(hash)
    }

    async fn fetch_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, String> {
        validate_download_url(url)?;
        let mut response = self.client.get(url).send().await.map_err(network_error)?;
        if !response.status().is_success() { return Err(format!("Официальный Minecraft API ответил HTTP {}", response.status())); }
        let content_length = response.content_length().unwrap_or(0);
        if content_length as usize > MAX_JSON_BYTES { return Err("Ответ Minecraft API слишком большой".into()); }
        let mut body = Vec::with_capacity(content_length as usize);
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if body.len().saturating_add(chunk.len()) > MAX_JSON_BYTES { return Err("Ответ Minecraft API слишком большой".into()); }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| "Minecraft API вернул некорректный JSON".into())
    }

    async fn download_verified(&self, url: &str, target: &Path, sha1: &str, expected_size: Option<u64>) -> Result<u64, String> {
        validate_download_url(url)?;
        validate_sha1(sha1)?;
        if let Some(size) = expected_size {
            if size > MAX_DOWNLOAD_BYTES { return Err("Файл Minecraft превышает безопасный лимит размера".into()); }
            if target.exists() && verify_file(target, sha1, Some(size)).await? { return Ok(size); }
        } else if target.exists() && verify_file(target, sha1, None).await? {
            return Ok(fs::metadata(target).map_err(io_error)?.len());
        }
        ensure_parent(target)?;
        reject_symlink(target)?;
        let mut response = self.client.get(url).send().await.map_err(network_error)?;
        if !response.status().is_success() { return Err(format!("Не удалось скачать файл Minecraft: HTTP {}", response.status())); }
        if let (Some(expected), Some(length)) = (expected_size, response.content_length()) {
            if length != expected { return Err("Размер файла Minecraft не совпадает с официальными метаданными".into()); }
        }
        if response.content_length().is_some_and(|size| size > MAX_DOWNLOAD_BYTES) { return Err("Файл Minecraft превышает безопасный лимит размера".into()); }
        let temporary = target.with_extension(format!("part-{}", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::File::create(&temporary).await.map_err(io_error)?;
        let mut hash = Sha1::new();
        let mut received = 0_u64;
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            received = received.checked_add(chunk.len() as u64).ok_or_else(|| "Размер файла Minecraft переполнен".to_owned())?;
            if received > expected_size.unwrap_or(MAX_DOWNLOAD_BYTES) || received > MAX_DOWNLOAD_BYTES {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err("Файл Minecraft превышает безопасный лимит размера".into());
            }
            hash.update(&chunk);
            file.write_all(&chunk).await.map_err(io_error)?;
        }
        file.flush().await.map_err(io_error)?;
        file.sync_all().await.map_err(io_error)?;
        drop(file);
        let actual = format!("{:x}", hash.finalize());
        if actual != sha1 || expected_size.is_some_and(|size| size != received) {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err("Контрольная сумма скачанного файла Minecraft не совпала".into());
        }
        tokio::fs::rename(&temporary, target).await.map_err(io_error)?;
        Ok(received)
    }

    /// Runs only the officially published Forge/NeoForge installer in a new instance-local
    /// runtime directory. The existing Mojang preparation remains the source of Vanilla data;
    /// this path never writes to the user's global `.minecraft`.
    async fn prepare_official_loader(&self, core: &LauncherCore, instance_id: &str) -> Result<OfficialPrepared, String> {
        let instance = core.instance(instance_id).map_err(core_error)?;
        if !is_official_loader(&instance.loader) { return Err("Запрошен неофициальный loader pipeline".into()); }
        let root = core.data_directory();
        let runtime = isolated_runtime_path(&root, &instance.id)?;
        let expected_artifact = instance.loader_version.as_deref().map(|version| official_artifact_version(&instance.loader, &instance.minecraft_version, version));

        // A persisted runtime is accepted only after verification against its generated JSON
        // and every library named in that profile. Otherwise it is rebuilt in a fresh stage.
        if let Some(expected) = expected_artifact.as_deref() {
            self.set_phase("installing", "Проверка результата");
            if let Ok(verified) = loaders::verify(&instance.loader, &runtime, &instance.minecraft_version, expected).await {
                self.set_phase("installing", "Подготовка Vanilla");
                let base = self.prepare_base_vanilla(core, instance_id).await?;
                let loader_version = verified.loader_version.clone();
                let plan = prepare_official_launch_plan(base, &runtime, verified).await?;
                return Ok(OfficialPrepared { plan, loader_version, newly_installed: false });
            }
        }

        self.set_phase("installing", "Подготовка Vanilla");
        let base = self.prepare_base_vanilla(core, instance_id).await?;
        self.set_phase("installing", "Проверка Java");
        let required_java = base.version.java_version.as_ref().map(|value| value.major_version).unwrap_or(8);
        let java = self.resolve_or_download_java(&base.instance, &root, required_java).await?;

        let stage_parent = root.join(".staging").join(format!("official-loader-{}", uuid::Uuid::new_v4()));
        let stage_runtime = stage_parent.join("minecraft");
        ensure_directory(&stage_runtime)?;
        let log_path = PathBuf::from(&base.instance.game_directory).join("logs").join("newest-loader-install.log");
        let progress = |message: &str| self.set_phase("installing", message);
        let installation = async {
            materialize_vanilla_for_official_installer(&base, &stage_runtime)?;
            let request = InstallerRequest {
                client: &self.client, cache_root: &root, minecraft_root: &stage_runtime, java: &java,
                minecraft_version: &base.instance.minecraft_version, requested_version: base.instance.loader_version.as_deref(),
                log_path: &log_path, progress: &progress,
            };
            let installed = loaders::install(&request, &base.instance.loader).await?;
            if installed.loader != base.instance.loader { return Err("Installer вернул несовпадающий loader".into()); }
            commit_isolated_runtime(&runtime, &stage_runtime, &root.join(".staging"))?;
            let artifact = official_artifact_version(&base.instance.loader, &base.instance.minecraft_version, &installed.loader_version);
            let verified = loaders::verify(&base.instance.loader, &runtime, &base.instance.minecraft_version, &artifact).await?;
            if verified.version_id != installed.version_id { return Err("Version JSON после переноса installer runtime не совпадает с проверенным результатом".into()); }
            let plan = prepare_official_launch_plan(base, &runtime, verified).await?;
            Ok::<OfficialPrepared, String>(OfficialPrepared { plan, loader_version: installed.loader_version, newly_installed: true })
        }.await;
        let _ = fs::remove_dir_all(&stage_parent);
        installation
    }

    /// Use a local JVM when possible. If the computer has no compatible JVM, install a
    /// checksum-verified Temurin JRE in the launcher cache. The archive URL and SHA-256
    /// are supplied together by Adoptium's official release API.
    async fn resolve_or_download_java(&self, instance: &Instance, root: &Path, required: u32) -> Result<PathBuf, String> {
        if let Some(java) = find_java(instance, root, required) { return Ok(java); }
        if !(8..=99).contains(&required) {
            return Err(format!("Minecraft запросил неподдерживаемую версию Java {required}"));
        }
        self.set_phase("launching", &format!("Скачиваем Temurin Java {required}"));
        let java = self.download_java_runtime(root, required).await?;
        if java_major_version(&java).is_some_and(|actual| actual >= required) { return Ok(java); }
        Err(format!("Скачанная Java не соответствует требованию Minecraft: нужна Java {required}+"))
    }

    async fn download_java_runtime(&self, root: &Path, required: u32) -> Result<PathBuf, String> {
        let platform = AdoptiumPlatform::current()?;
        let runtime = java_runtime_path(root, required, platform);
        if let Some(java) = existing_java(&runtime, required) { return Ok(java); }

        let api_url = format!(
            "{ADOPTIUM_ASSETS_API}/{required}/hotspot?architecture={}&image_type=jre&os={}&vendor=eclipse",
            platform.architecture, platform.os,
        );
        let releases: Vec<AdoptiumRelease> = self.fetch_adoptium_json(&api_url).await?;
        let release = releases.into_iter().next()
            .ok_or_else(|| format!("Adoptium пока не выпустил Temurin Java {required} для {} {}", platform.os, platform.architecture))?;
        validate_sha256(&release.binary.package.checksum)?;
        if release.binary.package.size == 0 || release.binary.package.size > MAX_JAVA_ARCHIVE_BYTES {
            return Err("Adoptium вернул Java-архив недопустимого размера".into());
        }
        let archive_kind = JavaArchiveKind::from_name(&release.binary.package.name)?;
        let downloads = root.join("cache").join("java").join("downloads");
        ensure_directory(&downloads)?;
        let archive = downloads.join(format!("{}.{}", release.binary.package.checksum.to_ascii_lowercase(), archive_kind.extension()));
        self.download_java_verified(
            &release.binary.package.link,
            &archive,
            &release.binary.package.checksum,
            release.binary.package.size,
        ).await?;

        self.set_phase("launching", "Устанавливаем скачанную Java");
        let stage = root.join("cache").join("java").join("staging").join(uuid::Uuid::new_v4().to_string());
        ensure_directory(&stage)?;
        let result = extract_java_archive(&archive, &stage, archive_kind).await.and_then(|_| {
            let payload = find_java_payload(&stage)?;
            let candidate = payload.join("bin").join(java_binary());
            if java_major_version(&candidate).is_none_or(|actual| actual < required) {
                return Err("Распакованный Java runtime не прошёл проверку версии".into());
            }
            if runtime.exists() { remove_runtime_directory(&runtime)?; }
            ensure_parent(&runtime)?;
            fs::rename(&payload, &runtime).map_err(io_error)?;
            Ok(runtime.join("bin").join(java_binary()))
        });
        let _ = fs::remove_dir_all(&stage);
        result
    }

    async fn fetch_adoptium_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, String> {
        validate_adoptium_api_url(url)?;
        let mut response = self.client.get(url).send().await.map_err(java_network_error)?;
        if !response.status().is_success() { return Err(format!("Adoptium API ответил HTTP {}", response.status())); }
        let length = response.content_length().unwrap_or(0);
        if length as usize > MAX_JSON_BYTES { return Err("Ответ Adoptium API слишком большой".into()); }
        let mut body = Vec::with_capacity(length as usize);
        while let Some(chunk) = response.chunk().await.map_err(java_network_error)? {
            if body.len().saturating_add(chunk.len()) > MAX_JSON_BYTES { return Err("Ответ Adoptium API слишком большой".into()); }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| "Adoptium API вернул некорректный JSON".into())
    }

    async fn download_java_verified(&self, url: &str, target: &Path, expected_sha256: &str, expected_size: u64) -> Result<(), String> {
        validate_adoptium_package_url(url)?;
        validate_sha256(expected_sha256)?;
        if expected_size == 0 || expected_size > MAX_JAVA_ARCHIVE_BYTES { return Err("Java-архив превышает безопасный лимит размера".into()); }
        if target.exists() && verify_sha256_file(target, expected_sha256, Some(expected_size)).await? { return Ok(()); }
        ensure_parent(target)?;
        reject_symlink(target)?;
        let client = reqwest::Client::builder()
            .user_agent(concat!("NewestLauncher/", env!("CARGO_PKG_VERSION"), " (desktop; Java manager)"))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(180))
            .redirect(Policy::limited(5))
            .build().map_err(java_network_error)?;
        let mut response = client.get(url).send().await.map_err(java_network_error)?;
        validate_adoptium_package_url(response.url().as_str())?;
        if !response.status().is_success() { return Err(format!("Не удалось скачать Java: HTTP {}", response.status())); }
        if response.content_length().is_some_and(|size| size != expected_size) {
            return Err("Размер Java-архива не совпадает с данными Adoptium".into());
        }
        let temporary = target.with_extension(format!("part-{}", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::File::create(&temporary).await.map_err(io_error)?;
        let mut hash = Sha256::new();
        let mut received = 0_u64;
        while let Some(chunk) = response.chunk().await.map_err(java_network_error)? {
            received = received.checked_add(chunk.len() as u64).ok_or_else(|| "Размер Java-архива переполнен".to_owned())?;
            if received > expected_size || received > MAX_JAVA_ARCHIVE_BYTES {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err("Java-архив превышает безопасный лимит размера".into());
            }
            hash.update(&chunk);
            file.write_all(&chunk).await.map_err(io_error)?;
        }
        file.flush().await.map_err(io_error)?;
        file.sync_all().await.map_err(io_error)?;
        drop(file);
        if received != expected_size || format!("{:x}", hash.finalize()) != expected_sha256.to_ascii_lowercase() {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err("Контрольная сумма скачанной Java не совпала".into());
        }
        tokio::fs::rename(&temporary, target).await.map_err(io_error)?;
        Ok(())
    }

    fn spawn_game(&self, core: Arc<LauncherCore>, mut command: Command, instance_id: String, game_directory: String) -> Result<GameStatus, String> {
        configure_game_process(&mut command);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| format!("Не удалось запустить Java: {error}"))?;
        let pid = child.id();
        let log_path = PathBuf::from(&game_directory).join("logs").join("newest-launcher.log");
        if let Some(output) = child.stdout.take() { relay_output(output, log_path.clone(), "stdout"); }
        if let Some(output) = child.stderr.take() { relay_output(output, log_path.clone(), "stderr"); }
        let child = Arc::new(Mutex::new(child));
        let started = Instant::now();
        {
            let mut runtime = self.runtime.lock().map_err(|_| "Состояние Minecraft недоступно")?;
            runtime.operation = None;
            runtime.active = Some(ActiveGame { instance_id: instance_id.clone(), child: child.clone() });
            runtime.status.phase = "running".into();
            runtime.status.message = "Minecraft запущен".into();
            runtime.status.pid = Some(pid);
            runtime.status.last_error = None;
        }
        let runtime = self.runtime.clone();
        thread::spawn(move || {
            let exit = loop {
                let result = child.lock().ok().and_then(|mut child| child.try_wait().ok()).flatten();
                if let Some(result) = result { break result; }
                thread::sleep(Duration::from_millis(250));
            };
            let elapsed = started.elapsed().as_secs();
            let _ = core.record_game_exit(&instance_id, elapsed);
            let code = exit.code();
            let failed = !exit.success();
            let error = failed.then(|| format!("Minecraft завершился с ошибкой (код {:?}). Подробности: {}", code, log_path.display()));
            let _ = append_game_log(&log_path, &format!("[Newest Launcher] Minecraft exited with code {:?} after {}s", code, elapsed));
            if let Ok(mut current) = runtime.lock() {
                if current.active.as_ref().is_some_and(|active| active.instance_id == instance_id) {
                    current.active = None;
                    current.status = GameStatus {
                        instance_id: Some(instance_id), phase: if failed { "failed".into() } else { "idle".into() },
                        message: if failed { "Minecraft завершился с ошибкой".into() } else { format!("Minecraft завершился (код {:?})", code) },
                        completed_files: 0, total_files: 0, completed_bytes: 0, total_bytes: 0, pid: None, last_error: error,
                    };
                }
            }
        });
        self.status()
    }
}

#[derive(Deserialize)]
struct VersionManifest { versions: Vec<ManifestEntry> }

#[derive(Deserialize)]
struct ManifestEntry { id: String, url: String, sha1: String }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionMeta {
    id: String,
    #[serde(default)]
    inherits_from: Option<String>,
    main_class: String,
    downloads: VersionDownloads,
    asset_index: AssetIndexReference,
    #[serde(default)]
    libraries: Vec<Library>,
    #[serde(default)]
    arguments: Value,
    #[serde(default)]
    minecraft_arguments: Option<String>,
    #[serde(default)]
    java_version: Option<JavaVersion>,
    #[serde(default)]
    logging: Option<Logging>,
    #[serde(default)]
    r#type: String,
}

#[derive(Deserialize)]
struct VersionDownloads { client: Download }

#[derive(Clone, Deserialize)]
struct Download {
    url: String,
    sha1: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetIndexReference { id: String, url: String, sha1: String, size: u64 }

#[derive(Deserialize)]
struct AssetIndex { objects: HashMap<String, Asset> }

#[derive(Deserialize)]
struct Asset { hash: String, size: u64 }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JavaVersion { major_version: u32 }

#[derive(Deserialize)]
struct Logging { client: Option<LoggingClient> }

#[derive(Deserialize)]
struct LoggingClient { argument: String, file: Download }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoaderProfile {
    inherits_from: Option<String>,
    main_class: String,
    #[serde(default)]
    arguments: Value,
    #[serde(default)]
    libraries: Vec<LoaderMavenLibrary>,
}

#[derive(Deserialize)]
struct LoaderMavenLibrary {
    name: String,
    url: String,
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Deserialize)]
struct LoaderListing { loader: LoaderListingVersion }

#[derive(Deserialize)]
struct LoaderListingVersion {
    version: String,
    #[serde(default)]
    stable: Option<bool>,
}

#[derive(Deserialize)]
struct AdoptiumRelease { binary: AdoptiumBinary }

#[derive(Deserialize)]
struct AdoptiumBinary { package: AdoptiumPackage }

#[derive(Deserialize)]
struct AdoptiumPackage {
    checksum: String,
    link: String,
    name: String,
    size: u64,
}

#[derive(Clone, Copy)]
struct AdoptiumPlatform {
    os: &'static str,
    architecture: &'static str,
}

impl AdoptiumPlatform {
    fn current() -> Result<Self, String> {
        let os = if cfg!(target_os = "windows") { "windows" } else if cfg!(target_os = "macos") { "mac" } else if cfg!(target_os = "linux") { "linux" } else {
            return Err("Автоматическая установка Java не поддерживается для этой ОС".into());
        };
        let architecture = if cfg!(target_arch = "x86_64") { "x64" } else if cfg!(target_arch = "aarch64") { "aarch64" } else if cfg!(target_arch = "x86") { "x32" } else if cfg!(target_arch = "arm") { "arm" } else {
            return Err("Автоматическая установка Java не поддерживается для этой архитектуры".into());
        };
        Ok(Self { os, architecture })
    }
}

#[derive(Clone, Copy)]
enum JavaArchiveKind { TarGz, Zip }

impl JavaArchiveKind {
    fn from_name(name: &str) -> Result<Self, String> {
        if name.ends_with(".tar.gz") { Ok(Self::TarGz) }
        else if name.ends_with(".zip") { Ok(Self::Zip) }
        else { Err("Adoptium вернул неподдерживаемый формат Java-архива".into()) }
    }

    fn extension(self) -> &'static str {
        match self { Self::TarGz => "tar.gz", Self::Zip => "zip" }
    }
}

#[derive(Deserialize)]
struct Library {
    #[serde(default)]
    downloads: LibraryDownloads,
    #[serde(default)]
    rules: Vec<Value>,
    #[serde(default)]
    natives: HashMap<String, String>,
}

#[derive(Default, Deserialize)]
struct LibraryDownloads {
    artifact: Option<Download>,
    #[serde(default)]
    classifiers: HashMap<String, Download>,
}

struct CachedLibrary { path: PathBuf, download: Download }
struct Libraries { classpath: Vec<CachedLibrary>, natives: Vec<CachedLibrary> }
struct PlannedDownload { target: PathBuf, source: Download }

struct PreparedVersion {
    instance: Instance,
    version: VersionMeta,
    cache: PathBuf,
    assets_root: PathBuf,
    library_directory: PathBuf,
    client_path: PathBuf,
    classpath: Vec<PathBuf>,
    native_directory: PathBuf,
    logging_argument: Option<String>,
    logging_path: Option<PathBuf>,
}

struct OfficialPrepared {
    plan: PreparedVersion,
    loader_version: String,
    newly_installed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfficialLoaderVersionJson {
    id: String,
    inherits_from: String,
    main_class: String,
    #[serde(default)]
    arguments: Value,
    #[serde(default)]
    r#type: String,
}

fn is_official_loader(loader: &str) -> bool { matches!(loader, "forge" | "neoforge") }

fn isolated_runtime_path(root: &Path, instance_id: &str) -> Result<PathBuf, String> {
    let id = safe_component(instance_id)?;
    let path = root.join("instances").join(id).join("runtime");
    if path.exists() { reject_symlink(&path)?; }
    Ok(path)
}

fn official_artifact_version(loader: &str, minecraft_version: &str, loader_version: &str) -> String {
    if loader == "forge" && !loader_version.starts_with(&format!("{minecraft_version}-")) {
        format!("{minecraft_version}-{loader_version}")
    } else { loader_version.to_owned() }
}

fn materialize_vanilla_for_official_installer(base: &PreparedVersion, destination: &Path) -> Result<(), String> {
    let minecraft_version = safe_component(&base.version.id)?;
    // Forge's documented unattended client action checks for this standard launcher file
    // before doing any work. The dedicated runtime is a launcher-compatible Minecraft root,
    // so an empty profile registry is the minimal valid state and remains isolated here.
    let launcher_profiles = destination.join("launcher_profiles.json");
    if launcher_profiles.exists() { return Err("Staging runtime уже содержит launcher profile".into()); }
    fs::write(&launcher_profiles, b"{\"profiles\":{}}\n").map_err(io_error)?;
    let versions = destination.join("versions").join(minecraft_version);
    ensure_directory(&versions)?;
    let version_json = base.cache.join("versions").join(minecraft_version).join("version.json");
    copy_isolated_file(&version_json, &versions.join(format!("{minecraft_version}.json")))?;
    copy_isolated_file(&base.client_path, &versions.join(format!("{minecraft_version}.jar")))?;
    for library in &base.classpath {
        let relative = library.strip_prefix(&base.library_directory)
            .map_err(|_| "Vanilla runtime содержит library вне проверенного кэша".to_owned())?;
        let relative = safe_relative(&relative.to_string_lossy())?;
        copy_isolated_file(library, &destination.join("libraries").join(relative))?;
    }
    Ok(())
}

fn copy_isolated_file(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() { return Err("Vanilla cache содержит небезопасный файл".into()); }
    ensure_parent(destination)?;
    if destination.exists() { return Err("Staging runtime уже содержит файл".into()); }
    fs::copy(source, destination).map_err(io_error)?;
    Ok(())
}

fn commit_isolated_runtime(target: &Path, staged: &Path, staging_root: &Path) -> Result<(), String> {
    reject_symlink(staged)?;
    let metadata = fs::symlink_metadata(staged).map_err(io_error)?;
    if !metadata.is_dir() { return Err("Staging runtime отсутствует".into()); }
    ensure_parent(target)?;
    let backup = staging_root.join(format!("official-loader-backup-{}", uuid::Uuid::new_v4()));
    let had_existing = target.exists();
    if had_existing {
        reject_symlink(target)?;
        fs::rename(target, &backup).map_err(io_error)?;
    }
    if let Err(error) = fs::rename(staged, target) {
        if had_existing { let _ = fs::rename(&backup, target); }
        return Err(io_error(error));
    }
    if had_existing { fs::remove_dir_all(&backup).map_err(io_error)?; }
    Ok(())
}

async fn prepare_official_launch_plan(mut base: PreparedVersion, runtime: &Path, verified: loaders::VerifiedLoader) -> Result<PreparedVersion, String> {
    let loader_json: OfficialLoaderVersionJson = read_json(&verified.version_json).await?;
    if loader_json.id != verified.version_id || loader_json.inherits_from != base.instance.minecraft_version || loader_json.main_class.is_empty() {
        return Err("Проверенный version JSON loader имеет несогласованные metadata".into());
    }
    let minecraft_version = safe_component(&base.version.id)?.to_owned();
    let client_path = runtime.join("versions").join(&minecraft_version).join(format!("{minecraft_version}.jar"));
    let client_metadata = fs::symlink_metadata(&client_path).map_err(io_error)?;
    if client_metadata.file_type().is_symlink() || !client_metadata.is_file() || client_metadata.len() == 0 {
        return Err("Isolated runtime не содержит подготовленный Vanilla client JAR".into());
    }
    let library_directory = runtime.join("libraries");
    let mut classpath = Vec::new();
    let mut seen = HashSet::new();
    for source in &base.classpath {
        let relative = source.strip_prefix(&base.library_directory)
            .map_err(|_| "Vanilla runtime содержит library вне проверенного кэша".to_owned())?;
        let target = library_directory.join(relative);
        let metadata = fs::symlink_metadata(&target).map_err(io_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 { return Err("Isolated runtime не содержит Vanilla library".into()); }
        if seen.insert(target.clone()) { classpath.push(target); }
    }
    for library in verified.library_paths {
        let metadata = fs::symlink_metadata(&library).map_err(io_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 { return Err("Installer создал небезопасную library".into()); }
        if seen.insert(library.clone()) { classpath.push(library); }
    }
    base.version.id = loader_json.id;
    base.version.main_class = loader_json.main_class;
    base.version.arguments = merge_arguments(&base.version.arguments, &loader_json.arguments);
    if !loader_json.r#type.is_empty() { base.version.r#type = loader_json.r#type; }
    base.client_path = client_path;
    base.classpath = classpath;
    base.library_directory = library_directory;
    Ok(base)
}

fn merge_arguments(base: &Value, loader: &Value) -> Value {
    let mut merged = base.as_object().cloned().unwrap_or_default();
    for group in ["jvm", "game"] {
        let mut values = base.get(group).and_then(Value::as_array).cloned().unwrap_or_default();
        values.extend(loader.get(group).and_then(Value::as_array).cloned().unwrap_or_default());
        if !values.is_empty() { merged.insert(group.into(), Value::Array(values)); }
    }
    Value::Object(merged)
}

fn loader_maven_location(cache: &Path, library: &LoaderMavenLibrary) -> Result<(PathBuf, String), String> {
    let parts = library.name.split(':').collect::<Vec<_>>();
    if !(3..=4).contains(&parts.len()) || parts.iter().any(|part| part.is_empty() || part.len() > 200) {
        return Err("Профиль загрузчика содержит неподдерживаемую Maven-координату".into());
    }
    if !parts.iter().all(|part| part.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte))) {
        return Err("Профиль загрузчика содержит небезопасную Maven-координату".into());
    }
    let group = parts[0].replace('.', "/");
    let classifier = if parts.len() == 4 { format!("-{}", parts[3]) } else { String::new() };
    let relative = format!("{group}/{}/{}/{}-{}{}.jar", parts[1], parts[2], parts[1], parts[2], classifier);
    let relative_path = safe_relative(&relative)?;
    let base = url::Url::parse(&library.url).map_err(|_| "Профиль загрузчика содержит некорректный Maven URL".to_owned())?;
    let url = base.join(&relative).map_err(|_| "Не удалось собрать URL Maven-библиотеки".to_owned())?.to_string();
    validate_download_url(&url)?;
    Ok((cache.join("libraries").join(relative_path), url))
}

fn select_libraries(libraries: &[Library], cache: &Path) -> Result<Libraries, String> {
    let mut classpath = Vec::new();
    let mut natives = Vec::new();
    let mut paths = HashSet::new();
    for library in libraries {
        if !rules_apply(&library.rules) { continue; }
        if let Some(download) = &library.downloads.artifact {
            let path = library_path(cache, download)?;
            if paths.insert(path.clone()) { classpath.push(CachedLibrary { path, download: download.clone() }); }
        }
        if let Some(template) = library.natives.get(current_os()) {
            let classifier = template.replace("${arch}", current_arch());
            if let Some(download) = library.downloads.classifiers.get(&classifier) {
                let path = library_path(cache, download)?;
                if paths.insert(path.clone()) { natives.push(CachedLibrary { path, download: download.clone() }); }
            } else {
                return Err("Официальные метаданные библиотеки не содержат native для этой архитектуры".into());
            }
        }
    }
    Ok(Libraries { classpath, natives })
}

fn library_path(cache: &Path, download: &Download) -> Result<PathBuf, String> {
    let relative = download.path.as_deref().ok_or_else(|| "В библиотеке Minecraft отсутствует путь файла".to_owned())?;
    safe_relative(relative).map(|relative| cache.join("libraries").join(relative))
}

fn build_command(plan: &PreparedVersion, profile: &Profile, java: PathBuf) -> Result<Command, String> {
    let separator = if cfg!(windows) { ";" } else { ":" };
    let mut classpath = plan.classpath.clone();
    classpath.push(plan.client_path.clone());
    let classpath = std::env::join_paths(classpath.iter()).map_err(|_| "Не удалось собрать classpath Minecraft")?
        .to_string_lossy().into_owned();
    let assets = &plan.assets_root;
    let asset_index_name = &plan.version.asset_index.id;
    let mut values = HashMap::new();
    values.insert("auth_player_name", profile.username.clone());
    values.insert("version_name", plan.version.id.clone());
    values.insert("game_directory", plan.instance.game_directory.clone());
    values.insert("assets_root", assets.to_string_lossy().into_owned());
    values.insert("assets_index_name", asset_index_name.clone());
    values.insert("auth_uuid", profile.uuid.clone());
    values.insert("auth_access_token", "0".into());
    values.insert("clientid", "".into());
    values.insert("auth_xuid", "".into());
    values.insert("user_type", "legacy".into());
    values.insert("version_type", if plan.version.r#type.is_empty() { "release".into() } else { plan.version.r#type.clone() });
    values.insert("natives_directory", plan.native_directory.to_string_lossy().into_owned());
    values.insert("launcher_name", "Newest Launcher".into());
    values.insert("launcher_version", env!("CARGO_PKG_VERSION").into());
    values.insert("classpath", classpath);
    values.insert("classpath_separator", separator.into());
    values.insert("library_directory", plan.library_directory.to_string_lossy().into_owned());
    values.insert("resolution_width", plan.instance.resolution.width.to_string());
    values.insert("resolution_height", plan.instance.resolution.height.to_string());
    // javaw.exe intentionally has no console on Windows. Use its java.exe sibling instead
    // so that the launcher can reliably capture a JVM startup failure, then suppress the
    // console window with CREATE_NO_WINDOW in `configure_game_process`.
    let mut command = Command::new(console_java(&java));
    let mut jvm = argument_group(plan.version.arguments.get("jvm"), &values);
    if jvm.is_empty() {
        jvm = vec![format!("-Djava.library.path={}", values["natives_directory"]), "-cp".into(), values["classpath"].clone()];
    }
    if let (Some(argument), Some(path)) = (&plan.logging_argument, &plan.logging_path) {
        jvm.push(argument.replace("${path}", &path.to_string_lossy()));
    }
    command.args(jvm);
    command.arg(format!("-Xmx{}M", plan.instance.ram_mb));
    command.args(&plan.instance.jvm_args);
    command.arg(&plan.version.main_class);
    let mut game = argument_group(plan.version.arguments.get("game"), &values);
    if game.is_empty() {
        if let Some(arguments) = &plan.version.minecraft_arguments {
            game = split_legacy_arguments(arguments).into_iter().map(|argument| expand(&argument, &values)).collect();
        }
    }
    if game.is_empty() { return Err("Version JSON не содержит игровые аргументы".into()); }
    command.args(game).current_dir(&plan.instance.game_directory);
    Ok(command)
}

fn argument_group(value: Option<&Value>, values: &HashMap<&str, String>) -> Vec<String> {
    let Some(items) = value.and_then(Value::as_array) else { return vec![]; };
    let mut arguments = Vec::new();
    for item in items {
        match item {
            Value::String(argument) => arguments.push(expand(argument, values)),
            Value::Object(object) if rules_apply(object.get("rules").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])) => {
                match object.get("value") {
                    Some(Value::String(argument)) => arguments.push(expand(argument, values)),
                    Some(Value::Array(items)) => arguments.extend(items.iter().filter_map(Value::as_str).map(|argument| expand(argument, values))),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    arguments
}

fn rules_apply(rules: &[Value]) -> bool {
    if rules.is_empty() { return true; }
    let mut allowed = false;
    for rule in rules {
        let Some(object) = rule.as_object() else { continue; };
        let os_matches = match object.get("os").and_then(Value::as_object) {
            None => true,
            Some(os) => os.get("name").and_then(Value::as_str).map_or(true, |name| name == current_os())
                && os.get("arch").and_then(Value::as_str).map_or(true, |arch| arch == std::env::consts::ARCH || (arch == "x86" && current_arch() == "32")),
        };
        let feature_matches = object.get("features").and_then(Value::as_object).map_or(true, |features| {
            features.iter().all(|(feature, expected)| feature_enabled(feature) == expected.as_bool().unwrap_or(false))
        });
        if os_matches && feature_matches {
            allowed = object.get("action").and_then(Value::as_str).unwrap_or("allow") == "allow";
        }
    }
    allowed
}

fn feature_enabled(feature: &str) -> bool {
    matches!(feature, "has_custom_resolution")
}

fn expand(input: &str, values: &HashMap<&str, String>) -> String {
    let mut result = input.to_owned();
    for (key, value) in values { result = result.replace(&format!("${{{key}}}"), value); }
    result
}

fn split_legacy_arguments(input: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in input.chars() {
        if escaped { current.push(character); escaped = false; continue; }
        if character == '\\' { escaped = true; continue; }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) { quote = None; } else if quote.is_none() { quote = Some(character); } else { current.push(character); }
            continue;
        }
        if character.is_whitespace() && quote.is_none() {
            if !current.is_empty() { values.push(std::mem::take(&mut current)); }
        } else { current.push(character); }
    }
    if !current.is_empty() { values.push(current); }
    values
}

fn find_java(instance: &Instance, root: &Path, required: u32) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = &instance.java_path { candidates.push(PathBuf::from(path)); }
    if let Some(home) = std::env::var_os("JAVA_HOME") { candidates.push(PathBuf::from(home).join("bin").join(java_binary())); }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|folder| folder.join(java_binary())));
    }
    if let Ok(platform) = AdoptiumPlatform::current() {
        candidates.push(java_runtime_path(root, required, platform).join("bin").join(java_binary()));
    }
    let mut seen = HashSet::new();
    for path in candidates {
        if !seen.insert(path.clone()) || !path.is_file() { continue; }
        if java_major_version(&path).is_some_and(|actual| actual >= required) { return Some(path); }
    }
    None
}

fn java_binary() -> &'static str { if cfg!(windows) { "javaw.exe" } else { "java" } }

fn console_java(java: &Path) -> PathBuf {
    if cfg!(windows) && java.file_name().is_some_and(|name| name.eq_ignore_ascii_case("javaw.exe")) {
        let console = java.with_file_name("java.exe");
        if console.is_file() { return console; }
    }
    java.to_owned()
}

fn configure_game_process(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW keeps java.exe from flashing a console while retaining piped
        // stdout/stderr. javaw.exe cannot provide the same reliable diagnostic channel.
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

fn java_major_version(path: &Path) -> Option<u32> {
    // javaw deliberately has no console on Windows, so probe its java.exe
    // sibling but retain javaw.exe as the executable used to start the game.
    let probe = if cfg!(windows) && path.file_name().is_some_and(|name| name.eq_ignore_ascii_case("javaw.exe")) {
        let console_java = path.with_file_name("java.exe");
        console_java.is_file().then_some(console_java).unwrap_or_else(|| path.to_owned())
    } else { path.to_owned() };
    let output = Command::new(probe).arg("-version").output().ok()?;
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    parse_java_major(&text)
}

fn java_runtime_path(root: &Path, major: u32, platform: AdoptiumPlatform) -> PathBuf {
    root.join("cache").join("java").join("temurin").join(major.to_string())
        .join(format!("{}-{}", platform.os, platform.architecture)).join("runtime")
}

fn existing_java(runtime: &Path, required: u32) -> Option<PathBuf> {
    let java = runtime.join("bin").join(java_binary());
    java_major_version(&java).filter(|actual| *actual >= required).map(|_| java)
}

fn find_java_payload(stage: &Path) -> Result<PathBuf, String> {
    let direct = stage.join("bin").join(java_binary());
    if direct.is_file() { return Ok(stage.to_owned()); }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(stage).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let metadata = entry.metadata().map_err(io_error)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            let directory = entry.path();
            if directory.join("bin").join(java_binary()).is_file() { candidates.push(directory); }
        }
    }
    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        _ => Err("Архив Java не содержит ожидаемую папку runtime".into()),
    }
}

fn remove_runtime_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() { return Err("Кэш Java содержит небезопасный путь".into()); }
    fs::remove_dir_all(path).map_err(io_error)
}

fn parse_java_major(text: &str) -> Option<u32> {
    let marker = "version \"";
    let start = text.find(marker)? + marker.len();
    let version = text[start..].split('"').next()?;
    let version = version.strip_prefix("1.").unwrap_or(version);
    version.split(['.', '-', '+']).next()?.parse().ok()
}

async fn verify_file(path: &Path, expected_sha1: &str, expected_size: Option<u64>) -> Result<bool, String> {
    let path = path.to_owned();
    let sha1 = expected_sha1.to_owned();
    tokio::task::spawn_blocking(move || {
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if !metadata.file_type().is_file() || expected_size.is_some_and(|size| metadata.len() != size) { return Ok(false); }
        let mut file = File::open(path).map_err(io_error)?;
        let mut hash = Sha1::new();
        let mut buffer = [0_u8; 1024 * 128];
        loop {
            let amount = file.read(&mut buffer).map_err(io_error)?;
            if amount == 0 { break; }
            hash.update(&buffer[..amount]);
        }
        Ok(format!("{:x}", hash.finalize()) == sha1)
    }).await.map_err(|_| "Проверка файла Minecraft была прервана")?
}

async fn verify_sha256_file(path: &Path, expected_sha256: &str, expected_size: Option<u64>) -> Result<bool, String> {
    let path = path.to_owned();
    let sha256 = expected_sha256.to_ascii_lowercase();
    tokio::task::spawn_blocking(move || {
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if !metadata.file_type().is_file() || expected_size.is_some_and(|size| metadata.len() != size) { return Ok(false); }
        let mut file = File::open(path).map_err(io_error)?;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 1024 * 128];
        loop {
            let amount = file.read(&mut buffer).map_err(io_error)?;
            if amount == 0 { break; }
            hash.update(&buffer[..amount]);
        }
        Ok(format!("{:x}", hash.finalize()) == sha256)
    }).await.map_err(|_| "Проверка Java-архива была прервана")?
}

async fn extract_java_archive(archive: &Path, destination: &Path, kind: JavaArchiveKind) -> Result<(), String> {
    let archive = archive.to_owned();
    let destination = destination.to_owned();
    tokio::task::spawn_blocking(move || match kind {
        JavaArchiveKind::TarGz => extract_java_tar_gz(&archive, &destination),
        JavaArchiveKind::Zip => extract_java_zip(&archive, &destination),
    }).await.map_err(|_| "Распаковка Java была прервана")?
}

fn extract_java_tar_gz(archive: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(io_error)?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut extracted = 0_u64;
    for item in archive.entries().map_err(|_| "Java-архив повреждён")? {
        let mut item = item.map_err(|_| "Java-архив повреждён")?;
        let path = item.path().map_err(|_| "Java-архив содержит некорректный путь")?;
        let relative = safe_archive_relative(&path)?;
        let output = destination.join(relative);
        let kind = item.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&output).map_err(io_error)?;
            continue;
        }
        // Temurin's Linux archives use symbolic links for duplicate legal notices.
        // They are not needed by the JVM, and omitting them avoids ever creating a
        // link which could later escape the runtime cache.
        if kind.is_symlink() { continue; }
        if !kind.is_file() { return Err("Java-архив содержит неподдерживаемую ссылку или тип файла".into()); }
        let parent = output.parent().ok_or_else(|| "Java-архив содержит некорректный путь".to_owned())?;
        fs::create_dir_all(parent).map_err(io_error)?;
        let mut target = OpenOptions::new().create_new(true).write(true).open(&output).map_err(io_error)?;
        let written = std::io::copy(&mut item, &mut target).map_err(io_error)?;
        extracted = extracted.checked_add(written).ok_or_else(|| "Распакованная Java превышает лимит".to_owned())?;
        if extracted > MAX_JAVA_EXTRACTED_BYTES { return Err("Распакованная Java превышает лимит".into()); }
        set_executable_permissions(&output, item.header().mode().unwrap_or(0));
    }
    Ok(())
}

fn extract_java_zip(archive: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "Java-архив повреждён")?;
    let mut extracted = 0_u64;
    for index in 0..archive.len() {
        let mut item = archive.by_index(index).map_err(|_| "Java-архив повреждён")?;
        let relative = item.enclosed_name().map(PathBuf::from).ok_or_else(|| "Java-архив содержит небезопасный путь".to_owned())?;
        let output = destination.join(relative);
        if item.is_dir() { fs::create_dir_all(&output).map_err(io_error)?; continue; }
        let parent = output.parent().ok_or_else(|| "Java-архив содержит некорректный путь".to_owned())?;
        fs::create_dir_all(parent).map_err(io_error)?;
        let mut target = OpenOptions::new().create_new(true).write(true).open(&output).map_err(io_error)?;
        let written = std::io::copy(&mut item, &mut target).map_err(io_error)?;
        extracted = extracted.checked_add(written).ok_or_else(|| "Распакованная Java превышает лимит".to_owned())?;
        if extracted > MAX_JAVA_EXTRACTED_BYTES { return Err("Распакованная Java превышает лимит".into()); }
        set_executable_permissions(&output, item.unix_mode().unwrap_or(0));
    }
    Ok(())
}

fn safe_archive_relative(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() || path.components().count() > 32 || !path.components().all(|part| matches!(part, Component::Normal(_))) {
        return Err("Java-архив содержит небезопасный путь".into());
    }
    Ok(path.to_owned())
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    if mode & 0o111 != 0 {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777));
    }
}

#[cfg(not(unix))]
fn set_executable_permissions(_: &Path, _: u32) {}

async fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let data = tokio::fs::read(path).await.map_err(io_error)?;
    if data.len() > MAX_JSON_BYTES { return Err("JSON Minecraft слишком большой".into()); }
    serde_json::from_slice(&data)
        .map_err(|error| format!("Официальный JSON Minecraft имеет неподдерживаемый формат: {error}"))
}

async fn extract_natives(archives: Vec<PathBuf>, destination: PathBuf) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if destination.exists() {
            let metadata = fs::symlink_metadata(&destination).map_err(io_error)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() { return Err("Небезопасная папка natives".into()); }
            fs::remove_dir_all(&destination).map_err(io_error)?;
        }
        fs::create_dir_all(&destination).map_err(io_error)?;
        let mut written = 0_u64;
        for archive in archives {
            let file = File::open(&archive).map_err(io_error)?;
            let mut zip = zip::ZipArchive::new(file).map_err(|_| "Native-архив Minecraft повреждён")?;
            for index in 0..zip.len() {
                let mut item = zip.by_index(index).map_err(|_| "Native-архив Minecraft повреждён")?;
                let Some(relative) = item.enclosed_name().map(PathBuf::from) else { return Err("Native-архив содержит небезопасный путь".into()); };
                if relative.components().next().is_some_and(|part| part.as_os_str().eq_ignore_ascii_case("META-INF")) { continue; }
                let output = destination.join(relative);
                if item.is_dir() { fs::create_dir_all(&output).map_err(io_error)?; continue; }
                if let Some(parent) = output.parent() { fs::create_dir_all(parent).map_err(io_error)?; }
                let mut target = OpenOptions::new().create(true).truncate(true).write(true).open(&output).map_err(io_error)?;
                let amount = std::io::copy(&mut item, &mut target).map_err(io_error)?;
                written = written.checked_add(amount).ok_or_else(|| "Native-файлы превышают лимит".to_owned())?;
                if written > MAX_NATIVE_BYTES { return Err("Native-файлы превышают лимит".into()); }
            }
        }
        Ok(())
    }).await.map_err(|_| "Распаковка natives была прервана")?
}

fn relay_output<R: Read + Send + 'static>(source: R, log_path: PathBuf, stream: &'static str) {
    thread::spawn(move || {
        for line in BufReader::new(source).lines().map_while(Result::ok) {
            let _ = append_game_log(&log_path, &format!("[{stream}] {line}"));
        }
    });
}

fn append_game_log(path: &Path, line: &str) -> Result<(), String> {
    ensure_parent(path)?;
    reject_symlink(path)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(io_error)?;
    writeln!(file, "{line}").map_err(io_error)
}

fn validate_download_url(input: &str) -> Result<(), String> {
    let url = url::Url::parse(input).map_err(|_| "Некорректный URL Minecraft".to_owned())?;
    let allowed = [
        "launchermeta.mojang.com", "piston-meta.mojang.com", "piston-data.mojang.com", "resources.download.minecraft.net", "libraries.minecraft.net",
        "meta.fabricmc.net", "meta.quiltmc.org", "maven.fabricmc.net", "maven.quiltmc.org",
    ];
    if url.scheme() != "https" || !allowed.contains(&url.host_str().unwrap_or_default()) || !url.username().is_empty() || url.password().is_some() {
        return Err("Minecraft metadata содержит неразрешённый адрес загрузки".into());
    }
    Ok(())
}

fn validate_adoptium_api_url(input: &str) -> Result<(), String> {
    let url = url::Url::parse(input).map_err(|_| "Некорректный URL Adoptium API".to_owned())?;
    if url.scheme() != "https" || url.host_str() != Some("api.adoptium.net") || !url.username().is_empty() || url.password().is_some() {
        return Err("Java manager получил неразрешённый адрес Adoptium API".into());
    }
    Ok(())
}

fn validate_adoptium_package_url(input: &str) -> Result<(), String> {
    let url = url::Url::parse(input).map_err(|_| "Некорректный URL Java-архива".to_owned())?;
    let allowed = ["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];
    if url.scheme() != "https" || !allowed.contains(&url.host_str().unwrap_or_default()) || !url.username().is_empty() || url.password().is_some() {
        return Err("Java manager получил неразрешённый адрес Java-архива".into());
    }
    Ok(())
}

fn validate_sha1(hash: &str) -> Result<(), String> {
    if hash.len() != 40 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err("Minecraft metadata содержит некорректную SHA-1 сумму".into()); }
    Ok(())
}

fn validate_sha256(hash: &str) -> Result<(), String> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err("Adoptium API содержит некорректную SHA-256 сумму".into()); }
    Ok(())
}

fn safe_component(value: &str) -> Result<&str, String> {
    if value.is_empty() || value.len() > 120 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-+".contains(&byte)) {
        return Err("Minecraft metadata содержит небезопасный идентификатор".into());
    }
    Ok(value)
}

fn safe_relative(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.len() > 4096 { return Err("Minecraft metadata содержит небезопасный путь".into()); }
    let path = Path::new(value);
    if path.components().count() > 32 || !path.components().all(|part| matches!(part, Component::Normal(_))) {
        return Err("Minecraft metadata содержит небезопасный путь".into());
    }
    Ok(path.to_owned())
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    if path.exists() { reject_symlink(path)?; }
    fs::create_dir_all(path).map_err(io_error)
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "Некорректный путь Minecraft".to_owned())?;
    ensure_directory(parent)
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) => Err("Небезопасный путь Minecraft".into()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn current_os() -> &'static str {
    if cfg!(target_os = "windows") { "windows" } else if cfg!(target_os = "macos") { "osx" } else { "linux" }
}

fn current_arch() -> &'static str { if cfg!(target_pointer_width = "64") { "64" } else { "32" } }
fn core_error(error: newest_launcher_core::CoreError) -> String { error.to_string() }
fn io_error(error: std::io::Error) -> String { format!("Ошибка файловой системы Minecraft: {error}") }
fn network_error(error: reqwest::Error) -> String { format!("Minecraft недоступен: {error}") }
fn java_network_error(error: reqwest::Error) -> String { format!("Java runtime недоступен: {error}") }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_download_paths_and_hosts() {
        assert!(safe_relative("../outside").is_err());
        assert!(safe_relative("a/../../outside").is_err());
        assert!(safe_archive_relative(Path::new("../outside")).is_err());
        assert!(validate_download_url("https://example.invalid/client.jar").is_err());
        assert!(validate_download_url("http://piston-data.mojang.com/client.jar").is_err());
        assert!(validate_adoptium_api_url("https://api.adoptium.net/v3/assets/latest/21/hotspot").is_ok());
        assert!(validate_adoptium_package_url("https://github.com/adoptium/temurin/releases/download/jre.tar.gz").is_ok());
        assert!(validate_adoptium_package_url("https://example.invalid/jre.tar.gz").is_err());
    }

    #[test]
    fn adoptium_release_accepts_official_package_shape() {
        let json = r#"[{"binary":{"package":{"checksum":"2413149700df0f7d440500a84a8f764c535f21e5a5e87d38328b64eec2c5b500","link":"https://github.com/adoptium/temurin21-binaries/releases/download/jre.tar.gz","name":"OpenJDK21U-jre_x64_linux_hotspot.tar.gz","size":52059408}}}]"#;
        let releases: Vec<AdoptiumRelease> = serde_json::from_str(json).unwrap();
        assert_eq!(releases[0].binary.package.size, 52_059_408);
        assert_eq!(JavaArchiveKind::from_name(&releases[0].binary.package.name).unwrap().extension(), "tar.gz");
    }

    #[test]
    fn java_version_parser_handles_common_outputs() {
        assert_eq!(parse_java_major("openjdk version \"21.0.12\""), Some(21));
        assert_eq!(parse_java_major("java version \"1.8.0_392\""), Some(8));
        assert_eq!(parse_java_major("not a Java version"), None);
    }

    #[test]
    fn legacy_arguments_preserve_quoted_values() {
        assert_eq!(split_legacy_arguments(r#"--username "Alex One" --demo"#), vec!["--username", "Alex One", "--demo"]);
    }

    #[test]
    fn version_metadata_accepts_camel_case_java_requirement() {
        let json = r#"{
          "id":"1.21.1", "mainClass":"net.minecraft.client.main.Main", "type":"release",
          "downloads":{"client":{"url":"https://piston-data.mojang.com/client.jar","sha1":"0000000000000000000000000000000000000000","size":1}},
          "assetIndex":{"id":"17","url":"https://piston-meta.mojang.com/assets.json","sha1":"0000000000000000000000000000000000000000","size":1},
          "javaVersion":{"majorVersion":21}
        }"#;
        let metadata: VersionMeta = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.java_version.unwrap().major_version, 21);
    }

    async fn exercise_official_loader(loader: &str, minecraft_version: &str, loader_version: &str) -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("newest-launcher-official-loader-{}", uuid::Uuid::new_v4()));
        let outcome = async {
            let core = LauncherCore::open(root.clone()).map_err(core_error)?;
            let snapshot = core.create_instance(newest_launcher_core::InstanceInput {
                name: format!("{loader} integration"), minecraft_version: minecraft_version.into(), loader: loader.into(),
                loader_version: Some(loader_version.into()), java_path: None, ram_mb: 2048, jvm_args: vec![], resolution: Default::default(),
            }).map_err(core_error)?;
            let id = snapshot.active_instance_id.clone().ok_or_else(|| "Интеграционный instance не создан".to_owned())?;
            let service = MinecraftService::new().map_err(network_error)?;
            let snapshot = service.install(&core, id.clone()).await?;
            let instance = snapshot.instances.into_iter().find(|instance| instance.id == id).ok_or_else(|| "Интеграционный instance не сохранён".to_owned())?;
            if instance.installation_state != "installed" || instance.loader_version.as_deref() != Some(loader_version) {
                return Err("Официальный loader не был зарегистрирован как установленный".into());
            }
            // `MinecraftService::install` calls prepare_official_launch_plan before it commits
            // the instance. Verify the committed result once more through the independent
            // loader verifier, without re-downloading all Vanilla assets during this network test.
            let runtime = isolated_runtime_path(&core.data_directory(), &id)?;
            let artifact = official_artifact_version(loader, minecraft_version, loader_version);
            let verified = loaders::verify(loader, &runtime, minecraft_version, &artifact).await?;
            if verified.library_paths.is_empty() || !verified.version_json.is_file() {
                return Err("Version JSON official loader не создал пригодный classpath".into());
            }
            Ok(())
        }.await;
        let _ = fs::remove_dir_all(&root);
        outcome
    }

    #[tokio::test]
    #[ignore = "downloads and runs the official Forge installer"]
    async fn official_forge_pipeline_builds_a_launch_plan() {
        exercise_official_loader("forge", "1.20.1", "47.4.10").await.unwrap();
    }

    #[tokio::test]
    #[ignore = "downloads and runs the official NeoForge installer"]
    async fn official_neoforge_pipeline_builds_a_launch_plan() {
        exercise_official_loader("neoforge", "1.21.1", "21.1.251").await.unwrap();
    }
}
