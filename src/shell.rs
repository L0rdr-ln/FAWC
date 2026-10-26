//! The programs that make the session a desktop: started by the compositor once its socket exists.

use std::process::Command;

/// Start a task bar (`fde-panel`, installed next to the compositor) for the monitor called
/// `output`. `FDE_NO_PANEL=1` or `shell.panel = false` skips it.
pub fn start_panel(config: &crate::config::Config, output: &str) {
    if std::env::var_os("FDE_NO_PANEL").is_some() || !config.shell.panel {
        return;
    }
    let panel = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("fde-panel")))
        .filter(|p| p.exists());
    match panel {
        Some(p) => {
            if let Err(e) = Command::new(&p).env("FDE_PANEL_OUTPUT", output).spawn() {
                tracing::warn!("cannot start {}: {e}", p.display());
            }
        }
        None => tracing::info!("no fde-panel next to the compositor: starting without a task bar"),
    }
}

/// The rest of the session: the notification daemon and the autostart programs.
/// `FDE_NO_NOTIFY=1` skips the daemon; `FDE_AUTOSTART` may name one more program.
pub fn start_other_clients(config: &crate::config::Config) {
    // the notification daemon needs the session bus
    if std::env::var_os("FDE_NO_NOTIFY").is_none() && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        start_sibling("fde-notify");
    }
    // background music, if switched on
    if config.music.enabled && std::env::var_os("FDE_NO_MUSIC").is_none() {
        start_sibling("fde-music");
    }
    let extra = std::env::var("FDE_AUTOSTART").ok();
    for cmd in config.shell.autostart.iter().chain(extra.iter()) {
        if let Err(e) = Command::new("sh").arg("-c").arg(cmd).spawn() {
            tracing::warn!("cannot run autostart command {cmd}: {e}");
        }
    }
}

/// Start a program that lives next to the compositor (`fde-lock`).
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

static TERMINATE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_terminate(_: libc::c_int) {
    TERMINATE.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// End the session cleanly when the compositor is asked to terminate (the "Log out" button, a
/// `kill`, the console closing): the main loops look at `termination_requested` every frame, stop,
/// and give the graphics session back. Also tells the programs it starts which process to ask
/// (`FDE_PID`).
pub fn watch_signals() {
    std::env::set_var("FDE_PID", std::process::id().to_string());
    unsafe {
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(sig, on_terminate as extern "C" fn(libc::c_int) as libc::sighandler_t);
        }
    }
}

/// Has the compositor been asked to terminate?
pub fn termination_requested() -> bool {
    TERMINATE.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_terminate_signal_asks_the_session_to_end_instead_of_killing_it() {
        super::watch_signals();
        assert!(!super::termination_requested());
        unsafe { libc::raise(libc::SIGTERM) };
        assert!(super::termination_requested(), "the handler noted the request");
        assert_eq!(std::env::var("FDE_PID").unwrap(), std::process::id().to_string());
    }
}
