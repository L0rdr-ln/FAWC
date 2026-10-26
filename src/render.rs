//! Drawing a frame: wallpaper, then every window bottom to top (blur what is behind it, glass
//! panel, the client), all into an offscreen scene texture.

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement};
use smithay::backend::renderer::element::{AsRenderElements, Kind};
use smithay::backend::renderer::gles::{GlesError, GlesRenderer, GlesTexture};
use smithay::backend::renderer::utils::draw_render_elements;
use smithay::backend::renderer::{Bind, Color32F, Frame, Offscreen, Renderer, Texture};
use smithay::desktop::{PopupManager, Window, WindowSurfaceType};
use smithay::utils::{Buffer as BufferCoord, Logical, Physical, Rectangle, Size, Transform};

use crate::glass::{Glass, GlassStyle};
use crate::layout::outer_rect;
use crate::state::Fawc;

/// The scene texture of an output of `size`.
pub fn create_scene(renderer: &mut GlesRenderer, size: Size<i32, Physical>) -> Result<GlesTexture, GlesError> {
    renderer.create_buffer(Fourcc::Abgr8888, Size::<i32, BufferCoord>::from((size.w, size.h)))
}

fn phys(r: Rectangle<i32, Logical>) -> Rectangle<i32, Physical> {
    Rectangle::new((r.loc.x, r.loc.y).into(), (r.size.w, r.size.h).into())
}

impl Fawc {
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
        {
            let mut target = renderer.bind(scene)?;
            let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
            frame.clear(Color32F::new(0.12, 0.42, 0.70, 1.0), &[full])?;
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

            let mut target = renderer.bind(scene)?;
            let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
            glass.draw_panel_part(&mut frame, &blurred, src, outer_p, &style)?;
            draw_render_elements(&mut frame, 1.0, &elements, &[full])?;
            let _ = frame.finish()?;
        }
        let _ = WindowSurfaceType::ALL;
        Ok(())
    }
}
