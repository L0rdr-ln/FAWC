# FAWC roadmap

- [x] M0 glass renderer: dual Kawase blur, glass panel shader (rounded, tint, gloss, edge), pixel
      tests on software GL, CI
- [ ] M1 a compositor you can start: winit backend (nested), xdg-shell windows drawn with the
      glass renderer, keyboard and pointer, move/resize, close/maximize/minimize
- [ ] M2 title text and orb buttons drawn by the compositor, themes (`[style]` compatible)
- [ ] M3 DRM/KMS + libinput backend (real hardware, VMs), VT switching, multi-monitor
- [ ] M4 layer-shell (wallpaper, panel, launcher), blur behind panels, fractional scale
- [ ] M5 animations: open/close fades and zoom, minimize genie-ish, Flip-3D style window switcher
- [ ] M6 the Aero desktop: wallpapers, panel, start orb, sounds, one-command installer
- [ ] later: xwayland, screen capture, lock screen, window rules
