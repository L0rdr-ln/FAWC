//! GLSL ES 1.00 shaders for the glass look. They are Smithay "custom texture shaders": the
//! renderer fills in the `//_DEFINES_` line and provides `tex`, `alpha` and `v_coords`.

/// Dual Kawase blur, downsample pass. `half_pixel` is half a texel of the *source* texture.
pub const BLUR_DOWN: &str = r#"
#version 100
//_DEFINES_
precision highp float;
uniform sampler2D tex;
uniform float alpha;
varying vec2 v_coords;
uniform vec2 half_pixel;
void main() {
    vec2 uv = v_coords;
    vec4 sum = texture2D(tex, uv) * 4.0;
    sum += texture2D(tex, uv - half_pixel);
    sum += texture2D(tex, uv + half_pixel);
    sum += texture2D(tex, uv + vec2(half_pixel.x, -half_pixel.y));
    sum += texture2D(tex, uv - vec2(half_pixel.x, -half_pixel.y));
    gl_FragColor = sum / 8.0 * alpha;
}
"#;

/// Dual Kawase blur, upsample pass.
pub const BLUR_UP: &str = r#"
#version 100
//_DEFINES_
precision highp float;
uniform sampler2D tex;
uniform float alpha;
varying vec2 v_coords;
uniform vec2 half_pixel;
void main() {
    vec2 uv = v_coords;
    vec4 sum = texture2D(tex, uv + vec2(-half_pixel.x * 2.0, 0.0));
    sum += texture2D(tex, uv + vec2(-half_pixel.x, half_pixel.y)) * 2.0;
    sum += texture2D(tex, uv + vec2(0.0, half_pixel.y * 2.0));
    sum += texture2D(tex, uv + vec2(half_pixel.x, half_pixel.y)) * 2.0;
    sum += texture2D(tex, uv + vec2(half_pixel.x * 2.0, 0.0));
    sum += texture2D(tex, uv + vec2(half_pixel.x, -half_pixel.y)) * 2.0;
    sum += texture2D(tex, uv + vec2(0.0, -half_pixel.y * 2.0));
    sum += texture2D(tex, uv + vec2(-half_pixel.x, -half_pixel.y)) * 2.0;
    gl_FragColor = sum / 12.0 * alpha;
}
"#;

/// The glass panel of a window: the blurred backdrop (the texture, stretched over the window's
/// outer rectangle) shaped as a rounded rectangle, tinted, with a gloss band over the title bar,
/// a light inner edge and a thin dark rim. Output is premultiplied.
pub const GLASS: &str = r#"
#version 100
//_DEFINES_
precision highp float;
uniform sampler2D tex;
uniform float alpha;
varying vec2 v_coords;
uniform vec2 size;          // outer size in pixels
uniform vec2 uv_origin;     // where the window starts in the blurred texture (0..1)
uniform vec2 uv_size;       // how much of the blurred texture the window covers (0..1)
uniform float radius;       // corner radius in pixels
uniform float border;       // border width in pixels
uniform float titlebar;     // title bar height in pixels (below the border)
uniform vec4 tint_frame;    // glass tint over border + title bar (straight alpha)
uniform vec4 tint_client;   // tint over the client area
uniform float gloss;        // 0..1
uniform float highlight;    // 0..1 light line inside the border
uniform float focus;        // 0..1
uniform float reflections;  // 0..1
uniform float glow;         // 0..1
uniform float rim;          // 0..1
uniform float button_style; // 0 orbs, 1 flat glyphs, 2 capsule group, 3 rounded square gel
uniform float button_left;  // 1 = buttons at the left
uniform float radius_bottom;// radius of the bottom corners
uniform float orb_size;     // button diameter in pixels (0 = none)
uniform float orb_gap;
uniform float orb_margin;
uniform vec3 orb_close;
uniform vec3 orb_max;
uniform vec3 orb_min;

