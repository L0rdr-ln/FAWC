//! The configuration file reaches a running compositor: read at start, applied when it changes,
//! and a broken edit changes nothing.

use fde::state::Fde;
use smithay::reexports::wayland_server::Display;

#[test]
fn the_config_file_is_read_reloaded_and_a_bad_edit_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file = dir.path().join("fde.toml");
    std::fs::write(&file, "[appearance]\naccent = \"#ff0000\"\ntransparency = 0.9\nblur = 2\n[shell]\nterminal = \"xterm\"\n").unwrap();
    std::env::set_var("FDE_CONFIG", &file);
    std::env::remove_var("FDE_TERMINAL");

    let display: Display<Fde> = Display::new().unwrap();
    let mut state = Fde::new(&display.handle(), (800, 600).into());
    assert_eq!(state.style.tint_frame, [1.0, 0.0, 0.0, 0.9], "read at start");
    assert_eq!(state.style.blur_levels, 2);
    assert_eq!(state.terminal, "xterm");

    // an edit is applied
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&file, "[appearance]\naccent = \"#0000ff\"\nblur = 5\n[shell]\nterminal = \"alacritty\"\n[input]\nrepeat_rate = 40\n").unwrap();
    state.check_config(true);
    assert_eq!(state.style.tint_frame[..3], [0.0, 0.0, 1.0], "the accent follows");
    assert_eq!(state.style.tint_frame[3], 0.38, "keys that are gone go back to their default");
    assert_eq!(state.style.blur_levels, 5);
    assert_eq!(state.terminal, "alacritty");
    assert_eq!(state.config.input.repeat_rate, 40);

    // a broken edit changes nothing
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&file, "[appearance]\nblur = 99\n").unwrap();
    state.check_config(true);
    assert_eq!(state.style.blur_levels, 5, "the old configuration stays");
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&file, "this is [not toml").unwrap();
    state.check_config(true);
    assert_eq!(state.terminal, "alacritty");

    // and fixing the file works again
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&file, "[appearance]\nblur = 1\n").unwrap();
    state.check_config(true);
    assert_eq!(state.style.blur_levels, 1);
    assert_eq!(state.terminal, "foot");
}

#[test]
fn the_session_locks_by_itself_when_idle_and_activity_resets_the_timer() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("idle.toml");
    std::fs::write(&file, "[session]\nidle_lock_seconds = 1\n").unwrap();
    // the other test in this file uses the same variable: tests of one file share the process
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("FDE_CONFIG", &file);
    let display: Display<Fde> = Display::new().unwrap();
    let mut state = Fde::new(&display.handle(), (800, 600).into());
    assert!(!state.check_idle(), "not idle yet");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(state.check_idle(), "idle for a second: the lock starts");
    assert!(!state.check_idle(), "once");
    state.note_activity();
    assert!(!state.check_idle(), "activity resets the timer");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(state.check_idle(), "and it can lock again after more idleness");
}

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn write_png(path: &std::path::Path, rgb: [u8; 3]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(file, 2, 2);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = (0..4).flat_map(|_| rgb).collect();
    enc.write_header().unwrap().write_image_data(&data).unwrap();
}

