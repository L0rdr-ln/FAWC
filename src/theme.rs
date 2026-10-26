//! Themes: a look for the whole desktop in one small TOML file. FDE ships the six members of the
//! Frutiger family: Aero, Metro, Eco, Dorific, Vector Flourish and Aqua; more can be put
//! in `~/.config/fde/themes/NAME.toml` (or `/etc/fde/themes`, `/usr/share/fde/themes`) and chosen
//! with `appearance.theme = "NAME"`. Keys the user sets in `[appearance]` (accent, transparency,
//! blur, gloss) still win over the theme's own values.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Buttons {
    /// "orb" (round glass), "flat" (glyphs only), "square" (rounded square gel) or "capsule"
    /// (one glass pill with three segments, hung from the top edge)
    pub style: String,
    /// "right" (close at the far right) or "left" (close at the far left, like Aqua)
    pub side: String,
    pub size: f32,
    pub gap: f32,
    pub margin: f32,
    pub close: String,
    pub maximize: String,
    pub minimize: String,
}

impl Default for Buttons {
    fn default() -> Self {
        Buttons { style: "orb".into(), side: "right".into(), size: 17.0, gap: 7.0, margin: 8.0, close: "#e64f2e".into(), maximize: "#54ba4a".into(), minimize: "#3d9be0".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ThemeWallpaper {
    /// "photo" (the bundled picture), "solid" or "gradient"
    pub kind: String,
    pub color: String,
    pub color2: String,
}

impl Default for ThemeWallpaper {
    fn default() -> Self {
        ThemeWallpaper { kind: "photo".into(), color: "#1c3a7a".into(), color2: "#050b24".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    pub description: String,
    pub accent: String,
    pub transparency: f32,
    pub blur: u32,
    pub gloss: f32,
    pub radius: f32,
    /// radius of the two bottom corners; negative = the same as `radius`
    pub radius_bottom: f32,
    pub border: f32,
    pub titlebar: f32,
    pub client_tint: [f32; 4],
    pub highlight: f32,
    pub reflections: f32,
    pub glow: f32,
    pub shadow: f32,
    pub rim: f32,
    pub title_ink: String,
    pub title_ink_dim: String,
    pub title_glow: f32,
    /// "left" or "center"
    pub title_align: String,
    pub buttons: Buttons,
    pub wallpaper: ThemeWallpaper,
}

impl Default for Theme {
    fn default() -> Self {
        // Frutiger Aero
        Theme {
            name: "Frutiger Aero".into(),
            description: String::new(),
            accent: "#4aa8ee".into(),
            transparency: 0.38,
            blur: 3,
            gloss: 0.8,
            radius: 12.0,
            radius_bottom: -1.0,
            border: 5.0,
            titlebar: 32.0,
            client_tint: [0.85, 0.93, 1.0, 0.10],
            highlight: 0.9,
            reflections: 1.0,
            glow: 1.0,
            shadow: 1.0,
            rim: 1.0,
            title_ink: "#06233d".into(),
            title_ink_dim: "#465868".into(),
            title_glow: 0.95,
            title_align: "left".into(),
            buttons: Buttons::default(),
            wallpaper: ThemeWallpaper::default(),
        }
    }
}

/// The themes that come with FDE: (id, file text).
pub const BUILTIN: &[(&str, &str)] = &[
    ("aero", include_str!("../themes/aero.toml")),
    ("frutiger-metro", include_str!("../themes/frutiger-metro.toml")),
    ("eco", include_str!("../themes/eco.toml")),
    ("dorific", include_str!("../themes/dorific.toml")),
    ("vector-flourish", include_str!("../themes/vector-flourish.toml")),
    ("aqua", include_str!("../themes/aqua.toml")),
];

fn dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("FDE_THEMES") {
        v.push(PathBuf::from(p));
    }
    if let Some(h) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))) {
        v.push(h.join("fde/themes"));
    }
    v.push(PathBuf::from("/etc/fde/themes"));
    v.push(PathBuf::from("/usr/share/fde/themes"));
    v
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Theme {
    pub fn parse(text: &str) -> Result<Theme, String> {
        let t: Theme = toml::from_str(text).map_err(|e| format!("{e}"))?;
        for (what, c) in [("accent", &t.accent), ("title_ink", &t.title_ink), ("title_ink_dim", &t.title_ink_dim)] {
            crate::config::parse_color(c).ok_or_else(|| format!("{what}: \"{c}\" is not #rrggbb"))?;
        }
        for (what, c) in [("buttons.close", &t.buttons.close), ("buttons.maximize", &t.buttons.maximize), ("buttons.minimize", &t.buttons.minimize)] {
            crate::config::parse_color(c).ok_or_else(|| format!("{what}: \"{c}\" is not #rrggbb"))?;
        }
        if !matches!(t.buttons.style.as_str(), "orb" | "flat" | "square" | "capsule") {
            return Err("buttons.style must be \"orb\", \"flat\", \"square\" or \"capsule\"".into());
        }
        if !matches!(t.buttons.side.as_str(), "left" | "right") {
            return Err("buttons.side must be \"left\" or \"right\"".into());
        }
        if !matches!(t.title_align.as_str(), "left" | "center") {
            return Err("title_align must be \"left\" or \"center\"".into());
        }
        if !matches!(t.wallpaper.kind.as_str(), "photo" | "solid" | "gradient") {
            return Err("wallpaper.kind must be \"photo\", \"solid\" or \"gradient\"".into());
        }
        for (what, c) in [("wallpaper.color", &t.wallpaper.color), ("wallpaper.color2", &t.wallpaper.color2)] {
            crate::config::parse_color(c).ok_or_else(|| format!("{what}: \"{c}\" is not #rrggbb"))?;
        }
        Ok(t)
    }

    /// The theme called `id`: the user's own files first, then the ones that come with FDE.
    pub fn find(id: &str) -> Result<Theme, String> {
        if !valid_id(id) {
            return Err(format!("\"{id}\" is not a theme name"));
        }
        for d in dirs() {
            let p = d.join(format!("{id}.toml"));
            if let Ok(text) = std::fs::read_to_string(&p) {
                return Theme::parse(&text).map_err(|e| format!("{}: {e}", p.display()));
            }
        }
        match BUILTIN.iter().find(|(n, _)| *n == id) {
            Some((_, text)) => Theme::parse(text),
            None => Err(format!("unknown theme \"{id}\"")),
        }
    }

    /// Every theme there is: (id, name), the built-in ones first.
    pub fn list() -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for (id, text) in BUILTIN {
            if let Ok(t) = Theme::parse(text) {
                out.push((id.to_string(), t.name));
            }
        }
        for d in dirs() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "toml").unwrap_or(false)).collect();
            files.sort();
            for p in files {
                let Some(id) = p.file_stem().and_then(|s| s.to_str()).map(String::from) else { continue };
                if out.iter().any(|(i, _)| *i == id) || !valid_id(&id) {
                    continue;
                }
                if let Ok(t) = Theme::find(&id) {
                    out.push((id, t.name));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_theme_parses_and_aero_is_the_default_look() {
        for (id, text) in BUILTIN {
            Theme::parse(text).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
        let aero = Theme::find("aero").unwrap();
        let d = Theme::default();
        assert_eq!((aero.accent.as_str(), aero.radius, aero.border, aero.titlebar), (d.accent.as_str(), d.radius, d.border, d.titlebar));
        assert_eq!(BUILTIN.len(), 6);
    }

    #[test]
    fn bad_themes_and_names_are_refused() {
        assert!(Theme::find("../etc/passwd").is_err());
        assert!(Theme::find("no-such-theme").is_err());
        assert!(Theme::parse("accent = \"red\"").is_err());
        assert!(Theme::parse("[buttons]\\nstyle = \"round\"").is_err());
    }
}

/// The colours the shell programs (task bar, menu, lock screen, notifications) draw with,
/// derived from the theme and the user's accent. Re-read from the files every few seconds, so a
/// theme chosen in the settings reaches a running task bar.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub accent: [f32; 3],
    /// colour of the soft shadow behind text (None: flat themes draw no shadow)
    pub text_glow: Option<[u8; 3]>,
    /// how strong the white shine is, 0 (flat) .. 1 (Frutiger Aero)
    pub shine: f32,
    /// 1 for solid themes, 0 for see-through glass
    pub solid: f32,
    pub orb: [f32; 3],
}

impl Default for Palette {
    fn default() -> Self {
        Palette::from_config(&crate::config::Config::default())
    }
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

impl Palette {
    pub fn from_config(c: &crate::config::Config) -> Palette {
        let t = c.theme();
        let (accent, _, _, gloss) = c.look();
        let a = crate::config::parse_color(&accent).unwrap_or([0.29, 0.66, 0.93]);
        let flat = matches!(t.buttons.style.as_str(), "flat" | "capsule");
        let orb = if flat { mix(a, [1.0; 3], 0.55) } else { crate::config::parse_color(&t.buttons.maximize).unwrap_or([0.33, 0.73, 0.29]) };
        let dark = mix(a, [0.0; 3], 0.62);
        Palette {
            accent: a,
            text_glow: if t.title_glow > 0.0 { Some([(dark[0] * 255.0) as u8, (dark[1] * 255.0) as u8, (dark[2] * 255.0) as u8]) } else { None },
            shine: (gloss / 0.8).clamp(0.0, 1.0),
            solid: if t.transparency >= 0.95 { 1.0 } else { 0.0 },
            orb,
        }
    }

    /// The accent, lit from above: `t` is 0 at the top of a surface, 1 at the bottom.
    pub fn tone(&self, t: f32) -> [f32; 3] {
        let k = 0.78 + 0.22 * (1.0 - t);
        [self.accent[0] * k, self.accent[1] * k, self.accent[2] * k]
    }

    /// The accent mixed towards white.
    pub fn light(&self, amount: f32) -> [f32; 3] {
        mix(self.accent, [1.0; 3], amount)
    }

    /// The accent mixed towards black.
    pub fn dark(&self, amount: f32) -> [f32; 3] {
        mix(self.accent, [0.0; 3], amount)
    }

    /// The glow behind white text as `text.line` wants it.
    pub fn glow(&self, strength: f32) -> Option<([u8; 3], f32)> {
        self.text_glow.map(|c| (c, strength))
    }
}

/// The palette of the running desktop's configuration (cached; looked at again every 2 s).
pub fn palette() -> Palette {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, Palette)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((when, p)) = *c {
        if when.elapsed() < Duration::from_secs(2) {
            return p;
        }
    }
    let p = crate::config::Config::load().map(|cfg| Palette::from_config(&cfg)).unwrap_or_default();
    *c = Some((Instant::now(), p));
    p
}
