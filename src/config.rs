//! The one configuration file: `~/.config/fde/fde.toml`, with system defaults in
//! `/etc/fde/fde.toml` (the user's file wins, key by key). Missing keys mean defaults; a file
//! that does not parse is reported and the previous configuration stays.
//! See docs/SETTINGS.md for the plan this belongs to.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::Deserialize;

use crate::glass::GlassStyle;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// the look of the whole desktop: a theme name (`aero`, `metro`, `nordic`, `sakura`, `midnight` or a file in `themes/`)
    pub theme: String,
    /// accent colour of the glass, "#rrggbb"
    pub accent: String,
    /// how strongly the glass is tinted, 0 (clear) .. 1
    pub transparency: f32,
    /// blur levels 1..6: more is softer and costs more
    pub blur: u32,
    /// the gloss band on the title bar, 0 .. 1
    pub gloss: f32,
    pub animations: bool,
    /// windows bend and swing on springs while they are dragged
    pub wobbly_windows: bool,
    /// the mouse cursor theme (a directory with `cursors/` in an icon path); empty = `$XCURSOR_THEME`
    pub cursor_theme: String,
    /// cursor size in pixels at scale 1; 0 = `$XCURSOR_SIZE`, else 24
    pub cursor_size: u32,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance { theme: "aero".into(), accent: "#4aa8ee".into(), transparency: 0.38, blur: 3, gloss: 0.8, animations: true, wobbly_windows: true, cursor_theme: String::new(), cursor_size: 0 }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Wallpaper {
    /// a PNG, JPEG or WebP file; empty = the built-in sky
    pub path: String,
    /// fill | fit | stretch | center | tile
    pub mode: String,
}

impl Default for Wallpaper {
    fn default() -> Self {
        Wallpaper { path: String::new(), mode: "fill".into() }
    }
}

pub const WALLPAPER_MODES: [&str; 5] = ["fill", "fit", "stretch", "center", "tile"];

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Music {
    /// play background music with `fde-music`
    pub enabled: bool,
    /// a music file, or a folder of them
    pub path: String,
    /// 0 (silent) .. 1 (full)
    pub volume: f32,
    /// play the files in random order instead of by name
    pub shuffle: bool,
}

