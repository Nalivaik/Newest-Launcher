//! Persistent, isolated launcher profiles and the filesystem boundary used by the desktop runtime.
mod archive;
mod filesystem;
mod models;
mod store;

pub use models::{ContentRecord, Instance, InstanceInput, LogEntry, Profile, Resolution, Settings, Snapshot, StorageUsage};

use fs2::FileExt;
use std::{fs::{self, File, OpenOptions}, path::{Path, PathBuf}, sync::Mutex};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Invalid configuration: {0}")]
    Invalid(&'static str),
    #[error("The requested instance does not exist")]
    NotFound,
    #[error("The launcher data directory is in use by another process")]
    Busy,
    #[error("The launcher store is damaged; the original file has been preserved")]
    CorruptStore,
    #[error("This data was created by a newer launcher; upgrade before opening it")]
    FutureSchema,
    #[error("Unsafe filesystem path or symbolic link")]
    UnsafePath,
    #[error("Archive or directory exceeds the supported size limits")]
    LimitExceeded,
    #[error("Unsupported or damaged instance archive")]
    InvalidArchive,
    #[error("Launcher state lock is unavailable")]
    Poisoned,
    #[error("Filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Archive operation failed: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// One writer owns the store for the lifetime of this value. Mutex serialization also keeps
/// filesystem mutations and the JSON index in the same transaction within a process.
pub struct LauncherCore {
    root: PathBuf,
    state: Mutex<Snapshot>,
    _lock: File,
}

impl LauncherCore {
    pub fn open(root: PathBuf) -> Result<Self> {
        if root.exists() && fs::symlink_metadata(&root)?.file_type().is_symlink() {
            return Err(CoreError::UnsafePath);
        }
        fs::create_dir_all(&root)?;
        let root = fs::canonicalize(root)?;
        for name in ["instances", "trash", "cache", "logs", "exports", ".staging"] {
            filesystem::ensure_directory(&root, Path::new(name))?;
        }
        let lock_path = root.join("launcher.lock");
        filesystem::reject_link_or_special(&lock_path)?;
        let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(lock_path)?;
        lock.try_lock_exclusive().map_err(|_| CoreError::Busy)?;
        let (state, needs_save) = store::load(&root)?;
        if needs_save { store::save(&root, &state)?; }
        let core = Self { root, state: Mutex::new(state), _lock: lock };
        core.log("info", "launcher.opened", "Launcher data opened");
        Ok(core)
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        Ok(self.state.lock().map_err(|_| CoreError::Poisoned)?.clone())
    }

    /// Returns a copy of one instance after validating its identifier. The desktop runtime uses
    /// this instead of accepting a game path from the webview.
    pub fn instance(&self, id: &str) -> Result<Instance> {
        models::validate_id(id)?;
        let state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        state.instances.iter().find(|item| item.id == id).cloned().ok_or(CoreError::NotFound)
    }

    pub fn active_profile(&self) -> Result<Option<Profile>> {
        let state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(state.active_profile_id.as_ref().and_then(|id| state.profiles.iter().find(|profile| &profile.id == id)).cloned())
    }

    /// The value is derived from the locked data root, never supplied by the frontend.
    pub fn data_directory(&self) -> PathBuf { self.root.clone() }

    pub fn set_installation_state(&self, id: &str, installation_state: &str) -> Result<Snapshot> {
        if !["not_installed", "installed", "corrupted"].contains(&installation_state) {
            return Err(CoreError::Invalid("unknown installation state"));
        }
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let mut next = state.clone();
        let instance = next.instances.iter_mut().find(|item| item.id == id).ok_or(CoreError::NotFound)?;
        instance.installation_state = installation_state.to_owned();
        store::save(&self.root, &next)?;
        *state = next;
        self.log("info", "instance.installation_state", "Instance installation state changed");
        Ok(state.clone())
    }

    /// Commits the exact loader version only after the desktop installer has validated the
    /// files it created. Keeping this transaction in core makes the installed loader survive
    /// restarts and prevents the UI from treating a requested version as an installed one.
    pub fn complete_loader_installation(&self, id: &str, loader: &str, loader_version: String) -> Result<Snapshot> {
        models::validate_id(id)?;
        if !["forge", "neoforge"].contains(&loader) { return Err(CoreError::Invalid("unknown official loader")); }
        models::validate_version(&loader_version)?;
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let mut next = state.clone();
        let instance = next.instances.iter_mut().find(|item| item.id == id).ok_or(CoreError::NotFound)?;
        if instance.loader != loader { return Err(CoreError::Invalid("loader changed during installation")); }
        instance.loader_version = Some(loader_version);
        instance.installation_state = "installed".into();
        store::save(&self.root, &next)?;
        *state = next;
        self.log("info", "instance.loader_installed", "Official mod loader installation verified");
        Ok(state.clone())
    }

    /// Persists only package metadata after the desktop backend has written and verified the
    /// corresponding file inside this instance. Reinstalling the same project replaces its
    /// previous record instead of accumulating stale duplicates.
    pub fn record_content_installation(&self, id: &str, content_type: &str, record: ContentRecord) -> Result<Snapshot> {
        models::validate_id(id)?;
        record.validate()?;
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let mut next = state.clone();
        let instance = next.instances.iter_mut().find(|item| item.id == id).ok_or(CoreError::NotFound)?;
        let records = match content_type {
            "mod" => &mut instance.mods,
            "resourcepack" => &mut instance.resource_packs,
            "shader" => &mut instance.shader_packs,
            _ => return Err(CoreError::Invalid("unknown content type")),
        };
        records.retain(|item| item.project_id != record.project_id);
        records.push(record);
        store::save(&self.root, &next)?;
        *state = next;
        self.log("info", "content.installed", "Modrinth content installed and verified");
        Ok(state.clone())
    }

    pub fn record_game_exit(&self, id: &str, elapsed_seconds: u64) -> Result<Snapshot> {
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let mut next = state.clone();
        let instance = next.instances.iter_mut().find(|item| item.id == id).ok_or(CoreError::NotFound)?;
        instance.last_played = Some(models::now());
        instance.total_playtime_seconds = instance.total_playtime_seconds.saturating_add(elapsed_seconds);
        store::save(&self.root, &next)?;
        *state = next;
        self.log("info", "game.exited", "Minecraft process exited");
        Ok(state.clone())
    }

    pub fn create_instance(&self, input: InstanceInput) -> Result<Snapshot> {
        input.validate()?;
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let instance = Instance::new(&self.root, Uuid::new_v4().to_string(), input);
        let stage = filesystem::Stage::new(&self.root)?;
        filesystem::create_game_directories(stage.path())?;
        let target = self.instance_path(&instance.id, false)?;
        fs::rename(stage.path(), &target)?;
        let mut next = state.clone();
        next.active_instance_id = Some(instance.id.clone());
        next.instances.push(instance);
        if let Err(error) = store::save(&self.root, &next) {
            let _ = fs::rename(&target, stage.path());
            return Err(error);
        }
        *state = next;
        self.log("info", "instance.created", "Instance created");
        Ok(state.clone())
    }

    pub fn update_instance(&self, id: &str, input: InstanceInput) -> Result<Snapshot> {
        input.validate()?;
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let mut next = state.clone();
        let instance = next.instances.iter_mut().find(|item| item.id == id).ok_or(CoreError::NotFound)?;
        let requires_reinstall = instance.minecraft_version != input.minecraft_version || instance.loader != input.loader || instance.loader_version != input.loader_version;
        instance.apply(input);
        if requires_reinstall { instance.installation_state = "not_installed".into(); }
        store::save(&self.root, &next)?;
        *state = next;
        self.log("info", "instance.updated", "Instance settings updated");
        Ok(state.clone())
    }

    pub fn clone_instance(&self, id: &str, name: &str) -> Result<Snapshot> {
        models::validate_name(name)?;
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let original = state.instances.iter().find(|item| item.id == id).ok_or(CoreError::NotFound)?;
        let mut instance = original.clone();
        instance.id = Uuid::new_v4().to_string();
        instance.name = name.trim().to_owned();
        instance.created_at = models::now();
        instance.last_played = None;
        instance.total_playtime_seconds = 0;
        instance.game_directory = self.root.join("instances").join(&instance.id).join("game").to_string_lossy().into_owned();
        let stage = filesystem::Stage::new(&self.root)?;
        filesystem::copy_tree(&self.instance_path(id, false)?, stage.path())?;
        let target = self.instance_path(&instance.id, false)?;
        fs::rename(stage.path(), &target)?;
        let mut next = state.clone();
        next.active_instance_id = Some(instance.id.clone());
        next.instances.push(instance);
        if let Err(error) = store::save(&self.root, &next) {
            let _ = fs::rename(&target, stage.path());
            return Err(error);
        }
        *state = next;
        self.log("info", "instance.cloned", "Instance cloned");
        Ok(state.clone())
    }

    /// Deletion is recoverable: files are renamed into the private trash directory.
    pub fn delete_instance(&self, id: &str) -> Result<Snapshot> {
        self.move_instance(id, true)
    }

    pub fn restore_instance(&self, id: &str) -> Result<Snapshot> {
        self.move_instance(id, false)
    }

    fn move_instance(&self, id: &str, deleting: bool) -> Result<Snapshot> {
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let mut next = state.clone();
        let from = if deleting { &mut next.instances } else { &mut next.trash };
        let position = from.iter().position(|item| item.id == id).ok_or(CoreError::NotFound)?;
        let mut instance = from.remove(position);
        let source = self.instance_path(id, !deleting)?;
        let target = self.instance_path(id, deleting)?;
        filesystem::verify_tree(&source)?;
        if target.exists() { return Err(CoreError::UnsafePath); }
        instance.game_directory = target.join("game").to_string_lossy().into_owned();
        if deleting {
            next.trash.push(instance);
            if next.active_instance_id.as_deref() == Some(id) {
                next.active_instance_id = next.instances.first().map(|item| item.id.clone());
            }
        } else {
            next.instances.push(instance);
            next.active_instance_id = Some(id.to_owned());
        }
        fs::rename(&source, &target)?;
        if let Err(error) = store::save(&self.root, &next) {
            let _ = fs::rename(&target, &source);
            return Err(error);
        }
        *state = next;
        if deleting { self.log("info", "instance.trashed", "Instance moved to trash"); }
        else { self.log("info", "instance.restored", "Instance restored from trash"); }
        Ok(state.clone())
    }

    pub fn select_instance(&self, id: &str) -> Result<Snapshot> {
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        if !state.instances.iter().any(|item| item.id == id) { return Err(CoreError::NotFound); }
        let mut next = state.clone();
        next.active_instance_id = Some(id.to_owned());
        store::save(&self.root, &next)?;
        *state = next;
        Ok(state.clone())
    }

    pub fn create_offline_profile(&self, username: String) -> Result<Snapshot> {
        models::validate_username(&username)?;
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        if state.profiles.iter().any(|profile| profile.kind == "offline" && profile.username.eq_ignore_ascii_case(&username)) {
            return Err(CoreError::Invalid("an offline profile with this username already exists"));
        }
        let profile = Profile::new_offline(username);
        let mut next = state.clone();
        next.active_profile_id = Some(profile.id.clone());
        next.profiles.push(profile);
        store::save(&self.root, &next)?;
        *state = next;
        self.log("info", "profile.offline_created", "Offline profile created");
        Ok(state.clone())
    }

    pub fn select_profile(&self, id: &str) -> Result<Snapshot> {
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        if !state.profiles.iter().any(|profile| profile.id == id) { return Err(CoreError::NotFound); }
        let mut next = state.clone();
        next.active_profile_id = Some(id.to_owned());
        if let Some(profile) = next.profiles.iter_mut().find(|profile| profile.id == id) { profile.last_used = Some(models::now()); }
        store::save(&self.root, &next)?;
        *state = next;
        self.log("info", "profile.selected", "Active profile changed");
        Ok(state.clone())
    }

    pub fn delete_offline_profile(&self, id: &str) -> Result<Snapshot> {
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let mut next = state.clone();
        let position = next.profiles.iter().position(|profile| profile.id == id && profile.kind == "offline").ok_or(CoreError::NotFound)?;
        next.profiles.remove(position);
        if next.active_profile_id.as_deref() == Some(id) { next.active_profile_id = next.profiles.first().map(|profile| profile.id.clone()); }
        store::save(&self.root, &next)?;
        *state = next;
        self.log("info", "profile.offline_deleted", "Offline profile deleted");
        Ok(state.clone())
    }

    pub fn save_settings(&self, settings: Settings) -> Result<Snapshot> {
        settings.validate()?;
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let mut next = state.clone();
        next.settings = settings;
        store::save(&self.root, &next)?;
        *state = next;
        self.log("info", "settings.saved", "Launcher settings saved");
        Ok(state.clone())
    }

    pub fn export_instance(&self, id: &str, destination: &Path) -> Result<()> {
        let state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let instance = state.instances.iter().find(|item| item.id == id).ok_or(CoreError::NotFound)?;
        archive::export(&self.root, instance, &self.instance_path(id, false)?, destination)?;
        self.log("info", "instance.exported", "Instance archive exported");
        Ok(())
    }

    pub fn import_instance(&self, source: &Path) -> Result<Snapshot> {
        let mut state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let stage = filesystem::Stage::new(&self.root)?;
        let input = archive::import(source, stage.path())?;
        let instance = Instance::new(&self.root, Uuid::new_v4().to_string(), input);
        filesystem::create_game_directories(stage.path())?;
        let target = self.instance_path(&instance.id, false)?;
        fs::rename(stage.path(), &target)?;
        let mut next = state.clone();
        next.active_instance_id = Some(instance.id.clone());
        next.instances.push(instance);
        if let Err(error) = store::save(&self.root, &next) {
            let _ = fs::rename(&target, stage.path());
            return Err(error);
        }
        *state = next;
        self.log("info", "instance.imported", "Instance archive imported");
        Ok(state.clone())
    }

    pub fn folder_path(&self, kind: &str, instance_id: Option<&str>) -> Result<PathBuf> {
        let state = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        let relative = match kind {
            "data" => PathBuf::new(),
            "logs" | "cache" | "exports" | "trash" => PathBuf::from(kind),
            "instance" | "game" | "mods" | "resourcepacks" | "shaderpacks" | "screenshots" | "saves" => {
                let id = instance_id.or(state.active_instance_id.as_deref()).ok_or(CoreError::NotFound)?;
                if !state.instances.iter().any(|item| item.id == id) { return Err(CoreError::NotFound); }
                let mut path = PathBuf::from("instances").join(id);
                if kind != "instance" { path.push("game"); }
                if !matches!(kind, "instance" | "game") { path.push(kind); }
                path
            }
            _ => return Err(CoreError::Invalid("unknown folder kind")),
        };
        filesystem::ensure_directory(&self.root, &relative)
    }

    pub fn storage_usage(&self) -> Result<StorageUsage> {
        let _guard = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(StorageUsage {
            instances_bytes: filesystem::tree_size(&self.root.join("instances"))?,
            cache_bytes: filesystem::tree_size(&self.root.join("cache"))?,
            trash_bytes: filesystem::tree_size(&self.root.join("trash"))?,
            free_bytes: fs2::available_space(&self.root)?,
            total_bytes: fs2::total_space(&self.root)?,
        })
    }

    pub fn read_logs(&self) -> Result<Vec<LogEntry>> {
        let _guard = self.state.lock().map_err(|_| CoreError::Poisoned)?;
        store::read_logs(&self.root)
    }

    fn instance_path(&self, id: &str, trashed: bool) -> Result<PathBuf> {
        models::validate_id(id)?;
        let relative = PathBuf::from(if trashed { "trash" } else { "instances" }).join(id);
        filesystem::checked_path(&self.root, &relative)
    }

    fn log(&self, level: &str, event: &str, message: &str) {
        // Logging must never turn a successful persistent mutation into a reported failure.
        let _ = store::append_log(&self.root, LogEntry {
            timestamp: models::now(), level: level.to_owned(), event: event.to_owned(), message: message.to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn input(name: &str) -> InstanceInput {
        InstanceInput {
            name: name.into(), minecraft_version: "1.21.11".into(), loader: "fabric".into(),
            loader_version: Some("0.16.9".into()), java_path: Some("/usr/bin/java".into()),
            ram_mb: 4096, jvm_args: vec!["-XX:+UseG1GC".into()], resolution: Resolution::default(),
        }
    }

    #[test]
    fn instances_are_isolated_persistent_and_recoverable() {
        let directory = tempfile::tempdir().unwrap();
        let core = LauncherCore::open(directory.path().join("NewestLauncher")).unwrap();
        let first = core.create_instance(input("Adventure")).unwrap();
        let first_id = first.active_instance_id.clone().unwrap();
        assert!(directory.path().join("NewestLauncher/instances").join(&first_id).join("game/mods").is_dir());

        let cloned = core.clone_instance(&first_id, "Adventure copy").unwrap();
        let clone_id = cloned.active_instance_id.clone().unwrap();
        assert_ne!(clone_id, first_id);
        assert!(directory.path().join("NewestLauncher/instances").join(&clone_id).join("game/shaderpacks").is_dir());

        let trashed = core.delete_instance(&clone_id).unwrap();
        assert_eq!(trashed.instances.len(), 1);
        assert_eq!(trashed.trash.len(), 1);
        assert!(directory.path().join("NewestLauncher/trash").join(&clone_id).is_dir());
        let restored = core.restore_instance(&clone_id).unwrap();
        assert_eq!(restored.instances.len(), 2);
        drop(core);

        let reopened = LauncherCore::open(directory.path().join("NewestLauncher")).unwrap();
        let snapshot = reopened.snapshot().unwrap();
        assert_eq!(snapshot.instances.len(), 2);
        assert_eq!(snapshot.active_instance_id.as_deref(), Some(clone_id.as_str()));
    }

    #[test]
    fn archive_round_trip_drops_machine_specific_java_configuration() {
        let source = tempfile::tempdir().unwrap();
        let core = LauncherCore::open(source.path().join("data")).unwrap();
        let created = core.create_instance(input("Portable")).unwrap();
        let id = created.active_instance_id.unwrap();
        let archive = source.path().join("data/exports/portable.zip");
        core.export_instance(&id, &archive).unwrap();
        drop(core);

        let target = tempfile::tempdir().unwrap();
        let imported = LauncherCore::open(target.path().join("data")).unwrap().import_instance(&archive).unwrap();
        let instance = imported.instances.first().unwrap();
        assert_eq!(instance.name, "Portable");
        assert_eq!(instance.java_path, None);
        assert!(instance.jvm_args.is_empty());
        assert!(target.path().join("data/instances").join(&instance.id).join("game/resourcepacks").is_dir());
    }

    #[test]
    fn old_store_is_migrated_without_destroying_original_schema() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("data")).unwrap();
        let original = r#"{"instances":[],"selectedInstanceId":null}"#;
        fs::write(directory.path().join("data/state.json"), original).unwrap();
        let core = LauncherCore::open(directory.path().join("data")).unwrap();
        assert_eq!(core.snapshot().unwrap().schema_version, 2);
        assert_eq!(fs::read_to_string(directory.path().join("data/state.v0.backup.json")).unwrap(), original);
    }

    #[test]
    fn damaged_store_is_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("data")).unwrap();
        let original = b"this is not JSON";
        let store = directory.path().join("data/state.json");
        fs::write(&store, original).unwrap();
        assert!(matches!(LauncherCore::open(directory.path().join("data")), Err(CoreError::CorruptStore)));
        assert_eq!(fs::read(&store).unwrap(), original);
    }

    #[test]
    fn offline_profiles_are_persistent_and_never_claim_online_identity() {
        let directory = tempfile::tempdir().unwrap();
        let core = LauncherCore::open(directory.path().join("data")).unwrap();
        let state = core.create_offline_profile("Builder_42".into()).unwrap();
        let profile = state.profiles.first().unwrap();
        assert_eq!(profile.kind, "offline");
        assert_eq!(profile.username, "Builder_42");
        assert_eq!(profile.skin_url, None);
        assert_eq!(state.active_profile_id.as_deref(), Some(profile.id.as_str()));
        let id = profile.id.clone();
        drop(core);
        let reopened = LauncherCore::open(directory.path().join("data")).unwrap();
        assert_eq!(reopened.snapshot().unwrap().active_profile_id.as_deref(), Some(id.as_str()));
        assert!(reopened.create_offline_profile("Builder_42".into()).is_err());
    }

    #[test]
    fn changing_game_version_requires_a_new_verified_installation() {
        let directory = tempfile::tempdir().unwrap();
        let core = LauncherCore::open(directory.path().join("data")).unwrap();
        let created = core.create_instance(input("Vanilla")).unwrap();
        let id = created.active_instance_id.unwrap();
        assert_eq!(core.set_installation_state(&id, "installed").unwrap().instances[0].installation_state, "installed");
        let mut changed = input("Vanilla");
        changed.minecraft_version = "1.21.1".into();
        assert_eq!(core.update_instance(&id, changed).unwrap().instances[0].installation_state, "not_installed");
    }

    #[test]
    fn verified_official_loader_version_is_persistent() {
        let directory = tempfile::tempdir().unwrap();
        let data_directory = directory.path().join("data");
        let core = LauncherCore::open(data_directory.clone()).unwrap();
        let mut forge = input("Forge");
        forge.loader = "forge".into();
        forge.loader_version = None;
        let created = core.create_instance(forge).unwrap();
        let id = created.active_instance_id.unwrap();

        let completed = core.complete_loader_installation(&id, "forge", "47.4.10".into()).unwrap();
        let instance = completed.instances.iter().find(|item| item.id == id).unwrap();
        assert_eq!(instance.loader_version.as_deref(), Some("47.4.10"));
        assert_eq!(instance.installation_state, "installed");

        drop(core);
        let reopened = LauncherCore::open(data_directory).unwrap();
        let instance = reopened.instance(&id).unwrap();
        assert_eq!(instance.loader, "forge");
        assert_eq!(instance.loader_version.as_deref(), Some("47.4.10"));
        assert_eq!(instance.installation_state, "installed");
    }

    #[test]
    fn verified_modrinth_content_is_persistent_and_replaces_project_version() {
        let directory = tempfile::tempdir().unwrap();
        let data_directory = directory.path().join("data");
        let core = LauncherCore::open(data_directory.clone()).unwrap();
        let id = core.create_instance(input("Content")).unwrap().active_instance_id.unwrap();
        let record = |version_id: &str, version_number: &str| ContentRecord {
            project_id: "AANobbMI".into(), version_id: version_id.into(), title: "Sodium".into(),
            version_number: version_number.into(), filename: "sodium.jar".into(),
            sha512: "a".repeat(128), installed_at: models::now(),
        };
        core.record_content_installation(&id, "mod", record("first", "0.6.0")).unwrap();
        let snapshot = core.record_content_installation(&id, "mod", record("second", "0.7.0")).unwrap();
        let instance = snapshot.instances.iter().find(|item| item.id == id).unwrap();
        assert_eq!(instance.mods.len(), 1);
        assert_eq!(instance.mods[0].version_id, "second");

        drop(core);
        let reopened = LauncherCore::open(data_directory).unwrap();
        let instance = reopened.instance(&id).unwrap();
        assert_eq!(instance.mods.len(), 1);
        assert_eq!(instance.mods[0].version_number, "0.7.0");
    }
}
