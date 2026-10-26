//! Small status readouts for the task bar: volume (PipeWire or PulseAudio) and battery. Plain
//! commands and files, nothing resident: the bar looks every couple of seconds.

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume {
    pub percent: u32,
    pub muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Battery {
    pub percent: u32,
    pub charging: bool,
}

/// "Volume: 0.45" or "Volume: 0.45 [MUTED]", what `wpctl get-volume` prints.
pub fn parse_wpctl(out: &str) -> Option<Volume> {
    let rest = out.trim().strip_prefix("Volume:")?.trim();
    let value: f64 = rest.split_whitespace().next()?.parse().ok()?;
    Some(Volume { percent: (value * 100.0).round().max(0.0) as u32, muted: rest.contains("[MUTED]") })
}

/// The first percentage of `pactl get-sink-volume` ("Volume: front-left: 29491 /  45% / ...") and
/// the answer of `pactl get-sink-mute` ("Mute: yes").
pub fn parse_pactl(volume: &str, mute: &str) -> Option<Volume> {
    let percent = volume.split('/').find_map(|part| part.trim().strip_suffix('%')?.trim().parse::<u32>().ok())?;
    Some(Volume { percent, muted: mute.trim().ends_with("yes") })
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    // never wait long on a sound server that does not answer: the task bar draws on this thread
    let out = Command::new("timeout").arg("1").arg(program).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The volume of the default output, if a sound server answers.
pub fn volume() -> Option<Volume> {
    if let Some(v) = output("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]).and_then(|o| parse_wpctl(&o)) {
        return Some(v);
    }
    let vol = output("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])?;
    let mute = output("pactl", &["get-sink-mute", "@DEFAULT_SINK@"]).unwrap_or_default();
    parse_pactl(&vol, &mute)
}

/// Make the volume `delta` percent louder (or quieter, never above 100 %).
pub fn change_volume(delta: i32) {
    let arg = format!("{}%{}", delta.abs(), if delta >= 0 { "+" } else { "-" });
    if Command::new("wpctl").args(["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", &arg]).status().is_err() {
        let _ = Command::new("pactl").args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{}{}%", if delta >= 0 { "+" } else { "-" }, delta.abs())]).status();
    }
}

/// Set the volume of the default output to `percent` (0 .. 100).
pub fn set_volume(percent: u32) {
    let value = format!("{:.2}", percent.min(100) as f64 / 100.0);
    if Command::new("wpctl").args(["set-volume", "@DEFAULT_AUDIO_SINK@", &value]).status().is_err() {
        let _ = Command::new("pactl").args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", percent.min(100))]).status();
    }
}

/// Mute or unmute the default output.
pub fn set_mute(muted: bool) {
    let v = if muted { "1" } else { "0" };
    if Command::new("wpctl").args(["set-mute", "@DEFAULT_AUDIO_SINK@", v]).status().is_err() {
        let _ = Command::new("pactl").args(["set-sink-mute", "@DEFAULT_SINK@", v]).status();
    }
}

pub fn toggle_mute() {
    if Command::new("wpctl").args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]).status().is_err() {
        let _ = Command::new("pactl").args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"]).status();
    }
}

/// The first battery under `root` (`/sys/class/power_supply`), if the machine has one.
pub fn battery_in(root: &std::path::Path) -> Option<Battery> {
    let mut dirs: Vec<_> = std::fs::read_dir(root).ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        let kind = std::fs::read_to_string(d.join("type")).unwrap_or_default();
        if kind.trim() != "Battery" {
            continue;
        }
        let percent: u32 = std::fs::read_to_string(d.join("capacity")).ok()?.trim().parse().ok()?;
        let status = std::fs::read_to_string(d.join("status")).unwrap_or_default();
        return Some(Battery { percent: percent.min(100), charging: matches!(status.trim(), "Charging" | "Full") });
    }
    None
}

pub fn battery() -> Option<Battery> {
    battery_in(std::path::Path::new("/sys/class/power_supply"))
}

/// What a status label looks like.
pub fn volume_label(v: Volume) -> String {
    if v.muted {
        "Vol muted".to_string()
    } else {
        format!("Vol {}%", v.percent)
    }
}

pub fn battery_label(b: Battery) -> String {
    format!("Bat {}%{}", b.percent, if b.charging { "+" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_are_read_from_wpctl_and_pactl_output() {
        assert_eq!(parse_wpctl("Volume: 0.45\n"), Some(Volume { percent: 45, muted: false }));
        assert_eq!(parse_wpctl("Volume: 1.00 [MUTED]\n"), Some(Volume { percent: 100, muted: true }));
        assert_eq!(parse_wpctl("nonsense"), None);
        let v = "Volume: front-left: 29491 /  45% / -20.00 dB,   front-right: 29491 /  45% / -20.00 dB\n";
        assert_eq!(parse_pactl(v, "Mute: no\n"), Some(Volume { percent: 45, muted: false }));
        assert_eq!(parse_pactl(v, "Mute: yes\n").unwrap().muted, true);
        assert_eq!(volume_label(Volume { percent: 45, muted: false }), "Vol 45%");
        assert_eq!(volume_label(Volume { percent: 45, muted: true }), "Vol muted");
    }

    #[test]
    fn the_battery_comes_from_the_first_battery_directory() {
        let dir = tempfile::tempdir().unwrap();
        let ac = dir.path().join("AC");
        let bat = dir.path().join("BAT0");
        std::fs::create_dir_all(&ac).unwrap();
        std::fs::create_dir_all(&bat).unwrap();
        std::fs::write(ac.join("type"), "Mains\n").unwrap();
        std::fs::write(bat.join("type"), "Battery\n").unwrap();
        std::fs::write(bat.join("capacity"), "87\n").unwrap();
        std::fs::write(bat.join("status"), "Charging\n").unwrap();
        assert_eq!(battery_in(dir.path()), Some(Battery { percent: 87, charging: true }));
        assert_eq!(battery_label(battery_in(dir.path()).unwrap()), "Bat 87%+");
        std::fs::remove_dir_all(&bat).unwrap();
        assert_eq!(battery_in(dir.path()), None, "a desktop machine has none");
    }

    #[test]
    fn volume_changes_run_wpctl_with_a_cap_at_100_percent() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let script = dir.path().join("wpctl");
        std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" >> {}\n", log.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:{}", dir.path().display(), std::env::var("PATH").unwrap_or_default());
        std::env::set_var("PATH", path);
        change_volume(5);
        change_volume(-5);
        toggle_mute();
        set_volume(35);
        set_volume(250);
        set_mute(true);
        let text = std::fs::read_to_string(&log).unwrap();
        assert_eq!(text, "set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+\nset-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%-\nset-mute @DEFAULT_AUDIO_SINK@ toggle\nset-volume @DEFAULT_AUDIO_SINK@ 0.35\nset-volume @DEFAULT_AUDIO_SINK@ 1.00\nset-mute @DEFAULT_AUDIO_SINK@ 1\n");
    }
}
