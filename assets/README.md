# Assets

- `wallpaper.webp`: the default desktop wallpaper (a Frutiger Aero landscape from the Frutiger
  Aero Archive, "daniele" collection). Added at the repository owner's request, who states that
  it is free to use. If its author or the archive says otherwise, remove the file: FAWC then falls
  back to its generated sky (`wallpaper::bundled()` returns None only if the file cannot be
  decoded; delete the `include_bytes!` line in `src/wallpaper.rs` and `configured()` falls back).

Everything else in the project's artwork is generated in code.

## Logo and mascot

`logo.svg` (a glass orb over sky and hill) and `mascot.svg` ("Aero-chan", the OS-tan: sky-blue hair, glossy orb clips, a sprout) are drawn by hand for this project and are MIT like the rest. `docs/social-preview.png` is the 1280x640 picture for the GitHub repository card (upload it under Settings > Social preview).