#[test]
fn the_wallpaper_file_is_loaded_and_follows_edits() {
    let dir = tempfile::tempdir().unwrap();
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (red, blue) = (dir.path().join("red.png"), dir.path().join("blue.png"));
    write_png(&red, [200, 10, 10]);
    write_png(&blue, [10, 10, 200]);
    let file = dir.path().join("wall.toml");
    std::fs::write(&file, format!("[wallpaper]\npath = \"{}\"\nmode = \"stretch\"\n", red.display())).unwrap();
    std::env::set_var("FDE_CONFIG", &file);

    let display: Display<Fde> = Display::new().unwrap();
    let mut state = Fde::new(&display.handle(), (800, 600).into());
    assert_eq!(state.wallpaper_image.as_ref().unwrap().rgba[..3], [200, 10, 10]);

    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&file, format!("[wallpaper]\npath = \"{}\"\n", blue.display())).unwrap();
    state.check_config(true);
    assert_eq!(state.wallpaper_image.as_ref().unwrap().rgba[..3], [10, 10, 200], "the new picture");
    assert_eq!(state.config.wallpaper.mode, "fill");

    // a mode that does not exist is refused: nothing changes
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&file, format!("[wallpaper]\npath = \"{}\"\nmode = \"wobble\"\n", red.display())).unwrap();
    state.check_config(true);
    assert_eq!(state.wallpaper_image.as_ref().unwrap().rgba[..3], [10, 10, 200]);

    // a path that is not a picture: the wallpaper that comes with FDE (and a note in the log)
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&file, "[wallpaper]\npath = \"/nonexistent/x.png\"\n").unwrap();
    state.check_config(true);
    assert!(state.wallpaper_image.as_ref().map(|i| i.width >= 1900).unwrap_or(false), "the bundled picture");

    // no path at all: the same
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&file, "[appearance]\nblur = 2\n").unwrap();
    state.check_config(true);
    assert!(state.wallpaper_image.as_ref().map(|i| i.width >= 1900).unwrap_or(false));
}

#[test]
fn a_theme_sets_the_look_unless_the_user_set_it_by_hand() {
    use fde::config::Config;
    use fde::glass::GlassStyle;
    let metro = Config::parse("[appearance]\ntheme = \"frutiger-metro\"\n").unwrap();
    let mut s = GlassStyle::default();
    metro.apply_style(&mut s);
    assert_eq!((s.radius, s.radius_bottom, s.border, s.button_style), (4.0, 4.0, 2.0, 3.0));
    assert_eq!(s.tint_frame[3], 0.82, "metro's title bar is nearly solid");
    // a hand-set accent still wins
    let own = Config::parse("[appearance]\ntheme = \"frutiger-metro\"\naccent = \"#ff0000\"\n").unwrap();
    own.apply_style(&mut s);
    assert_eq!(&s.tint_frame[..3], &[1.0, 0.0, 0.0]);
    assert_eq!(s.tint_frame[3], 0.82, "the transparency still comes from the theme");
    assert!(Config::parse("[appearance]\ntheme = \"nope\"\n").unwrap_err().contains("theme"));
}

#[test]
fn a_theme_file_in_the_themes_folder_can_be_used() {
    use fde::config::Config;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mine.toml"), "name = \"Mine\"\naccent = \"#112233\"\nradius = 3\n").unwrap();
    std::env::set_var("FDE_THEMES", dir.path());
    let c = Config::parse("[appearance]\ntheme = \"mine\"\n").unwrap();
    assert_eq!(c.look().0, "#112233");
    assert_eq!(c.theme().radius, 3.0);
    assert!(fde::theme::Theme::list().iter().any(|(id, n)| id == "mine" && n == "Mine"));
    std::env::remove_var("FDE_THEMES");
}

/// Not a test: `FDE_PREVIEW_OUT=file.png FDE_CONFIG=... cargo test --test config shell_preview -- --ignored`
/// draws the task bar and start menu with the configured theme (for looking at).
#[test]
#[ignore]
fn shell_preview() {
    let Some(out) = std::env::var_os("FDE_PREVIEW_OUT") else { return };
    let mut text = fde::text::TextRenderer::default();
    let tasks = vec![
        fde::panel::Task { title: "Terminal".into(), active: true, minimized: false },
        fde::panel::Task { title: "Files".into(), active: false, minimized: false },
        fde::panel::Task { title: "Music".into(), active: false, minimized: true },
    ];
    let (bw, bh) = (600, 40);
    let bar = fde::panel::draw(&mut text, bw, bh, 1, false, "12:34", &tasks);
    let names: Vec<String> = ["Terminal", "Files", "Settings", "Music"].iter().map(|s| s.to_string()).collect();
    let menu = fde::panel::draw_menu(&mut text, 1, "", &names, Some(1), &fde::panel::PowerView { hover: Some(2), armed: Some(4) });
    let (mw, mh) = fde::panel::menu_size();
    let (w, h) = (600usize, 40 + 10 + mh as usize);
    let mut img = vec![0u8; w * h * 4];
    for px in img.chunks_exact_mut(4) {
        px.copy_from_slice(&[235, 240, 245, 255]);
    }
    let put = |img: &mut Vec<u8>, src: &[u8], sw: usize, sh: usize, ox: usize, oy: usize| {
        for y in 0..sh {
            for x in 0..sw {
                let s = (y * sw + x) * 4;
                let d = ((oy + y) * w + ox + x) * 4;
                let a = src[s + 3] as u32;
                for c in 0..3 {
                    img[d + c] = ((src[s + c] as u32) + img[d + c] as u32 * (255 - a) / 255).min(255) as u8;
                }
            }
        }
    };
    // the panel's pictures are ARGB with the red and blue swapped (shm); swap back for the PNG
    let swap = |b: Vec<u8>| -> Vec<u8> { b };
    put(&mut img, &swap(bar), bw as usize, bh as usize, 0, 0);
    put(&mut img, &swap(menu), mw as usize, mh as usize, 0, 50);
    let file = std::fs::File::create(out).unwrap();
    let mut enc = png::Encoder::new(file, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&img).unwrap();
}

