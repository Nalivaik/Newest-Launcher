//! Conversion for paths passed to third-party desktop processes.
//!
//! `std::fs::canonicalize` returns `\\?\` verbatim paths on Windows. Rust's filesystem API
//! supports them, but the Java launcher and class loader do not consistently accept that form
//! for `-jar`, `-cp`, or `-D` path values. Keep verbatim paths for our own filesystem operations
//! and convert only at the process boundary.

use std::path::{Path, PathBuf};

pub(crate) fn command_path(path: &Path) -> PathBuf {
    PathBuf::from(command_path_string(path))
}

pub(crate) fn command_path_string(path: &Path) -> String {
    strip_windows_verbatim_prefix(&path.to_string_lossy())
}

fn strip_windows_verbatim_prefix(value: &str) -> String {
    const VERBATIM: &str = "\\\\?\\";
    const VERBATIM_UNC: &str = "\\\\?\\UNC\\";
    if value.len() >= VERBATIM_UNC.len() && value[..VERBATIM_UNC.len()].eq_ignore_ascii_case(VERBATIM_UNC) {
        return format!("\\\\{}", &value[VERBATIM_UNC.len()..]);
    }
    value.strip_prefix(VERBATIM).unwrap_or(value).to_owned()
}

#[cfg(test)]
mod tests {
    use super::strip_windows_verbatim_prefix;

    #[test]
    fn strips_verbatim_prefix_only_for_external_windows_paths() {
        assert_eq!(strip_windows_verbatim_prefix(r"\\?\C:\Users\Alex\runtime"), r"C:\Users\Alex\runtime");
        assert_eq!(strip_windows_verbatim_prefix(r"\\?\UNC\server\share\runtime"), r"\\server\share\runtime");
        assert_eq!(strip_windows_verbatim_prefix("/home/alex/runtime"), "/home/alex/runtime");
    }
}
