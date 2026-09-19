use crate::{CoreError, Result};
use std::{fs::{self, File, OpenOptions}, io::{self, Read}, path::{Component, Path, PathBuf}};

pub(crate) const MAX_BYTES: u64 = 20 * 1024 * 1024 * 1024;
pub(crate) const MAX_FILES: usize = 100_000;
const MAX_DEPTH: usize = 48;

/// Treat every path read from a store/archive as untrusted. Never follow symlinks,
/// junctions, device files, pipes, or absolute/parent-relative archive names.
pub(crate) fn reject_link_or_special(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
                return Err(CoreError::UnsafePath);
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 { return Err(CoreError::UnsafePath); }
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn checked_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    reject_link_or_special(&path)?;
    for component in relative.components() {
        let Component::Normal(name) = component else { return Err(CoreError::UnsafePath); };
        path.push(name);
        reject_link_or_special(&path)?;
    }
    Ok(path)
}

pub(crate) fn ensure_directory(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    reject_link_or_special(&path)?;
    for component in relative.components() {
        let Component::Normal(name) = component else { return Err(CoreError::UnsafePath); };
        path.push(name);
        reject_link_or_special(&path)?;
        if !path.exists() { fs::create_dir(&path)?; }
        if !fs::symlink_metadata(&path)?.is_dir() { return Err(CoreError::UnsafePath); }
    }
    Ok(path)
}

pub(crate) fn check_external_path(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir)) {
        return Err(CoreError::UnsafePath);
    }
    for ancestor in path.ancestors() { reject_link_or_special(ancestor)?; }
    Ok(())
}

pub(crate) fn create_game_directories(instance_root: &Path) -> Result<()> {
    for relative in ["game", "game/mods", "game/resourcepacks", "game/shaderpacks", "game/screenshots", "game/saves", "game/logs"] {
        ensure_directory(instance_root, Path::new(relative))?;
    }
    Ok(())
}

pub(crate) struct Stage(tempfile::TempDir);
impl Stage {
    pub(crate) fn new(root: &Path) -> Result<Self> {
        let path = ensure_directory(root, Path::new(".staging"))?;
        Ok(Self(tempfile::Builder::new().prefix("instance-").tempdir_in(path)?))
    }
    pub(crate) fn path(&self) -> &Path { self.0.path() }
}

#[derive(Default)]
struct Budget { bytes: u64, entries: usize }
impl Budget {
    fn add(&mut self, bytes: u64) -> Result<()> {
        self.entries += 1;
        self.bytes = self.bytes.checked_add(bytes).ok_or(CoreError::LimitExceeded)?;
        if self.entries > MAX_FILES || self.bytes > MAX_BYTES { return Err(CoreError::LimitExceeded); }
        Ok(())
    }
}

pub(crate) fn tree_size(root: &Path) -> Result<u64> {
    let mut budget = Budget::default();
    walk(root, 0, &mut budget, &mut |_, _| Ok(()))?;
    Ok(budget.bytes)
}

pub(crate) fn verify_tree(root: &Path) -> Result<()> { tree_size(root).map(|_| ()) }

fn walk(root: &Path, depth: usize, budget: &mut Budget, visit: &mut impl FnMut(&Path, bool) -> Result<()>) -> Result<()> {
    if depth > MAX_DEPTH { return Err(CoreError::LimitExceeded); }
    reject_link_or_special(root)?;
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() { return Err(CoreError::UnsafePath); }
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        reject_link_or_special(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        budget.add(if metadata.is_file() { metadata.len() } else { 0 })?;
        visit(&path, metadata.is_dir())?;
        if metadata.is_dir() { walk(&path, depth + 1, budget, visit)?; }
    }
    Ok(())
}

pub(crate) fn visit_tree(root: &Path, mut visit: impl FnMut(&Path, bool) -> Result<()>) -> Result<()> {
    walk(root, 0, &mut Budget::default(), &mut visit)
}

pub(crate) fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let mut copied = 0u64;
    visit_tree(source, |path, directory| {
        let relative = path.strip_prefix(source).map_err(|_| CoreError::UnsafePath)?;
        let target = checked_path(destination, relative)?;
        if directory {
            fs::create_dir(&target)?;
        } else {
            let source_size = fs::symlink_metadata(path)?.len();
            let mut reader = File::open(path)?.take(source_size.saturating_add(1));
            let mut writer = OpenOptions::new().create_new(true).write(true).open(target)?;
            let size = io::copy(&mut reader, &mut writer)?;
            copied = copied.checked_add(size).ok_or(CoreError::LimitExceeded)?;
            if size != source_size || copied > MAX_BYTES { return Err(CoreError::LimitExceeded); }
            writer.sync_all()?;
        }
        Ok(())
    })
}
