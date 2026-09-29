//! Runtime control of the tracing log level.
//!
//! Native builds install a reload handle into the `tracing` subscriber created
//! in `main`; changing the level in the UI then reconfigures it live. The web
//! build has no reloadable subscriber, so [`set_level`] is a no-op there.

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::sync::{Mutex, OnceLock};

    type ReloadFn = Box<dyn Fn(&str) + Send + Sync>;

    static RELOAD: OnceLock<Mutex<Option<ReloadFn>>> = OnceLock::new();

    fn slot() -> &'static Mutex<Option<ReloadFn>> {
        RELOAD.get_or_init(|| Mutex::new(None))
    }

    /// Store the subscriber's reload closure. Called once from `main`.
    pub fn install(reload: ReloadFn) {
        *slot().lock().unwrap() = Some(reload);
    }

    /// Apply a new filter (e.g. `"debug"`) to the active subscriber.
    pub fn set_level(level: &str) {
        if let Some(reload) = slot().lock().unwrap().as_ref() {
            reload(level);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use imp::{install, set_level};

/// No-op on the web, where the console logger is not reloadable.
#[cfg(target_arch = "wasm32")]
pub fn set_level(_level: &str) {}
