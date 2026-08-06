//! Linux implementation: the PulseAudio native protocol over a unix socket.
//!
//! Compiled only for `target_os = "linux"`, where the `pulseaudio` crate is a dependency.
//! Every other target compiles [`crate::stub`] instead, whose constructors return
//! [`oxisound_core::OxiSoundError::Unsupported`].

pub(crate) mod conn;
pub(crate) mod device;
pub(crate) mod stream;

pub use conn::DEFAULT_CLIENT_NAME;
pub use device::PulseDevice;
pub use stream::{PulseDuplexStream, PulseInputStream, PulseOutputStream};
