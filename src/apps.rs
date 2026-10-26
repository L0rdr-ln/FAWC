//! The applications a start menu lists, read from freedesktop `.desktop` files.

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub name: String,
    pub exec: String,
}

/// One `.desktop` file: an application to show, or None (hidden, not an application, no command).
pub fn parse_desktop(text: &str) -> Option<App> {
    let (mut name, mut exec, mut kind) = (None, None, None);
    let mut hidden = false;
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k {
                "Name" => name = Some(v.to_string()),
                "Exec" => exec = Some(v.to_string()),
                "Type" => kind = Some(v.to_string()),
                "NoDisplay" | "Hidden" if v == "true" => hidden = true,
                _ => {}
            }
        }
    }
    if hidden || kind.as_deref() != Some("Application") {
        return None;
    }
    // field codes (%f %U ...) mean "the files to open": none here
    let exec = exec?;
    let exec: Vec<&str> = exec.split_whitespace().filter(|w| !(w.starts_with('%') && w.len() == 2)).collect();
    let exec = exec.join(" ");
    if exec.is_empty() {
        return None;
    }
    Some(App { name: name?, exec })
}

/// All applications installed, sorted by name.
pub fn installed() -> Vec<App> {
    let mut dirs = vec!["/usr/share/applications".to_string(), "/usr/local/share/applications".to_string()];
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(format!("{home}/.local/share/applications"));
    }
    let mut apps = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            if e.path().extension().map(|x| x == "desktop").unwrap_or(false) {
                if let Some(a) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse_desktop(&t)) {
                    apps.push(a);
                }
            }
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.name == b.name);
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_applications_and_skips_the_rest() {
        let a = parse_desktop("[Desktop Entry]\nType=Application\nName=Files\nName[de]=Dateien\nExec=nautilus --new-window %U\n[Desktop Action x]\nName=Other\nExec=nope\n").unwrap();
        assert_eq!(a, App { name: "Files".into(), exec: "nautilus --new-window".into() });
        assert!(parse_desktop("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n").is_none());
        assert!(parse_desktop("[Desktop Entry]\nType=Link\nName=X\nExec=x\n").is_none());
    }
}
