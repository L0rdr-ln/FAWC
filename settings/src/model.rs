//! What each control of the settings app changes in the configuration file. No GTK in here, so
//! it is tested without a display.

use fde::config::Setting;

/// "#rrggbb" from 0..1 colour channels.
pub fn hex_color(r: f32, g: f32, b: f32) -> String {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
}

/// A slider's value rounded to two decimals, so the file stays readable (0.38, not 0.3800000011).
pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn appearance(accent: &str, transparency: f64, blur: u32, gloss: f64, animations: bool) -> Vec<(&'static str, Setting)> {
    vec![
        ("appearance.animations", Setting::Bool(animations)),
        ("appearance.accent", Setting::Str(accent.to_string())),
        ("appearance.transparency", Setting::Float(round2(transparency))),
        ("appearance.blur", Setting::Int(blur as i64)),
        ("appearance.gloss", Setting::Float(round2(gloss))),
    ]
}

pub fn wobbly(on: bool) -> Vec<(&'static str, Setting)> {
    vec![("appearance.wobbly_windows", Setting::Bool(on))]
}

pub fn keyboard(layout: &str, repeat_rate: u32, repeat_delay: u32) -> Vec<(&'static str, Setting)> {
    vec![
        ("input.keyboard_layout", Setting::Str(layout.trim().to_string())),
        ("input.repeat_rate", Setting::Int(repeat_rate as i64)),
        ("input.repeat_delay", Setting::Int(repeat_delay as i64)),
    ]
}

pub fn shell(terminal: &str, launcher: &str, panel: bool) -> Vec<(&'static str, Setting)> {
    vec![
        ("shell.terminal", Setting::Str(terminal.trim().to_string())),
        ("shell.launcher", Setting::Str(launcher.trim().to_string())),
        ("shell.panel", Setting::Bool(panel)),
    ]
}

pub fn wallpaper(path: &str, mode: &str) -> Vec<(&'static str, Setting)> {
    vec![("wallpaper.path", Setting::Str(path.trim().to_string())), ("wallpaper.mode", Setting::Str(mode.to_string()))]
}

pub fn music(enabled: bool, path: &str, volume: f64, shuffle: bool) -> Vec<(&'static str, Setting)> {
    vec![
        ("music.enabled", Setting::Bool(enabled)),
        ("music.path", Setting::Str(path.trim().to_string())),
        ("music.volume", Setting::Float(round2(volume))),
        ("music.shuffle", Setting::Bool(shuffle)),
    ]
}

pub fn session(idle_lock_minutes: u32) -> Vec<(&'static str, Setting)> {
    vec![("session.idle_lock_seconds", Setting::Int(idle_lock_minutes as i64 * 60))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use fde::config::{set, Config};

    #[test]
    fn every_page_writes_a_file_the_compositor_accepts_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("fde.toml");
        set(&f, &appearance(&hex_color(1.0, 0.5, 0.0), 0.61234, 4, 0.5, false)).unwrap();
        set(&f, &keyboard(" de ", 30, 400)).unwrap();
        set(&f, &shell("alacritty", "", false)).unwrap();
        set(&f, &session(10)).unwrap();
        set(&f, &music(true, " /home/me/Music ", 0.456, true)).unwrap();
        set(&f, &wallpaper(" /tmp/a.png ", "tile")).unwrap();
        let c = Config::parse(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(c.appearance.accent, "#ff8000");
        assert_eq!((c.appearance.transparency, c.appearance.blur, c.appearance.gloss, c.appearance.animations), (0.61, 4, 0.5, false));
        assert_eq!((c.input.keyboard_layout.as_str(), c.input.repeat_rate, c.input.repeat_delay), ("de", 30, 400));
        assert_eq!((c.shell.terminal.as_str(), c.shell.panel), ("alacritty", false));
        assert_eq!(c.session.idle_lock_seconds, 600);
        assert_eq!((c.music.enabled, c.music.path.as_str(), c.music.volume, c.music.shuffle), (true, "/home/me/Music", 0.46, true));
        assert!(set(&f, &music(true, "", 1.5, false)).is_err(), "a volume above 1 is refused");
        assert_eq!((c.wallpaper.path.as_str(), c.wallpaper.mode.as_str()), ("/tmp/a.png", "tile"));
        // a mode the compositor does not know is refused
        assert!(set(&f, &wallpaper("", "wobble")).is_err());
    }

    #[test]
    fn colours_are_clamped_and_rounded() {
        assert_eq!(hex_color(-1.0, 0.0, 2.0), "#0000ff");
        assert_eq!(hex_color(0.29, 0.66, 0.93), "#4aa8ed");
        assert_eq!(round2(0.3800000011), 0.38);
    }
}
