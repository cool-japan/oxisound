//! Open Sound Control (OSC) protocol implementation.
//!
//! Provides pure-Rust encoding, decoding, and optional UDP transport for OSC messages
//! and bundles, including all standard OSC type tags.
//!
//! # Example
//!
//! ```rust
//! use oxisound_osc::{OscArg, OscMessage, OscPacket, encode, decode};
//!
//! let msg = OscPacket::Message(OscMessage {
//!     address: "/synth/freq".to_string(),
//!     args: vec![OscArg::Float(440.0)],
//! });
//! let bytes = encode(&msg);
//! let decoded = decode(&bytes).unwrap();
//! assert_eq!(decoded, msg);
//! ```

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

use alloc::{string::String, vec::Vec};

#[cfg(feature = "std")]
mod client;
mod decode;
mod encode;
#[cfg(feature = "std")]
mod server;

#[cfg(feature = "std")]
pub use client::OscSender;
pub use decode::decode;
pub use encode::encode;
#[cfg(feature = "std")]
pub use server::OscReceiver;

/// An OSC time tag (NTP format: seconds since 1900-01-01 + fractional seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OscTimeTag {
    pub seconds: u32,
    pub fractional: u32,
}

impl OscTimeTag {
    /// Execute-immediately sentinel defined by the OSC 1.0 spec (section 5.1).
    pub const IMMEDIATE: Self = Self {
        seconds: 0,
        fractional: 1,
    };
}

/// A typed OSC argument value.
#[derive(Debug, Clone, PartialEq)]
pub enum OscArg {
    Int(i32),
    Float(f32),
    /// Must not contain an embedded NUL (`'\0'`) byte — see [`encode`]'s
    /// precondition docs. Non-ASCII (but NUL-free) UTF-8 round-trips correctly even though
    /// strict OSC 1.0 restricts string content to printable ASCII.
    String(String),
    Blob(Vec<u8>),
    Long(i64),
    Double(f64),
    TimeTag(OscTimeTag),
    Char(char),
    Color(u8, u8, u8, u8),
    Midi([u8; 4]),
    Bool(bool),
    Nil,
    Impulse,
    Array(Vec<OscArg>),
}

impl OscArg {
    /// Returns the OSC type tag character for this argument.
    pub fn type_tag(&self) -> char {
        match self {
            OscArg::Int(_) => 'i',
            OscArg::Float(_) => 'f',
            OscArg::String(_) => 's',
            OscArg::Blob(_) => 'b',
            OscArg::Long(_) => 'h',
            OscArg::Double(_) => 'd',
            OscArg::TimeTag(_) => 't',
            OscArg::Char(_) => 'c',
            OscArg::Color(..) => 'r',
            OscArg::Midi(_) => 'm',
            OscArg::Bool(true) => 'T',
            OscArg::Bool(false) => 'F',
            OscArg::Nil => 'N',
            OscArg::Impulse => 'I',
            OscArg::Array(_) => '[',
        }
    }
}

/// A complete OSC message: an address pattern and zero or more typed arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct OscMessage {
    /// Must not contain an embedded NUL (`'\0'`) byte — see [`encode`]'s
    /// precondition docs.
    pub address: String,
    pub args: Vec<OscArg>,
}

/// An OSC bundle: a time-tagged collection of OSC packets.
#[derive(Debug, Clone, PartialEq)]
pub struct OscBundle {
    pub time: OscTimeTag,
    pub elements: Vec<OscPacket>,
}

/// Either a single OSC message or an OSC bundle.
#[derive(Debug, Clone, PartialEq)]
pub enum OscPacket {
    Message(OscMessage),
    Bundle(OscBundle),
}

/// Errors produced during OSC encode or decode operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscError(pub String);

impl core::fmt::Display for OscError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "OSC error: {}", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for OscError {}
