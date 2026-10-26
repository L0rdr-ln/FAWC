# FDE roadmap

Each milestone is usable on its own. Everything marked done has tests (software GL, headless, real
Wayland and X11 clients) unless it says otherwise. **Nothing has run on real hardware yet**; that
is the first thing to do (see "Next").

## Compositor
- [x] glass renderer: dual Kawase blur, glass panel shader, drop shadows, reflections, inner glow
- [x] windows: xdg-shell with server-side glass frames, move / resize / maximize / minimize /
      fullscreen, popups, focus, snapping (Alt+arrows, drag to an edge), wobbly windows (a real
      triangle mesh), open / minimize / close animations
- [x] input: keyboard and pointer, key shortcuts, pointer lock and confine and relative motion
      (games), cursor themes and named cursors, cursor-shape
- [x] workspaces (Alt+1..9), window switcher with pictures (Alt+Tab)
- [x] protocols: layer-shell, ext-background-effect, ext-session-lock, wlr-foreign-toplevel,
      xdg-activation, linux-dmabuf (import itself untested without a GPU), idle-inhibit, viewporter,
      fractional-scale, wlr-screencopy, clipboard and primary selection (Wayland and X11)
- [x] XWayland: frames, focus, maximize / fullscreen / close, menus, clipboard
- [x] config: one TOML file, applied live; themes (six Frutiger themes and your own); night light;
      monitors (scale and position live, resolution at the next start)
- [~] DRM/KMS + libinput backend, multi-monitor, hot-plug: written, compiled, **never run on hardware**
- [ ] X11 programs at fractional scale (they look small or blurry on HiDPI), resize/move requests
      from X11 programs
- [ ] touch and tablet input, input methods (CJK typing), keyboard-shortcut inhibit
- [ ] turning displays off when idle (DPMS), variable refresh rate, HDR and colour management,
      direct scanout, multi-GPU laptops
- [ ] window rules, session restore, hot corners / overview

## Shell
- [x] `fde-panel`: task bar (start orb, task list, clock, workspace dots, volume and battery),
      start menu with search and a power row (lock, log out, sleep, restart, shut down)
- [x] `fde-notify`: org.freedesktop.Notifications with buttons and default actions
- [x] `fde-lock`: lock screen, idle lock
- [x] `fde-shot`: screenshots (Print key)
- [x] `fde-music`: background music from your own files
- [x] session: environment for programs and portals, polkit agent if installed, portal config for
      screen sharing, a session entry for display managers (docs/SESSION.md)
- [ ] notification history, do-not-disturb
- [ ] system tray (StatusNotifierItem), network and Bluetooth status, an on-screen display
- [ ] a greeter (greetd); today a display manager starts `start-fde`

## Desktop suite
- [x] `fde-settings`: appearance (themes), displays (scale, resolution, position, night light),
      wallpaper, music, keyboard, desktop, session
- [x] settings: sound (volume and mute)
- [ ] settings: network, power, defaults, a monitor layout picture
- [ ] theme pictures for the wallpapers (the folder and the licence rules are ready:
      `assets/wallpapers/README.md`; only free-to-use pictures)
- [ ] the mascot picture (brief and prompt: `docs/MASCOT.md`)
- [ ] GTK4 and Qt styling to match, an icon theme, sounds
- [ ] file manager, a portal backend of its own, installer / one-command setup
- [ ] packaging (Debian / Arch), accessibility basics (high contrast, screen reader)

## Next
1. Check CI and fix what is red; run the VM script (`tools/vm-setup.sh`) and report what breaks on
   real hardware (DRM, audio, dmabuf, multi-monitor, X11 and the portals).
2. Finish what hardware testing shows; then the open items above in the order that hurts most.
