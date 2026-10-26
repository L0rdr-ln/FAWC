//! Keyboard and pointer: compositor shortcuts, focus, moving and resizing windows.

use smithay::wayland::seat::WaylandFocus;
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent, KeyState,
    KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
};
use smithay::desktop::WindowSurfaceType;
use smithay::input::keyboard::{keysyms, FilterResult};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};
use smithay::wayland::shell::wlr_layer::{KeyboardInteractivity, Layer};

use crate::layout::{Button, Hit};
use crate::state::{Drag, Edges, Fde};

const BTN_LEFT: u32 = 0x110;

#[derive(Debug, Clone, Copy)]
enum Action {
    Terminal,
    Quit,
    Close,
    Maximize,
    Cycle(i32),
    CancelSwitcher,
    ToggleDnd,
    ToggleHelp,
    CloseHelp,
    Restore,
    Lock,
    Snap(crate::windows::Snap),
    Workspace(usize),
    MoveToWorkspace(usize),
    WorkspaceStep(i32),
    Up,
    Down,
    Screenshot,
    SwitchVt(i32),
}

/// The keyboard shortcuts, as the cheat sheet (Alt+F1) shows them.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Alt+Return", "open a terminal"),
    ("Alt+Tab", "switch windows (hold Alt; Shift+Tab goes back)"),
    ("Alt+Q / Alt+F4", "close the window"),
    ("Alt+F", "maximize or restore"),
    ("Alt+Left / Right", "snap to the left or right half"),
    ("Alt+Up / Down", "maximize; restore, un-snap or minimize"),
    ("Alt+M", "bring back a minimized window"),
    ("Alt+1 .. 9", "go to a workspace (Shift: send the window there)"),
    ("Alt+Ctrl+Left / Right", "previous or next workspace"),
    ("Alt+L", "lock the screen"),
    ("Alt+N", "do not disturb on or off"),
    ("Print", "screenshot of this monitor"),
    ("Alt+F1", "this list"),
    ("Alt+Esc", "end the session"),
];

/// The cursor for resizing at `e`.
pub fn edges_cursor(e: &Edges) -> &'static str {
    match (e.left, e.right, e.top, e.bottom) {
        (true, _, true, _) | (_, true, _, true) => "nwse-resize",
        (_, true, true, _) | (true, _, _, true) => "nesw-resize",
        (true, _, _, _) | (_, true, _, _) => "ew-resize",
        _ => "ns-resize",
    }
}

