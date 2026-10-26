//! Draw only when something changed. A picture is needed again when a program committed, input
//! arrived, a monitor or the configuration changed (`request_redraw`), while an animation runs
//! (`animating`), and at least every half second (the clock of night light, safety net).

use std::time::{Duration, Instant};

use crate::state::Fde;

/// The longest a monitor goes without being drawn.
pub const IDLE_REDRAW: Duration = Duration::from_millis(500);

impl Fde {
    /// Fast mode: one blur pass, no drop shadows, no wobble. Chosen with `appearance.performance`.
    pub fn fast_mode(&self) -> bool {
        match self.config.appearance.performance.as_str() {
            "fast" => true,
            "quality" => false,
            _ => self.software_renderer,
        }
    }

    /// Look at which graphics driver is drawing: software ones ("llvmpipe", "softpipe", "swrast")
    /// are slow with blur, so `auto` turns fast mode on.
    pub fn detect_renderer(&mut self, renderer: &mut smithay::backend::renderer::gles::GlesRenderer) {
        let name = renderer
            .with_context(|gl| unsafe {
                let p = gl.GetString(smithay::backend::renderer::gles::ffi::RENDERER);
                if p.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(p as *const _).to_string_lossy().to_lowercase()
                }
            })
            .unwrap_or_default();
        self.software_renderer = ["llvmpipe", "softpipe", "swrast", "software"].iter().any(|k| name.contains(k));
        tracing::info!("graphics: {name}{}", if self.software_renderer { " (software: fast mode when appearance.performance is auto)" } else { "" });
    }

    /// Something on screen may have changed.
    pub fn request_redraw(&mut self) {
        self.redraw_serial = self.redraw_serial.wrapping_add(1);
    }

    /// Is something moving that needs a new picture every frame?
    pub fn animating(&self) -> bool {
        !self.ghosts.is_empty()
            || !self.wobbles.is_empty()
            || self.hud.is_some()
            || self.space.elements().any(|w| self.open_progress(w) < 1.0 || self.minimize_progress(w).is_some())
    }

    /// Does a monitor that last drew `drawn` (see `redraw_serial`) at `last` need a new picture?
    pub fn needs_redraw(&self, drawn: u64, last: Instant) -> bool {
        drawn != self.redraw_serial || last.elapsed() >= IDLE_REDRAW || self.animating()
    }
}

/// How long frames take, logged every five seconds (`RUST_LOG=info`): the first thing to look at
/// when the desktop feels slow.
pub struct FrameStats {
    since: Instant,
    frames: u32,
    render: Duration,
    present: Duration,
    worst: Duration,
}

impl Default for FrameStats {
    fn default() -> Self {
        FrameStats { since: Instant::now(), frames: 0, render: Duration::ZERO, present: Duration::ZERO, worst: Duration::ZERO }
    }
}

impl FrameStats {
    /// A frame took `render` to draw and `present` to hand to the screen.
    pub fn add(&mut self, render: Duration, present: Duration) {
        self.frames += 1;
        self.render += render;
        self.present += present;
        self.worst = self.worst.max(render + present);
        if self.since.elapsed() >= Duration::from_secs(5) {
            let n = self.frames.max(1) as f64;
            tracing::info!(
                "frames: {} in {:.1}s, drawing {:.1} ms and handing over {:.1} ms on average, slowest {:.0} ms",
                self.frames,
                self.since.elapsed().as_secs_f64(),
                self.render.as_secs_f64() * 1000.0 / n,
                self.present.as_secs_f64() * 1000.0 / n,
                self.worst.as_secs_f64() * 1000.0
            );
            *self = FrameStats::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_desktop_is_drawn_only_when_asked_or_every_half_second() {
        let display = smithay::reexports::wayland_server::Display::<Fde>::new().unwrap();
        let mut s = Fde::new(&display.handle(), (800, 600).into());
        let (mut drawn, mut last) = (s.redraw_serial, Instant::now());
        assert!(!s.needs_redraw(drawn, last), "nothing happened: no new picture");
        s.request_redraw();
        assert!(s.needs_redraw(drawn, last), "something changed");
        drawn = s.redraw_serial;
        last = Instant::now();
        assert!(!s.needs_redraw(drawn, last));
        assert!(s.needs_redraw(drawn, Instant::now() - Duration::from_millis(600)), "the safety net");
        s.hud = Some((Instant::now(), "x".into()));
        assert!(s.needs_redraw(drawn, last), "an announcement is animating");
    }
}