// the glyph of a button (0 close, 1 maximize, 2 minimize) around the origin, half size h, line t
float glyph(int k, vec2 o, float h, float t) {
    if (k == 0) {
        float dd = min(abs(o.x - o.y), abs(o.x + o.y)) * 0.70711;
        return (1.0 - smoothstep(t - 0.6, t + 0.6, dd)) * step(max(abs(o.x), abs(o.y)), h + 0.5);
    } else if (k == 1) {
        float q = max(abs(o.x), abs(o.y));
        return (1.0 - smoothstep(t - 0.6, t + 0.6, abs(q - h))) * step(q, h + t + 1.0);
    }
    return (1.0 - smoothstep(t - 0.6, t + 0.6, abs(o.y + h * 0.7))) * step(abs(o.x), h + 0.5);
}

float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + vec2(r);
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 local = (v_coords - uv_origin) / uv_size;       // 0..1 over the window
    vec2 px = local * size;
    vec2 centered = px - size * 0.5;
    float d = rounded_box(centered, size * 0.5, centered.y < 0.0 ? radius : radius_bottom);
    float cover = clamp(0.5 - d, 0.0, 1.0);              // 1 inside, anti-aliased edge
    if (cover <= 0.0) { gl_FragColor = vec4(0.0); return; }

    vec3 base = texture2D(tex, v_coords).rgb;
    // lift saturation a little, like a sheet of tinted glass
    float l = dot(base, vec3(0.299, 0.587, 0.114));
    base = mix(vec3(l), base, 1.35) + vec3(0.03);

    bool in_client = px.x > border && px.x < size.x - border && px.y > border + titlebar && px.y < size.y - border;
    vec4 tint = in_client ? tint_client : tint_frame;
    vec3 rgb = mix(base, tint.rgb, tint.a);

    // gloss band on the upper half of the title bar, its lower edge sagging to the right
    if (px.y < border + titlebar) {
        float t = (px.y - border) / max(titlebar, 1.0);                  // 0 top .. 1 bottom
        float edge = 0.40 + 0.22 * smoothstep(0.0, 1.0, px.x / size.x) - 0.12 * sin(px.x / size.x * 3.14159);
        float band = (1.0 - smoothstep(edge - 0.06, edge + 0.02, t)) * (1.0 - t / max(edge, 0.01) * 0.82);
        rgb = mix(rgb, vec3(1.0), clamp(band, 0.0, 1.0) * gloss * (0.35 + 0.65 * focus));
    }
    // the buttons: close, maximize, minimize
    if (orb_size > 0.0) {
        bool capsule = button_style > 1.5 && button_style < 2.5;
        for (int i = 0; i < 3; i++) {
            vec3 c = i == 0 ? orb_close : (i == 1 ? orb_max : orb_min);
            float r = orb_size * 0.5;
            vec2 center;
            vec2 half_box = vec2(r);
            if (capsule) {
                float w_close = orb_size * 2.6;
                float w_other = orb_size * 1.6;
                float h = orb_size + 1.0;
                float rx = size.x - orb_margin;
                float w = i == 0 ? w_close : w_other;
                float cx = i == 0 ? rx - w_close * 0.5
                         : (i == 1 ? rx - w_close - orb_gap - w_other * 0.5
                                   : rx - w_close - orb_gap * 2.0 - w_other * 1.5);
                center = vec2(cx, 1.0 + h * 0.5);
                half_box = vec2(w * 0.5, h * 0.5);
            } else {
                float step_x = orb_size + orb_gap;
                float slot = float(i);
                if (button_left > 0.5) { slot = i == 0 ? 0.0 : (i == 1 ? 2.0 : 1.0); }
                float cx = button_left > 0.5 ? border + orb_margin + r + slot * step_x
                                             : size.x - border - orb_margin - r - slot * step_x;
                center = vec2(cx, border + titlebar * 0.5);
            }
            vec2 o = px - center;
            float t = max(0.9, orb_size * 0.07);
            float gh = r * 0.62;
            if (capsule) {
                // a glass pill segment: light at the top, a bright line, a dark outline
                float bd = rounded_box(o, half_box, 3.0 * (orb_size / 17.0));
                float cov = clamp(0.5 - bd, 0.0, 1.0);
                if (cov > 0.0) {
                    float v = clamp((o.y / half_box.y) * 0.5 + 0.5, 0.0, 1.0);
                    vec3 body = i == 0 ? mix(vec3(0.93, 0.45, 0.38), vec3(0.74, 0.12, 0.08), smoothstep(0.35, 0.65, v))
                                       : mix(vec3(0.96, 0.98, 1.0), vec3(0.62, 0.72, 0.86), smoothstep(0.35, 0.65, v));
                    body = mix(body, vec3(0.5), (1.0 - focus) * 0.6);
                    body = mix(body, vec3(1.0), (1.0 - smoothstep(0.0, 0.5, v)) * 0.25 * reflections);
                    float edge = smoothstep(1.2, 0.0, abs(bd + 0.5));
                    body = mix(body, vec3(0.05, 0.1, 0.2), edge * 0.55);
                    float inner = smoothstep(1.1, 0.0, abs(bd + 1.8)) * (1.0 - v);
                    body = mix(body, vec3(1.0), inner * 0.5);
                    rgb = mix(rgb, body, cov * (0.55 + 0.45 * focus));
                    vec3 ink = i == 0 ? vec3(1.0) : vec3(0.1, 0.17, 0.26);
                    float g = glyph(i, o, gh * 0.95, t);
                    rgb = mix(rgb, ink, clamp(g, 0.0, 1.0) * cov);
                }
            } else if (button_style > 0.5 && button_style < 1.5) {
                // flat glyphs only
                float g = glyph(i, o, gh, t);
                rgb = mix(rgb, c, clamp(g, 0.0, 1.0) * (0.55 + 0.45 * focus));
            } else if (button_style > 2.5) {
                // a rounded square of gel with a white glyph
                float bd = rounded_box(o, half_box, orb_size * 0.22);
                float cov = clamp(0.5 - bd, 0.0, 1.0);
                if (cov > 0.0) {
                    float v = clamp((o.y / half_box.y) * 0.5 + 0.5, 0.0, 1.0);
                    vec3 body = mix(mix(c, vec3(1.0), 0.45), c * 0.8, smoothstep(0.0, 0.9, v));
                    body = mix(body, vec3(0.5), (1.0 - focus) * 0.6);
                    float edge = smoothstep(1.2, 0.0, abs(bd + 0.5));
                    body = mix(body, vec3(1.0), edge * 0.8);
                    rgb = mix(rgb, body, cov);
                    float g = glyph(i, o, gh * 0.9, t * 1.2);
                    rgb = mix(rgb, vec3(1.0), clamp(g, 0.0, 1.0) * cov);
                }
            } else {
                float od = length(o) - r;
                float orb_cover = clamp(0.5 - od, 0.0, 1.0);
                if (orb_cover > 0.0) {
                    vec2 lp = (o - vec2(-0.3, -0.45) * r) / r;      // light from the upper left
                    float lit = clamp(1.0 - length(lp) * 0.85, 0.0, 1.0);
                    vec3 body = mix(c * 0.62, c, smoothstep(0.0, 0.55, lit));
                    body = mix(body, mix(c, vec3(1.0), 0.55), smoothstep(0.55, 1.0, lit));
                    body = mix(body, vec3(0.5), 1.0 - focus * 0.85);   // dimmed when the window is not focused
                    float glint = smoothstep(0.0, 1.0, 1.0 - length((o - vec2(0.0, -r * 0.42)) / vec2(r * 0.72, r * 0.42)));
                    body = mix(body, vec3(1.0), glint * 0.75 * (0.3 + 0.7 * reflections));
                    float orb_rim = smoothstep(1.2, 0.0, abs(od + 0.5));
                    body = mix(body, vec3(0.0), orb_rim * 0.35);
                    rgb = mix(rgb, body, orb_cover);
                }
            }
        }
    }
    // diagonal reflections across the whole pane: two soft streaks, lit from the upper left
    {
        float u = (px.x + px.y * 0.55) / (size.x + size.y * 0.55);
        float streak = smoothstep(0.10, 0.27, u) * (1.0 - smoothstep(0.27, 0.40, u))      // broad
                     + 0.55 * smoothstep(0.47, 0.52, u) * (1.0 - smoothstep(0.52, 0.60, u)); // narrow
        float fade = 1.0 - smoothstep(0.0, 1.0, px.y / size.y) * 0.55;                     // weaker towards the bottom
        rgb = mix(rgb, vec3(1.0), clamp(streak * fade, 0.0, 1.0) * gloss * reflections * (in_client ? 0.10 : 0.16) * (0.5 + 0.5 * focus));
    }
    // a soft inner glow along the edge, as if the glass were lit from within
    {
        float inside = -d;                                                                  // px from the edge
        float edge_glow = (1.0 - smoothstep(0.0, border * 2.2 + 3.0, inside)) * smoothstep(0.0, 2.0, inside);
        rgb = mix(rgb, vec3(0.85, 0.95, 1.0), edge_glow * glow * 0.34 * (0.45 + 0.55 * focus));
    }
    // light line just inside the border and a dark hairline at the very edge
    float inner = smoothstep(1.2, 0.0, abs(d + border - 0.5));
    rgb = mix(rgb, vec3(1.0), inner * highlight * 0.55);
    float rim_line = smoothstep(1.5, 0.0, abs(d + 0.5));
    rgb = mix(rgb, vec3(0.0), rim_line * rim * 0.25);

    gl_FragColor = vec4(rgb * cover, cover) * alpha;
}
"#;

