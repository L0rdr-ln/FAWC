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
uniform float orb_size;     // button diameter in pixels (0 = none)
uniform float orb_gap;
uniform float orb_margin;
uniform vec3 orb_close;
uniform vec3 orb_max;
uniform vec3 orb_min;

float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + vec2(r);
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 local = (v_coords - uv_origin) / uv_size;       // 0..1 over the window
    vec2 px = local * size;
    vec2 centered = px - size * 0.5;
    float d = rounded_box(centered, size * 0.5, radius);
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
    // round glass buttons at the right of the title bar: close, maximize, minimize (right to left)
    if (orb_size > 0.0) {
        for (int i = 0; i < 3; i++) {
            vec3 c = i == 0 ? orb_close : (i == 1 ? orb_max : orb_min);
            vec2 center = vec2(size.x - border - orb_margin - orb_size * 0.5 - float(i) * (orb_size + orb_gap),
                               border + titlebar * 0.5);
            vec2 o = px - center;
            float r = orb_size * 0.5;
            float od = length(o) - r;
            float orb_cover = clamp(0.5 - od, 0.0, 1.0);
            if (orb_cover > 0.0) {
                vec2 lp = (o - vec2(-0.3, -0.45) * r) / r;      // light from the upper left
                float lit = clamp(1.0 - length(lp) * 0.85, 0.0, 1.0);
                vec3 body = mix(c * 0.62, c, smoothstep(0.0, 0.55, lit));
                body = mix(body, mix(c, vec3(1.0), 0.55), smoothstep(0.55, 1.0, lit));
                body = mix(body, vec3(0.5), 1.0 - focus * 0.85);   // dimmed when the window is not focused
                float glint = smoothstep(0.0, 1.0, 1.0 - length((o - vec2(0.0, -r * 0.42)) / vec2(r * 0.72, r * 0.42)));
                body = mix(body, vec3(1.0), glint * 0.75);
                float orb_rim = smoothstep(1.2, 0.0, abs(od + 0.5));
                body = mix(body, vec3(0.0), orb_rim * 0.35);
                rgb = mix(rgb, body, orb_cover);
            }
        }
    }
    // diagonal reflections across the whole pane: two soft streaks, lit from the upper left
    {
        float u = (px.x + px.y * 0.55) / (size.x + size.y * 0.55);
        float streak = smoothstep(0.10, 0.27, u) * (1.0 - smoothstep(0.27, 0.40, u))      // broad
                     + 0.55 * smoothstep(0.47, 0.52, u) * (1.0 - smoothstep(0.52, 0.60, u)); // narrow
        float fade = 1.0 - smoothstep(0.0, 1.0, px.y / size.y) * 0.55;                     // weaker towards the bottom
        rgb = mix(rgb, vec3(1.0), clamp(streak * fade, 0.0, 1.0) * gloss * (in_client ? 0.10 : 0.16) * (0.5 + 0.5 * focus));
    }
    // a soft inner glow along the edge, as if the glass were lit from within
    {
        float inside = -d;                                                                  // px from the edge
        float glow = (1.0 - smoothstep(0.0, border * 2.2 + 3.0, inside)) * smoothstep(0.0, 2.0, inside);
        rgb = mix(rgb, vec3(0.85, 0.95, 1.0), glow * 0.34 * (0.45 + 0.55 * focus));
    }
    // light line just inside the border and a dark hairline at the very edge
    float inner = smoothstep(1.2, 0.0, abs(d + border - 0.5));
    rgb = mix(rgb, vec3(1.0), inner * highlight * 0.55);
    float rim = smoothstep(1.5, 0.0, abs(d + 0.5));
    rgb = mix(rgb, vec3(0.0), rim * 0.25);

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
