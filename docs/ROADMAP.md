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
- [ ] M2c theme files (orb colours, fonts, shapes), wallpaper and output settings in the file
- [ ] M3 (written, never run on hardware) DRM/KMS + libinput backend (real hardware, VMs), VT switching, multi-monitor, HiDPI
- [ ] M4 (layer-shell and ext-background-effect-v1 done and tested) and ext-session-lock-v1 with `fawc-lock` done and tested — idle lock, screen capture, `ext-background-effect` (blur on
      request), fractional scale, output management
- [ ] M5 animations: open/close fade + zoom, minimize, workspace slide, Flip-3D style switcher

## Shell
- [x] S1a `fawc-panel`: glass task bar with start orb and clock (layer-shell client, started by the compositor)
- [x] S1b task list (wlr-foreign-toplevel-management: list, activate, minimize by click)
- [x] S1c start menu (installed apps from .desktop files, glass, closes on Esc or leaving it)
- [ ] S1d launcher with search, tray, notifications
- [ ] S2 tray (StatusNotifierItem), volume, network, battery, notifications, OSD
- [x] S3a lock screen `fawc-lock` (Alt+L, password via unix_chkpwd)
- [ ] S3b power menu, idle lock, greeter (greetd), session entry, autostart, polkit agent

## Desktop suite
- [ ] D1 `fawc-settings` (plan in [SETTINGS.md](SETTINGS.md); first step: one config file both the compositor and the app use): appearance, wallpaper, displays, input, sound, network, power
- [ ] D2 GTK4 theme + Qt style + icon theme + cursors + sounds + wallpapers (original art)
- [ ] D3 portal backend, screenshots/screen recording, file manager, installer / one-command setup
- [ ] D4 packaging (Debian/Arch), documentation, accessibility basics