#[test]
fn the_window_buttons_can_be_clicked_in_every_theme_and_button_style() {
    use fde::config::Config;
    use fde::glass::GlassStyle;
    use fde::layout::{button_center, hit, outer_rect, Button, Hit};
    use smithay::utils::{Logical, Rectangle};
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // the styles the built-in themes do not use, as theme files of their own
    let dir = tempfile::tempdir().unwrap();
    for (id, body) in [
        ("capsule", "[buttons]\nstyle = \"capsule\"\ngap = 1\nmargin = 6\n"),
        ("left", "title_align = \"center\"\n[buttons]\nside = \"left\"\nsize = 12\n"),
        ("flat", "[buttons]\nstyle = \"flat\"\n"),
    ] {
        std::fs::write(dir.path().join(format!("{id}.toml")), body).unwrap();
    }
    std::env::set_var("FDE_THEMES", dir.path());
    for (id, _) in fde::theme::Theme::list() {
        let cfg = Config::parse(&format!("[appearance]\ntheme = \"{id}\"\n")).unwrap();
        let mut style = GlassStyle::default();
        cfg.apply_style(&mut style);
        let client = Rectangle::<i32, Logical>::new((200, 200).into(), (400, 300).into());
        let outer = outer_rect(client, &style);
        let mut seen = Vec::new();
        for b in [Button::Close, Button::Maximize, Button::Minimize] {
            let c = button_center(outer, &style, b);
            assert_eq!(hit(client, &style, c), Some(Hit::Button(b)), "{id}: {b:?} at {c:?}");
            assert!(c.x > outer.loc.x as f64 && c.x < (outer.loc.x + outer.size.w) as f64, "{id}: inside the window");
            seen.push(c.x as i32);
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 3, "{id}: three different places");
    }
    std::env::remove_var("FDE_THEMES");
}

#[test]
fn a_theme_uses_its_picture_when_the_file_exists_and_its_colours_when_not() {
    use fde::config::Config;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let pics = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pic.toml"), "[wallpaper]\nkind = \"solid\"\ncolor = \"#102030\"\nfile = \"pic.png\"\n").unwrap();
    std::env::set_var("FDE_THEMES", dir.path());
    std::env::set_var("FDE_WALLPAPERS", pics.path());
    let cfg = Config::parse("[appearance]\ntheme = \"pic\"\n").unwrap();
    // no file yet: the colour
    let img = fde::wallpaper::for_config(&cfg).unwrap();
    assert_eq!(&img.rgba[..3], &[0x10, 0x20, 0x30]);
    // a 2x2 red picture appears
    let file = std::fs::File::create(pics.path().join("pic.png")).unwrap();
    let mut enc = png::Encoder::new(file, 2, 2);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&[255, 0, 0, 255].repeat(4)).unwrap();
    let img = fde::wallpaper::for_config(&cfg).unwrap();
    assert_eq!((img.width, &img.rgba[..3]), (2, &[255u8, 0, 0][..]));
    // a file name with a path is refused
    std::fs::write(dir.path().join("bad.toml"), "[wallpaper]\nfile = \"../x.png\"\n").unwrap();
    assert!(Config::parse("[appearance]\ntheme = \"bad\"\n").is_err());
    std::env::remove_var("FDE_THEMES");
    std::env::remove_var("FDE_WALLPAPERS");
}

