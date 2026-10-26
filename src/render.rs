//! Drawing a frame: wallpaper, then every window bottom to top (blur what is behind it, glass
//! panel, the client), all into an offscreen scene texture.

use smithay::wayland::seat::WaylandFocus;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement};
use smithay::backend::renderer::element::{AsRenderElements, Kind};
use smithay::backend::renderer::gles::{GlesError, GlesRenderer, GlesTexture};
use smithay::backend::renderer::utils::draw_render_elements;
use smithay::backend::renderer::{Bind, Color32F, Frame, ImportMem, Offscreen, Renderer, Texture};
use smithay::desktop::{layer_map_for_output, PopupManager, Window, WindowSurfaceType};
use smithay::wayland::shell::wlr_layer::Layer;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;
use smithay::utils::{Buffer as BufferCoord, Logical, Physical, Rectangle, Size, Transform};

use crate::glass::{Glass, GlassStyle};
use crate::layout::outer_rect;
use crate::state::{Fde, TitleTex};

/// The scene texture of an output of `size`.
pub fn create_scene(renderer: &mut GlesRenderer, size: Size<i32, Physical>) -> Result<GlesTexture, GlesError> {
    renderer.create_buffer(Fourcc::Abgr8888, Size::<i32, BufferCoord>::from((size.w, size.h)))
}

/// One monitor as the renderer sees it: where it is in the shared logical space and how many
/// physical pixels one logical pixel is.
#[derive(Clone, Copy)]
struct View {
    origin: smithay::utils::Point<i32, Logical>,
    scale: f64,
}

fn sc(v: i32, s: f64) -> i32 {
    (v as f64 * s).round() as i32
}

impl View {
    /// A rectangle of the shared space as pixels of this monitor's scene.
    fn rect(&self, r: Rectangle<i32, Logical>) -> Rectangle<i32, Physical> {
        let x0 = sc(r.loc.x - self.origin.x, self.scale);
        let y0 = sc(r.loc.y - self.origin.y, self.scale);
        let x1 = sc(r.loc.x - self.origin.x + r.size.w, self.scale);
        let y1 = sc(r.loc.y - self.origin.y + r.size.h, self.scale);
        Rectangle::new((x0, y0).into(), ((x1 - x0).max(0), (y1 - y0).max(0)).into())
    }
}

