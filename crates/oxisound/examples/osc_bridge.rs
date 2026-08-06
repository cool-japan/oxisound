//! OSC round-trip demo: binds a local UDP `OscReceiver`, spawns a listener thread,
//! then sends a handful of `OscMessage`s to it via `OscSender` and prints what was
//! decoded on receipt.
//!
//! This exercises the full OSC transport path (encode -> UDP send -> UDP recv ->
//! decode) end to end on the loopback interface, with no external hardware needed.
//!
//! Usage: cargo run --example osc_bridge --features osc

use std::time::Duration;

use oxisound::{OscArg, OscReceiver, OscSender};

/// Loopback address used for both ends of the demo socket pair.
const ADDR: &str = "127.0.0.1:57150";

fn main() {
    let receiver = match OscReceiver::bind(ADDR) {
        Ok(r) => r,
        Err(e) => {
            println!("Failed to bind OSC receiver on {ADDR}: {e}");
            return;
        }
    };
    if let Err(e) = receiver.set_timeout(Some(Duration::from_secs(2))) {
        println!("Failed to set receive timeout: {e}");
        return;
    }

    // Messages to send, one per `recv()` call on the listener thread.
    let messages: Vec<(&str, Vec<OscArg>)> = vec![
        ("/synth/note", vec![OscArg::Int(60), OscArg::Float(0.8)]),
        ("/synth/freq", vec![OscArg::Float(440.0)]),
        ("/synth/gate", vec![OscArg::Bool(false)]),
    ];
    let expected = messages.len();

    let listener = std::thread::spawn(move || {
        for _ in 0..expected {
            match receiver.recv() {
                Ok(packet) => println!("received: {packet:?}"),
                Err(e) => {
                    println!("recv error: {e}");
                    break;
                }
            }
        }
    });

    // Give the listener a moment to start blocking on recv() before we send.
    std::thread::sleep(Duration::from_millis(100));

    let sender = match OscSender::connect(ADDR) {
        Ok(s) => s,
        Err(e) => {
            println!("Failed to connect OSC sender: {e}");
            return;
        }
    };

    for (address, args) in messages {
        println!("sending: {address} {args:?}");
        if let Err(e) = sender.send_message(address, args) {
            println!("send failed: {e}");
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if let Err(e) = listener.join() {
        println!("listener thread panicked: {e:?}");
    }

    println!("Done.");
}
