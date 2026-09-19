use crate::{filesystem, CoreError, Instance, InstanceInput, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs::{self, File, OpenOptions}, io::{self, Read, Write}, path::{Path, PathBuf}};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MANIFEST_LIMIT: u64 = 128 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveManifest { format: String, schema_version: u32, instance: InstanceInput }

pub(crate) fn export(root: &Path, instance: &Instance, source: &Path, destination: &Path) -> Result<()> {
    filesystem::check_external_path(destination)?;
    let parent = destination.parent().ok_or(CoreError::UnsafePath)?;
    let parent = fs::canonicalize(parent)?;
    // A selected export path must not overwrite store files or end up inside its own input tree.
    if parent.starts_with(root) && parent != root.join("exports") { return Err(CoreError::UnsafePath); }
    if destination.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase).as_deref() != Some("zip") {
        return Err(CoreError::Invalid("instance exports use the .zip extension"));
    }
    if destination.exists() { return Err(CoreError::Invalid("export destination already exists")); }
    filesystem::verify_tree(source)?;
    let mut temporary = tempfile::Builder::new().prefix(".newest-export-").tempfile_in(&parent)?;
    {
        let mut archive = ZipWriter::new(temporary.as_file_mut());
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).unix_permissions(0o600);
        let mut input = instance.input();
        // Executable paths and custom JVM switches are machine-specific and must not be
        // trusted when another user imports an archive.
        input.java_path = None;
        input.jvm_args.clear();
        let manifest = ArchiveManifest { format: "newest-launcher-instance".into(), schema_version: 1, instance: input };
        archive.start_file("instance.json", options)?;
        archive.write_all(&serde_json::to_vec_pretty(&manifest).map_err(|_| CoreError::InvalidArchive)?)?;
        let mut copied = 0u64;
        filesystem::visit_tree(&source.join("game"), |path, directory| {
            let relative = path.strip_prefix(source).map_err(|_| CoreError::UnsafePath)?;
            let portable = relative.components().map(|part| part.as_os_str().to_str().ok_or(CoreError::UnsafePath)).collect::<Result<Vec<_>>>()?.join("/");
            validate_member_name(&portable)?;
            if directory {
                archive.add_directory(format!("{portable}/"), options)?;
            } else {
                archive.start_file(portable, options)?;
                let expected = fs::symlink_metadata(path)?.len();
                let size = io::copy(&mut File::open(path)?.take(expected.saturating_add(1)), &mut archive)?;
                copied = copied.checked_add(size).ok_or(CoreError::LimitExceeded)?;
                if size != expected || copied > filesystem::MAX_BYTES { return Err(CoreError::LimitExceeded); }
            }
            Ok(())
        })?;
        archive.finish()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist_noclobber(destination).map_err(|error| CoreError::Io(error.error))?;
    Ok(())
}

pub(crate) fn import(source: &Path, stage: &Path) -> Result<InstanceInput> {
    filesystem::check_external_path(source)?;
    if fs::metadata(source)?.len() > filesystem::MAX_BYTES { return Err(CoreError::LimitExceeded); }
    let mut archive = ZipArchive::new(File::open(source)?)?;
    if archive.len() > filesystem::MAX_FILES { return Err(CoreError::LimitExceeded); }
    let mut total = 0u64;
    let mut names = HashSet::new();
    let mut manifest_index = None;
    // Validate every entry before writing anything, including entries not currently recognized.
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let path = validate_member_name(entry.name())?;
        let name = entry.name().trim_end_matches('/').to_ascii_lowercase();
        if !names.insert(name) { return Err(CoreError::InvalidArchive); }
        if let Some(mode) = entry.unix_mode() {
            let file_type = mode & 0o170000;
            if file_type != 0 && file_type != 0o100000 && file_type != 0o040000 { return Err(CoreError::UnsafePath); }
        }
        if entry.is_symlink() { return Err(CoreError::UnsafePath); }
        total = total.checked_add(entry.size()).ok_or(CoreError::LimitExceeded)?;
        if total > filesystem::MAX_BYTES { return Err(CoreError::LimitExceeded); }
        if path == Path::new("instance.json") && !entry.is_dir() {
            if entry.size() > MANIFEST_LIMIT { return Err(CoreError::LimitExceeded); }
            manifest_index = Some(index);
        } else if !path.starts_with("game") { return Err(CoreError::InvalidArchive); }
        else if path == Path::new("game") && !entry.is_dir() { return Err(CoreError::InvalidArchive); }
    }
    let mut manifest_bytes = Vec::new();
    archive.by_index(manifest_index.ok_or(CoreError::InvalidArchive)?)?.take(MANIFEST_LIMIT + 1).read_to_end(&mut manifest_bytes)?;
    if manifest_bytes.len() as u64 > MANIFEST_LIMIT { return Err(CoreError::LimitExceeded); }
    let mut manifest: ArchiveManifest = serde_json::from_slice(&manifest_bytes).map_err(|_| CoreError::InvalidArchive)?;
    if manifest.format != "newest-launcher-instance" || manifest.schema_version != 1 { return Err(CoreError::InvalidArchive); }
    manifest.instance.java_path = None;
    manifest.instance.jvm_args.clear();
    manifest.instance.validate()?;
    let mut extracted = 0u64;
    for index in 0..archive.len() {
        if Some(index) == manifest_index { continue; }
        let mut entry = archive.by_index(index)?;
        let relative = validate_member_name(entry.name())?;
        if entry.is_dir() {
            filesystem::ensure_directory(stage, &relative)?;
        } else {
            filesystem::ensure_directory(stage, relative.parent().ok_or(CoreError::UnsafePath)?)?;
            let destination = filesystem::checked_path(stage, &relative)?;
            let mut writer = OpenOptions::new().create_new(true).write(true).open(destination)?;
            let expected = entry.size();
            let written = io::copy(&mut (&mut entry).take(expected.saturating_add(1)), &mut writer)?;
            extracted = extracted.checked_add(written).ok_or(CoreError::LimitExceeded)?;
            if written != expected || extracted > filesystem::MAX_BYTES { return Err(CoreError::InvalidArchive); }
            writer.sync_all()?;
        }
    }
    Ok(manifest.instance)
}

fn validate_member_name(name: &str) -> Result<PathBuf> {
    if name.is_empty() || name.len() > 4096 || name.starts_with('/') || name.contains('\\') || name.contains(':') || name.chars().any(char::is_control) {
        return Err(CoreError::UnsafePath);
    }
    let normalized = name.strip_suffix('/').unwrap_or(name);
    let mut path = PathBuf::new();
    let components: Vec<_> = normalized.split('/').collect();
    if components.len() > 48 { return Err(CoreError::LimitExceeded); }
    for part in components {
        if part.is_empty() || part == "." || part == ".." || part.len() > 255 || part.ends_with('.') || part.ends_with(' ') {
            return Err(CoreError::UnsafePath);
        }
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit()) {
            return Err(CoreError::UnsafePath);
        }
        path.push(part);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_portable_archive_paths_are_rejected() {
        for path in ["../outside", "/etc/passwd", "game/../../outside", "game\\evil", "C:/evil", "game/NUL.txt", "game/a:stream", "game//evil", "game/file.", "game/./evil", "game/\0bad"] {
            assert!(validate_member_name(path).is_err(), "accepted {path:?}");
        }
        assert_eq!(validate_member_name("game/saves/Мой мир/level.dat").unwrap(), Path::new("game/saves/Мой мир/level.dat"));
    }
}
