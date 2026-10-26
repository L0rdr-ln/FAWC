//! The programs that make the session a desktop: started by the compositor once its socket exists.

use std::process::Command;

/// Start the task bar (`fawc-panel`, installed next to the compositor). `FAWC_NO_PANEL=1` skips
/// it; `FAWC_AUTOSTART` may name more programs to start (a shell command line).
pub fn start_session_clients(config: &crate::config::Config) {
    if std::env::var_os("FAWC_NO_PANEL").is_none() && config.shell.panel {
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
    let extra = std::env::var("FAWC_AUTOSTART").ok();
    for cmd in config.shell.autostart.iter().chain(extra.iter()) {
        if let Err(e) = Command::new("sh").arg("-c").arg(cmd).spawn() {
            tracing::warn!("cannot run autostart command {cmd}: {e}");
        }
    }
}

/// Start a program that lives next to the compositor (`fawc-lock`).
pub fn start_sibling(name: &str) {
    let path = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join(name)));
    match path {
        Some(p) if p.exists() => {
            if let Err(e) = Command::new(&p).spawn() {
                tracing::warn!("cannot start {}: {e}", p.display());
            }
        }
        _ => tracing::warn!("{name} is not installed next to the compositor"),
    }
}