impl Fde {
    /// The window's title as a texture and where to draw it (in scene pixels; `style` and `outer`
    /// are already in those pixels, `scale` is the monitor's), or None if there is no title.
    fn title_texture(
        &mut self,
        renderer: &mut GlesRenderer,
        window: &Window,
        style: &GlassStyle,
        outer: Rectangle<i32, Physical>,
        focused: bool,
        scale: f64,
    ) -> Result<Option<(GlesTexture, Rectangle<i32, Physical>)>, GlesError> {
        let Some(surface) = window.wl_surface().map(|s| s.into_owned()) else { return Ok(None) };
        let surface = &surface;
        let title = match window.x11_surface() {
            Some(x) => x.title(),
            None => with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .and_then(|d| d.lock().ok().and_then(|d| d.title.clone()))
            })
            .unwrap_or_default(),
        };
        let pad = sc(4, scale).max(1);
        let buttons = crate::layout::buttons_width(style) as i32;
        let max_width = if style.title_center > 0.5 {
            outer.size.w - 2 * (style.border as i32 + buttons) - sc(12, scale)
        } else {
            outer.size.w - 2 * style.border as i32 - sc(12, scale) - buttons
        };
        let key = (surface.id(), (scale * 1000.0) as i32);
        let stale = match self.titles.get(&key) {
            Some(t) => t.title != title || t.max_width != max_width || t.focused != focused,
            None => true,
        };
        if stale {
            self.titles.remove(&key);
            let ink = if focused { style.title_ink } else { style.title_ink_dim };
            let color = [(ink[0] * 255.0).round() as u8, (ink[1] * 255.0).round() as u8, (ink[2] * 255.0).round() as u8];
            let glow = if style.title_glow > 0.0 { Some(([255, 255, 255], style.title_glow * if focused { 1.0 } else { 0.74 })) } else { None };
            if let Some(img) = self.text.line(&title, max_width + 2 * pad, 13.0 * scale as f32, color, glow, pad) {
                let tex = renderer.import_memory(
                    &img.rgba,
                    Fourcc::Abgr8888,
                    Size::<i32, BufferCoord>::from((img.width, img.height)),
                    false,
                )?;
                self.titles.insert(
                    key.clone(),
                    TitleTex { title: title.clone(), max_width, focused, texture: tex, size: (img.width, img.height), pad },
                );
            }
        }
        Ok(self.titles.get(&key).map(|t| {
            let x = if style.title_center > 0.5 {
                outer.loc.x + (outer.size.w - t.size.0) / 2
            } else {
                outer.loc.x + style.border as i32 + sc(10, scale) - t.pad
            };
            let y = outer.loc.y + style.border as i32 + (style.titlebar as i32 - t.size.1) / 2;
            (t.texture.clone(), Rectangle::new((x, y).into(), (t.size.0, t.size.1).into()))
        }))
    }

    /// Draw the layer surfaces of the given layers (in that order) into the scene.
    /// Blur what is behind `rect` (pixels of `scene`) and draw it back there: the effect a
    /// client asks for with ext-background-effect.
    fn blur_behind(
        &self,
        renderer: &mut GlesRenderer,
        glass: &Glass,
        scene: &mut GlesTexture,
        rect: Rectangle<i32, Physical>,
        margin: i32,
    ) -> Result<(), GlesError> {
        let size = scene.size();
        let full = Rectangle::<i32, Physical>::from_size((size.w, size.h).into());
        let Some(rect) = rect.intersection(full) else { return Ok(()) };
        tracing::debug!("blur behind {rect:?}");
        let region = Rectangle::<i32, Physical>::new(
            (rect.loc.x - margin, rect.loc.y - margin).into(),
            (rect.size.w + 2 * margin, rect.size.h + 2 * margin).into(),
        )
        .intersection(full)
        .unwrap_or(rect);
        let blurred = glass.blur(renderer, scene, region, self.style.blur_levels)?;
        let bs = blurred.size();
        let src = Rectangle::<f64, BufferCoord>::new(
            (
                (rect.loc.x - region.loc.x) as f64 / region.size.w as f64 * bs.w as f64,
                (rect.loc.y - region.loc.y) as f64 / region.size.h as f64 * bs.h as f64,
            )
                .into(),
            (
                rect.size.w as f64 / region.size.w as f64 * bs.w as f64,
                rect.size.h as f64 / region.size.h as f64 * bs.h as f64,
            )
                .into(),
        );
        let mut target = renderer.bind(scene)?;
        let mut frame = renderer.render(&mut target, (size.w, size.h).into(), Transform::Normal)?;
        frame.render_texture_from_to(&blurred, src, rect, &[rect], &[], Transform::Normal, 1.0, None, &[])?;
        let _ = frame.finish()?;
        Ok(())
    }

    fn draw_layers(
        &mut self,
        renderer: &mut GlesRenderer,
        glass: &Glass,
        scene: &mut GlesTexture,
        size: Size<i32, Physical>,
        output: &smithay::output::Output,
        view: View,
        layers: &[Layer],
    ) -> Result<(), GlesError> {
        let full = Rectangle::<i32, Physical>::from_size(size);
        for l in layers {
            let surfaces: Vec<_> = {
                let map = layer_map_for_output(output);
                map.layers_on(*l).cloned().collect()
            };
            for ls in surfaces {
                let geo = {
                    let map = layer_map_for_output(output);
                    match map.layer_geometry(&ls) {
                        Some(g) => g,
                        None => continue,
                    }
                };
                // blur behind the part the client asked for
                if let Some(b) = crate::background_effect::blur_bounds(ls.wl_surface()) {
                    let r = Rectangle::<i32, Physical>::new(
                        (sc(geo.loc.x + b.loc.x, view.scale), sc(geo.loc.y + b.loc.y, view.scale)).into(),
                        (sc(b.size.w, view.scale), sc(b.size.h, view.scale)).into(),
                    );
                    self.blur_behind(renderer, glass, scene, r, sc(24, view.scale))?;
                }
                // AsRenderElements gives front-to-back; drawing wants back to front, below
                let mut e = ls.render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                    renderer,
                    (sc(geo.loc.x, view.scale), sc(geo.loc.y, view.scale)).into(),
                    view.scale.into(),
                    1.0,
                );
                e.reverse();
                if e.is_empty() {
                    continue;
                }
                let mut target = renderer.bind(scene)?;
                let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
                draw_render_elements(&mut frame, view.scale, &e, &[full])?;
                let _ = frame.finish()?;
            }
        }
        Ok(())
    }

    /// The session is locked: only the lock screen's surface over the wallpaper. Once a frame
    /// without anything else is done, the lock is confirmed to the client.
    fn draw_lock(
        &mut self,
        renderer: &mut GlesRenderer,
        scene: &mut GlesTexture,
        size: Size<i32, Physical>,
        output: &smithay::output::Output,
        view: View,
    ) -> Result<(), GlesError> {
        let full = Rectangle::<i32, Physical>::from_size(size);
        if let Some((_, ls)) = self.lock_surfaces.iter().find(|(o, _)| o == output) {
            let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                render_elements_from_surface_tree(renderer, ls.wl_surface(), (0, 0), view.scale, 1.0, Kind::Unspecified);
            let mut target = renderer.bind(scene)?;
            let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
            draw_render_elements(&mut frame, view.scale, &elements, &[full])?;
            let _ = frame.finish()?;
        }
        // confirmed when every monitor has a lock surface (or none was ever going to come)
        if self.lock_confirmation.is_some() && self.all_outputs().iter().all(|o| self.lock_surfaces.iter().any(|(lo, _)| lo == o)) {
            if let Some(confirmation) = self.lock_confirmation.take() {
                confirmation.lock();
            }
        }
        Ok(())
    }

    /// Draw the first monitor's picture into `scene` (its size in physical pixels).
    pub fn render_scene(
        &mut self,
        renderer: &mut GlesRenderer,
        glass: &Glass,
        scene: &mut GlesTexture,
    ) -> Result<(), GlesError> {
        let output = self.output.clone();
        self.render_output(renderer, glass, scene, &output)
    }

    /// Draw what `output` shows into `scene` (the output's mode in physical pixels).
    pub fn render_output(
        &mut self,
        renderer: &mut GlesRenderer,
        glass: &Glass,
        scene: &mut GlesTexture,
        output: &smithay::output::Output,
    ) -> Result<(), GlesError> {
        let size: Size<i32, Physical> = {
            let s = scene.size();
            (s.w, s.h).into()
        };
        let full = Rectangle::<i32, Physical>::from_size(size);
        let out_rect = self.output_rect(output);
        let view = View { origin: out_rect.loc, scale: self.output_scale(output) };
        self.sync_foreign_toplevels();
        self.check_config(false);
        self.check_idle();
        self.space.refresh();
        self.tick_animations();

        // wallpaper (one texture per size of monitor)
        let key = (size.w, size.h);
        if !self.wallpapers.contains_key(&key) {
            let px = match &self.wallpaper_image {
                Some(img) => crate::wallpaper::lay_out(img, size.w, size.h, &self.config.wallpaper.mode),
                None => crate::wallpaper::procedural(size.w, size.h, false),
            };
            let tex = renderer.import_memory(&px, Fourcc::Abgr8888, Size::<i32, BufferCoord>::from((size.w, size.h)), false)?;
            self.wallpapers.insert(key, tex);
        }
        {
            let mut target = renderer.bind(scene)?;
            let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
            frame.clear(Color32F::new(0.0, 0.0, 0.0, 1.0), &[full])?;
            if let Some(wp) = self.wallpapers.get(&key) {
                frame.render_texture_from_to(
                    wp,
                    Rectangle::from_size((size.w as f64, size.h as f64).into()),
                    full,
                    &[full],
                    &[],
                    Transform::Normal,
                    1.0,
                    None,
                    &[],
                )?;
            }
            let _ = frame.finish()?;
        }

        if self.locked {
            self.draw_lock(renderer, scene, size, output, view)?;
        } else {
            // wallpaper and bottom layers
            self.draw_layers(renderer, glass, scene, size, output, view, &[Layer::Background, Layer::Bottom])?;

            // windows, back to front
            let windows: Vec<Window> = self.space.elements().cloned().collect();
            let focus = self.seat.get_keyboard().and_then(|k| k.current_focus());
            for window in &windows {
                let Some(client) = self.space.element_geometry(window) else { continue };
                if self.is_override_redirect(window) {
                    let outer_full = Rectangle::<i32, Logical>::new(client.loc, client.size);
                    if outer_full.intersection(out_rect).is_none() {
                        continue; // on another monitor
                    }
                    let k = view.scale;
                    let loc = view.rect(client).loc;
                    let g = window.geometry().loc;
                    let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = window.render_elements(
                        renderer,
                        loc - smithay::utils::Point::<i32, Physical>::from((sc(g.x, k), sc(g.y, k))),
                        smithay::utils::Scale::from(k),
                        1.0,
                    );
                    let mut target = renderer.bind(scene)?;
                    let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
                    draw_render_elements(&mut frame, k, &elements, &[full])?;
                    let _ = frame.finish()?;
                    continue;
                }
                if self.is_fullscreen(window) && self.minimize_progress(window).is_none() {
                    if !self.fullscreen_on(output) || client.loc != out_rect.loc {
                        continue; // on another monitor (or minimized)
                    }
                    let k = view.scale;
                    let mut elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = Vec::new();
                    elements.extend(window.render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                        renderer,
                        (0, 0).into(),
                        smithay::utils::Scale::from(k),
                        1.0,
                    ));
                    let mut target = renderer.bind(scene)?;
                    let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
                    draw_render_elements(&mut frame, k, &elements, &[full])?;
                    let _ = frame.finish()?;
                    continue;
                }
                // opening: fade in and grow from 92 % around the middle
                let progress = self.open_progress(window);
                let e = crate::windows::ease_out(progress);
                let (mut alpha, mut zoom): (f32, f64) = (e, 0.92 + 0.08 * e as f64);
                let outer_full = outer_rect(client, &self.style);
                if outer_full.intersection(out_rect).is_none() {
                    continue; // on another monitor
                }
                let mut center = (
                    outer_full.loc.x as f64 + outer_full.size.w as f64 / 2.0,
                    outer_full.loc.y as f64 + outer_full.size.h as f64 / 2.0,
                );
                // minimizing: fade and shrink towards the task bar button (or the bottom middle)
                let mut shift = (0.0f64, 0.0f64);
                if let Some(p) = self.minimize_progress(window) {
                    let target = self
                        .minimize_target(window)
                        .map(|t| (t.loc.x as f64 + t.size.w as f64 / 2.0, t.loc.y as f64 + t.size.h as f64 / 2.0))
                        .unwrap_or((out_rect.loc.x as f64 + out_rect.size.w as f64 / 2.0, (out_rect.loc.y + out_rect.size.h) as f64));
                    let (a, z, c) = crate::windows::minimize_pose(p, center, target);
                    alpha = a;
                    zoom = z;
                    shift = (c.0 - center.0, c.1 - center.1);
                    center = c;
                }
                let zoomed = |r: Rectangle<i32, Logical>| -> Rectangle<i32, Logical> {
                    let (w, h) = ((r.size.w as f64 * zoom).round() as i32, (r.size.h as f64 * zoom).round() as i32);
                    // scale about the old middle, then move by `shift`
                    let old = (center.0 - shift.0, center.1 - shift.1);
                    let x = (center.0 + (r.loc.x as f64 - old.0) * zoom).round() as i32;
                    let y = (center.1 + (r.loc.y as f64 - old.1) * zoom).round() as i32;
                    Rectangle::new((x, y).into(), (w, h).into())
                };
                let client = zoomed(client);
                let outer = zoomed(outer_full);
                let outer_p = view.rect(outer);
                let Some(visible) = outer_p.intersection(full) else { continue };
                // a window that is bending is drawn into a texture of its own first
                let wob = self
                    .wobbles
                    .iter()
                    .find(|(w, _)| w == window)
                    .filter(|(_, b)| b.displacement() > 0.4)
                    .map(|(_, b)| b.clone());
                // a window that was asked to close is also drawn through a texture, so that its last
                // picture can fade out when it is gone
                let snap = self.close_requested(window);
                let wm = sc(24, view.scale);
                let mut tmp: Option<GlesTexture> = if wob.is_some() || snap {
                    Some(create_scene(renderer, (outer_p.size.w + 2 * wm, outer_p.size.h + 2 * wm).into())?)
                } else {
                    None
                };
                let shift: smithay::utils::Point<i32, Physical> =
                    if tmp.is_some() { (wm - outer_p.loc.x, wm - outer_p.loc.y).into() } else { (0, 0).into() };
                let outer_d = Rectangle::<i32, Physical>::new(outer_p.loc + shift, outer_p.size);
                let tsize: Size<i32, Physical> = match &tmp {
                    Some(t) => (t.size().w, t.size().h).into(),
                    None => size,
                };
                let clip = Rectangle::<i32, Physical>::from_size(tsize);
                let focused = focus
                    .as_ref()
                    .and_then(|f| window.wl_surface().map(|s| &*s == f))
                    .unwrap_or(false);
                let style = GlassStyle { focus: if focused { 1.0 } else { 0.0 }, ..self.style }.scaled(view.scale as f32);

                // what is behind the window, blurred
                let margin = sc(24, view.scale);
                let region = Rectangle::<i32, Physical>::new(
                    (visible.loc.x - margin, visible.loc.y - margin).into(),
                    (visible.size.w + 2 * margin, visible.size.h + 2 * margin).into(),
                )
                .intersection(full)
                .unwrap_or(visible);
                let blurred = glass.blur(renderer, scene, region, style.blur_levels)?;
                let bs = blurred.size();
                let src = Rectangle::<f64, BufferCoord>::new(
                    (
                        (outer_p.loc.x - region.loc.x) as f64 / region.size.w as f64 * bs.w as f64,
                        (outer_p.loc.y - region.loc.y) as f64 / region.size.h as f64 * bs.h as f64,
                    )
                        .into(),
                    (
                        outer_p.size.w as f64 / region.size.w as f64 * bs.w as f64,
                        outer_p.size.h as f64 / region.size.h as f64 * bs.h as f64,
                    )
                        .into(),
                );

                // the client's surfaces (and its popups) as render elements; a surface of
                // logical size L is L * scale * zoom pixels
                let k = view.scale * zoom;
                let loc = view.rect(client).loc + shift;
                let mut elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = Vec::new();
                if let Some(top_surface) = window.wl_surface().map(|s| s.into_owned()) {
                    for (popup, offset) in PopupManager::popups_for_surface(&top_surface) {
                        let d = offset - popup.geometry().loc;
                        let popup_loc = loc + smithay::utils::Point::<i32, Physical>::from((sc(d.x, k), sc(d.y, k)));
                        elements.extend(render_elements_from_surface_tree(
                            renderer,
                            popup.wl_surface(),
                            popup_loc,
                            k,
                            alpha,
                            Kind::Unspecified,
                        ));
                    }
                }
                let g = window.geometry().loc;
                elements.extend(window.render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                    renderer,
                    loc - smithay::utils::Point::<i32, Physical>::from((sc(g.x, k), sc(g.y, k))),
                    smithay::utils::Scale::from(k),
                    alpha,
                ));

                // the title (re-rendered only when it, its room or the focus change)
                let title_draw = self.title_texture(renderer, window, &style, outer_d, focused, view.scale)?;

                let to_texture = tmp.is_some();
                let target_tex: &mut GlesTexture = match tmp.as_mut() {
                    Some(t) => t,
                    None => &mut *scene,
                };
                let mut target = renderer.bind(target_tex)?;
                let mut frame = renderer.render(&mut target, tsize, Transform::Normal)?;
                if to_texture {
                    frame.clear(Color32F::new(0.0, 0.0, 0.0, 0.0), &[clip])?;
                }
                glass.draw_shadow(&mut frame, &blurred, outer_d, &style, view.scale as f32, alpha)?;
                glass.draw_panel_part(&mut frame, &blurred, src, outer_d, &style, alpha)?;
                if let Some((tex, dest)) = &title_draw {
                    let src_full = Rectangle::<f64, BufferCoord>::from_size((tex.size().w as f64, tex.size().h as f64).into());
                    frame.render_texture_from_to(tex, src_full, *dest, &[Rectangle::from_size(dest.size)], &[], Transform::Normal, alpha, None, &[])?;
                }
                draw_render_elements(&mut frame, k, &elements, &[clip])?;
                let _ = frame.finish()?;
                drop(target);

                // closing: draw the window from its texture and keep the texture
                if let (Some(tex), None) = (&tmp, &wob) {
                    let dest = Rectangle::<i32, Physical>::new((outer_p.loc.x - wm, outer_p.loc.y - wm).into(), (tex.size().w, tex.size().h).into());
                    let src = Rectangle::<f64, BufferCoord>::from_size((tex.size().w as f64, tex.size().h as f64).into());
                    let mut target = renderer.bind(scene)?;
                    let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
                    frame.render_texture_from_to(tex, src, dest, &[Rectangle::from_size(dest.size)], &[], Transform::Normal, 1.0, None, &[])?;
                    let _ = frame.finish()?;
                    drop(target);
                    if let Some(ws) = window.wl_surface() {
                        let margin = 24;
                        self.snapshots.insert(
                            ws.id(),
                            crate::state::Snapshot {
                                texture: tex.clone(),
                                rect: Rectangle::new((outer_full.loc.x - margin, outer_full.loc.y - margin).into(), (outer_full.size.w + 2 * margin, outer_full.size.h + 2 * margin).into()),
                                scale: view.scale,
                            },
                        );
                    }
                }

                // the bent window: the texture drawn as a grid of small pieces
                if let (Some(tex), Some(b)) = (&tmp, &wob) {
                    let n = crate::wobble::N;
                    let points: Vec<(f32, f32)> = (0..n * n)
                        .map(|k| {
                            let q = b.pos[k / n][k % n];
                            (((q.0 - view.origin.x as f64) * view.scale) as f32, ((q.1 - view.origin.y as f64) * view.scale) as f32)
                        })
                        .collect();
                    let mut target = renderer.bind(scene)?;
                    let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
                    glass.mesh.draw(&mut frame, tex, (size.w, size.h), &points, n)?;
                    let _ = frame.finish()?;
                }
            }
            let _ = WindowSurfaceType::ALL;

            // windows that have just closed fade out
            let ghosts: Vec<(crate::state::Snapshot, f32)> =
                self.ghosts.iter().map(|g| (g.snapshot.clone(), g.since.elapsed().as_secs_f32() / crate::windows::CLOSE_SECONDS)).collect();
            for (g, t) in ghosts {
                if t >= 1.0 || (g.scale - view.scale).abs() > 1e-6 || g.rect.intersection(out_rect).is_none() {
                    continue;
                }
                let e = crate::windows::ease_out(t);
                let (alpha, zoom) = (1.0 - e, 1.0 - 0.06 * e as f64);
                let full_rect = view.rect(g.rect);
                let (w, h) = ((full_rect.size.w as f64 * zoom).round() as i32, (full_rect.size.h as f64 * zoom).round() as i32);
                let dest = Rectangle::<i32, Physical>::new(
                    (full_rect.loc.x + (full_rect.size.w - w) / 2, full_rect.loc.y + (full_rect.size.h - h) / 2).into(),
                    (w, h).into(),
                );
                let src = Rectangle::<f64, BufferCoord>::from_size((g.texture.size().w as f64, g.texture.size().h as f64).into());
                let mut target = renderer.bind(scene)?;
                let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
                frame.render_texture_from_to(&g.texture, src, dest, &[Rectangle::from_size(dest.size)], &[], Transform::Normal, alpha, None, &[])?;
                let _ = frame.finish()?;
            }

            // panels, launchers, notifications above the windows
            if self.fullscreen_on(output) {
                self.draw_layers(renderer, glass, scene, size, output, view, &[Layer::Overlay])?;
            } else {
                self.draw_layers(renderer, glass, scene, size, output, view, &[Layer::Top, Layer::Overlay])?;
            }
        }

        // the window switcher: the windows as small pictures, the chosen one lit
        let pointer_here = self.output_at(self.pointer_location).map(|o| &o == output).unwrap_or(false);
        if self.switcher.is_some() && pointer_here {
            self.draw_switcher(renderer, scene, size, view)?;
        }

        if self.help_open && pointer_here {
            self.draw_help(renderer, scene, size, view)?;
        }

        // "Workspace 2" for a moment after switching
        if let Some((since, text)) = self.hud.clone() {
            let age = since.elapsed().as_secs_f32();
            if age > 1.0 {
                self.hud = None;
            } else if let Some(img) = self.text.line(&text, 600, 34.0 * view.scale as f32, [255, 255, 255], Some(([8, 40, 90], 0.95)), (6.0 * view.scale) as i32) {
                let tex = renderer.import_memory(&img.rgba, Fourcc::Abgr8888, Size::<i32, BufferCoord>::from((img.width, img.height)), false)?;
                let alpha = if age < 0.7 { 1.0 } else { 1.0 - (age - 0.7) / 0.3 };
                let dest = Rectangle::<i32, Physical>::new(((size.w - img.width) / 2, size.h / 4 - img.height / 2).into(), (img.width, img.height).into());
                let src = Rectangle::<f64, BufferCoord>::from_size((img.width as f64, img.height as f64).into());
                let mut target = renderer.bind(scene)?;
                let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
                frame.render_texture_from_to(&tex, src, dest, &[Rectangle::from_size(dest.size)], &[], Transform::Normal, alpha, None, &[])?;
                let _ = frame.finish()?;
            }
        }

        // screenshots that want no cursor are taken before it is drawn
        self.fulfill_screencopy(renderer, output, scene, false);

        // our own mouse cursor, on the monitor the pointer is on
        let p = self.pointer_location;
        let on_this = p.x >= out_rect.loc.x as f64
            && p.y >= out_rect.loc.y as f64
            && p.x < (out_rect.loc.x + out_rect.size.w) as f64
            && p.y < (out_rect.loc.y + out_rect.size.h) as f64;
        if self.draw_cursor && on_this {
            self.draw_cursor_on(renderer, scene, size, view)?;
        }
        self.fulfill_screencopy(renderer, output, scene, true);

        // night light last, so screenshots keep their colours
        if let Some(t) = self.night_light_tint() {
            let px = [(t[0] * 255.0).round() as u8, (t[1] * 255.0).round() as u8, (t[2] * 255.0).round() as u8, 255];
            let tex = renderer.import_memory(&px, Fourcc::Abgr8888, Size::<i32, BufferCoord>::from((1, 1)), false)?;
            let (w, h) = (size.w as f32, size.h as f32);
            let mut target = renderer.bind(scene)?;
            let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
            glass.mesh.draw_multiply(&mut frame, &tex, (size.w, size.h), &[(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)], 2)?;
            let _ = frame.finish()?;
        }
        Ok(())
    }

    /// The shortcut cheat sheet: a dimmed screen and a panel with the keys and what they do.
    fn draw_help(
        &mut self,
        renderer: &mut GlesRenderer,
        scene: &mut GlesTexture,
        size: Size<i32, Physical>,
        view: View,
    ) -> Result<(), GlesError> {
        let full = Rectangle::<i32, Physical>::from_size(size);
        let (row_h, key_w, desc_w, pad) = (sc(26, view.scale), sc(210, view.scale), sc(420, view.scale), sc(22, view.scale));
        let rows = crate::input::SHORTCUTS.len() as i32;
        let (pw, ph) = (key_w + desc_w + 2 * pad, (rows + 2) * row_h + 2 * pad);
        let panel = Rectangle::<i32, Physical>::new(((size.w - pw) / 2, (size.h - ph) / 2).into(), (pw.min(size.w), ph.min(size.h)).into());
        let mut lines: Vec<(GlesTexture, Rectangle<i32, Physical>)> = Vec::new();
        let px = 14.0 * view.scale as f32;
        let glow = Some(([8, 40, 90], 0.9));
        let mut add = |text: &str, x: i32, y: i32, width: i32, size_px: f32, color: [u8; 3], this: &mut Fde, renderer: &mut GlesRenderer| -> Result<(), GlesError> {
            if let Some(img) = this.text.line(text, width, size_px, color, glow, sc(3, view.scale)) {
                let tex = renderer.import_memory(&img.rgba, Fourcc::Abgr8888, Size::<i32, BufferCoord>::from((img.width, img.height)), false)?;
                lines.push((tex, Rectangle::new((x, y).into(), (img.width, img.height).into())));
            }
            Ok(())
        };
        add("Keyboard shortcuts", panel.loc.x + pad, panel.loc.y + pad, pw - 2 * pad, 18.0 * view.scale as f32, [255, 255, 255], self, renderer)?;
        for (i, (keys, what)) in crate::input::SHORTCUTS.iter().enumerate() {
            let y = panel.loc.y + pad + (i as i32 + 2) * row_h;
            add(keys, panel.loc.x + pad, y, key_w, px, [190, 230, 255], self, renderer)?;
            add(what, panel.loc.x + pad + key_w, y, desc_w, px, [255, 255, 255], self, renderer)?;
        }
        let mut target = renderer.bind(scene)?;
        let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
        frame.draw_solid(full, &[full], Color32F::new(0.0, 0.0, 0.0, 0.45))?;
        frame.draw_solid(panel, &[Rectangle::from_size(panel.size)], Color32F::new(0.05, 0.16, 0.32, 0.85))?;
        for (tex, dest) in &lines {
            let src = Rectangle::<f64, BufferCoord>::from_size((tex.size().w as f64, tex.size().h as f64).into());
            frame.render_texture_from_to(tex, src, *dest, &[Rectangle::from_size(dest.size)], &[], Transform::Normal, 1.0, None, &[])?;
        }
        let _ = frame.finish()?;
        Ok(())
    }

    /// The switcher over the whole monitor: a dimmed backdrop and a row of window pictures with titles.
    fn draw_switcher(
        &mut self,
        renderer: &mut GlesRenderer,
        scene: &mut GlesTexture,
        size: Size<i32, Physical>,
        view: View,
    ) -> Result<(), GlesError> {
        let Some(sw) = &self.switcher else { return Ok(()) };
        let (windows, index) = (sw.windows.clone(), sw.index);
        let full = Rectangle::<i32, Physical>::from_size(size);
        let (slot_w, slot_h, gap, pad) = (sc(210, view.scale), sc(150, view.scale), sc(14, view.scale), sc(18, view.scale));
        let per_row = (((size.w - 2 * pad - sc(60, view.scale)) / (slot_w + gap)).max(1) as usize).min(windows.len());
        // a window of the row that always contains the selection
        let first = if index < per_row { 0 } else { index + 1 - per_row };
        let shown: Vec<(usize, Window)> = windows.iter().cloned().enumerate().skip(first).take(per_row).collect();
        let row_w = shown.len() as i32 * slot_w + (shown.len() as i32 - 1) * gap;
        let (bg_w, bg_h) = (row_w + 2 * pad, slot_h + 2 * pad);
        let bg = Rectangle::<i32, Physical>::new(((size.w - bg_w) / 2, (size.h - bg_h) / 2).into(), (bg_w, bg_h).into());
        let x0 = bg.loc.x + pad;
        let y0 = bg.loc.y + pad;

        // what to draw, collected first (the text renderer needs `self`)
        let mut thumbs: Vec<(Vec<WaylandSurfaceRenderElement<GlesRenderer>>, f64)> = Vec::new();
        let mut titles: Vec<(GlesTexture, Rectangle<i32, Physical>)> = Vec::new();
        for (slot, (_, w)) in shown.iter().enumerate() {
            let sx = x0 + slot as i32 * (slot_w + gap);
            let g = w.geometry();
            let (gw, gh) = (g.size.w.max(1) as f64, g.size.h.max(1) as f64);
            let room_w = (slot_w - sc(16, view.scale)) as f64;
            let room_h = (slot_h - sc(36, view.scale)) as f64;
            let k = (room_w / gw).min(room_h / gh).min(view.scale); // never bigger than it is
            let (tw, th) = ((gw * k).round() as i32, (gh * k).round() as i32);
            let origin = smithay::utils::Point::<i32, Physical>::from((sx + (slot_w - tw) / 2, y0 + sc(8, view.scale) + (room_h as i32 - th) / 2));
            let at = origin - smithay::utils::Point::<i32, Physical>::from(((g.loc.x as f64 * k).round() as i32, (g.loc.y as f64 * k).round() as i32));
            let elements = w.render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(renderer, at, smithay::utils::Scale::from(k), 1.0);
            thumbs.push((elements, k));
            let title = self.window_title(w);
            if let Some(img) = self.text.line(&title, slot_w - sc(10, view.scale), 12.0 * view.scale as f32, [255, 255, 255], Some(([8, 40, 90], 0.9)), sc(3, view.scale)) {
                let tex = renderer.import_memory(&img.rgba, Fourcc::Abgr8888, Size::<i32, BufferCoord>::from((img.width, img.height)), false)?;
                let dest = Rectangle::<i32, Physical>::new((sx + (slot_w - img.width) / 2, y0 + slot_h - sc(24, view.scale)).into(), (img.width, img.height).into());
                titles.push((tex, dest));
            }
        }

        let mut target = renderer.bind(scene)?;
        let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
        frame.draw_solid(full, &[full], Color32F::new(0.0, 0.0, 0.0, 0.35))?;
        frame.draw_solid(bg, &[Rectangle::from_size(bg.size)], Color32F::new(0.05, 0.16, 0.32, 0.72))?;
        for (slot, (i, _)) in shown.iter().enumerate() {
            let sx = x0 + slot as i32 * (slot_w + gap);
            if *i == index {
                let lit = Rectangle::<i32, Physical>::new((sx - sc(6, view.scale), y0 - sc(6, view.scale)).into(), (slot_w + sc(12, view.scale), slot_h + sc(12, view.scale)).into());
                frame.draw_solid(lit, &[Rectangle::from_size(lit.size)], Color32F::new(0.45, 0.75, 1.0, 0.55))?;
            }
            let inner = Rectangle::<i32, Physical>::new((sx, y0).into(), (slot_w, slot_h).into());
            frame.draw_solid(inner, &[Rectangle::from_size(inner.size)], Color32F::new(0.0, 0.0, 0.0, 0.25))?;
        }
        for (elements, k) in &thumbs {
            draw_render_elements(&mut frame, *k, elements, &[full])?;
        }
        for (tex, dest) in &titles {
            let src = Rectangle::<f64, BufferCoord>::from_size((tex.size().w as f64, tex.size().h as f64).into());
            frame.render_texture_from_to(tex, src, *dest, &[Rectangle::from_size(dest.size)], &[], Transform::Normal, 1.0, None, &[])?;
        }
        let _ = frame.finish()?;
        Ok(())
    }

    /// The cursor: what the client under the pointer asked for (a name from the theme, a picture
    /// of its own, or none), else what the compositor wants (resize arrows on borders).
    fn draw_cursor_on(
        &mut self,
        renderer: &mut GlesRenderer,
        scene: &mut GlesTexture,
        size: Size<i32, Physical>,
        view: View,
    ) -> Result<(), GlesError> {
        use smithay::input::pointer::CursorImageStatus;
        let full = Rectangle::<i32, Physical>::from_size(size);
        let p = self.pointer_location;
        let pos = (sc((p.x - view.origin.x as f64) as i32, view.scale), sc((p.y - view.origin.y as f64) as i32, view.scale));
        let client_has_pointer = self.seat.get_pointer().map(|ptr| ptr.current_focus().is_some()).unwrap_or(false);
        let status = if client_has_pointer { self.cursor_status.clone() } else { CursorImageStatus::Named(smithay::input::pointer::CursorIcon::Default) };
        let name: String = match &status {
            CursorImageStatus::Hidden => return Ok(()),
            CursorImageStatus::Surface(surface) => {
                // a picture the client drew itself, with its hot spot
                let hot = with_states(surface, |s| {
                    s.data_map
                        .get::<smithay::input::pointer::CursorImageSurfaceData>()
                        .map(|d| d.lock().unwrap().hotspot)
                        .unwrap_or_default()
                });
                let loc: smithay::utils::Point<i32, Physical> = (pos.0 - sc(hot.x, view.scale), pos.1 - sc(hot.y, view.scale)).into();
                let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                    render_elements_from_surface_tree(renderer, surface, loc, view.scale, 1.0, Kind::Cursor);
                let mut target = renderer.bind(scene)?;
                let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
                draw_render_elements(&mut frame, view.scale, &elements, &[full])?;
                let _ = frame.finish()?;
                return Ok(());
            }
            CursorImageStatus::Named(icon) => {
                // the client asked for a name; where the compositor owns the pointer, its choice
                if client_has_pointer { icon.name().to_string() } else { self.hover_cursor.to_string() }
            }
        };
        let scale_px = view.scale.round().max(1.0) as u32;
        let key = format!("{name}@{scale_px}");
        if !self.cursor_textures.contains_key(&key) {
            let img = self.cursor_theme.image(&name, scale_px);
            let tex = renderer.import_memory(&img.rgba, Fourcc::Abgr8888, Size::<i32, BufferCoord>::from((img.width, img.height)), false)?;
            self.cursor_textures.insert(key.clone(), (tex, img));
        }
        let (tex, img) = self.cursor_textures.get(&key).expect("cursor texture");
        let dest = Rectangle::<i32, Physical>::new((pos.0 - img.xhot, pos.1 - img.yhot).into(), (img.width, img.height).into());
        let mut target = renderer.bind(scene)?;
        let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
        frame.render_texture_from_to(
            tex,
            Rectangle::from_size((img.width as f64, img.height as f64).into()),
            dest,
            &[Rectangle::from_size(dest.size)],
            &[],
            Transform::Normal,
            1.0,
            None,
            &[],
        )?;
        let _ = frame.finish()?;
        Ok(())
    }
}
