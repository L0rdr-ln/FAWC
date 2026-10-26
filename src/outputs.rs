//! Monitors: several outputs side by side in one logical space, each with its own scale
//! (HiDPI: 2 physical pixels per logical one, or 1.5 ...). Windows live in the shared space and
//! may be on any of them; each monitor is drawn from its own part of it.

use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};

use crate::state::Fde;

/// One monitor as the settings app sees it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OutputInfo {
    pub name: String,
    pub width: i32,
    pub height: i32,
    /// the mode in use, "WIDTHxHEIGHT@HZ"
    pub mode: String,
    /// every mode the monitor has (the nested window and tests may know only the current one)
    pub modes: Vec<String>,
    pub scale: f64,
    pub x: i32,
    pub y: i32,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct OutputsFile {
    #[serde(default)]
    output: Vec<OutputInfo>,
}

/// "1920x1080@60" from a size and a refresh in millihertz.
pub fn format_mode(w: i32, h: i32, mhz: i32) -> String {
    let hz = mhz as f64 / 1000.0;
    if (hz - hz.round()).abs() < 0.05 {
        format!("{w}x{h}@{}", hz.round() as i32)
    } else {
        format!("{w}x{h}@{hz:.2}")
    }
}

/// Where the compositor lists its monitors for the settings app.
pub fn outputs_state_path() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| std::path::PathBuf::from(d).join("fde/outputs.toml"))
}

/// The monitors the running compositor reports (empty when none is running).
pub fn read_outputs_state() -> Vec<OutputInfo> {
    let Some(path) = outputs_state_path() else { return Vec::new() };
    std::fs::read_to_string(path).ok().and_then(|t| toml::from_str::<OutputsFile>(&t).ok()).map(|f| f.output).unwrap_or_default()
}

impl Fde {
    /// Every monitor, the first one first.
    pub fn all_outputs(&self) -> Vec<Output> {
        std::iter::once(self.output.clone()).chain(self.extra_outputs.iter().cloned()).collect()
    }

    pub fn output_scale(&self, output: &Output) -> f64 {
        output.current_scale().fractional_scale()
    }

    /// The monitor's size in logical pixels: its mode divided by its scale.
    pub fn output_logical_size(&self, output: &Output) -> Size<i32, Logical> {
        let scale = self.output_scale(output);
        output
            .current_mode()
            .map(|m| ((m.size.w as f64 / scale).round() as i32, (m.size.h as f64 / scale).round() as i32).into())
            .unwrap_or((1280, 720).into())
    }

    /// Where the monitor is in the shared logical space.
    pub fn output_rect(&self, output: &Output) -> Rectangle<i32, Logical> {
        let loc = self.space.output_geometry(output).map(|g| g.loc).unwrap_or_default();
        Rectangle::new(loc, self.output_logical_size(output))
    }

    /// Add a monitor: `mode_px` is its mode in physical pixels. Scale, mode and position come from
    /// `[outputs.NAME]` in the configuration; without a position it goes to the right of the others.
    pub fn add_output(&mut self, name: &str, mode_px: (i32, i32), refresh_mhz: i32) -> Output {
        let output = Output::new(
            name.to_string(),
            PhysicalProperties { size: (0, 0).into(), subpixel: Subpixel::Unknown, make: "FDE".into(), model: name.to_string() },
        );
        let mode = Mode { size: mode_px.into(), refresh: refresh_mhz };
        let oc = self.config.outputs.get(name).cloned().unwrap_or_default();
        let to_the_right = self
            .all_outputs()
            .iter()
            .map(|o| {
                let r = self.output_rect(o);
                r.loc.x + r.size.w
            })
            .max()
            .unwrap_or(0);
        let loc: Point<i32, Logical> = (oc.x.unwrap_or(to_the_right), oc.y.unwrap_or(0)).into();
        output.change_current_state(Some(mode), Some(Transform::Normal), Some(Scale::Fractional(oc.scale)), Some(loc));
        output.set_preferred(mode);
        let global = output.create_global::<Fde>(&self.display_handle);
        self.output_globals.push((output.clone(), global));
        self.space.map_output(&output, loc);
        self.extra_outputs.push(output.clone());
        if self.session_started {
            crate::shell::start_panel(&self.config, name);
        }
        tracing::info!("output {name}: {}x{} at scale {} placed at {},{}", mode_px.0, mode_px.1, oc.scale, loc.x, loc.y);
        self.write_outputs_state();
        output
    }

