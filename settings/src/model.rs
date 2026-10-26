//! What each control of the settings app changes in the configuration file. No GTK in here, so
//! it is tested without a display.

use fawc::config::Setting;

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

pub fn session(idle_lock_minutes: u32) -> Vec<(&'static str, Setting)> {
    vec![("session.idle_lock_seconds", Setting::Int(idle_lock_minutes as i64 * 60))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use fawc::config::{set, Config};

    #[test]
    fn every_page_writes_a_file_the_compositor_accepts_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("fawc.toml");
        set(&f, &appearance(&hex_color(1.0, 0.5, 0.0), 0.61234, 4, 0.5, false)).unwrap();
        set(&f, &keyboard(" de ", 30, 400)).unwrap();
        set(&f, &shell("alacritty", "", false)).unwrap();
        set(&f, &session(10)).unwrap();
        let c = Config::parse(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(c.appearance.accent, "#ff8000");
        assert_eq!((c.appearance.transparency, c.appearance.blur, c.appearance.gloss, c.appearance.animations), (0.61, 4, 0.5, false));
        assert_eq!((c.input.keyboard_layout.as_str(), c.input.repeat_rate, c.input.repeat_delay), ("de", 30, 400));
        assert_eq!((c.shell.terminal.as_str(), c.shell.panel), ("alacritty", false));
        assert_eq!(c.session.idle_lock_seconds, 600);
    }

    #[test]
    fn colours_are_clamped_and_rounded() {
        assert_eq!(hex_color(-1.0, 0.0, 2.0), "#0000ff");
        assert_eq!(hex_color(0.29, 0.66, 0.93), "#4aa8ed");
        assert_eq!(round2(0.3800000011), 0.38);
    }
}
