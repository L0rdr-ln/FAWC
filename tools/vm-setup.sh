#!/usr/bin/env bash
# One command to get FDE built and ready on a fresh Debian/Ubuntu machine (a test VM, say).
#
#   sudo apt install -y git
#   git clone https://github.com/L0rdr-ln/fde && fde/tools/vm-setup.sh
#
# Afterwards:  ~/start-fde    starts FDE: nested if the machine has a desktop, else on the
#                              hardware (run it on a console: Ctrl+Alt+F3, log in, then ~/start-fde)
set -eu
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
say() { printf '\n==> %s\n' "$*"; }
command -v apt-get >/dev/null || { echo "this script knows apt (Debian/Ubuntu) only"; exit 1; }

say "installing packages (sudo asks for your password)"
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
    build-essential pkg-config curl git \
    libudev-dev libseat-dev libinput-dev libgbm-dev libegl-dev libgles-dev \
    libxkbcommon-dev libxkbcommon-x11-0 libdrm-dev libwayland-dev \
    libgl1-mesa-dri libegl1-mesa-dev mesa-utils seatd foot adwaita-icon-theme fonts-dejavu-core dbus dbus-user-session libglib2.0-bin libgtk-4-dev libasound2-dev pipewire pipewire-pulse pipewire-alsa wireplumber xwayland xclip xdg-desktop-portal xdg-desktop-portal-wlr xdg-desktop-portal-gtk policykit-1-gnome grim

if ! command -v cargo >/dev/null; then
    say "installing Rust"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# a `cargo` that is only rustup's launcher (Ubuntu's rustup package) has no toolchain until one is chosen
if ! cargo --version >/dev/null 2>&1; then
    say "choosing the stable Rust toolchain"
    if command -v rustup >/dev/null; then
        rustup default stable
    else
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env"
    fi
fi
cargo --version

say "building FDE (several minutes the first time)"
cd "$ROOT"
cargo build --release --workspace

# the settings app in the start menu
mkdir -p "$HOME/.local/share/applications"
cat >"$HOME/.local/share/applications/fde-settings.desktop" <<ENTRY
[Desktop Entry]
Type=Application
Name=Settings
Exec=$ROOT/target/release/fde-settings
ENTRY

say "checking the glass renderer on software GL"
LIBGL_ALWAYS_SOFTWARE=1 cargo test --release 2>&1 | tail -5 || echo "(some tests failed, see above)"

say "telling the desktop portals to use FDE's screen capture for screen sharing"
mkdir -p "$HOME/.config/xdg-desktop-portal"
cp "$ROOT/data/fde-portals.conf" "$HOME/.config/xdg-desktop-portal/fde-portals.conf"

say "writing the launcher ~/start-fde"
cat >"$HOME/start-fde" <<LAUNCH
#!/usr/bin/env bash
# Starts FDE. Log: ~/fde-last.log
export FDE_TERMINAL=\${FDE_TERMINAL:-foot}
export RUST_LOG=\${RUST_LOG:-info}
# virtual GPUs
export WLR_NO_HARDWARE_CURSORS=1
"$ROOT/target/release/fde" "\$@" 2>&1 | tee "\$HOME/fde-last.log"
LAUNCH
chmod +x "$HOME/start-fde"

# FDE comes with a wallpaper (assets/wallpaper.webp). To use another picture from the web, run
# FDE_WALLPAPER_URL=https://... tools/vm-setup.sh  (or tools/get-wallpaper.sh URL later).
wall_url="${FDE_WALLPAPER_URL:-none}"
if [ "$wall_url" != "none" ]; then
    say "downloading the wallpaper"
    "$ROOT/tools/get-wallpaper.sh" "$wall_url" fill || echo "(no wallpaper downloaded: the built-in picture stays; try tools/get-wallpaper.sh URL later)"
fi

# the seat daemon lets a normal user open the GPU and input devices on a console
sudo systemctl enable --now seatd 2>/dev/null || true
sudo usermod -aG _seatd "$USER" 2>/dev/null || sudo usermod -aG seat "$USER" 2>/dev/null || true

cat <<DONE

Done. Start it with:  ~/start-fde
 - in a desktop session it opens as a window (nested)
 - on a console (Ctrl+Alt+F3, log in) it takes over the screen; if it says it cannot open the
   seat, log out and in once (the seatd group was just added) or run it with sudo
Keys: Alt+Return terminal, Alt+drag move, Alt+Esc quit, Alt+Tab cycle, Alt+f maximize, Alt+l lock,
Ctrl+Alt+F1..F12 switch console.
DONE
