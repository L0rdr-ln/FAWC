//! The programs that make the session a desktop: started by the compositor once its socket exists.

use std::process::Command;

/// Start a task bar (`fawc-panel`, installed next to the compositor) for the monitor called
/// `output`. `FAWC_NO_PANEL=1` or `shell.panel = false` skips it.
pub fn start_panel(config: &crate::config::Config, output: &str) {
    if std::env::var_os("FAWC_NO_PANEL").is_some() || !config.shell.panel {
        return;
    }
    let panel = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("fawc-panel")))
        .filter(|p| p.exists());
    match panel {
        Some(p) => {
            if let Err(e) = Command::new(&p).env("FAWC_PANEL_OUTPUT", output).spawn() {
                tracing::warn!("cannot start {}: {e}", p.display());
            }
        }
        None => tracing::info!("no fawc-panel next to the compositor: starting without a task bar"),
    }
}

/// The rest of the session: the notification daemon and the autostart programs.
/// `FAWC_NO_NOTIFY=1` skips the daemon; `FAWC_AUTOSTART` may name one more program.
pub fn start_other_clients(config: &crate::config::Config) {
    // the notification daemon needs the session bus
    if std::env::var_os("FAWC_NO_NOTIFY").is_none() && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        start_sibling("fawc-notify");
    }
    // background music, if switched on
    if config.music.enabled && std::env::var_os("FAWC_NO_MUSIC").is_none() {
        start_sibling("fawc-music");
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