/// A client surface (premultiplied) clipped to the window's rounded shape.
pub const ROUND_CLIP: &str = r#"
#version 100
//_DEFINES_
#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif
precision highp float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif
uniform float alpha;
varying vec2 v_coords;
uniform vec2 size;
uniform float radius;
float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + vec2(r);
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}
void main() {
    vec4 color = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0);
#endif
    float d = rounded_box(v_coords * size - size * 0.5, size * 0.5, radius);
    gl_FragColor = color * clamp(0.5 - d, 0.0, 1.0) * alpha;
}
"#;

/// A soft drop shadow around a rounded window, drawn under the glass. Nothing inside the
/// window itself (the glass is see-through, a shadow there would show).
pub const SHADOW: &str = r#"
#version 100
//_DEFINES_
precision highp float;
uniform sampler2D tex;
uniform float alpha;
varying vec2 v_coords;
uniform vec2 size;      // the area drawn: the window plus `spread` all round
uniform float spread;   // px between the window and the edge of the area
uniform float radius;   // the window's corner radius
uniform float shift_y;  // how far the shadow falls below the window
uniform float soft;     // how far the shadow fades out
uniform float strength; // 0..1
float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + vec2(r);
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}
void main() {
    vec2 px = v_coords * size - size * 0.5;
    vec2 win_half = size * 0.5 - vec2(spread);
    float d_window = rounded_box(px, win_half, radius);
    float d_shadow = rounded_box(px - vec2(0.0, shift_y), win_half, radius);
    float s = (1.0 - smoothstep(-soft * 0.35, soft, d_shadow));
    s = s * s;                                              // a longer, softer tail
    float outside = smoothstep(-0.5, 0.5, d_window);        // 0 inside the window
    float a = s * outside * strength;
    gl_FragColor = vec4(vec3(0.02, 0.06, 0.14) * a, a) * alpha;
}
"#;
