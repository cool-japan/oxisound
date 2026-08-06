//! UDP-based OSC sender (std-feature-gated).

use alloc::{format, string::ToString, vec::Vec};
use std::net::{SocketAddr, UdpSocket};

use crate::{OscArg, OscError, OscMessage, OscPacket, encode};

/// Sends OSC packets over UDP to a fixed target address.
///
/// # Example
///
/// ```rust,no_run
/// use oxisound_osc::{OscArg, OscSender};
///
/// let sender = OscSender::connect("127.0.0.1:57120")?;
/// sender.send_message("/synth/note", vec![OscArg::Int(60), OscArg::Float(0.8)])?;
/// # Ok::<(), oxisound_osc::OscError>(())
/// ```
pub struct OscSender {
    socket: UdpSocket,
    target: SocketAddr,
}

impl OscSender {
    /// Creates a sender that will transmit packets to `target` (e.g., `"127.0.0.1:57120"`).
    ///
    /// Binds an ephemeral local port automatically.
    pub fn connect(target: &str) -> Result<Self, OscError> {
        let socket =
            UdpSocket::bind("0.0.0.0:0").map_err(|e| OscError(format!("bind failed: {e}")))?;
        let target: SocketAddr = target
            .parse()
            .map_err(|e| OscError(format!("invalid address '{target}': {e}")))?;
        Ok(OscSender { socket, target })
    }

    /// Encodes and sends an `OscPacket`.
    pub fn send(&self, packet: &OscPacket) -> Result<(), OscError> {
        let data = encode(packet);
        log::trace!("OscSender: sending {} bytes to {}", data.len(), self.target);
        self.socket
            .send_to(&data, self.target)
            .map(|_| ())
            .map_err(|e| OscError(format!("send failed: {e}")))
    }

    /// Convenience method: constructs and sends a single OSC message.
    pub fn send_message(&self, address: &str, args: Vec<OscArg>) -> Result<(), OscError> {
        self.send(&OscPacket::Message(OscMessage {
            address: address.to_string(),
            args,
        }))
    }
}
