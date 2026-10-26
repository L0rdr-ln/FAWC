#!/usr/bin/env bash
# Download a wallpaper picture and make it the FAWC wallpaper.
#
#   tools/get-wallpaper.sh URL [fill|fit|stretch|center|tile]
#
# The picture is saved in ~/.local/share/fawc/wallpapers/ and `wallpaper.path` in
# ~/.config/fawc/fawc.toml is pointed at it (the running desktop follows at once). Nothing is
# stored in the FAWC repository: the picture stays your own copy of somebody else's work, so
# check that the site's terms allow you to use it.
set -eu
url="${1:?usage: get-wallpaper.sh URL [fill|fit|stretch|center|tile]}"
mode="${2:-fill}"
case "$mode" in fill|fit|stretch|center|tile) ;; *) echo "mode must be fill, fit, stretch, center or tile"; exit 1;; esac

dir="${XDG_DATA_HOME:-$HOME/.local/share}/fawc/wallpapers"
cfg="${FAWC_CONFIG:-${XDG_CONFIG_HOME:-$HOME/.config}/fawc/fawc.toml}"
mkdir -p "$dir" "$(dirname "$cfg")"
name="$(basename "${url%%\?*}")"
[ -n "$name" ] || name="wallpaper.jpg"
file="$dir/$name"

echo "downloading $url"
tmp="$(mktemp "$dir/.download.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
curl -fL --retry 2 -o "$tmp" "$url"
# it must be a picture we can read: JPEG or PNG, found by the first bytes
magic="$(head -c 4 "$tmp" | od -An -tx1 | tr -d ' \n')"
case "$magic" in
    ffd8*|89504e47|52494646) ;;   # JPEG, PNG, WebP (RIFF)
    *) echo "that is not a JPEG, PNG or WebP picture (the first bytes are $magic)"; exit 1;;
esac
mv "$tmp" "$file"
trap - EXIT

# write the two keys, keeping the rest of the file
python3 - "$cfg" "$file" "$mode" <<'PY'
import re, sys
cfg, path, mode = sys.argv[1:4]
try:
    text = open(cfg).read()
except FileNotFoundError:
    text = ""
def set_key(text, section, key, value):
    line = f'{key} = "{value}"'
    sec = re.search(rf'^\[{section}\]\s*$', text, re.M)
    if not sec:
        return text.rstrip("\n") + ("\n\n" if text.strip() else "") + f"[{section}]\n{line}\n"
    start = sec.end()
    nxt = re.search(r'^\[', text[start:], re.M)
    end = start + nxt.start() if nxt else len(text)
    body = text[start:end]
    if re.search(rf'^{key}\s*=', body, re.M):
        body = re.sub(rf'^{key}\s*=.*$', line, body, flags=re.M)
    else:
        body = "\n" + line + body
    return text[:start] + body + text[end:]
text = set_key(text, "wallpaper", "path", path)
text = set_key(text, "wallpaper", "mode", mode)
open(cfg, "w").write(text)
PY
echo "wallpaper set: $file ($mode)"
