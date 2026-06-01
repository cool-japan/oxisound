//! Play a Standard MIDI File (.mid) via a MIDI output port.
//!
//! Usage: cargo run --example play_midi --features smf,midi -- <file.mid> [port_index]
//! Without a file argument: lists available MIDI output ports and exits.

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        println!("Usage: play_midi <file.mid> [port_index]");
        println!("\nAvailable MIDI output ports:");
        match oxisound::enumerate_midi_devices() {
            Ok(devs) => {
                for (i, d) in devs.iter().enumerate() {
                    println!("  [{}] {}", i, d.name);
                }
                if devs.is_empty() {
                    println!("  (none found)");
                }
            }
            Err(e) => println!("  Error enumerating: {e}"),
        }
        return;
    }

    let path = std::path::Path::new(&args[1]);
    let port: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            println!("Failed to read {}: {e}", path.display());
            return;
        }
    };

    let smf = match oxisound::parse_smf(&data) {
        Ok(s) => s,
        Err(e) => {
            println!("Failed to parse SMF: {e}");
            return;
        }
    };

    let event_count = oxisound::SmfPlayer::new(smf.clone()).midi_events().count();
    println!(
        "Loaded: {} tracks, {} MIDI events, {:?}",
        smf.tracks.len(),
        event_count,
        smf.division,
    );

    let mut output = match oxisound::open_midi_output(port) {
        Ok(o) => o,
        Err(e) => {
            println!("Failed to open MIDI port {port}: {e}");
            return;
        }
    };

    println!("Playing via port {port}...");
    let player = oxisound::SmfPlayer::new(smf);
    match player.play(output.as_mut()) {
        Ok(()) => println!("Playback complete."),
        Err(e) => println!("Playback error: {e}"),
    }
}