#[test]
fn nested_keys_can_be_written_and_monitor_settings_apply_to_a_running_compositor() {
    use fde::config::{set, Config, Setting};
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("fde.toml");
    std::fs::write(&file, "# my settings\n[appearance]\naccent = \"#112233\" # blue-ish\n").unwrap();
    set(
        &file,
        &[
            ("outputs.fde-0.scale", Setting::Float(1.5)),
            ("outputs.fde-0.x", Setting::Int(0)),
            ("outputs.fde-0.y", Setting::Int(0)),
            ("outputs.DP-1.mode", Setting::Str("1920x1080@60".into())),
        ],
    )
    .unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("# my settings") && text.contains("# blue-ish"), "comments stay: {text}");
    let c = Config::parse(&text).unwrap();
    assert_eq!(c.outputs["fde-0"].scale, 1.5);
    assert_eq!(c.outputs["DP-1"].mode, "1920x1080@60");
    // a bad value for a monitor is refused
    assert!(set(&file, &[("outputs.fde-0.scale", Setting::Float(9.0))]).is_err());

    // the running compositor follows: scale, and the list of monitors for the settings app
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let display = smithay::reexports::wayland_server::Display::<Fde>::new().unwrap();
    let mut state = Fde::new(&display.handle(), (1920, 1080).into());
    state.write_outputs_state();
    let before = fde::outputs::read_outputs_state();
    assert_eq!(before.len(), 1);
    assert_eq!((before[0].name.as_str(), before[0].scale, before[0].width), ("fde-0", 1.0, 1920));
    state.apply_output_config(&c);
    let after = fde::outputs::read_outputs_state();
    assert_eq!(after[0].scale, 1.5, "the new scale is in effect and reported");
    assert_eq!(state.output_logical_size(&state.output.clone()), (1280, 720).into());
}

#[test]
fn the_number_of_workspaces_is_checked() {
    use fde::config::Config;
    assert_eq!(Config::parse("").unwrap().session.workspaces, 4);
    assert_eq!(Config::parse("[session]\nworkspaces = 2\n").unwrap().session.workspaces, 2);
    assert!(Config::parse("[session]\nworkspaces = 0\n").is_err());
    assert!(Config::parse("[session]\nworkspaces = 12\n").is_err());
}

#[test]
fn night_light_settings_are_checked() {
    use fde::config::{parse_clock, Config};
    assert_eq!(parse_clock("07:30"), Some(450));
    assert_eq!(parse_clock("24:00"), None);
    assert_eq!(parse_clock("noon"), None);
    let c = Config::parse("[display]\nnight_light = true\nnight_light_temperature = 3000\n").unwrap();
    assert!(c.display.night_light && c.display.night_light_temperature == 3000);
    assert!(!Config::parse("").unwrap().display.night_light, "off by default");
    assert!(Config::parse("[display]\nnight_light_temperature = 900\n").is_err());
    assert!(Config::parse("[display]\nnight_light_from = \"8pm\"\n").is_err());
    assert!(Config::parse("[display]\nnight_light_from = \"\"\nnight_light_to = \"\"\n").is_ok(), "empty means always");
}

#[test]
fn alt_n_toggles_do_not_disturb_and_says_so_on_screen() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let display = smithay::reexports::wayland_server::Display::<Fde>::new().unwrap();
    let mut state = Fde::new(&display.handle(), (800, 600).into());
    assert!(!fde::notify::dnd_on());
    state.toggle_do_not_disturb();
    assert!(fde::notify::dnd_on());
    assert_eq!(state.hud.as_ref().map(|h| h.1.as_str()), Some("Do not disturb: on"));
    state.toggle_do_not_disturb();
    assert!(!fde::notify::dnd_on());
    assert_eq!(state.hud.as_ref().map(|h| h.1.as_str()), Some("Do not disturb: off"));
}