impl Default for Music {
    fn default() -> Self {
        Music { enabled: false, path: String::new(), volume: 0.5, shuffle: false }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Input {
    pub repeat_rate: i32,
    pub repeat_delay: i32,
    /// xkb layout, e.g. "us" or "de"; empty keeps the system default
    pub keyboard_layout: String,
}

impl Default for Input {
    fn default() -> Self {
        Input { repeat_rate: 25, repeat_delay: 600, keyboard_layout: String::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Shell {
    pub terminal: String,
    /// started by the start orb when no applications are installed
    pub launcher: String,
    /// start the task bar with the compositor
    pub panel: bool,
    /// programs (shell command lines) started with the session
    pub autostart: Vec<String>,
}

impl Default for Shell {
    fn default() -> Self {
        Shell { terminal: "foot".into(), launcher: String::new(), panel: true, autostart: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Session {
    /// lock after this many idle seconds; 0 = never
    pub idle_lock_seconds: u32,
}

/// One monitor, by connector name (`DP-1`, `HDMI-A-1`, `eDP-1`; `fde-0` for the nested window).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// how many physical pixels make one logical pixel: 1, 1.25, 1.5, 2 ...
    pub scale: f64,
    /// "WIDTHxHEIGHT" or "WIDTHxHEIGHT@HZ"; empty = the monitor's preferred mode
    pub mode: String,
    /// position of the top left corner in the shared logical space; both or none
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// false turns the monitor off
    pub enabled: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        OutputConfig { scale: 1.0, mode: String::new(), x: None, y: None, enabled: true }
    }
}

/// "1920x1080" or "1920x1080@60" as (width, height, refresh in Hz).
pub fn parse_mode(s: &str) -> Option<(i32, i32, Option<f64>)> {
    let (size, hz) = match s.split_once('@') {
        Some((a, b)) => (a, Some(b.parse::<f64>().ok()?)),
        None => (s, None),
    };
    let (w, h) = size.split_once('x')?;
    let (w, h): (i32, i32) = (w.parse().ok()?, h.parse().ok()?);
    (w > 0 && h > 0).then_some((w, h, hz))
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub outputs: std::collections::BTreeMap<String, OutputConfig>,
    pub appearance: Appearance,
    pub wallpaper: Wallpaper,
    pub music: Music,
    pub input: Input,
    pub shell: Shell,
    pub session: Session,
    /// which of accent, transparency, blur, gloss the user's files set (they win over the theme)
    #[serde(skip)]
    pub explicit: Vec<String>,
}

/// Where the files are: `$FDE_CONFIG` replaces both, else system then user.
pub fn paths() -> Vec<PathBuf> {
    if let Some(p) = std::env::var_os("FDE_CONFIG") {
        return vec![PathBuf::from(p)];
    }
    let mut v = vec![PathBuf::from("/etc/fde/fde.toml")];
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(b) = base {
        v.push(b.join("fde/fde.toml"));
    }
    v
}

/// `over` on top of `base`: tables merge key by key, everything else is replaced.
fn merge(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot) => merge(slot, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (slot, v) => *slot = v,
    }
}

impl Config {
    /// Parse one file's text, with the checks that serde cannot make.
    pub fn parse(text: &str) -> Result<Config, String> {
        let value: toml::Value = text.parse().map_err(|e| format!("{e}"))?;
        Config::from_value(value)
    }

    fn from_value(value: toml::Value) -> Result<Config, String> {
        let explicit: Vec<String> = value
            .get("appearance")
            .and_then(|a| a.as_table())
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default();
        let mut c: Config = value.try_into().map_err(|e| format!("{e}"))?;
        c.explicit = explicit;
        c.validate()?;
        Ok(c)
    }

    fn validate(&self) -> Result<(), String> {
        crate::theme::Theme::find(&self.appearance.theme).map_err(|e| format!("appearance.theme: {e}"))?;
        parse_color(&self.appearance.accent).ok_or_else(|| format!("appearance.accent: \"{}\" is not #rrggbb", self.appearance.accent))?;
        if !(0.0..=1.0).contains(&self.appearance.transparency) {
            return Err("appearance.transparency must be between 0 and 1".into());
        }
        if !(1..=6).contains(&self.appearance.blur) {
            return Err("appearance.blur must be between 1 and 6".into());
        }
        for (name, o) in &self.outputs {
            if !(0.5..=4.0).contains(&o.scale) {
                return Err(format!("outputs.{name}.scale must be between 0.5 and 4"));
            }
            if !o.mode.is_empty() && parse_mode(&o.mode).is_none() {
                return Err(format!("outputs.{name}.mode: \"{}\" is not WIDTHxHEIGHT or WIDTHxHEIGHT@HZ", o.mode));
            }
            if o.x.is_some() != o.y.is_some() {
                return Err(format!("outputs.{name}: x and y go together"));
            }
        }
        if !WALLPAPER_MODES.contains(&self.wallpaper.mode.as_str()) {
            return Err(format!("wallpaper.mode must be one of {}", WALLPAPER_MODES.join(", ")));
        }
        if !(0.0..=1.0).contains(&self.music.volume) {
            return Err("music.volume must be between 0 and 1".into());
        }
        if self.appearance.cursor_size > 256 {
            return Err("appearance.cursor_size must be at most 256".into());
        }
        if self.input.repeat_rate < 0 || self.input.repeat_delay < 0 {
            return Err("input.repeat_rate and input.repeat_delay must not be negative".into());
        }
        Ok(())
    }

    /// Read and merge the files that exist. Errors name the file; nothing partial is returned.
    pub fn load() -> Result<Config, String> {
        let mut merged = toml::Value::Table(Default::default());
        for p in paths() {
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let v: toml::Value = text.parse().map_err(|e| format!("{}: {e}", p.display()))?;
            merge(&mut merged, v);
        }
        Config::from_value(merged)
    }

    /// The theme in use (aero if it cannot be read; `validate` already complained).
    pub fn theme(&self) -> crate::theme::Theme {
        crate::theme::Theme::find(&self.appearance.theme).unwrap_or_default()
    }

    fn set_by_user(&self, key: &str) -> bool {
        self.explicit.iter().any(|k| k == key)
    }

    /// The accent, transparency, blur and gloss that are in effect: the user's own values where
    /// the files set them, else the theme's.
    pub fn look(&self) -> (String, f32, u32, f32) {
        let t = self.theme();
        let a = &self.appearance;
        (
            if self.set_by_user("accent") { a.accent.clone() } else { t.accent },
            if self.set_by_user("transparency") { a.transparency } else { t.transparency },
            if self.set_by_user("blur") { a.blur } else { t.blur },
            if self.set_by_user("gloss") { a.gloss } else { t.gloss },
        )
    }

    /// Put the theme and the user's look into a glass style.
    pub fn apply_style(&self, style: &mut GlassStyle) {
        let t = self.theme();
        let (accent, transparency, blur, gloss) = self.look();
        let [r, g, b] = parse_color(&accent).unwrap_or([0.29, 0.66, 0.93]);
        let col = |s: &str| parse_color(s).unwrap_or([1.0, 1.0, 1.0]);
        style.radius = t.radius;
        style.border = t.border;
        style.titlebar = t.titlebar;
        style.tint_frame = [r, g, b, transparency];
        style.tint_client = t.client_tint;
        style.gloss = gloss;
        style.highlight = t.highlight;
        style.reflections = t.reflections;
        style.glow = t.glow;
        style.shadow = t.shadow;
        style.rim = t.rim;
        style.flat_buttons = if t.buttons.style == "flat" { 1.0 } else { 0.0 };
        style.orb_size = t.buttons.size;
        style.orb_gap = t.buttons.gap;
        style.orb_margin = t.buttons.margin;
        style.orb_close = col(&t.buttons.close);
        style.orb_max = col(&t.buttons.maximize);
        style.orb_min = col(&t.buttons.minimize);
        style.title_ink = col(&t.title_ink);
        style.title_ink_dim = col(&t.title_ink_dim);
        style.title_glow = t.title_glow;
        style.blur_levels = blur as usize;
    }
}

pub fn parse_color(s: &str) -> Option<[f32; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|v| v as f32 / 255.0);
    Some([c(0)?, c(2)?, c(4)?])
}

/// Notices when a config file changed (modification times of every path, existing or not).
pub struct Watcher {
    last: Vec<Option<SystemTime>>,
}

impl Watcher {
    pub fn new() -> Watcher {
        Watcher { last: Self::stamps() }
    }

    fn stamps() -> Vec<Option<SystemTime>> {
        paths().iter().map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()).collect()
    }

    /// True once after every change.
    pub fn changed(&mut self) -> bool {
        let now = Self::stamps();
        if now != self.last {
            self.last = now;
            true
        } else {
            false
        }
    }
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_is_the_defaults_and_keys_are_optional() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
        let c = Config::parse("[appearance]\nblur = 5\n").unwrap();
        assert_eq!(c.appearance.blur, 5);
        assert_eq!(c.appearance.accent, "#4aa8ee");
        assert!(c.shell.panel);
    }

    #[test]
    fn bad_values_are_refused_with_a_reason() {
        assert!(Config::parse("[appearance]\naccent = \"blue\"\n").unwrap_err().contains("accent"));
        assert!(Config::parse("[appearance]\nblur = 0\n").unwrap_err().contains("blur"));
        assert!(Config::parse("[appearance]\ntransparency = 2.0\n").unwrap_err().contains("transparency"));
        assert!(Config::parse("[appearance\n").is_err());
        assert!(Config::parse("[appearance]\nblur = \"much\"\n").is_err());
    }

    #[test]
    fn the_user_file_wins_key_by_key() {
        let mut base: toml::Value = "[appearance]\nblur = 2\ngloss = 0.5\n[shell]\nterminal = \"xterm\"\n".parse().unwrap();
        merge(&mut base, "[appearance]\nblur = 4\n".parse().unwrap());
        let c = Config::from_value(base).unwrap();
        assert_eq!(c.appearance.blur, 4);
        assert_eq!(c.appearance.gloss, 0.5);
        assert_eq!(c.shell.terminal, "xterm");
    }

    #[test]
    fn outputs_are_configured_by_connector_name_and_checked() {
        let c = Config::parse("[outputs.\"DP-1\"]\nscale = 1.5\nmode = \"2560x1440@144\"\nx = 0\ny = 0\n[outputs.\"HDMI-A-1\"]\nenabled = false\n").unwrap();
        assert_eq!(c.outputs["DP-1"].scale, 1.5);
        assert_eq!(parse_mode(&c.outputs["DP-1"].mode), Some((2560, 1440, Some(144.0))));
        assert!(!c.outputs["HDMI-A-1"].enabled);
        assert!(c.outputs["HDMI-A-1"].x.is_none());
        assert_eq!(parse_mode("1920x1080"), Some((1920, 1080, None)));
        assert_eq!(parse_mode("big"), None);
        assert!(Config::parse("[outputs.A]\nscale = 9.0\n").unwrap_err().contains("scale"));
        assert!(Config::parse("[outputs.A]\nmode = \"x\"\n").unwrap_err().contains("mode"));
        assert!(Config::parse("[outputs.A]\nx = 5\n").unwrap_err().contains("x and y"));
    }

    #[test]
    fn the_accent_and_transparency_reach_the_glass() {
        let c = Config::parse("[appearance]\naccent = \"#ff0000\"\ntransparency = 0.7\nblur = 2\ngloss = 0.1\n").unwrap();
        let mut style = GlassStyle::default();
        c.apply_style(&mut style);
        assert_eq!(style.tint_frame, [1.0, 0.0, 0.0, 0.7]);
        assert_eq!((style.blur_levels, style.gloss), (2, 0.1));
    }
}

/// Where the settings app writes: `$FDE_CONFIG`, else the user's file.
pub fn user_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FDE_CONFIG") {
        return Some(PathBuf::from(p));
    }
    paths().into_iter().next_back()
}

