//! The one configuration file: `~/.config/fawc/fawc.toml`, with system defaults in
//! `/etc/fawc/fawc.toml` (the user's file wins, key by key). Missing keys mean defaults; a file
//! that does not parse is reported and the previous configuration stays.
//! See docs/SETTINGS.md for the plan this belongs to.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::Deserialize;

use crate::glass::GlassStyle;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// accent colour of the glass, "#rrggbb"
    pub accent: String,
    /// how strongly the glass is tinted, 0 (clear) .. 1
    pub transparency: f32,
    /// blur levels 1..6: more is softer and costs more
    pub blur: u32,
    /// the gloss band on the title bar, 0 .. 1
    pub gloss: f32,
    pub animations: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance { accent: "#4aa8ee".into(), transparency: 0.38, blur: 3, gloss: 0.8, animations: true }
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

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub appearance: Appearance,
    pub input: Input,
    pub shell: Shell,
    pub session: Session,
}

/// Where the files are: `$FAWC_CONFIG` replaces both, else system then user.
pub fn paths() -> Vec<PathBuf> {
    if let Some(p) = std::env::var_os("FAWC_CONFIG") {
        return vec![PathBuf::from(p)];
    }
    let mut v = vec![PathBuf::from("/etc/fawc/fawc.toml")];
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(b) = base {
        v.push(b.join("fawc/fawc.toml"));
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
        let c: Config = value.try_into().map_err(|e| format!("{e}"))?;
        c.validate()?;
        Ok(c)
    }

    fn validate(&self) -> Result<(), String> {
        parse_color(&self.appearance.accent).ok_or_else(|| format!("appearance.accent: \"{}\" is not #rrggbb", self.appearance.accent))?;
        if !(0.0..=1.0).contains(&self.appearance.transparency) {
            return Err("appearance.transparency must be between 0 and 1".into());
        }
        if !(1..=6).contains(&self.appearance.blur) {
            return Err("appearance.blur must be between 1 and 6".into());
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

    /// Put the look into a glass style (the rest of the style, sizes and orb colours, stays).
    pub fn apply_style(&self, style: &mut GlassStyle) {
        let [r, g, b] = parse_color(&self.appearance.accent).unwrap_or([0.29, 0.66, 0.93]);
        style.tint_frame = [r, g, b, self.appearance.transparency];
        style.gloss = self.appearance.gloss;
        style.blur_levels = self.appearance.blur as usize;
    }
}

fn parse_color(s: &str) -> Option<[f32; 3]> {
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
    fn the_accent_and_transparency_reach_the_glass() {
        let c = Config::parse("[appearance]\naccent = \"#ff0000\"\ntransparency = 0.7\nblur = 2\ngloss = 0.1\n").unwrap();
        let mut style = GlassStyle::default();
        c.apply_style(&mut style);
        assert_eq!(style.tint_frame, [1.0, 0.0, 0.0, 0.7]);
        assert_eq!((style.blur_levels, style.gloss), (2, 0.1));
    }
}

/// Where the settings app writes: `$FAWC_CONFIG`, else the user's file.
pub fn user_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FAWC_CONFIG") {
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
        let mut item = match v {
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
        let f = dir.path().join("sub/fawc.toml");
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