impl Fde {
    pub fn process_input_event<B: InputBackend>(&mut self, event: InputEvent<B>) {
        self.note_activity();
        match event {
            InputEvent::Keyboard { event } => {
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);
                let Some(kb) = self.seat.get_keyboard() else { return };
                let action = kb.input::<Action, _>(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    time,
                    |state, mods, handle| {
                        let sym = handle.modified_sym().raw();
                        if event.state() == KeyState::Pressed
                            && (keysyms::KEY_XF86Switch_VT_1..=keysyms::KEY_XF86Switch_VT_12).contains(&sym)
                        {
                            return FilterResult::Intercept(Action::SwitchVt((sym - keysyms::KEY_XF86Switch_VT_1 + 1) as i32));
                        }
                        // letting go of Alt picks the window the switcher shows
                        if event.state() == KeyState::Released
                            && state.switcher.is_some()
                            && matches!(handle.modified_sym().raw(), keysyms::KEY_Alt_L | keysyms::KEY_Alt_R)
                        {
                            state.switcher_commit = true;
                        }
                        // a locked session has no shortcuts besides the console switch
                        if state.locked {
                            return FilterResult::Forward;
                        }
                        if event.state() == KeyState::Pressed && handle.modified_sym().raw() == keysyms::KEY_Print {
                            return FilterResult::Intercept(Action::Screenshot);
                        }
                        if event.state() == KeyState::Pressed && state.help_open && handle.modified_sym().raw() == keysyms::KEY_Escape && !mods.alt {
                            return FilterResult::Intercept(Action::CloseHelp);
                        }
                        if event.state() == KeyState::Pressed && mods.alt && handle.modified_sym().raw() == keysyms::KEY_F1 {
                            return FilterResult::Intercept(Action::ToggleHelp);
                        }
                        if event.state() == KeyState::Pressed && mods.alt {
                            // workspaces: Alt+1..9 (by key position, so Shift does not change it)
                            let digit = (handle.raw_code().raw() as i64 - 8 - 1).clamp(0, 99) as usize;
                            if (1..=9).contains(&digit) {
                                return FilterResult::Intercept(if mods.shift { Action::MoveToWorkspace(digit - 1) } else { Action::Workspace(digit - 1) });
                            }
                            if mods.ctrl {
                                match handle.modified_sym().raw() {
                                    keysyms::KEY_Left => return FilterResult::Intercept(Action::WorkspaceStep(-1)),
                                    keysyms::KEY_Right => return FilterResult::Intercept(Action::WorkspaceStep(1)),
                                    _ => {}
                                }
                            }
                            let action = match handle.modified_sym().raw() {
                                keysyms::KEY_Return | keysyms::KEY_KP_Enter => Some(Action::Terminal),
                                keysyms::KEY_Escape if state.switcher.is_some() => Some(Action::CancelSwitcher),
                                keysyms::KEY_Escape => Some(Action::Quit),
                                keysyms::KEY_q | keysyms::KEY_Q | keysyms::KEY_F4 => Some(Action::Close),
                                keysyms::KEY_f | keysyms::KEY_F => Some(Action::Maximize),
                                keysyms::KEY_Tab => Some(Action::Cycle(1)),
                                keysyms::KEY_ISO_Left_Tab => Some(Action::Cycle(-1)),
                                keysyms::KEY_m | keysyms::KEY_M => Some(Action::Restore),
                                keysyms::KEY_l | keysyms::KEY_L => Some(Action::Lock),
                                keysyms::KEY_n | keysyms::KEY_N => Some(Action::ToggleDnd),
                                keysyms::KEY_Left => Some(Action::Snap(crate::windows::Snap::Left)),
                                keysyms::KEY_Right => Some(Action::Snap(crate::windows::Snap::Right)),
                                keysyms::KEY_Up => Some(Action::Up),
                                keysyms::KEY_Down => Some(Action::Down),
                                _ => None,
                            };
                            if let Some(a) = action {
                                return FilterResult::Intercept(a);
                            }
                        }
                        FilterResult::Forward
                    },
                );
                if let Some(a) = action {
                    self.run_action(a);
                }
                if std::mem::take(&mut self.switcher_commit) {
                    self.switcher_commit();
                }
            }
            InputEvent::PointerMotionAbsolute { event } => {
                let size = self.output_size();
                let pos: Point<f64, Logical> = event.position_transformed(size);
                self.pointer_moved(pos, Event::time_msec(&event));
            }
            InputEvent::PointerMotion { event } => {
                use smithay::backend::input::PointerMotionEvent;
                let delta = event.delta();
                let pos = self.pointer_location + delta;
                self.pointer_moved(pos, Event::time_msec(&event));
            }
            InputEvent::PointerButton { event } => {
                let pressed = event.state() == ButtonState::Pressed;
                self.pointer_button(event.button_code(), pressed, Event::time_msec(&event));
            }
            InputEvent::PointerAxis { event } => {
                let source = event.source();
                let mut frame = AxisFrame::new(Event::time_msec(&event)).source(source);
                for axis in [Axis::Horizontal, Axis::Vertical] {
                    let amount = event.amount(axis).unwrap_or_else(|| {
                        event.amount_v120(axis).unwrap_or(0.0) * 15.0 / 120.0
                    });
                    if amount != 0.0 {
                        frame = frame.value(axis, amount);
                        if let Some(v120) = event.amount_v120(axis) {
                            frame = frame.v120(axis, v120 as i32);
                        }
                    } else if source == AxisSource::Finger {
                        frame = frame.stop(axis);
                    }
                }
                if let Some(ptr) = self.seat.get_pointer() {
                    ptr.axis(self, frame);
                    ptr.frame(self);
                }
            }
            _ => {}
        }
    }

    fn run_action(&mut self, action: Action) {
        match action {
            Action::Terminal => {
                let term = self.terminal.clone();
                if let Err(e) = std::process::Command::new(&term).spawn() {
                    tracing::warn!("cannot start {term}: {e}");
                }
            }
            Action::Quit => self.running = false,
            Action::Close => {
                if let Some(w) = self.space.elements().last().cloned() {
                    self.close_window(&w);
                }
            }
            Action::Maximize => {
                if let Some(w) = self.space.elements().last().cloned() {
                    self.toggle_maximized(&w);
                }
            }
            Action::Cycle(d) => self.switcher_step(d),
            Action::CancelSwitcher => self.switcher_cancel(),
            Action::ToggleDnd => self.toggle_do_not_disturb(),
            Action::ToggleHelp => self.help_open = !self.help_open,
            Action::CloseHelp => self.help_open = false,
            Action::Restore => self.restore_window(),
            Action::Lock => crate::shell::start_sibling("fde-lock"),
            Action::Workspace(n) => self.switch_workspace(n),
            Action::WorkspaceStep(d) => self.switch_workspace_by(d),
            Action::MoveToWorkspace(n) => {
                if let Some(w) = self.space.elements().last().cloned() {
                    self.move_window_to_workspace(&w, n);
                }
            }
            Action::Snap(side) => {
                if let Some(w) = self.space.elements().last().cloned() {
                    self.snap_window(&w, side);
                }
            }
            Action::Up => {
                if let Some(w) = self.space.elements().last().cloned() {
                    self.unsnap_window(&w);
                    self.set_maximized(&w, true);
                }
            }
            Action::Down => {
                if let Some(w) = self.space.elements().last().cloned() {
                    if self.is_maximized(&w) {
                        self.set_maximized(&w, false);
                    } else if self.is_snapped(&w) {
                        self.unsnap_window(&w);
                    } else {
                        self.minimize_window(&w);
                    }
                }
            }
            Action::Screenshot => {
                // the monitor the pointer is on
                let output = self.output_at(self.pointer_location).unwrap_or_else(|| self.output.clone());
                crate::shell::start_sibling_with("fde-shot", &["--output", &output.name()]);
            }
            Action::SwitchVt(vt) => {
                if let Some(session) = self.session.as_mut() {
                    use smithay::backend::session::Session;
                    if let Err(e) = session.change_vt(vt) {
                        tracing::warn!("cannot switch to VT {vt}: {e}");
                    }
                }
            }
        }
    }

    fn alt_held(&self) -> bool {
        self.seat.get_keyboard().map(|k| k.modifier_state().alt).unwrap_or(false)
    }

    pub fn pointer_moved(&mut self, pos: Point<f64, Logical>, time: u32) {
        let pos = self.clamp_to_outputs(pos);
        if !self.locked && self.drag.is_none() {
            // programs that read the mouse directly (games) get the movement, and a grab keeps
            // the pointer where it is
            self.send_relative_motion(pos - self.pointer_location, time);
            if !self.motion_allowed(pos) {
                return;
            }
        }
        self.pointer_location = pos;

        // the lock screen has the pointer to itself
        if self.locked {
            let serial = SERIAL_COUNTER.next_serial();
            let Some(ptr) = self.seat.get_pointer() else { return };
            let under = self.output_at(pos);
            let focus = self
                .lock_surfaces
                .iter()
                .find(|(o, _)| Some(o) == under.as_ref())
                .map(|(o, l)| (l.wl_surface().clone(), self.output_rect(o).loc.to_f64()));
            ptr.motion(self, focus, &MotionEvent { location: pos, serial, time });
            ptr.frame(self);
            return;
        }

        // an ongoing drag takes the pointer
        if let Some(drag) = self.drag.clone() {
            match drag {
                Drag::Move { window, grab_offset } => {
                    let client_loc = (
                        (pos.x - grab_offset.x).round() as i32,
                        (pos.y - grab_offset.y).round() as i32,
                    );
                    self.space.map_element(window.clone(), client_loc, false);
                    self.sync_x11_position(&window);
                }
                Drag::Resize { window, edges, start, pointer_start } => {
                    self.resize_to(&window, edges, start, pos - pointer_start);
                }
            }
            return;
        }

        let serial = SERIAL_COUNTER.next_serial();
        let Some(ptr) = self.seat.get_pointer() else { return };
        let upper = self.layer_surface_under(pos, &[Layer::Overlay, Layer::Top]);
        let under = if upper.is_some() { None } else { self.window_under(pos) };
        let focus = if let Some((_, s, loc)) = &upper {
            Some((s.clone(), loc.to_f64()))
        } else {
            match &under {
                Some((window, client, Hit::Client)) => {
                    let local = pos - client.loc.to_f64();
                    window
                        .surface_under(local, WindowSurfaceType::ALL)
                        .map(|(s, loc)| (s, (client.loc + loc).to_f64()))
                }
                Some(_) => None,
                None => self
                    .layer_surface_under(pos, &[Layer::Bottom, Layer::Background])
                    .map(|(_, s, loc)| (s, loc.to_f64())),
            }
        };
        // where no client has the pointer, the compositor chooses the cursor: resize arrows on
        // the borders, the ordinary arrow elsewhere
        if focus.is_none() {
            self.hover_cursor = match &under {
                Some((_, _, Hit::Border(e))) => edges_cursor(e),
                _ => "default",
            };
        }
        self.pointer_focus_origin = focus.as_ref().map(|(_, o)| *o).unwrap_or_default();
        ptr.motion(self, focus, &MotionEvent { location: pos, serial, time });
        ptr.frame(self);
        self.refresh_constraint();
    }

    pub fn pointer_button(&mut self, button: u32, pressed: bool, time: u32) {
        let serial = SERIAL_COUNTER.next_serial();
        let pos = self.pointer_location;

        if self.locked {
            // straight to the lock screen
        } else if !pressed {
            if let Some(drag) = self.drag.take() {
                self.release_wobbles();
                if let Drag::Move { window, .. } = drag {
                    self.snap_if_at_edge(&window);
                }
                return;
            }
        } else if let Some((layer, _, _)) = self.layer_surface_under(pos, &[Layer::Overlay, Layer::Top]) {
            // a click on a panel or launcher: it may take the keyboard, the click goes to it
            if layer.cached_state().keyboard_interactivity != KeyboardInteractivity::None {
                self.focus_layer(&layer);
            }
        } else if button == BTN_LEFT {
            let under = self.window_under(pos);
            if let Some((window, client, hit)) = under {
                self.focus_window(&window);
                let alt = self.alt_held();
                match hit {
                    Hit::Client if alt => {
                        self.start_move(&window, client);
                        return;
                    }
                    Hit::Titlebar => {
                        self.start_move(&window, client);
                        return;
                    }
                    Hit::Button(b) => {
                        match b {
                            Button::Close => self.close_window(&window),
                            Button::Maximize => self.toggle_maximized(&window),
                            Button::Minimize => self.minimize_window(&window),
                        }
                        return;
                    }
                    Hit::Border(edges) => {
                        self.start_resize(&window, edges, client);
                        return;
                    }
                    Hit::Client => {}
                }
            } else if let Some(kb) = self.seat.get_keyboard() {
                kb.set_focus(self, None, serial);
            }
        }

        if let Some(ptr) = self.seat.get_pointer() {
            ptr.button(
                self,
                &ButtonEvent {
                    button,
                    state: if pressed { ButtonState::Pressed } else { ButtonState::Released },
                    serial,
                    time,
                },
            );
            ptr.frame(self);
        }
    }

    fn start_move(&mut self, window: &smithay::desktop::Window, client: Rectangle<i32, Logical>) {
        if self.is_maximized(window) {
            self.set_maximized(window, false);
            return;
        }
        let mut client = client;
        if self.is_snapped(window) {
            // pulled out of its half: the window gets its old size back, under the pointer
            self.unsnap_window(window);
            if let Some(g) = self.space.element_geometry(window) {
                let x = (self.pointer_location.x - g.size.w as f64 / 2.0).round() as i32;
                self.space.map_element(window.clone(), (x, client.loc.y), false);
                client = Rectangle::new((x, client.loc.y).into(), g.size);
            }
        }
        let grab_offset = self.pointer_location - client.loc.to_f64();
        self.hover_cursor = "grabbing";
        self.start_wobble(window, self.pointer_location);
        self.drag = Some(Drag::Move { window: window.clone(), grab_offset });
    }

    fn start_resize(&mut self, window: &smithay::desktop::Window, edges: Edges, client: Rectangle<i32, Logical>) {
        if self.is_maximized(window) {
            return;
        }
        self.drag = Some(Drag::Resize {
            window: window.clone(),
            edges,
            start: client,
            pointer_start: self.pointer_location,
        });
    }

    fn resize_to(
        &mut self,
        window: &smithay::desktop::Window,
        edges: Edges,
        start: Rectangle<i32, Logical>,
        delta: Point<f64, Logical>,
    ) {
        let (dx, dy) = (delta.x.round() as i32, delta.y.round() as i32);
        let (min_w, min_h) = (160, 80);
        let mut r = start;
        if edges.right {
            r.size.w = (start.size.w + dx).max(min_w);
        }
        if edges.bottom {
            r.size.h = (start.size.h + dy).max(min_h);
        }
        if edges.left {
            r.size.w = (start.size.w - dx).max(min_w);
            r.loc.x = start.loc.x + start.size.w - r.size.w;
        }
        if edges.top {
            r.size.h = (start.size.h - dy).max(min_h);
            r.loc.y = start.loc.y + start.size.h - r.size.h;
        }
        if let Some(top) = window.toplevel() {
            top.with_pending_state(|s| s.size = Some(r.size));
            top.send_pending_configure();
        }
        self.space.map_element(window.clone(), r.loc, false);
        if let Some(x) = window.x11_surface() {
            let _ = x.configure(Some(r));
        }
    }
}
