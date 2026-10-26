# Themes

A theme is one small TOML file. FDE comes with ten (`themes/` in the source):

| id | look | after |
|---|---|---|
| `aero` | glossy blue glass, round orbs, the sky and hill wallpaper (default) | the general Frutiger Aero look |
| `vista` | Vista Glass: translucent blue glass with diagonal reflections, black titles with a white glow, a capsule of three buttons hanging from the top edge | Windows Vista, 2006 |
| `seven` | Seven Glass: lighter, clearer glass, bright inner glow, capsule buttons | Windows 7, 2009 |
| `aqua` | light grey title bars, glossy red/yellow/green buttons on the left, centred titles, deep shadows | Mac OS X Aqua / Leopard, 2000-2009 |
| `luna` | bold blue title bars with rounded tops and square bottoms, square gel buttons, the sky-and-hill wallpaper | Windows XP, 2001 |
| `nature` | fresh green glass, leaf-coloured orbs, sky fading into grass | the nature wallpaper themes of the era |
| `dark-aero` | Frutiger Aero after dark | "Dark Aero" in the Frutiger family |
| `frutiger-metro` | flatter and crisper, saturated blue, a little gloss left | "Frutiger Metro" in the Frutiger family |

These are homages drawn from scratch to match how those looks are remembered; none of them uses
anything from the original systems.

Choose one in the settings (Appearance > Theme) or in `fde.toml`:

```toml
[appearance]
theme = "metro"
```

`accent`, `transparency`, `blur` and `gloss` in `[appearance]` still win over the theme's
values when they are set. Choosing a theme in the settings removes them, so the theme's own apply.

## Your own theme

Put `NAME.toml` in `~/.config/fde/themes/` (also read: `/etc/fde/themes`, `/usr/share/fde/themes`,
and `$FDE_THEMES`) and use `theme = "NAME"`. Every key is optional; missing ones are Frutiger Aero's.

```toml
name = "My theme"              # shown in the settings
description = "..."
accent = "#4aa8ee"             # the tint of frames, the task bar and menus
transparency = 0.38            # 0 clear .. 1 solid
blur = 3                       # 1..6
gloss = 0.8                    # the shine on the title bar, 0 .. 1
radius = 12                    # corner radius of windows (0 = square)
radius_bottom = -1             # the two bottom corners; -1 = the same as radius
title_align = "left"          # or "center"
border = 5                     # frame width
titlebar = 32
client_tint = [0.85, 0.93, 1.0, 0.10]   # r, g, b, strength over the window's content area
highlight = 0.9                # light line inside the border
reflections = 1.0              # diagonal streaks on the glass, 0 .. 1
glow = 1.0                     # soft light along the inner edge, 0 .. 1
shadow = 1.0                   # drop shadow strength, 0 .. 2
rim = 1.0                      # dark hairline at the edge
title_ink = "#06233d"          # title text colour (focused window)
title_ink_dim = "#465868"      # (other windows)
title_glow = 0.95              # white glow behind the title text, 0 = none

[buttons]
style = "orb"                  # "orb" (round glass), "flat" (glyphs only), "square" (gel squares) or "capsule" (one pill of three, hung from the top edge)
side = "right"                 # or "left": close first, like Aqua
size = 17
gap = 7
margin = 8
close = "#e64f2e"
maximize = "#54ba4a"
minimize = "#3d9be0"

[wallpaper]
kind = "photo"                 # "photo" (the bundled picture), "solid" or "gradient"
color = "#1c3a7a"              # solid colour, or the top of the gradient
color2 = "#050b24"             # the bottom of the gradient
```

A wallpaper you choose in the settings (`[wallpaper] path`) replaces the theme's. The task bar,
start menu, lock screen and notifications take their colours from the accent; they look at the
theme again every few seconds, so a change reaches them without a restart.
