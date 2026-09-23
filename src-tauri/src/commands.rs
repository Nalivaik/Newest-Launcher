use crate::{microsoft_auth::{MicrosoftAuth, MicrosoftLoginChallenge}, minecraft::{GameStatus, MinecraftService}, modrinth::ModrinthInstaller};
use newest_launcher_core::{InstanceInput, LauncherCore, LogEntry, Settings, Snapshot, StorageUsage};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

type CoreState<'a> = State<'a, Arc<LauncherCore>>;

async fn blocking<T: Send + 'static>(
    core: Arc<LauncherCore>,
    work: impl FnOnce(&LauncherCore) -> newest_launcher_core::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || work(&core))
        .await.map_err(|_| "Фоновая операция прервана".to_owned())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn bootstrap(core: CoreState<'_>) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), |core| core.snapshot()).await
}

#[tauri::command]
pub async fn create_instance(core: CoreState<'_>, input: InstanceInput) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), move |core| core.create_instance(input)).await
}

#[tauri::command]
pub async fn update_instance(core: CoreState<'_>, id: String, input: InstanceInput) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), move |core| core.update_instance(&id, input)).await
}

#[tauri::command]
pub async fn clone_instance(core: CoreState<'_>, id: String, name: String) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), move |core| core.clone_instance(&id, &name)).await
}

#[tauri::command]
pub async fn delete_instance(core: CoreState<'_>, id: String) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), move |core| core.delete_instance(&id)).await
}

#[tauri::command]
pub async fn restore_instance(core: CoreState<'_>, id: String) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), move |core| core.restore_instance(&id)).await
}

#[tauri::command]
pub async fn select_instance(core: CoreState<'_>, id: String) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), move |core| core.select_instance(&id)).await
}

#[tauri::command]
pub async fn create_offline_profile(core: CoreState<'_>, username: String) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), move |core| core.create_offline_profile(username)).await
}

#[tauri::command]
pub async fn select_profile(core: CoreState<'_>, id: String) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), move |core| core.select_profile(&id)).await
}

#[tauri::command]
pub async fn delete_offline_profile(core: CoreState<'_>, id: String) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), move |core| core.delete_offline_profile(&id)).await
}

#[tauri::command]
pub async fn start_microsoft_sign_in(app: AppHandle, auth: State<'_, MicrosoftAuth>) -> Result<MicrosoftLoginChallenge, String> {
    auth.begin_sign_in(&app).await
}

#[tauri::command]
pub async fn finish_microsoft_sign_in(
    core: CoreState<'_>, auth: State<'_, MicrosoftAuth>, challenge_id: String,
) -> Result<Snapshot, String> {
    auth.finish_sign_in(core.inner(), &challenge_id).await
}

#[tauri::command]
pub async fn install_minecraft(core: CoreState<'_>, minecraft: State<'_, MinecraftService>, id: String) -> Result<Snapshot, String> {
    minecraft.install(core.inner(), id).await
}

#[tauri::command]
pub async fn launch_minecraft(
    core: CoreState<'_>, minecraft: State<'_, MinecraftService>, auth: State<'_, MicrosoftAuth>, id: String,
) -> Result<GameStatus, String> {
    minecraft.launch(core.inner().clone(), auth.inner(), id).await
}

#[tauri::command]
pub fn stop_minecraft(minecraft: State<'_, MinecraftService>, id: String) -> Result<GameStatus, String> {
    minecraft.stop(&id)
}

#[tauri::command]
pub fn minecraft_status(minecraft: State<'_, MinecraftService>, id: String) -> Result<GameStatus, String> {
    minecraft.status(&id)
}

#[tauri::command]
pub async fn install_modrinth_content(
    core: CoreState<'_>, installer: State<'_, ModrinthInstaller>, id: String, project_id: String, content_type: String,
) -> Result<Snapshot, String> {
    installer.install(core.inner(), id, project_id, content_type).await
}

#[tauri::command]
pub async fn save_settings(core: CoreState<'_>, settings: Settings) -> Result<Snapshot, String> {
    blocking(core.inner().clone(), move |core| core.save_settings(settings)).await
}

#[tauri::command]
pub async fn storage_usage(core: CoreState<'_>) -> Result<StorageUsage, String> {
    blocking(core.inner().clone(), |core| core.storage_usage()).await
}

#[tauri::command]
pub async fn read_logs(core: CoreState<'_>) -> Result<Vec<LogEntry>, String> {
    blocking(core.inner().clone(), |core| core.read_logs()).await
}

#[tauri::command]
pub async fn open_folder(app: AppHandle, core: CoreState<'_>, kind: String, instance_id: Option<String>) -> Result<(), String> {
    let path = blocking(core.inner().clone(), move |core| core.folder_path(&kind, instance_id.as_deref())).await?;
    app.opener().open_path(path.to_string_lossy().into_owned(), None::<&str>).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn export_instance(app: AppHandle, core: CoreState<'_>, id: String) -> Result<bool, String> {
    // Paths are selected in a native dialog, never accepted from untrusted web content.
    let file = tauri::async_runtime::spawn_blocking(move || app.dialog().file()
        .add_filter("Newest instance", &["zip"]).set_file_name("newest-instance.zip").blocking_save_file())
        .await.map_err(|_| "Не удалось открыть диалог сохранения".to_owned())?;
    let Some(file) = file else { return Ok(false) };
    let path = file.into_path().map_err(|error| error.to_string())?;
    blocking(core.inner().clone(), move |core| core.export_instance(&id, &path)).await?;
    Ok(true)
}

#[tauri::command]
pub async fn import_instance(app: AppHandle, core: CoreState<'_>) -> Result<Option<Snapshot>, String> {
    let file = tauri::async_runtime::spawn_blocking(move || app.dialog().file()
        .add_filter("Newest instance", &["zip"]).blocking_pick_file())
        .await.map_err(|_| "Не удалось открыть диалог выбора файла".to_owned())?;
    let Some(file) = file else { return Ok(None) };
    let path = file.into_path().map_err(|error| error.to_string())?;
    blocking(core.inner().clone(), move |core| core.import_instance(&path)).await.map(Some)
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = url::Url::parse(&url).map_err(|_| "Некорректный адрес".to_owned())?;
    if parsed.scheme() != "https" || !matches!(parsed.host_str(), Some("modrinth.com" | "www.minecraft.net"))
        || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Этот адрес не разрешён".to_owned());
    }
    app.opener().open_url(url, None::<&str>).map_err(|error| error.to_string())
}
