# FAWC roadmap

Each milestone is usable on its own. Verified by tests on software GL / headless wherever
possible; hardware and VM runs are noted as such.

## Compositor
- [x] M0 glass renderer: dual Kawase blur, glass panel shader (rounded, tint, gloss, edge),
      pixel tests on software GL, CI
- [x] M1 a compositor you can start: winit backend (nested), xdg-shell windows drawn with the
      glass renderer, keyboard and pointer, move/resize, close/maximize/minimize, focus
- [x] M2a title text with glow and orb buttons drawn by the compositor
- [x] M2b one config file, applied live (accent, transparency, blur, gloss, terminal, keyboard, autostart)
- [x] look: window drop shadows, diagonal glass reflections and an inner edge glow, a richer built-in wallpaper (sun rays, clouds, hazy hills, a swoosh of light, soap bubbles)
- [x] wallpaper from a PNG file (`[wallpaper]` in the file, a Wallpaper page in settings)
- [ ] M2c theme files (orb colours, fonts, shapes), output settings page
- [x] cursor themes (XCursor, named and client-drawn cursors, resize arrows)
- [~] M3 DRM/KMS + libinput backend (written, never run on hardware), VT switching; multiple monitors and per-monitor scale (compositor side tested with virtual outputs; DRM side compiled only); shell programs draw at the monitor's scale; a task bar per monitor; hot-plug (compositor side tested, udev trigger compiled only)
- [ ] M4 (layer-shell and ext-background-effect-v1 done and tested) and ext-session-lock-v1 with `fawc-lock` done and tested — idle lock, screen capture, `ext-background-effect` (blur on
      request), fractional scale, output management
- [~] M5 animations: opening fade + zoom and minimizing into the task bar button done (`appearance.animations`); closing, workspace slide, Flip-3D style switcher to come

## Shell
- [x] S1a `fawc-panel`: glass task bar with start orb and clock (layer-shell client, started by the compositor)
- [x] S1b task list (wlr-foreign-toplevel-management: list, activate, minimize by click)
- [x] S1c start menu (installed apps from .desktop files, glass, closes on Esc or leaving it)
- [x] S1d notifications (`fawc-notify`: org.freedesktop.Notifications, bubbles, expiry, dismiss)
- [x] S1e search in the start menu (type to filter, arrows, Enter)
- [ ] S1f notification actions and history, tray
- [ ] S2 tray (StatusNotifierItem), volume, network, battery, notifications, OSD
- [x] S3a lock screen `fawc-lock` (Alt+L, password via unix_chkpwd)
- [x] S3b idle lock (`session.idle_lock_seconds`)
- [ ] S3c power menu, greeter (greetd), session entry, autostart, polkit agent

## Desktop suite
- [~] D1 first version done (appearance, keyboard, desktop, session pages); displays, sound, network, power to come. `fawc-settings` (plan in [SETTINGS.md](SETTINGS.md); first step: one config file both the compositor and the app use): appearance, wallpaper, displays, input, sound, network, power
- [ ] D2 GTK4 theme + Qt style + icon theme + cursors + sounds + wallpapers (original art)
- [ ] D3 portal backend, screenshots/screen recording, file manager, installer / one-command setup
- [ ] D4 packaging (Debian/Arch), documentation, accessibility basics