/// A value for [`set`].
#[derive(Debug, Clone, PartialEq)]
pub enum Setting {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    /// remove the key (the theme's value applies again)
    Unset,
}

/// Change keys of the file at `path` and save it, keeping everything else (comments, order,
/// unknown keys). `changes` are `("section.key", value)`. The result must still be a valid
/// configuration, or nothing is written. The file is replaced atomically, so the compositor
/// never reads half of it.
pub fn set(path: &std::path::Path, changes: &[(&str, Setting)]) -> Result<(), String> {
    use toml_edit::{value, DocumentMut};
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: DocumentMut = text.parse().map_err(|e| format!("{}: {e}", path.display()))?;
    for (key, v) in changes {
        let (section, name) = key.split_once('.').ok_or_else(|| format!("{key}: expected section.key"))?;
        if let Setting::Unset = v {
            if let Some(t) = doc.get_mut(section).and_then(|s| s.as_table_mut()) {
                t.remove(name);
            }
            continue;
        }
        let mut item = match v {
            Setting::Unset => unreachable!(),
            Setting::Str(s) => value(s.as_str()),
            Setting::Int(i) => value(*i),
            Setting::Float(f) => value(*f),
            Setting::Bool(b) => value(*b),
        };
        if !doc.contains_key(section) {
            doc[section] = toml_edit::table();
        }
        // keep the comment that sits after the old value
        if let (Some(old), Some(new)) = (doc[section].get(name).and_then(|i| i.as_value()), item.as_value_mut()) {
            *new.decor_mut() = old.decor().clone();
        }
        doc[section][name] = item;
    }
    let new_text = doc.to_string();
    // refuse to save something the compositor would reject
    Config::parse(&new_text)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, new_text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod write_tests {
    use super::*;

    #[test]
    fn set_keeps_comments_and_other_keys_and_refuses_bad_values() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("sub/fde.toml");
        // a new file, in a directory that does not exist yet
        set(&f, &[("appearance.blur", Setting::Int(4))]).unwrap();
        assert_eq!(Config::parse(&std::fs::read_to_string(&f).unwrap()).unwrap().appearance.blur, 4);

        std::fs::write(&f, "# my theme\n[appearance]\nblur = 2 # soft\naccent = \"#112233\"\n\n[custom]\nthing = 1\n").unwrap();
        set(&f, &[("appearance.blur", Setting::Int(5)), ("shell.terminal", Setting::Str("xterm".into())), ("shell.panel", Setting::Bool(false))]).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("# my theme") && text.contains("# soft"), "comments stay: {text}");
        assert!(text.contains("[custom]") && text.contains("thing = 1"), "unknown keys stay");
        let c = Config::parse(&text).unwrap();
        assert_eq!((c.appearance.blur, c.appearance.accent.as_str(), c.shell.terminal.as_str(), c.shell.panel), (5, "#112233", "xterm", false));

        // a value the compositor would reject is not written
        let before = std::fs::read_to_string(&f).unwrap();
        assert!(set(&f, &[("appearance.blur", Setting::Int(99))]).is_err());
        assert!(set(&f, &[("appearance.accent", Setting::Str("red".into()))]).is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), before);
    }
}
