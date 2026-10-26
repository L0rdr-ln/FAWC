# FAWC roadmap

Each milestone is usable on its own. Verified by tests on software GL / headless wherever
possible; hardware and VM runs are noted as such.

## Compositor
- [x] M0 glass renderer: dual Kawase blur, glass panel shader (rounded, tint, gloss, edge),
      pixel tests on software GL, CI
- [ ] M1 a compositor you can start: winit backend (nested), xdg-shell windows drawn with the
      glass renderer, keyboard and pointer, move/resize, close/maximize/minimize, focus
- [ ] M2 title text and orb buttons drawn by the compositor, themes (`[style]` compatible)
- [ ] M3 DRM/KMS + libinput backend (real hardware, VMs), VT switching, multi-monitor, HiDPI
- [ ] M4 (layer-shell and ext-background-effect-v1 done and tested) — session lock, idle, screen capture, `ext-background-effect` (blur on
      request), fractional scale, output management
- [ ] M5 animations: open/close fade + zoom, minimize, workspace slide, Flip-3D style switcher

## Shell
- [x] S1a `fawc-panel`: glass task bar with start orb and clock (layer-shell client, started by the compositor)
- [ ] S1b task list (foreign-toplevel), start menu / launcher, tray, notifications
- [ ] S2 tray (StatusNotifierItem), volume, network, battery, notifications, OSD
- [ ] S3 lock screen, power menu, greeter (greetd), session entry, autostart, polkit agent

## Desktop suite
- [ ] D1 `fawc-settings`: appearance, wallpaper, displays, input, sound, network, power
- [ ] D2 GTK4 theme + Qt style + icon theme + cursors + sounds + wallpapers (original art)
- [ ] D3 portal backend, screenshots/screen recording, file manager, installer / one-command setup
- [ ] D4 packaging (Debian/Arch), documentation, accessibility basics
