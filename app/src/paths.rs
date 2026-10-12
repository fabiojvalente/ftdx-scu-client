//! Cross-platform location for persisted application data.
//!
//! The desktop build previously assumed `$HOME`, which is unset on Windows, so
//! settings were silently never saved. Resolve the conventional per-user config
//! directory on each platform instead.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// Directory where the desktop build keeps `config.toml`, `settings.toml`, and
/// the layout files. Returns `None` if no suitable base directory can be found.
#[cfg(not(target_arch = "wasm32"))]
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Some(PathBuf::from(appdata).join("scu-client"));
        }
        // Fallback for stripped environments where APPDATA is unset.
        let profile = std::env::var_os("USERPROFILE")?;
        Some(PathBuf::from(profile).join(".config").join("scu-client"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(xdg).join("scu-client"));
        }
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join(".config").join("scu-client"))
    }
}
