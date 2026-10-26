//! For the shell programs (panel, lock screen, notifications): which scale to draw at. The
//! compositor says what each output's scale is (`wl_output.scale`) and which outputs a surface is
//! on (`wl_surface.enter`); the picture is drawn at the biggest of them, as whole pixels per
//! logical pixel, and the buffer is told its scale.

use std::collections::{HashMap, HashSet};

use wayland_client::backend::ObjectId;

#[derive(Default)]
pub struct ScaleTracker {
    scales: HashMap<ObjectId, i32>,
    entered: HashSet<ObjectId>,
}

impl ScaleTracker {
    pub fn set_output_scale(&mut self, output: ObjectId, factor: i32) {
        self.scales.insert(output, factor.max(1));
    }

    pub fn enter(&mut self, output: ObjectId) {
        self.entered.insert(output);
    }

    pub fn leave(&mut self, output: &ObjectId) {
        self.entered.remove(output);
    }

    /// The scale to draw at: the biggest among the outputs our surfaces are on, else among all
    /// outputs, else 1.
    pub fn current(&self) -> i32 {
        let on_surfaces = self.entered.iter().filter_map(|o| self.scales.get(o)).max();
        on_surfaces.or_else(|| self.scales.values().max()).copied().unwrap_or(1)
    }
}
