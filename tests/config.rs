//! The configuration file reaches a running compositor: read at start, applied when it changes,
//! and a broken edit changes nothing.

use fawc::state::Fawc;
use smithay::reexports::wayland_server::Display;

#[test]
fn the_config_file_is_read_reloaded_and_a_bad_edit_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file = dir.path().join("fawc.toml");
    std::fs::write(&file, "[appearance]\naccent = \"#ff0000\"\ntransparency = 0.9\nblur = 2\n[shell]\nterminal = \"xterm\"\n").unwrap();
    std::env::set_var("FAWC_CONFIG", &file);
    std::env::remove_var("FAWC_TERMINAL");

    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (800, 600).into());
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
    std::env::set_var("FAWC_CONFIG", &file);
    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (800, 600).into());
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
    std::env::set_var("FAWC_CONFIG", &file);

    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (800, 600).into());
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

    // a path that is not a picture: the built-in sky (and a note in the log)
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&file, "[wallpaper]\npath = \"/nonexistent/x.png\"\n").unwrap();
    state.check_config(true);
    assert!(state.wallpaper_image.is_none());
}
