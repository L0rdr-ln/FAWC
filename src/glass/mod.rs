//! The glass renderer: dual Kawase blur of what is behind a window, and the glass panel drawn
//! over it, all on the GPU through Smithay's GLES renderer.

pub mod shaders;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType,
};
use smithay::backend::renderer::{Bind, Frame, Offscreen, Renderer, Texture};
use smithay::utils::{Buffer as BufferCoord, Physical, Rectangle, Size, Transform};

/// How a window's glass looks.
#[derive(Debug, Clone, Copy)]
pub struct GlassStyle {
    pub radius: f32,
    pub border: f32,
    pub titlebar: f32,
    /// tint over the border and title bar (r, g, b, strength)
    pub tint_frame: [f32; 4],
    /// tint over the client area
    pub tint_client: [f32; 4],
    pub gloss: f32,
    pub highlight: f32,
    pub focus: f32,
    /// blur: number of down/up levels (more = blurrier) and the offset multiplier of a pass
    pub blur_levels: usize,
}

impl Default for GlassStyle {
    fn default() -> Self {
        GlassStyle {
            radius: 12.0,
            border: 5.0,
            titlebar: 32.0,
            tint_frame: [0.29, 0.66, 0.93, 0.38],
            tint_client: [0.85, 0.93, 1.0, 0.10],
            gloss: 0.8,
            highlight: 0.9,
            focus: 1.0,
            blur_levels: 3,
        }
    }
}

pub struct Glass {
    down: GlesTexProgram,
    up: GlesTexProgram,
    panel: GlesTexProgram,
    clip: GlesTexProgram,
}

fn rect_full(w: i32, h: i32) -> Rectangle<i32, Physical> {
    Rectangle::from_size((w, h).into())
}

impl Glass {
    pub fn new(renderer: &mut GlesRenderer) -> Result<Glass, GlesError> {
        let hp = [UniformName::new("half_pixel", UniformType::_2f)];
        let panel_uniforms = [
            UniformName::new("size", UniformType::_2f),
            UniformName::new("uv_origin", UniformType::_2f),
            UniformName::new("uv_size", UniformType::_2f),
            UniformName::new("radius", UniformType::_1f),
            UniformName::new("border", UniformType::_1f),
            UniformName::new("titlebar", UniformType::_1f),
            UniformName::new("tint_frame", UniformType::_4f),
            UniformName::new("tint_client", UniformType::_4f),
            UniformName::new("gloss", UniformType::_1f),
            UniformName::new("highlight", UniformType::_1f),
            UniformName::new("focus", UniformType::_1f),
        ];
        let clip_uniforms = [
            UniformName::new("size", UniformType::_2f),
            UniformName::new("radius", UniformType::_1f),
        ];
        Ok(Glass {
            down: renderer.compile_custom_texture_shader(shaders::BLUR_DOWN, &hp)?,
            up: renderer.compile_custom_texture_shader(shaders::BLUR_UP, &hp)?,
            panel: renderer.compile_custom_texture_shader(shaders::GLASS, &panel_uniforms)?,
            clip: renderer.compile_custom_texture_shader(shaders::ROUND_CLIP, &clip_uniforms)?,
        })
    }

    /// One pass of the blur: `src` (the part `src_rect`, in its pixels) into a new texture of
    /// `out_size`.
    fn pass(
        &self,
        renderer: &mut GlesRenderer,
        program: &GlesTexProgram,
        src: &GlesTexture,
        src_rect: Rectangle<f64, BufferCoord>,
        out_size: (i32, i32),
        offset: f32,
    ) -> Result<GlesTexture, GlesError> {
        let mut out: GlesTexture =
            renderer.create_buffer(Fourcc::Abgr8888, Size::from((out_size.0, out_size.1)))?;
        let tex_size = src.size();
        // half a texel of the source, times the offset of this pass
        let half = [offset * 0.5 / tex_size.w as f32, offset * 0.5 / tex_size.h as f32];
        {
            let mut target = renderer.bind(&mut out)?;
            let mut frame = renderer.render(&mut target, (out_size.0, out_size.1).into(), Transform::Normal)?;
            frame.render_texture_from_to(
                src,
                src_rect,
                rect_full(out_size.0, out_size.1),
                &[rect_full(out_size.0, out_size.1)],
                &[],
                Transform::Normal,
                1.0,
                Some(program),
                &[Uniform::new("half_pixel", (half[0], half[1]))],
            )?;
            let _ = frame.finish()?;
        }
        Ok(out)
    }

