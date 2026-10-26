# fde-settings: the plan

The control panel of the desktop. Not started; this page decides the shape before any code,
so the compositor, the shell and the settings app agree on one configuration.

## One configuration, one writer

Everything lives in **`~/.config/fde/fde.toml`** (system defaults in `/etc/fde/fde.toml`,
the user file wins per key). One file, sectioned, with comments kept when the app rewrites it
(`toml_edit`):

```toml
[appearance]
theme = "aero"            # a theme directory: colours, gloss, orb colours, fonts
accent = "#4aa8ee"
transparency = 0.38       # glass tint strength
blur = 3                  # blur levels (0 turns blur off: battery / weak GPUs)
animations = true

[wallpaper]
path = ""                 # empty = the built-in sky
mode = "fill"             # fill | fit | stretch | center | tile

[input]
keyboard_layout = "us"
repeat_rate = 25
repeat_delay = 600
tap_to_click = true
natural_scroll = false
pointer_accel = 0.0

[outputs."DP-1"]
mode = "2560x1440@144"
scale = 1.0
position = [0, 0]
primary = true

[shell]
panel_position = "bottom" # bottom | top
launcher = "foot"
autostart = ["nm-applet"]

[session]
idle_lock_seconds = 0     # 0 = never
idle_blank_seconds = 0
```

## How changes reach the running desktop

- The compositor and the shell programs **watch the file** (inotify) and apply what changed
  without a restart: theme and blur live, layout / input live, outputs through the output
  management protocol.
- The settings app only ever **writes the file**. No private channel to the compositor: if the
  app is closed, nothing breaks, and hand-editing works the same way.
- Things that are not FDE's (volume, Wi-Fi, bluetooth, power profile, battery) are not in the
  file: the app talks to **PipeWire, NetworkManager, BlueZ, UPower / power-profiles-daemon over
  D-Bus** and shows their state, exactly as GNOME Settings does.

## Pages (first version → later)

| page | v1 | later |
|---|---|---|
| Appearance | theme list with preview, accent colour, glass strength, blur on/off, animations | custom theme editor, window button layout |
| Wallpaper | choose an image or the built-in sky, fit mode | slideshow, per-output wallpaper |
| Displays | list outputs, resolution + refresh, scale, position (drag), primary | night light, colour profiles |
| Keyboard & mouse | layout, repeat, tap-to-click, natural scroll, speed | shortcuts editor, per-device settings |
| Sound | output / input device, volume, mute (PipeWire) | per-app volumes, alert sounds |
| Network | Wi-Fi list + connect, wired status (NetworkManager) | VPN, hotspot |
| Power | battery, profile, idle lock / blank, lid action | per-source settings |
| Default apps | terminal, browser, file manager, launcher | MIME associations |
| About | version, GPU, session info | |

## Toolkit

**GTK4**, with FDE's own CSS theme (no libadwaita: its look is fixed). GTK4 gives text input,
accessibility, input methods and file choosers for free, which a hand-drawn UI (like the panel
and lock screen) would need years to match. The glass look comes from `ext-background-effect-v1`
(the window asks the compositor to blur behind its translucent areas) plus the FDE GTK theme.
The panel and lock screen stay small hand-drawn layer-shell programs: they are few pixels of
fixed layout, they start instantly and they have no toolkit dependency.

## Order of work

1. **Done:** `src/config.rs` reads, merges and validates the file and the compositor watches it
   (appearance, input, shell, session sections; see `docs/fde.example.toml`). Tests: merge
   rules, bad values, live reload applies a change and a broken edit changes nothing.
   Still to move over: wallpaper, outputs.
2. **Done (first version, in `settings/`):** Appearance, Keyboard, Desktop and Session pages; GTK4 with FDE CSS; every control writes the file through `config::set`. Still to do: Wallpaper, pointer settings.
   Original plan: Appearance + Wallpaper + Keyboard & mouse (only need the file; testable end-to-end:
   change the file, the pixels / input change).
3. Displays (needs `wlr-output-management` in the compositor).
4. Sound / Network / Power (D-Bus; tested against fake D-Bus services where possible).
5. Default apps, About, polish, keyboard navigation and screen-reader labels.

## Not in the first version

Accounts and users, printers, Bluetooth pairing UI, region / language, a GUI for editing
shortcuts. Each has a sensible command-line or file fallback until its page exists.
