# Assets

- `wallpaper.webp`: the default desktop wallpaper (a Frutiger Aero landscape from the Frutiger
  Aero Archive, "daniele" collection). Added at the repository owner's request, who states that
  it is free to use. If its author or the archive says otherwise, remove the file: FAWC then falls
  back to its generated sky (`wallpaper::bundled()` returns None only if the file cannot be
  decoded; delete the `include_bytes!` line in `src/wallpaper.rs` and `configured()` falls back).

Everything else in the project's artwork is generated in code.
