//! fawc-music: plays the music named in `[music]` of fawc.toml in a loop, quietly, for as long
//! as the session runs. Volume, shuffle, the path and the on/off switch follow the file live.
//! With no sound card it keeps looking for one.

use std::fs::File;
use std::io::BufReader;
use std::time::{Duration, Instant};

use fawc::config::{Config, Watcher};
use fawc_music::{collect, shuffle};
use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink};

fn open_sink() -> Option<(OutputStream, Sink)> {
    match OutputStreamBuilder::open_default_stream() {
        Ok(stream) => {
            let sink = Sink::connect_new(stream.mixer());
            Some((stream, sink))
        }
        Err(e) => {
            eprintln!("fawc-music: no sound output ({e}); trying again in a while");
            None
        }
    }
}

/// `fawc-music --check FILE...`: decode each file completely and say what is in it, without any
/// sound output. Exit status 1 if one of them cannot be played.
fn check(files: &[String]) -> i32 {
    use rodio::Source;
    let mut bad = 0;
    for f in files {
        let decoded = File::open(f).map_err(|e| e.to_string()).and_then(|file| Decoder::try_from(BufReader::new(file)).map_err(|e| e.to_string()));
        match decoded {
            Ok(source) => {
                let (channels, rate) = (source.channels(), source.sample_rate());
                let samples = source.count();
                let seconds = samples as f64 / (channels as f64 * rate as f64);
                println!("{f}: ok, {channels} channel(s), {rate} Hz, {seconds:.1} s");
            }
            Err(e) => {
                println!("{f}: cannot be played: {e}");
                bad = 1;
            }
        }
    }
    bad
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--check") {
        std::process::exit(check(&args[1..]));
    }
    let mut watcher = Watcher::new();
    let mut config = Config::load().unwrap_or_default();
    let mut output: Option<(OutputStream, Sink)> = None;
    let mut playlist: Vec<std::path::PathBuf> = Vec::new();
    let mut next = 0usize;
    let mut playlist_for = (String::new(), false);
    let mut failures = 0u32;
    let mut retry_at = Instant::now();

    loop {
        if watcher.changed() {
            if let Ok(c) = Config::load() {
                config = c;
            }
        }
        let m = &config.music;
        if !m.enabled || m.path.is_empty() {
            // switched off: silence, and look again soon
            if let Some((_, sink)) = &output {
                sink.stop();
            }
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        if Instant::now() < retry_at {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        if output.is_none() {
            output = open_sink();
            if output.is_none() {
                retry_at = Instant::now() + Duration::from_secs(15);
                continue;
            }
        }
        let (_, sink) = output.as_ref().expect("sound output");
        sink.set_volume(m.volume);

        // the playlist follows the path and the shuffle switch
        if playlist_for != (m.path.clone(), m.shuffle) {
            playlist = collect(std::path::Path::new(&m.path));
            if m.shuffle {
                let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
                shuffle(&mut playlist, seed);
            }
            playlist_for = (m.path.clone(), m.shuffle);
            next = 0;
            sink.stop();
            eprintln!("fawc-music: {} track(s) in {}", playlist.len(), m.path);
        }
        if playlist.is_empty() {
            retry_at = Instant::now() + Duration::from_secs(30);
            continue;
        }
        // the next track goes in when the last one has ended
        if sink.empty() {
            let track = &playlist[next % playlist.len()];
            next += 1;
            match File::open(track).map_err(|e| e.to_string()).and_then(|f| Decoder::try_from(BufReader::new(f)).map_err(|e| e.to_string())) {
                Ok(source) => {
                    failures = 0;
                    sink.append(source);
                }
                Err(e) => {
                    eprintln!("fawc-music: cannot play {}: {e}", track.display());
                    failures += 1;
                    if failures as usize >= playlist.len() {
                        // none of them plays: wait before going through them again
                        failures = 0;
                        retry_at = Instant::now() + Duration::from_secs(60);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}
