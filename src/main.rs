use fawc::{backend_drm, backend_winit, demo, glass};

use smithay::utils::Rectangle;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("glass-demo") => {
            let out = args.get(2).map(String::as_str).unwrap_or("glass-demo.png");
            let mut renderer = demo::offscreen_renderer()?;
            let glass = glass::Glass::new(&mut renderer)?;
            let windows = vec![
                demo::Window {
                    outer: Rectangle::new((70, 120).into(), (440, 280).into()),
                    style: glass::GlassStyle { focus: 0.0, ..Default::default() },
                    client_alpha: 0.35,
                },
                demo::Window {
                    outer: Rectangle::new((330, 200).into(), (480, 290).into()),
                    style: glass::GlassStyle::default(),
                    client_alpha: 0.35,
                },
            ];
            let px = demo::render_scene(&mut renderer, &glass, &windows)?;
            demo::write_png(out, &px, demo::W, demo::H, false)?;
            println!("wrote {out}");
            Ok(())
        }
        Some("winit") => backend_winit::run(),
        Some("drm") => backend_drm::run(),
        // no argument: nested if there is a desktop to open a window in, else on the hardware
        None => {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
                backend_winit::run()
            } else {
                backend_drm::run()
            }
        }
        _ => {
            eprintln!("usage: fawc [winit|drm]        run nested in a window (winit) or on the hardware (drm); default: by environment\n       fawc glass-demo [out.png]");
            std::process::exit(2);
        }
    }
}
