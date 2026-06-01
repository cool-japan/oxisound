//! UDP-based OSC receiver (std-feature-gated).

use alloc::format;
use std::net::UdpSocket;

use crate::{OscError, OscPacket, decode};

/// Listens on a UDP socket and decodes incoming OSC packets.
pub struct OscReceiver {
    socket: UdpSocket,
}

impl OscReceiver {
    /// Binds a UDP socket to `addr` (e.g., `"0.0.0.0:57120"`).
    pub fn bind(addr: &str) -> Result<Self, OscError> {
        UdpSocket::bind(addr)
            .map(|socket| OscReceiver { socket })
            .map_err(|e| OscError(format!("bind failed: {e}")))
    }

    /// Blocks until an OSC packet arrives, then decodes and returns it.
    pub fn recv(&self) -> Result<OscPacket, OscError> {
        let mut buf = [0u8; 65536];
        let (n, _) = self
            .socket
            .recv_from(&mut buf)
            .map_err(|e| OscError(format!("recv failed: {e}")))?;
        log::trace!("OscReceiver: received {} bytes", n);
        decode(&buf[..n])
    }

    /// Configures the read timeout for `recv`.  `None` means block indefinitely.
    pub fn set_timeout(&self, timeout: Option<std::time::Duration>) -> Result<(), OscError> {
        self.socket
            .set_read_timeout(timeout)
            .map_err(|e| OscError(format!("set_timeout failed: {e}")))
    }
}
