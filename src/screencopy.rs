//! wlr-screencopy-unstable-v1: screenshots and screen recording (grim, wf-recorder, OBS, the
//! screen-sharing portal). A client asks for a monitor (or part of it), hands over a shared
//! memory buffer, and gets the picture the next time that monitor is drawn.

use std::sync::Mutex;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{Bind, ExportMem};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::{wl_buffer::WlBuffer, wl_shm};
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};
use smithay::utils::{Buffer as BufferCoord, Physical, Rectangle};
use smithay::wayland::shm::with_buffer_contents_mut;
use wayland_protocols_wlr::screencopy::v1::server::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};

use crate::state::Fde;

/// What one frame object was asked for.
pub struct FrameData {
    output: Output,
    /// the captured part, in physical pixels of the monitor
    rect: Rectangle<i32, Physical>,
    overlay_cursor: bool,
    used: Mutex<bool>,
}

/// A copy waiting for the monitor to be drawn.
pub struct Pending {
    frame: ZwlrScreencopyFrameV1,
    buffer: WlBuffer,
    output: Output,
    rect: Rectangle<i32, Physical>,
    overlay_cursor: bool,
    with_damage: bool,
}

#[derive(Default)]
pub struct Screencopy {
    pub pending: Vec<Pending>,
}

pub fn create_global(dh: &DisplayHandle) {
    dh.create_global::<Fde, ZwlrScreencopyManagerV1, ()>(3, ());
}

impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for Fde {
    fn bind(_: &mut Fde, _: &DisplayHandle, _: &Client, resource: New<ZwlrScreencopyManagerV1>, _: &(), data_init: &mut DataInit<'_, Fde>) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for Fde {
    fn request(
        state: &mut Fde,
        _: &Client,
        _: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Fde>,
    ) {
        let (frame, overlay_cursor, wl_output, region) = match request {
            zwlr_screencopy_manager_v1::Request::CaptureOutput { frame, overlay_cursor, output } => (frame, overlay_cursor, output, None),
            zwlr_screencopy_manager_v1::Request::CaptureOutputRegion { frame, overlay_cursor, output, x, y, width, height } => {
                (frame, overlay_cursor, output, Some((x, y, width, height)))
            }
            _ => return,
        };
        let overlay_cursor = overlay_cursor != 0;
        let output = state.output_for_resource(Some(&wl_output));
        let Some(mode) = output.current_mode() else {
            data_init.init(frame, FrameData { output, rect: Rectangle::default(), overlay_cursor, used: Mutex::new(true) }).failed();
            return;
        };
        let full = Rectangle::<i32, Physical>::from_size(mode.size);
        let scale = state.output_scale(&output);
        let rect = match region {
            None => full,
            Some((x, y, w, h)) => {
                let r = Rectangle::<i32, Physical>::new(
                    ((x as f64 * scale).round() as i32, (y as f64 * scale).round() as i32).into(),
                    (((w as f64 * scale).round() as i32).max(1), ((h as f64 * scale).round() as i32).max(1)).into(),
                );
                match r.intersection(full) {
                    Some(r) => r,
                    None => {
                        data_init.init(frame, FrameData { output, rect: full, overlay_cursor, used: Mutex::new(true) }).failed();
                        return;
                    }
                }
            }
        };
        let f = data_init.init(frame, FrameData { output, rect, overlay_cursor, used: Mutex::new(false) });
        f.buffer(wl_shm::Format::Xrgb8888, rect.size.w as u32, rect.size.h as u32, rect.size.w as u32 * 4);
        if f.version() >= 3 {
            f.buffer_done();
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, FrameData> for Fde {
    fn request(
        state: &mut Fde,
        _: &Client,
        frame: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        data: &FrameData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Fde>,
    ) {
        let (buffer, with_damage) = match request {
            zwlr_screencopy_frame_v1::Request::Copy { buffer } => (buffer, false),
            zwlr_screencopy_frame_v1::Request::CopyWithDamage { buffer } => (buffer, true),
            _ => return,
        };
        {
            let mut used = data.used.lock().unwrap();
            if *used {
                frame.post_error(zwlr_screencopy_frame_v1::Error::AlreadyUsed, "the frame was already used");
                return;
            }
            *used = true;
        }
        let ok = with_buffer_contents_mut(&buffer, |_, len, b| {
            b.format == wl_shm::Format::Xrgb8888
                && b.width == data.rect.size.w
                && b.height == data.rect.size.h
                && b.stride == data.rect.size.w * 4
                && (b.offset as usize) + (b.stride * b.height) as usize <= len
        })
        .unwrap_or(false);
        if !ok {
            frame.post_error(zwlr_screencopy_frame_v1::Error::InvalidBuffer, "the buffer does not match the frame");
            return;
        }
        state.screencopy.pending.push(Pending {
            frame: frame.clone(),
            buffer,
            output: data.output.clone(),
            rect: data.rect,
            overlay_cursor: data.overlay_cursor,
            with_damage,
        });
    }

    fn destroyed(state: &mut Fde, _: smithay::reexports::wayland_server::backend::ClientId, frame: &ZwlrScreencopyFrameV1, _: &FrameData) {
        state.screencopy.pending.retain(|p| &p.frame != frame);
    }
}

impl Fde {
    /// Answer the waiting copies of `output` that want the cursor (or not): the monitor's picture
    /// is in `scene`.
    pub fn fulfill_screencopy(&mut self, renderer: &mut GlesRenderer, output: &Output, scene: &mut GlesTexture, with_cursor: bool) {
        if self.screencopy.pending.is_empty() {
            return;
        }
        let (mine, rest): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut self.screencopy.pending)
            .into_iter()
            .partition(|p| &p.output == output && p.overlay_cursor == with_cursor && p.frame.is_alive());
        self.screencopy.pending = rest.into_iter().filter(|p| p.frame.is_alive()).collect();
        for p in mine {
            let region = Rectangle::<i32, BufferCoord>::new((p.rect.loc.x, p.rect.loc.y).into(), (p.rect.size.w, p.rect.size.h).into());
            let pixels = renderer
                .bind(scene)
                .map_err(|_| ())
                .and_then(|target| renderer.copy_framebuffer(&target, region, Fourcc::Abgr8888).map_err(|_| ()))
                .and_then(|m| renderer.map_texture(&m).map(|b| b.to_vec()).map_err(|_| ()));
            let Ok(px) = pixels else {
                p.frame.failed();
                continue;
            };
            let (w, h) = (p.rect.size.w as usize, p.rect.size.h as usize);
            let written = with_buffer_contents_mut(&p.buffer, |ptr, len, b| {
                let base = b.offset as usize;
                if base + w * h * 4 > len || px.len() < w * h * 4 {
                    return false;
                }
                let dst = unsafe { std::slice::from_raw_parts_mut(ptr.add(base), w * h * 4) };
                // RGBA bytes -> the bytes of XRGB8888 (B, G, R, X)
                for (d, s) in dst.chunks_exact_mut(4).zip(px.chunks_exact(4)) {
                    d.copy_from_slice(&[s[2], s[1], s[0], 255]);
                }
                true
            })
            .unwrap_or(false);
            if !written {
                p.frame.failed();
                continue;
            }
            p.frame.flags(zwlr_screencopy_frame_v1::Flags::empty());
            if p.with_damage && p.frame.version() >= 2 {
                p.frame.damage(0, 0, w as u32, h as u32);
            }
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            let secs = now.as_secs();
            p.frame.ready((secs >> 32) as u32, secs as u32, now.subsec_nanos());
        }
    }
}
