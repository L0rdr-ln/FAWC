# Assets

- `wallpaper.webp`: the default desktop wallpaper (a Frutiger Aero landscape from the Frutiger
  Aero Archive, "daniele" collection). Added at the repository owner's request, who states that
  it is free to use. If its author or the archive says otherwise, remove the file: FDE then falls
  back to its generated sky (`wallpaper::bundled()` returns None only if the file cannot be
  decoded; delete the `include_bytes!` line in `src/wallpaper.rs` and `configured()` falls back).

Everything else in the project's artwork is generated in code.

## Mascot

`mascot.svg` is "Aero-chan", the OS-tan: a manga-style character (sky-blue hair, glass sailor collar, glossy orb clips, a sprout), drawn by hand for this project and MIT like the rest. `docs/social-preview.png` is the 1280x640 picture for the GitHub repository card (upload it under Settings > Social preview).
