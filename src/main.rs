mod demo;
mod glass;

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
        _ => {
            eprintln!("usage: fawc glass-demo [out.png]");
            std::process::exit(2);
        }
    }
}
