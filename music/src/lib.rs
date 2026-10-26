//! What the music player decides without a sound card: which files are in the playlist and in
//! what order. `main.rs` plays them.

use std::path::{Path, PathBuf};

const EXTENSIONS: [&str; 8] = ["mp3", "ogg", "oga", "flac", "wav", "m4a", "aac", "opus"];

fn is_music(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// The music at `path`: the file itself, or the music files in the folder and its folders
/// (three levels down), sorted by name.
pub fn collect(path: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 3 {
                    walk(&p, depth + 1, out);
                }
            } else if is_music(&p) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    if path.is_file() {
        out.push(path.to_path_buf());
    } else {
        walk(path, 0, &mut out);
    }
    out.sort();
    out
}

/// A small deterministic shuffle (xorshift): the same `seed` gives the same order.
pub fn shuffle(items: &mut [PathBuf], seed: u64) {
    let mut x = seed | 1;
    for i in (1..items.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        items.swap(i, (x % (i as u64 + 1)) as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_gives_its_music_by_name_and_a_file_gives_itself() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["b.mp3", "a.OGG", "notes.txt", "cover.jpg", "sub/c.flac", "sub/deeper/d.wav", "e.mp3"] {
            let p = dir.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        let names: Vec<String> = collect(dir.path()).iter().map(|p| p.strip_prefix(dir.path()).unwrap().display().to_string()).collect();
        assert_eq!(names, vec!["a.OGG", "b.mp3", "e.mp3", "sub/c.flac", "sub/deeper/d.wav"]);
        assert_eq!(collect(&dir.path().join("b.mp3")), vec![dir.path().join("b.mp3")]);
        assert!(collect(&dir.path().join("missing")).is_empty());
        assert!(collect(&dir.path().join("notes.txt")).len() == 1, "a file named directly is taken as it is; playing it decides");
    }

    #[test]
    fn shuffling_keeps_every_track_once_and_depends_on_the_seed() {
        let base: Vec<PathBuf> = (0..12).map(|i| PathBuf::from(format!("{i}.mp3"))).collect();
        let (mut a, mut b, mut c) = (base.clone(), base.clone(), base.clone());
        shuffle(&mut a, 7);
        shuffle(&mut b, 7);
        shuffle(&mut c, 99);
        assert_eq!(a, b, "same seed, same order");
        assert_ne!(a, c, "another seed, another order");
        assert_ne!(a, base, "it is not left alone");
        let mut sorted = a.clone();
        sorted.sort_by_key(|p| p.file_stem().unwrap().to_str().unwrap().parse::<u32>().unwrap());
        assert_eq!(sorted, base, "nothing lost or doubled");
    }
}
