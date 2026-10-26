//! Drawing a frame: wallpaper, then every window bottom to top (blur what is behind it, glass
//! panel, the client), all into an offscreen scene texture.

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement};
use smithay::backend::renderer::element::{AsRenderElements, Kind};
use smithay::backend::renderer::gles::{GlesError, GlesRenderer, GlesTexture};
use smithay::backend::renderer::utils::draw_render_elements;
use smithay::backend::renderer::{Bind, Color32F, Frame, ImportMem, Offscreen, Renderer, Texture};
use smithay::desktop::{PopupManager, Window, WindowSurfaceType};
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;
use smithay::utils::{Buffer as BufferCoord, Logical, Physical, Rectangle, Size, Transform};

use crate::glass::{Glass, GlassStyle};
use crate::layout::outer_rect;
use crate::state::{Fawc, TitleTex};

/// The scene texture of an output of `size`.
pub fn create_scene(renderer: &mut GlesRenderer, size: Size<i32, Physical>) -> Result<GlesTexture, GlesError> {
    renderer.create_buffer(Fourcc::Abgr8888, Size::<i32, BufferCoord>::from((size.w, size.h)))
}

fn phys(r: Rectangle<i32, Logical>) -> Rectangle<i32, Physical> {
    Rectangle::new((r.loc.x, r.loc.y).into(), (r.size.w, r.size.h).into())
}

impl Fawc {
    /// The window's title as a texture and where to draw it, or None if there is no title.
    fn title_texture(
        &mut self,
        renderer: &mut GlesRenderer,
        window: &Window,
        style: &GlassStyle,
        outer: Rectangle<i32, Logical>,
        focused: bool,
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
        let pad = 4;
        let buttons = 3.0 * (style.orb_size + style.orb_gap) + style.orb_margin;
        let max_width = outer.size.w - 2 * style.border as i32 - 12 - buttons as i32;
        let id = surface.id();
        let stale = match self.titles.get(&id) {
            Some(t) => t.title != title || t.max_width != max_width || t.focused != focused,
            None => true,
        };
        if stale {
            self.titles.remove(&id);
            let (color, glow) = if focused { ([6, 35, 61], 0.95) } else { ([70, 88, 104], 0.7) };
            if let Some(img) = self.text.line(&title, max_width + 2 * pad, 13.0, color, Some(([255, 255, 255], glow)), pad) {
                let tex = renderer.import_memory(
                    &img.rgba,
                    Fourcc::Abgr8888,
                    Size::<i32, BufferCoord>::from((img.width, img.height)),
                    false,
                )?;
                self.titles.insert(
                    id.clone(),
                    TitleTex { title: title.clone(), max_width, focused, texture: tex, size: (img.width, img.height), pad },
                );
            }
        }
        Ok(self.titles.get(&id).map(|t| {
            let x = outer.loc.x + style.border as i32 + 10 - t.pad;
            let y = outer.loc.y + style.border as i32 + (style.titlebar as i32 - t.size.1) / 2;
            (t.texture.clone(), Rectangle::new((x, y).into(), (t.size.0, t.size.1).into()))
        }))
    }

    /// Draw everything into `scene` (the output's size). `glass` is the shader set.
    pub fn render_scene(
        &mut self,
        renderer: &mut GlesRenderer,
        glass: &Glass,
        scene: &mut GlesTexture,
    ) -> Result<(), GlesError> {
        let size: Size<i32, Physical> = {
            let s = scene.size();
            (s.w, s.h).into()
        };
        let full = Rectangle::<i32, Physical>::from_size(size);

        // wallpaper
        let stale = match &self.wallpaper {
            Some(t) => t.size().w != size.w || t.size().h != size.h,
            None => true,
        };
        if stale {
            let px = crate::wallpaper::procedural(size.w, size.h, false);
            self.wallpaper = Some(renderer.import_memory(
                &px,
                Fourcc::Abgr8888,
                Size::<i32, BufferCoord>::from((size.w, size.h)),
                false,
            )?);
        }
        {
            let mut target = renderer.bind(scene)?;
            let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
            frame.clear(Color32F::new(0.0, 0.0, 0.0, 1.0), &[full])?;
            if let Some(wp) = &self.wallpaper {
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

        // windows, back to front
        let windows: Vec<Window> = self.space.elements().cloned().collect();
        for window in &windows {
            let Some(client) = self.space.element_geometry(window) else { continue };
            let outer = outer_rect(client, &self.style);
            let outer_p = phys(outer);
            let Some(visible) = outer_p.intersection(full) else { continue };
            let focused = self
                .seat
                .get_keyboard()
                .and_then(|k| k.current_focus())
                .and_then(|f| window.toplevel().map(|t| t.wl_surface() == &f))
                .unwrap_or(false);
            let style = GlassStyle { focus: if focused { 1.0 } else { 0.0 }, ..self.style };

            // what is behind the window, blurred
            let margin = 24;
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

            // the client's surfaces (and its popups) as render elements
            let loc = client.loc.to_physical(1);
            let mut elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = Vec::new();
            if let Some(top) = window.toplevel() {
                for (popup, offset) in PopupManager::popups_for_surface(top.wl_surface()) {
                    let popup_loc = loc + (offset - popup.geometry().loc).to_physical(1);
                    elements.extend(render_elements_from_surface_tree(
                        renderer,
                        popup.wl_surface(),
                        popup_loc,
                        1.0,
                        1.0,
                        Kind::Unspecified,
                    ));
                }
            }
            elements.extend(window.render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                renderer,
                loc - window.geometry().loc.to_physical(1),
                1.0.into(),
                1.0,
            ));

            // the title (re-rendered only when it, its room or the focus change)
            let title_draw = self.title_texture(renderer, window, &style, outer, focused)?;

            let mut target = renderer.bind(scene)?;
            let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
            glass.draw_panel_part(&mut frame, &blurred, src, outer_p, &style)?;
            if let Some((tex, dest)) = &title_draw {
                let src_full = Rectangle::<f64, BufferCoord>::from_size((tex.size().w as f64, tex.size().h as f64).into());
                frame.render_texture_from_to(tex, src_full, *dest, &[Rectangle::from_size(dest.size)], &[], Transform::Normal, 1.0, None, &[])?;
            }
            draw_render_elements(&mut frame, 1.0, &elements, &[full])?;
            let _ = frame.finish()?;
        }
        let _ = WindowSurfaceType::ALL;

        if self.draw_cursor {
            if self.cursor_texture.is_none() {
                self.cursor_texture = Some(renderer.import_memory(
                    &crate::cursor::arrow(),
                    Fourcc::Abgr8888,
                    Size::<i32, BufferCoord>::from((crate::cursor::SIZE, crate::cursor::SIZE)),
                    false,
                )?);
            }
            let tex = self.cursor_texture.as_ref().expect("cursor texture");
            let loc = (self.pointer_location.x as i32 - 1, self.pointer_location.y as i32 - 1);
            let dest = Rectangle::<i32, Physical>::new(loc.into(), (crate::cursor::SIZE, crate::cursor::SIZE).into());
            let mut target = renderer.bind(scene)?;
            let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
            frame.render_texture_from_to(
                tex,
                Rectangle::from_size((crate::cursor::SIZE as f64, crate::cursor::SIZE as f64).into()),
                dest,
                &[Rectangle::from_size(dest.size)],
                &[],
                Transform::Normal,
                1.0,
                None,
                &[],
            )?;
            let _ = frame.finish()?;
        }
        Ok(())
    }
}
