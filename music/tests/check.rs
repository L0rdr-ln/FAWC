//! The player's `--check` mode, which decodes files without a sound card.

use std::process::Command;

fn wav(path: &std::path::Path, seconds: u32) {
    let rate = 8000u32;
    let n = rate * seconds;
    let mut data = Vec::new();
    for i in 0..n {
        data.extend_from_slice(&(((i as f32 * 0.06).sin() * 8000.0) as i16).to_le_bytes());
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    std::fs::write(path, out).unwrap();
}

#[test]
fn a_real_file_decodes_and_a_broken_one_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("tone.wav");
    wav(&good, 3);
    let bad = dir.path().join("broken.mp3");
    std::fs::write(&bad, b"this is not music at all").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_fde-music")).arg("--check").arg(&good).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{text}");
    assert!(text.contains("1 channel") && text.contains("8000 Hz") && text.contains("3.0 s"), "{text}");

    let out = Command::new(env!("CARGO_BIN_EXE_fde-music")).arg("--check").arg(&good).arg(&bad).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(out.status.code(), Some(1), "one bad file makes the exit status 1: {text}");
    assert!(text.contains("tone.wav: ok") && text.contains("broken.mp3: cannot be played"), "{text}");
}
