# Contributing to FAWC

Thanks for looking. FAWC is early: the most useful contribution right now is **running it and
telling us what breaks**, especially on real hardware (the DRM/KMS backend has only run nested).

## Reporting a problem

Open an issue with the template. Please include:

- how you started it (`~/start-fawc`, `cargo run`, nested or on a console),
- the log (`~/fawc-last.log`, or `RUST_LOG=info` output),
- your GPU / VM type and the distribution.

## Building and testing

```sh
sudo apt install pkg-config libudev-dev libseat-dev libinput-dev libgbm-dev libegl-dev \
    libgles-dev libxkbcommon-dev libdrm-dev libwayland-dev
cargo build
LIBGL_ALWAYS_SOFTWARE=1 cargo test     # unit tests, glass pixel tests, end-to-end tests
```

The tests need no display or GPU: they render on a software GL (llvmpipe), and the end-to-end
tests start real Wayland clients (including the real `fawc-panel` and `fawc-lock` binaries)
against the compositor inside the test process.

## House rules

- **A feature comes with a test that would fail without it.** Pixel tests for looks, a real
  client for protocols. "It compiled" is not a result; say what was run.
- Say honestly what is verified and what is not (hardware, other GPUs, other toolkits).
- Keep the style of the code around you: comments explain *why*, names are plain.
- All artwork (icons, wallpapers, sounds) must be original or under a compatible free licence.
  Nothing taken from Windows or other proprietary desktops. That includes the stock wallpapers, icons and sounds of Windows Vista/7 and the picture collections of "Frutiger Aero archives": they are other people's copyrighted work and must not be committed here. (Users can of course point `wallpaper.path` at any picture they own or like on their own machine.) Free sources are fine when the licence is clear: public domain / CC0 pictures, with the source and licence noted in the commit.
- Small, focused commits; one logical change each.

## Layout

`settings/` the GTK4 control panel (separate crate, needs `libgtk-4-dev`; `cargo test --workspace` builds it), `src/glass` the renderer, `src/*.rs` the compositor, `src/bin/` the shell programs
(`fawc-panel`, `fawc-lock`), `tests/e2e.rs` end-to-end tests, `docs/` architecture, roadmap and
plans.
