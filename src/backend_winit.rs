//! Running nested in a window of another desktop (and the first way to try FDE).

use std::sync::Arc;
use std::time::Duration;

use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{Frame, Renderer, Texture};
use smithay::backend::winit::{self, WinitEvent};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
use smithay::reexports::wayland_server::{Display, ListeningSocket};
use smithay::utils::{Rectangle, Transform};

use crate::glass::Glass;
use crate::render::create_scene;
use crate::state::{ClientState, Fde};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<Fde> = EventLoop::try_new()?;
    let mut display: Display<Fde> = Display::new()?;
    let dh = display.handle();

    let (mut backend, winit_loop) = winit::init::<GlesRenderer>().map_err(|e| format!("winit: {e:?}"))?;
    let size = backend.window_size();
    let mut state = Fde::new(&dh, (size.w, size.h).into());

    state.enable_dmabuf(smithay::backend::renderer::ImportDma::dmabuf_formats(backend.renderer()));
    let glass = Glass::new(backend.renderer())?;
    let mut scene: GlesTexture = create_scene(backend.renderer(), size)?;

    // the Wayland socket
    let socket = ListeningSocket::bind_auto("wayland", 1..33)?;
    let socket_name = socket.socket_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    tracing::info!("listening on WAYLAND_DISPLAY={socket_name}");
    std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    state.start_session();
    crate::xwayland::start(event_loop.handle(), &dh);
    event_loop.handle().insert_source(
        Generic::new(socket, Interest::READ, Mode::Level),
        |_, socket, state| {
            while let Some(stream) = socket.accept()? {
                let _ = state
                    .display_handle
                    .insert_client(stream, Arc::new(ClientState::default()));
            }
            Ok(PostAction::Continue)
        },
    )?;
    // the clients' requests
    let poll_fd = display.backend().poll_fd().try_clone_to_owned()?;
    event_loop.handle().insert_source(
        Generic::new(poll_fd, Interest::READ, Mode::Level),
        move |_, _, state| {
            display.dispatch_clients(state).expect("dispatch");
            display.flush_clients().expect("flush");
            Ok(PostAction::Continue)
        },
    )?;

    // winit events (input, resize, close)
    event_loop.handle().insert_source(winit_loop, |event, _, state| match event {
        WinitEvent::Resized { size, .. } => state.resize_output((size.w, size.h).into()),
        WinitEvent::Input(event) => state.process_input_event(event),
        WinitEvent::CloseRequested => state.running = false,
        _ => {}
    })?;

    // draw at about 60 frames a second
    let timer = Timer::from_duration(Duration::from_millis(16));
    event_loop.handle().insert_source(timer, |_, _, _| TimeoutAction::ToDuration(Duration::from_millis(16)))?;

    while state.running {
        event_loop.dispatch(Some(Duration::from_millis(16)), &mut state)?;

        let size = backend.window_size();
        if scene.size().w != size.w || scene.size().h != size.h {
            scene = create_scene(backend.renderer(), size)?;
        }
        state.render_scene(backend.renderer(), &glass, &mut scene)?;
        {
            let (renderer, mut target) = backend.bind()?;
            let mut frame = renderer.render(&mut target, size, Transform::Flipped180)?;
            let full = Rectangle::from_size(size);
            frame.render_texture_from_to(
                &scene,
                Rectangle::from_size((size.w as f64, size.h as f64).into()),
                full,
                &[full],
                &[],
                Transform::Normal,
                1.0,
                None,
                &[],
            )?;
            let _ = frame.finish()?;
        }
        // frame callbacks: tell the clients this frame is done
        let now = state.start_time.elapsed().as_millis() as u32;
        for window in state.space.elements() {
            window.send_frame(&state.output, state.start_time.elapsed(), Some(Duration::ZERO), |_, _| {
                Some(state.output.clone())
            });
        }
        let _ = now;
        backend.submit(Some(&[full_damage(size)]))?;
    }
    Ok(())
}

fn full_damage(size: smithay::utils::Size<i32, smithay::utils::Physical>) -> Rectangle<i32, smithay::utils::Physical> {
    Rectangle::from_size(size)
}
