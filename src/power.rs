//! Session and power actions of the start menu: lock, log out, sleep, restart, shut down.

use std::process::Command;

/// One button of the power row of the start menu.
pub struct PowerAction {
    pub id: &'static str,
    pub label: &'static str,
    /// the first click only asks "sure?"; the second does it
    pub confirm: bool,
}

pub const ACTIONS: [PowerAction; 5] = [
    PowerAction { id: "lock", label: "Lock", confirm: false },
    PowerAction { id: "logout", label: "Log out", confirm: true },
    PowerAction { id: "sleep", label: "Sleep", confirm: false },
    PowerAction { id: "restart", label: "Restart", confirm: true },
    PowerAction { id: "shutdown", label: "Shut down", confirm: true },
];

/// Do what `id` says. With `FDE_POWER_LOG=file` the id is appended to the file instead (tests, and
/// trying the menu without really shutting the machine down).
pub fn run(id: &str) -> Result<(), String> {
    if let Some(log) = std::env::var_os("FDE_POWER_LOG") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&log).map_err(|e| e.to_string())?;
        return writeln!(f, "{id}").map_err(|e| e.to_string());
    }
    match id {
        "lock" => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let lock = exe.parent().map(|d| d.join("fde-lock")).filter(|p| p.exists()).ok_or("fde-lock is not installed")?;
            Command::new(lock).spawn().map(|_| ()).map_err(|e| e.to_string())
        }
        "logout" => {
            // the compositor ends the session when it is asked to terminate
            let pid = std::env::var("FDE_PID").ok().and_then(|p| p.parse::<i32>().ok()).unwrap_or_else(|| unsafe { libc::getppid() });
            if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
                Ok(())
            } else {
                Err(format!("cannot stop the session (pid {pid})"))
            }
        }
        "sleep" => systemctl("suspend"),
        "restart" => systemctl("reboot"),
        "shutdown" => systemctl("poweroff"),
        other => Err(format!("unknown power action {other}")),
    }
}

fn systemctl(verb: &str) -> Result<(), String> {
    Command::new("systemctl").arg(verb).spawn().map(|_| ()).map_err(|e| format!("systemctl {verb}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_file_records_what_was_asked() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("power.log");
        std::env::set_var("FDE_POWER_LOG", &log);
        run("shutdown").unwrap();
        run("sleep").unwrap();
        std::env::remove_var("FDE_POWER_LOG");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "shutdown\nsleep\n");
        assert!(run("explode").is_err());
        // only the destructive ones ask twice
        let ask: Vec<&str> = ACTIONS.iter().filter(|a| a.confirm).map(|a| a.id).collect();
        assert_eq!(ask, ["logout", "restart", "shutdown"]);
    }
}
