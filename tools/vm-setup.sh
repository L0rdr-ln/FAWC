#!/usr/bin/env bash
# One command to get FAWC built and ready on a fresh Debian/Ubuntu machine (a test VM, say).
#
#   sudo apt install -y git
#   git clone https://github.com/L0rdr-ln/fawc && fawc/tools/vm-setup.sh
#
# Afterwards:  ~/start-fawc    starts FAWC: nested if the machine has a desktop, else on the
#                              hardware (run it on a console: Ctrl+Alt+F3, log in, then ~/start-fawc)
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
    libgl1-mesa-dri libegl1-mesa-dev mesa-utils seatd foot adwaita-icon-theme fonts-dejavu-core dbus dbus-user-session libglib2.0-bin

if ! command -v cargo >/dev/null; then
    say "installing Rust"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

say "building FAWC (several minutes the first time)"
cd "$ROOT"
cargo build --release

say "checking the glass renderer on software GL"
LIBGL_ALWAYS_SOFTWARE=1 cargo test --release 2>&1 | tail -5 || echo "(some tests failed, see above)"

say "writing the launcher ~/start-fawc"
cat >"$HOME/start-fawc" <<LAUNCH
#!/usr/bin/env bash
# Starts FAWC. Log: ~/fawc-last.log
export FAWC_TERMINAL=\${FAWC_TERMINAL:-foot}
export RUST_LOG=\${RUST_LOG:-info}
# virtual GPUs
export WLR_NO_HARDWARE_CURSORS=1
"$ROOT/target/release/fawc" "\$@" 2>&1 | tee "\$HOME/fawc-last.log"
LAUNCH
chmod +x "$HOME/start-fawc"

# the seat daemon lets a normal user open the GPU and input devices on a console
sudo systemctl enable --now seatd 2>/dev/null || true
sudo usermod -aG _seatd "$USER" 2>/dev/null || sudo usermod -aG seat "$USER" 2>/dev/null || true

cat <<DONE

Done. Start it with:  ~/start-fawc
 - in a desktop session it opens as a window (nested)
 - on a console (Ctrl+Alt+F3, log in) it takes over the screen; if it says it cannot open the
   seat, log out and in once (the seatd group was just added) or run it with sudo
Keys: Alt+Return terminal, Alt+drag move, Alt+Esc quit, Alt+Tab cycle, Alt+f maximize, Alt+l lock,
Ctrl+Alt+F1..F12 switch console.
DONE