    /// Apply the `[outputs.*]` of a new configuration to the monitors that are plugged in now:
    /// scale and position take effect at once (resolution and on/off need a restart).
    pub fn apply_output_config(&mut self, config: &crate::config::Config) {
        for o in self.all_outputs() {
            let oc = config.outputs.get(&o.name()).cloned().unwrap_or_default();
            let mut changed = false;
            if (oc.scale - self.output_scale(&o)).abs() > 1e-6 {
                o.change_current_state(None, None, Some(Scale::Fractional(oc.scale)), None);
                changed = true;
            }
            if let (Some(x), Some(y)) = (oc.x, oc.y) {
                let now = self.space.output_geometry(&o).map(|g| g.loc).unwrap_or_default();
                if now != (x, y).into() {
                    o.change_current_state(None, None, None, Some((x, y).into()));
                    self.space.map_output(&o, (x, y));
                    changed = true;
                }
            }
            if changed {
                smithay::desktop::layer_map_for_output(&o).arrange();
                tracing::info!("monitor {}: scale {}, now {:?}", o.name(), oc.scale, self.output_rect(&o));
            }
        }
        self.pointer_location = self.clamp_to_outputs(self.pointer_location);
        self.write_outputs_state();
    }

    /// Tell the settings app which monitors there are: `$XDG_RUNTIME_DIR/fde/outputs.toml`.
    pub fn write_outputs_state(&mut self) {
        self.request_redraw();
        let outputs: Vec<OutputInfo> = self
            .all_outputs()
            .iter()
            .map(|o| {
                let r = self.output_rect(o);
                let mode = o.current_mode();
                let name = o.name();
                let mut modes: Vec<String> = self
                    .available_modes
                    .get(&name)
                    .map(|v| v.iter().map(|&(w, h, mhz)| format_mode(w, h, mhz)).collect())
                    .unwrap_or_default();
                if let Some(m) = mode {
                    let cur = format_mode(m.size.w, m.size.h, m.refresh);
                    if !modes.contains(&cur) {
                        modes.insert(0, cur);
                    }
                }
                OutputInfo {
                    name,
                    width: mode.map(|m| m.size.w).unwrap_or(0),
                    height: mode.map(|m| m.size.h).unwrap_or(0),
                    mode: mode.map(|m| format_mode(m.size.w, m.size.h, m.refresh)).unwrap_or_default(),
                    modes,
                    scale: self.output_scale(o),
                    x: r.loc.x,
                    y: r.loc.y,
                }
            })
            .collect();
        let Some(path) = outputs_state_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match toml::to_string(&OutputsFile { output: outputs }) {
            Ok(text) => {
                let tmp = path.with_extension("toml.tmp");
                if std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
                    tracing::debug!("cannot write {}", path.display());
                }
            }
            Err(e) => tracing::debug!("outputs state: {e}"),
        }
    }

    /// The session starts: a task bar on every monitor, then the other programs.
    pub fn start_session(&mut self) {
        self.session_started = true;
        for o in self.all_outputs() {
            crate::shell::start_panel(&self.config, &o.name());
        }
        crate::shell::start_other_clients(&self.config);
        self.write_outputs_state();
        self.write_workspace_state();
    }

    /// A monitor was unplugged: windows on it move to the first monitor, layer surfaces on it
    /// (its task bar) are asked to close, its global goes away. The first monitor itself stays.
    pub fn remove_output(&mut self, output: &Output) {
        if *output == self.output {
            tracing::warn!("the first monitor ({}) cannot be removed", output.name());
            return;
        }
        let rect = self.output_rect(output);
        let home = self.output_rect(&self.output.clone());
        // windows whose middle is on it come to the first monitor, inside it
        let windows: Vec<_> = self.space.elements().cloned().collect();
        for w in windows {
            let Some(g) = self.space.element_geometry(&w) else { continue };
            let middle = (g.loc.x + g.size.w / 2, g.loc.y + g.size.h / 2);
            if middle.0 >= rect.loc.x && middle.0 < rect.loc.x + rect.size.w && middle.1 >= rect.loc.y && middle.1 < rect.loc.y + rect.size.h {
                let x = (home.loc.x + 60 + (g.loc.x - rect.loc.x).clamp(0, 200)).min(home.loc.x + (home.size.w - g.size.w).max(0));
                let y = (home.loc.y + 60 + (g.loc.y - rect.loc.y).clamp(0, 100)).min(home.loc.y + (home.size.h - g.size.h).max(0));
                self.space.map_element(w, (x, y), false);
            }
        }
        // its panels and so on
        for layer in smithay::desktop::layer_map_for_output(output).layers() {
            layer.layer_surface().send_close();
        }
        self.lock_surfaces.retain(|(o, _)| o != output);
        self.space.unmap_output(output);
        if let Some(i) = self.output_globals.iter().position(|(o, _)| o == output) {
            let (_, id) = self.output_globals.remove(i);
            self.display_handle.remove_global::<Fde>(id);
        }
        self.extra_outputs.retain(|o| o != output);
        self.pointer_location = self.clamp_to_outputs(self.pointer_location);
        tracing::info!("output {} removed", output.name());
        self.write_outputs_state();
    }

    /// The monitor under `p`, if any.
    pub fn output_at(&self, p: Point<f64, Logical>) -> Option<Output> {
        self.all_outputs().into_iter().find(|o| {
            let r = self.output_rect(o);
            p.x >= r.loc.x as f64 && p.y >= r.loc.y as f64 && p.x < (r.loc.x + r.size.w) as f64 && p.y < (r.loc.y + r.size.h) as f64
        })
    }

    /// `p` moved onto the nearest monitor if it is on none (the pointer cannot leave the screens).
    pub fn clamp_to_outputs(&self, p: Point<f64, Logical>) -> Point<f64, Logical> {
        let mut best: Option<(f64, Point<f64, Logical>)> = None;
        for o in self.all_outputs() {
            let r = self.output_rect(&o);
            let q: Point<f64, Logical> = (
                p.x.clamp(r.loc.x as f64, (r.loc.x + r.size.w - 1) as f64),
                p.y.clamp(r.loc.y as f64, (r.loc.y + r.size.h - 1) as f64),
            )
                .into();
            let d = (q.x - p.x).powi(2) + (q.y - p.y).powi(2);
            if best.as_ref().map(|(bd, _)| d < *bd).unwrap_or(true) {
                best = Some((d, q));
            }
        }
        best.map(|(_, q)| q).unwrap_or(p)
    }

    /// The monitor a window is mostly on: the one under its middle, else the nearest to it.
    pub fn output_for_window(&self, window: &smithay::desktop::Window) -> Output {
        let Some(g) = self.space.element_geometry(window) else { return self.output.clone() };
        let middle: Point<f64, Logical> = ((g.loc.x + g.size.w / 2) as f64, (g.loc.y + g.size.h / 2) as f64).into();
        self.output_at(middle).unwrap_or_else(|| {
            let p = self.clamp_to_outputs(middle);
            self.output_at(p).unwrap_or_else(|| self.output.clone())
        })
    }

    /// The monitor with this layer-shell / protocol output resource, else the first one.
    pub fn output_for_resource(&self, wl: Option<&smithay::reexports::wayland_server::protocol::wl_output::WlOutput>) -> Output {
        wl.and_then(Output::from_resource).filter(|o| self.all_outputs().contains(o)).unwrap_or_else(|| self.output.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::reexports::wayland_server::Display;

    fn state() -> (Display<Fde>, Fde) {
        let display: Display<Fde> = Display::new().unwrap();
        let s = Fde::new(&display.handle(), (800, 600).into());
        (display, s)
    }

    #[test]
    fn outputs_sit_side_by_side_and_the_pointer_stays_on_them() {
        let (_d, mut s) = state();
        let second = s.add_output("fde-1", (640, 480), 60_000);
        assert_eq!(s.output_rect(&s.output.clone()), Rectangle::new((0, 0).into(), (800, 600).into()));
        assert_eq!(s.output_rect(&second), Rectangle::new((800, 0).into(), (640, 480).into()));
        assert_eq!(s.output_at((100.0, 100.0).into()).unwrap(), s.output);
        assert_eq!(s.output_at((900.0, 100.0).into()).unwrap(), second);
        assert!(s.output_at((900.0, 550.0).into()).is_none(), "below the shorter monitor there is nothing");
        // clamped onto the nearest monitor
        assert_eq!(s.clamp_to_outputs((2000.0, 100.0).into()), (1439.0, 100.0).into());
        assert_eq!(s.clamp_to_outputs((1000.0, 590.0).into()), (1000.0, 479.0).into());
        assert_eq!(s.clamp_to_outputs((900.0, 590.0).into()), (799.0, 590.0).into(), "equally near: the closer monitor wins");
        assert_eq!(s.clamp_to_outputs((-5.0, -5.0).into()), (0.0, 0.0).into());
    }

    #[test]
    fn unplugging_a_monitor_brings_its_windows_home_and_the_pointer_back() {
        let (_d, mut s) = state();
        let second = s.add_output("fde-1", (640, 480), 60_000);
        s.pointer_location = (1000.0, 100.0).into();
        s.remove_output(&second);
        assert_eq!(s.all_outputs(), vec![s.output.clone()]);
        assert_eq!(s.pointer_location, (799.0, 100.0).into(), "back on the remaining monitor");
        assert!(s.output_at((1000.0, 100.0).into()).is_none());
        // the first monitor cannot be removed
        let first = s.output.clone();
        s.remove_output(&first);
        assert_eq!(s.all_outputs().len(), 1);
        // a monitor plugged in again takes the free place on the right
        let again = s.add_output("fde-1", (640, 480), 60_000);
        assert_eq!(s.output_rect(&again).loc, (800, 0).into());
    }

    #[test]
    fn scale_makes_the_logical_size_smaller() {
        let (_d, mut s) = state();
        let o = s.add_output("fde-1", (3840, 2160), 60_000);
        o.change_current_state(None, None, Some(Scale::Fractional(2.0)), None);
        assert_eq!(s.output_logical_size(&o), (1920, 1080).into());
        o.change_current_state(None, None, Some(Scale::Fractional(1.5)), None);
        assert_eq!(s.output_logical_size(&o), (2560, 1440).into());
    }
}