    /// Blur the part `region` (pixels of `scene`) and return the blurred texture (about the
    /// region's size divided by two).
    pub fn blur(
        &self,
        renderer: &mut GlesRenderer,
        scene: &GlesTexture,
        region: Rectangle<i32, Physical>,
        levels: usize,
    ) -> Result<GlesTexture, GlesError> {
        let levels = levels.clamp(1, 6);
        let src_rect = Rectangle::<f64, BufferCoord>::new(
            (region.loc.x as f64, region.loc.y as f64).into(),
            (region.size.w as f64, region.size.h as f64).into(),
        );
        let mut sizes = Vec::new();
        let (mut w, mut h) = (region.size.w, region.size.h);
        for _ in 0..levels {
            w = (w / 2).max(1);
            h = (h / 2).max(1);
            sizes.push((w, h));
        }
        // down
        let mut chain: Vec<GlesTexture> = Vec::new();
        for (i, s) in sizes.iter().enumerate() {
            let tex = if i == 0 {
                self.pass(renderer, &self.down, scene, src_rect, *s, 1.0)?
            } else {
                let prev = &chain[i - 1];
                let full = Rectangle::<f64, BufferCoord>::from_size(
                    (prev.size().w as f64, prev.size().h as f64).into(),
                );
                self.pass(renderer, &self.down, prev, full, *s, 1.0)?
            };
            chain.push(tex);
        }
        // up
        let mut current = chain.pop().expect("at least one level");
        while let Some(next) = chain.pop() {
            let s = next.size();
            let full = Rectangle::<f64, BufferCoord>::from_size(
                (current.size().w as f64, current.size().h as f64).into(),
            );
            current = self.pass(renderer, &self.up, &current, full, (s.w, s.h), 1.0)?;
        }
        Ok(current)
    }

    /// Draw the glass panel of a window into `frame`: `blurred` covers `dest` (the window's outer
    /// rectangle in the frame's pixels).
    pub fn draw_panel(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        blurred: &GlesTexture,
        dest: Rectangle<i32, Physical>,
        style: &GlassStyle,
    ) -> Result<(), GlesError> {
        let full = Rectangle::<f64, BufferCoord>::from_size(
            (blurred.size().w as f64, blurred.size().h as f64).into(),
        );
        frame.render_texture_from_to(
            blurred,
            full,
            dest,
            &[Rectangle::from_size(dest.size)],
            &[],
            Transform::Normal,
            1.0,
            Some(&self.panel),
            &[
                Uniform::new("size", (dest.size.w as f32, dest.size.h as f32)),
                Uniform::new("uv_origin", (0.0f32, 0.0f32)),
                Uniform::new("uv_size", (1.0f32, 1.0f32)),
                Uniform::new("radius", style.radius),
                Uniform::new("border", style.border),
                Uniform::new("titlebar", style.titlebar),
                Uniform::new("tint_frame", (style.tint_frame[0], style.tint_frame[1], style.tint_frame[2], style.tint_frame[3])),
                Uniform::new("tint_client", (style.tint_client[0], style.tint_client[1], style.tint_client[2], style.tint_client[3])),
                Uniform::new("gloss", style.gloss),
                Uniform::new("highlight", style.highlight),
                Uniform::new("focus", style.focus),
            ],
        )
    }

    /// Like `draw_panel`, but only the part `src` of `blurred` lies behind the window.
    pub fn draw_panel_part(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        blurred: &GlesTexture,
        src: Rectangle<f64, BufferCoord>,
        dest: Rectangle<i32, Physical>,
        style: &GlassStyle,
    ) -> Result<(), GlesError> {
        let bs = blurred.size();
        frame.render_texture_from_to(
            blurred,
            src,
            dest,
            &[Rectangle::from_size(dest.size)],
            &[],
            Transform::Normal,
            1.0,
            Some(&self.panel),
            &[
                Uniform::new("size", (dest.size.w as f32, dest.size.h as f32)),
                Uniform::new("uv_origin", ((src.loc.x / bs.w as f64) as f32, (src.loc.y / bs.h as f64) as f32)),
                Uniform::new("uv_size", ((src.size.w / bs.w as f64) as f32, (src.size.h / bs.h as f64) as f32)),
                Uniform::new("radius", style.radius),
                Uniform::new("border", style.border),
                Uniform::new("titlebar", style.titlebar),
                Uniform::new("tint_frame", (style.tint_frame[0], style.tint_frame[1], style.tint_frame[2], style.tint_frame[3])),
                Uniform::new("tint_client", (style.tint_client[0], style.tint_client[1], style.tint_client[2], style.tint_client[3])),
                Uniform::new("gloss", style.gloss),
                Uniform::new("highlight", style.highlight),
                Uniform::new("focus", style.focus),
            ],
        )
    }

    /// Draw a client's texture into `dest`, clipped to the rounded shape of `radius` (the shape
    /// is measured on `shape`, the window's outer rectangle, so the client area inside the frame
    /// shows only where the window is).
    pub fn draw_clipped(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        texture: &GlesTexture,
        dest: Rectangle<i32, Physical>,
        radius: f32,
    ) -> Result<(), GlesError> {
        let full = Rectangle::<f64, BufferCoord>::from_size(
            (texture.size().w as f64, texture.size().h as f64).into(),
        );
        frame.render_texture_from_to(
            texture,
            full,
            dest,
            &[Rectangle::from_size(dest.size)],
            &[],
            Transform::Normal,
            1.0,
            Some(&self.clip),
            &[
                Uniform::new("size", (dest.size.w as f32, dest.size.h as f32)),
                Uniform::new("radius", radius),
            ],
        )
    }
}
