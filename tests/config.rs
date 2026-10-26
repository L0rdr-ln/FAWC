//! The configuration file reaches a running compositor: read at start, applied when it changes,
//! and a broken edit changes nothing.

use fawc::state::Fawc;
use smithay::reexports::wayland_server::Display;

#[test]
fn the_config_file_is_read_reloaded_and_a_bad_edit_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
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
