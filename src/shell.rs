//! The programs that make the session a desktop: started by the compositor once its socket exists.

use std::process::Command;

/// Start the task bar (`fawc-panel`, installed next to the compositor). `FAWC_NO_PANEL=1` skips
/// it; `FAWC_AUTOSTART` may name more programs to start (a shell command line).
pub fn start_session_clients() {
    if std::env::var_os("FAWC_NO_PANEL").is_none() {
        let panel = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("fawc-panel")))
            .filter(|p| p.exists());
        match panel {
            Some(p) => {
                if let Err(e) = Command::new(&p).spawn() {
                    tracing::warn!("cannot start {}: {e}", p.display());
                }
            }
            None => tracing::info!("no fawc-panel next to the compositor: starting without a task bar"),
        }
    }
    if let Ok(cmd) = std::env::var("FAWC_AUTOSTART") {
        if let Err(e) = Command::new("sh").arg("-c").arg(&cmd).spawn() {
            tracing::warn!("cannot run FAWC_AUTOSTART: {e}");
        }
    }
}
