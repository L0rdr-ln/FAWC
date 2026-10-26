# FAWC desktop: architecture

Goal: a complete desktop environment in the Frutiger Aero style (glass, gloss, sky and water),
the way GNOME or KDE are complete: compositor, shell, settings, session, theme, apps.

The rule that makes this possible for a small team: **write the parts that define the look and
the experience, reuse the plumbing every Linux desktop shares.**

## What FAWC writes itself

| part | what it is | language / base |
|---|---|---|
| `fawc` (compositor) | windows, input, outputs, the glass renderer, animations, layer-shell, session lock, screen capture, `ext-background-effect` (blur on request) | Rust, Smithay |
| `fawc-shell` | panel (start orb, task bar, tray, clock, volume/network/battery), start menu / launcher, notifications, OSD, lock screen, power menu, desktop (wallpaper, icons) | Rust, GTK4 + layer-shell |
| `fawc-settings` | the control panel: appearance, wallpaper, displays, input, sound, network, power, defaults | Rust, GTK4 |
| `fawc-session` | session start (`wayland-sessions` entry), autostart, a greeter for greetd, polkit agent | Rust |
| theme | GTK4 and libadwaita-free CSS theme, Qt style, icon theme, cursor theme, sounds, wallpapers: all original artwork | CSS, SVG |
| portal backend | `xdg-desktop-portal-fawc` (screenshot, screencast, file chooser via GTK, settings) | Rust |

## What FAWC reuses (talked to over D-Bus, not rewritten)

NetworkManager (network), PipeWire + WirePlumber (audio), UPower (battery), logind (suspend,
lock, seats), polkit (privileges), udisks2 (disks), BlueZ (bluetooth), xdg-desktop-portal
(sandboxed apps), AccountsService, systemd user units, fontconfig. Apps: `foot` (terminal) at
first, any file manager (Thunar/Nautilus) until `fawc-files` exists.

## How the glass reaches every program

Only the compositor can blur "what is behind". So FAWC implements the Wayland protocol
`ext-background-effect-v1`: a client tells the compositor *"blur the area behind this part of my
window"* and draws its own translucent pixels there. The shell, GTK4 apps with the FAWC theme
and Qt apps can all ask for it, and the compositor's renderer (`src/glass`) does the blur on the
GPU. Windows without the protocol get the glass frame around them (title bar, border), which the
compositor draws itself.

## Layers

```
 apps (GTK4/Qt/Electron/...)  <->  Wayland  <->  fawc (compositor, glass renderer)
 fawc-shell / fawc-settings   <->  Wayland + D-Bus  <->  NetworkManager, PipeWire, UPower, logind...
 fawc-session / greeter       <->  greetd, systemd user session, polkit
```

## Honest scope

GNOME and KDE took hundreds of people years. The plan is a coherent MVP in stages, each one
usable on its own, and no part before the one below it works (see the roadmap). The risky
parts are hardware (DRM/KMS on real GPUs, multi-monitor, HiDPI), toolkit integration (blur for
GTK/Qt) and the amount of art (icons, wallpapers, sounds): all of it has to be original, not
Microsoft's.
