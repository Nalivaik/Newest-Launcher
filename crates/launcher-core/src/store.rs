use crate::{filesystem, models, CoreError, LogEntry, Result, Settings, Snapshot};
use serde_json::{json, Value};
use std::{collections::HashSet, fs::{self, File, OpenOptions}, io::{BufRead, BufReader, Read, Write}, path::Path};

const STORE_LIMIT: u64 = 8 * 1024 * 1024;
const LOG_LIMIT: u64 = 1024 * 1024;
const LOG_BACKUPS: usize = 3;

pub(crate) fn load(root: &Path) -> Result<(Snapshot, bool)> {
    let path = filesystem::checked_path(root, Path::new("state.json"))?;
    if !path.exists() {
        return Ok((Snapshot { schema_version: 2, instances: vec![], trash: vec![], active_instance_id: None,
            profiles: vec![], active_profile_id: None, settings: Settings::default(), data_directory: root.to_string_lossy().into_owned() }, true));
    }
    let mut data = Vec::new();
    File::open(&path)?.take(STORE_LIMIT + 1).read_to_end(&mut data)?;
    if data.len() as u64 > STORE_LIMIT { return Err(CoreError::CorruptStore); }
    let mut value: Value = serde_json::from_slice(&data).map_err(|_| CoreError::CorruptStore)?;
    let version = match value.get("schemaVersion") {
        None => 0,
        Some(version) => version.as_u64().ok_or(CoreError::CorruptStore)?,
    };
    if version > 2 { return Err(CoreError::FutureSchema); }
    if version == 0 { migrate_v0(&mut value)?; }
    if version <= 1 { migrate_v1(&mut value)?; }
    let mut state: Snapshot = serde_json::from_value(value).map_err(|_| CoreError::CorruptStore)?;
    validate(&mut state, root).map_err(|_| CoreError::CorruptStore)?;
    if version < 2 {
        let backup = filesystem::checked_path(root, Path::new(&format!("state.v{version}.backup.json")))?;
        // Preserve the original schema before writing the migrated store; never clobber a backup.
        if !backup.exists() {
            let mut file = OpenOptions::new().create_new(true).write(true).open(backup)?;
            file.write_all(&data)?;
            file.sync_all()?;
        }
    }
    Ok((state, version < 2))
}

fn migrate_v0(value: &mut Value) -> Result<()> {
    let object = value.as_object_mut().ok_or(CoreError::CorruptStore)?;
    object.insert("schemaVersion".into(), json!(1));
    let previous_selection = object.remove("selectedInstanceId").unwrap_or(Value::Null);
    object.entry("activeInstanceId").or_insert(previous_selection);
    object.entry("trash").or_insert(json!([]));
    object.entry("settings").or_insert(json!({}));
    object.entry("dataDirectory").or_insert(json!(""));
    for list in ["instances", "trash"] {
        let items = object.get_mut(list).and_then(Value::as_array_mut).ok_or(CoreError::CorruptStore)?;
        for value in items {
            let item = value.as_object_mut().ok_or(CoreError::CorruptStore)?;
            for (key, default) in [
                ("loader", json!("vanilla")), ("loaderVersion", Value::Null), ("gameDirectory", json!("")),
                ("icon", Value::Null), ("createdAt", json!(models::now())), ("lastPlayed", Value::Null),
                ("totalPlaytimeSeconds", json!(0)), ("javaPath", Value::Null), ("ramMb", json!(4096)),
                ("jvmArgs", json!([])), ("resolution", json!({})), ("installationState", json!("not_installed")),
                ("mods", json!([])), ("resourcePacks", json!([])), ("shaderPacks", json!([])),
            ] { item.entry(key).or_insert(default); }
        }
    }
    Ok(())
}

fn migrate_v1(value: &mut Value) -> Result<()> {
    let object = value.as_object_mut().ok_or(CoreError::CorruptStore)?;
    object.insert("schemaVersion".into(), json!(2));
    object.entry("profiles").or_insert(json!([]));
    object.entry("activeProfileId").or_insert(Value::Null);
    Ok(())
}

