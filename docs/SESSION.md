# The session: what FDE sets up for the programs it runs

When the compositor starts the session it

- sets `XDG_CURRENT_DESKTOP=FDE`, `XDG_SESSION_DESKTOP=fde`, `XDG_SESSION_TYPE=wayland` (and `DISPLAY`
  once XWayland is up) and hands them, with `WAYLAND_DISPLAY`, to the D-Bus session (`dbus-update-activation-environment`)
  so that services started by the bus, like the screen-sharing portal, find the display;
- starts a polkit authentication agent if one is installed (`policykit-1-gnome` and friends; turn off with
  `shell.polkit_agent = false` or `FDE_NO_POLKIT=1`), so programs can ask for an administrator password;
- starts the task bar, the notification daemon, the music player if switched on, and your autostart programs.

## Screen sharing and screenshots in programs

Browsers and chat programs share the screen through `xdg-desktop-portal`, whose wlr backend
(`xdg-desktop-portal-wlr`) uses FDE's `wlr-screencopy`. Tell the portal to use it for FDE:

    cp data/fde-portals.conf ~/.config/xdg-desktop-portal/fde-portals.conf

(`tools/vm-setup.sh` does this.) `grim` and `fde-shot` use the same capture directly.

## Logging in from a display manager

`data/fde.desktop` is a session entry for display managers (copy it to `/usr/share/wayland-sessions/`);
it starts `start-fde`.
