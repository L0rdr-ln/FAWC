//! Keyboard and pointer: compositor shortcuts, focus, moving and resizing windows.

use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent, KeyState,
    KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
};
use smithay::desktop::WindowSurfaceType;
use smithay::input::keyboard::{keysyms, FilterResult};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};

use crate::layout::{Button, Hit};
use crate::state::{Drag, Edges, Fawc};

const BTN_LEFT: u32 = 0x110;

#[derive(Debug, Clone, Copy)]
enum Action {
    Terminal,
    Quit,
    Close,
    Maximize,
    Cycle,
    Restore,
}

impl Fawc {
    pub fn process_input_event<B: InputBackend>(&mut self, event: InputEvent<B>) {
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
                    |_state, mods, handle| {
                        if event.state() == KeyState::Pressed && mods.alt {
                            let action = match handle.modified_sym().raw() {
                                keysyms::KEY_Return | keysyms::KEY_KP_Enter => Some(Action::Terminal),
                                keysyms::KEY_Escape => Some(Action::Quit),
                                keysyms::KEY_q | keysyms::KEY_Q | keysyms::KEY_F4 => Some(Action::Close),
                                keysyms::KEY_f | keysyms::KEY_F => Some(Action::Maximize),
                                keysyms::KEY_Tab | keysyms::KEY_ISO_Left_Tab => Some(Action::Cycle),
                                keysyms::KEY_m | keysyms::KEY_M => Some(Action::Restore),
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
            }
            InputEvent::PointerMotionAbsolute { event } => {
                let size = self.output_size();
                let pos: Point<f64, Logical> = event.position_transformed(size);
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
            Action::Cycle => self.cycle_windows(),
            Action::Restore => self.restore_window(),
        }
    }

    fn alt_held(&self) -> bool {
        self.seat.get_keyboard().map(|k| k.modifier_state().alt).unwrap_or(false)
    }

    pub fn pointer_moved(&mut self, pos: Point<f64, Logical>, time: u32) {
        let size = self.output_size();
        let pos = (pos.x.clamp(0.0, size.w as f64 - 1.0), pos.y.clamp(0.0, size.h as f64 - 1.0)).into();
        self.pointer_location = pos;

        // an ongoing drag takes the pointer
        if let Some(drag) = self.drag.clone() {
            match drag {
                Drag::Move { window, grab_offset } => {
                    let client_loc = (
                        (pos.x - grab_offset.x).round() as i32,
                        (pos.y - grab_offset.y).round() as i32,
                    );
                    self.space.map_element(window, client_loc, false);
                }
                Drag::Resize { window, edges, start, pointer_start } => {
                    self.resize_to(&window, edges, start, pos - pointer_start);
                }
            }
            return;
        }

        let serial = SERIAL_COUNTER.next_serial();
        let Some(ptr) = self.seat.get_pointer() else { return };
        let under = self.window_under(pos);
        let focus = match &under {
            Some((window, client, Hit::Client)) => {
                let local = pos - client.loc.to_f64();
                window
                    .surface_under(local, WindowSurfaceType::ALL)
                    .map(|(s, loc)| (s, (client.loc + loc).to_f64()))
            }
            _ => None,
        };
        ptr.motion(self, focus, &MotionEvent { location: pos, serial, time });
        ptr.frame(self);
    }

    pub fn pointer_button(&mut self, button: u32, pressed: bool, time: u32) {
        let serial = SERIAL_COUNTER.next_serial();
        let pos = self.pointer_location;

        if !pressed {
            if self.drag.take().is_some() {
                return;
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
        let grab_offset = self.pointer_location - client.loc.to_f64();
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
    }
}
