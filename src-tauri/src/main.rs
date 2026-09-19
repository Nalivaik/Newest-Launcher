#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod loaders;
mod minecraft;
mod metadata;
mod modrinth;

use newest_launcher_core::LauncherCore;
use std::sync::Arc;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            app.manage(Arc::new(LauncherCore::open(root)?));
            app.manage(metadata::MetadataClient::new()?);
            app.manage(modrinth::ModrinthInstaller::new()?);
            app.manage(minecraft::MinecraftService::new()?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap, commands::create_instance, commands::update_instance,
            commands::clone_instance, commands::delete_instance, commands::restore_instance,
            commands::select_instance, commands::save_settings, commands::open_folder,
            commands::create_offline_profile, commands::select_profile, commands::delete_offline_profile,
            commands::storage_usage, commands::read_logs, commands::export_instance,
            commands::import_instance, commands::open_external, metadata::modrinth_metadata,
            commands::install_minecraft, commands::launch_minecraft, commands::stop_minecraft,
            commands::minecraft_status, commands::install_modrinth_content,
        ])
        .run(tauri::generate_context!())
        .expect("Newest Launcher failed to start; check the system WebView and data-directory permissions");
}
