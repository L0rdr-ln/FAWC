# FAWC

A complete desktop in the Frutiger Aero style (compositor, shell, settings, theme), starting with
a Wayland compositor with **real glass**: the blur behind a window, the gloss on its title bar
and the translucency of its frame are drawn on the GPU by the compositor itself, in the spirit of
Frutiger Aero (Windows Vista/7 Aero, skeuomorphic glass, sky and water).

Written in Rust on [Smithay](https://github.com/Smithay/smithay), **not** on wlroots: wlroots'
scene graph can only stack pictures and rectangles, it cannot read back what is behind a window,
so a true blur is impossible there. FAWC runs its own render pass: it draws the scene bottom to
top into an offscreen texture, and before every glass window it blurs the part of that texture
behind the window (dual Kawase blur), draws the glass panel over it (rounded shape, tint, gloss,
light edge) and then the client.

![the glass demo scene](docs/glass-demo.png)

*`fawc glass-demo`: two glass windows over a sky; the stripes behind them are really blurred.*

## Status

Early. What exists:

- the glass renderer (`src/glass`): blur, glass panel, rounded clipping, as GLES shaders;
- an offscreen demo (`fawc glass-demo out.png`) and pixel tests that run on a software GL
  (llvmpipe), so the look is developed and checked without a display.

The goal is a full desktop suite, the way GNOME or KDE are: see
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for what is written here and what is reused, and
[docs/ROADMAP.md](docs/ROADMAP.md) for the stages.

## Build

```sh
sudo apt install pkg-config libudev-dev libseat-dev libinput-dev libgbm-dev libegl-dev \
    libgles-dev libxkbcommon-dev libdrm-dev libwayland-dev
cargo build
LIBGL_ALWAYS_SOFTWARE=1 cargo test        # software GL is enough
cargo run -- glass-demo glass-demo.png
```

## Relation to sfwc

[sfwc](https://github.com/L0rdr-ln/Simple-Floating-Wayland-Compositor) is the small floating
compositor on wlroots. FAWC is a separate project with a different goal (looks first); it reuses
ideas (theme format with a `[style]` section, headless pixel tests), not code.
