//! Drawing a frame: wallpaper, then every window bottom to top (blur what is behind it, glass
//! panel, the client), all into an offscreen scene texture.

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
        let Some(top) = window.toplevel() else { return Ok(None) };
        let surface = top.wl_surface();
        let title = with_states(surface, |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok().and_then(|d| d.title.clone()))
        })
        .unwrap_or_default();
        let pad = sc(4, scale).max(1);
        let buttons = 3.0 * (style.orb_size + style.orb_gap) + style.orb_margin;
        let max_width = outer.size.w - 2 * style.border as i32 - sc(12, scale) - buttons as i32;
        let key = (surface.id(), (scale * 1000.0) as i32);
        let stale = match self.titles.get(&key) {
            Some(t) => t.title != title || t.max_width != max_width || t.focused != focused,
            None => true,
        };
        if stale {
            self.titles.remove(&key);
            let (color, glow) = if focused { ([6, 35, 61], 0.95) } else { ([70, 88, 104], 0.7) };
            if let Some(img) = self.text.line(&title, max_width + 2 * pad, 13.0 * scale as f32, color, Some(([255, 255, 255], glow)), pad) {
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
            let x = outer.loc.x + style.border as i32 + sc(10, scale) - t.pad;
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
                let wm = sc(24, view.scale);
                let mut tmp: Option<GlesTexture> = match &wob {
                    Some(_) => Some(create_scene(renderer, (outer_p.size.w + 2 * wm, outer_p.size.h + 2 * wm).into())?),
                    None => None,
                };
                let shift: smithay::utils::Point<i32, Physical> =
                    if wob.is_some() { (wm - outer_p.loc.x, wm - outer_p.loc.y).into() } else { (0, 0).into() };
                let outer_d = Rectangle::<i32, Physical>::new(outer_p.loc + shift, outer_p.size);
                let tsize: Size<i32, Physical> = match &tmp {
                    Some(t) => (t.size().w, t.size().h).into(),
                    None => size,
                };
                let clip = Rectangle::<i32, Physical>::from_size(tsize);
                let focused = focus
                    .as_ref()
                    .and_then(|f| window.toplevel().map(|t| t.wl_surface() == f))
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
                if let Some(top) = window.toplevel() {
                    for (popup, offset) in PopupManager::popups_for_surface(top.wl_surface()) {
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

                let target_tex: &mut GlesTexture = match tmp.as_mut() {
                    Some(t) => t,
                    None => &mut *scene,
                };
                let mut target = renderer.bind(target_tex)?;
                let mut frame = renderer.render(&mut target, tsize, Transform::Normal)?;
                if wob.is_some() {
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

                // the bent window: the texture drawn as a grid of small pieces
                if let (Some(tex), Some(b)) = (&tmp, &wob) {
                    let (tw, th) = (tex.size().w as f64, tex.size().h as f64);
                    let n = crate::wobble::N;
                    let p = |i: usize, j: usize| -> (f64, f64) {
                        ((b.pos[i][j].0 - view.origin.x as f64) * view.scale, (b.pos[i][j].1 - view.origin.y as f64) * view.scale)
                    };
                    let mut target = renderer.bind(scene)?;
                    let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
                    for i in 0..n - 1 {
                        for j in 0..n - 1 {
                            let (tl, tr, bl, br) = (p(i, j), p(i, j + 1), p(i + 1, j), p(i + 1, j + 1));
                            let x0 = ((tl.0 + bl.0) / 2.0).round() as i32;
                            let x1 = ((tr.0 + br.0) / 2.0).round() as i32;
                            let y0 = ((tl.1 + tr.1) / 2.0).round() as i32;
                            let y1 = ((bl.1 + br.1) / 2.0).round() as i32;
                            if x1 <= x0 || y1 <= y0 {
                                continue;
                            }
                            let dest = Rectangle::<i32, Physical>::new((x0, y0).into(), (x1 - x0, y1 - y0).into());
                            if dest.intersection(full).is_none() {
                                continue;
                            }
                            let cell = Rectangle::<f64, BufferCoord>::new(
                                (tw * j as f64 / (n - 1) as f64, th * i as f64 / (n - 1) as f64).into(),
                                (tw / (n - 1) as f64, th / (n - 1) as f64).into(),
                            );
                            frame.render_texture_from_to(tex, cell, dest, &[Rectangle::from_size(dest.size)], &[], Transform::Normal, 1.0, None, &[])?;
                        }
                    }
                    let _ = frame.finish()?;
                }
            }
            let _ = WindowSurfaceType::ALL;

            // panels, launchers, notifications above the windows
            if self.fullscreen_on(output) {
                self.draw_layers(renderer, glass, scene, size, output, view, &[Layer::Overlay])?;
            } else {
                self.draw_layers(renderer, glass, scene, size, output, view, &[Layer::Top, Layer::Overlay])?;
            }
        }

        // our own mouse cursor, on the monitor the pointer is on
        let p = self.pointer_location;
        let on_this = p.x >= out_rect.loc.x as f64
            && p.y >= out_rect.loc.y as f64
            && p.x < (out_rect.loc.x + out_rect.size.w) as f64
            && p.y < (out_rect.loc.y + out_rect.size.h) as f64;
        if self.draw_cursor && on_this {
            self.draw_cursor_on(renderer, scene, size, view)?;
        }
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