fn validate(state: &mut Snapshot, root: &Path) -> Result<()> {
    state.settings.validate()?;
    if state.instances.len() + state.trash.len() > 10_000 { return Err(CoreError::LimitExceeded); }
    let mut ids = HashSet::new();
    for (items, directory) in [(&mut state.instances, "instances"), (&mut state.trash, "trash")] {
        for item in items {
            models::validate_id(&item.id)?;
            if !ids.insert(item.id.clone()) { return Err(CoreError::CorruptStore); }
            item.input().validate()?;
            if !["not_installed", "installed", "corrupted"].contains(&item.installation_state.as_str()) {
                return Err(CoreError::CorruptStore);
            }
            if chrono::DateTime::parse_from_rfc3339(&item.created_at).is_err() || item.last_played.as_ref().is_some_and(|date| chrono::DateTime::parse_from_rfc3339(date).is_err()) {
                return Err(CoreError::CorruptStore);
            }
            for records in [&item.mods, &item.resource_packs, &item.shader_packs] {
                if records.len() > 10_000 { return Err(CoreError::LimitExceeded); }
                let mut projects = HashSet::new();
                for record in records {
                    record.validate()?;
                    if !projects.insert(&record.project_id) { return Err(CoreError::CorruptStore); }
                }
            }
            // Storage paths are derived only from a validated UUID, never from serialized metadata.
            let path = filesystem::checked_path(root, &Path::new(directory).join(&item.id).join("game"))?;
            item.game_directory = path.to_string_lossy().into_owned();
            item.icon = None;
        }
    }
    if state.active_instance_id.as_ref().is_some_and(|id| !state.instances.iter().any(|item| &item.id == id)) {
        return Err(CoreError::CorruptStore);
    }
    if state.profiles.len() > 100 { return Err(CoreError::LimitExceeded); }
    let mut profile_ids = HashSet::new();
    let mut offline_usernames = HashSet::new();
    for profile in &state.profiles {
        profile.validate()?;
        if !profile_ids.insert(profile.id.clone()) { return Err(CoreError::CorruptStore); }
        if profile.kind == "offline" && !offline_usernames.insert(profile.username.to_ascii_lowercase()) { return Err(CoreError::CorruptStore); }
    }
    if state.active_profile_id.as_ref().is_some_and(|id| !state.profiles.iter().any(|profile| &profile.id == id)) { return Err(CoreError::CorruptStore); }
    state.data_directory = root.to_string_lossy().into_owned();
    Ok(())
}

pub(crate) fn save(root: &Path, snapshot: &Snapshot) -> Result<()> {
    let path = filesystem::checked_path(root, Path::new("state.json"))?;
    let bytes = serde_json::to_vec_pretty(snapshot).map_err(|_| CoreError::CorruptStore)?;
    if bytes.len() as u64 > STORE_LIMIT { return Err(CoreError::LimitExceeded); }
    let mut temporary = tempfile::Builder::new().prefix(".state-").tempfile_in(root)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| CoreError::Io(error.error))?;
    // sync after rename makes the directory entry durable on Unix. A late sync failure
    // cannot safely roll back the already-committed JSON, so it is best effort.
    #[cfg(unix)]
    if let Ok(directory) = File::open(root) { let _ = directory.sync_all(); }
    Ok(())
}

pub(crate) fn append_log(root: &Path, entry: LogEntry) -> Result<()> {
    let directory = filesystem::checked_path(root, Path::new("logs"))?;
    let current = filesystem::checked_path(&directory, Path::new("launcher.jsonl"))?;
    let bytes = serde_json::to_vec(&entry).map_err(|_| CoreError::CorruptStore)?;
    if current.exists() && fs::metadata(&current)?.len() + bytes.len() as u64 + 1 > LOG_LIMIT {
        let oldest = filesystem::checked_path(&directory, Path::new(&format!("launcher.{LOG_BACKUPS}.jsonl")))?;
        if oldest.exists() { fs::remove_file(oldest)?; }
        for number in (1..LOG_BACKUPS).rev() {
            let from = filesystem::checked_path(&directory, Path::new(&format!("launcher.{number}.jsonl")))?;
            let to = filesystem::checked_path(&directory, Path::new(&format!("launcher.{}.jsonl", number + 1)))?;
            if from.exists() { fs::rename(from, to)?; }
        }
        let next = filesystem::checked_path(&directory, Path::new("launcher.1.jsonl"))?;
        fs::rename(&current, next)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(current)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub(crate) fn read_logs(root: &Path) -> Result<Vec<LogEntry>> {
    let directory = filesystem::checked_path(root, Path::new("logs"))?;
    let mut entries = std::collections::VecDeque::with_capacity(1000);
    for number in (0..=LOG_BACKUPS).rev() {
        let name = if number == 0 { "launcher.jsonl".to_owned() } else { format!("launcher.{number}.jsonl") };
        let path = filesystem::checked_path(&directory, Path::new(&name))?;
        if !path.exists() { continue; }
        for line in BufReader::new(File::open(path)?.take(LOG_LIMIT + 4096)).lines() {
            let line = line?;
            if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
                if entries.len() == 1000 { entries.pop_front(); }
                entries.push_back(entry);
            }
        }
    }
    Ok(entries.into())
}
