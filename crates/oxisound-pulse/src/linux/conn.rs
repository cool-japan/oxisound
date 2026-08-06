//! Bounded connection and handshake with the PulseAudio server.
//!
//! Everything here is Linux-only: it is the only place where the `pulseaudio` crate's
//! socket-facing API is touched.

use std::ffi::{CStr, CString};
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};

use oxisound_core::OxiSoundError;
use pulseaudio::{Client, ClientError, protocol};

use crate::env::{ProcessEnv, load_cookie_from_env, resolve_server_socket};
use crate::timeout::PULSE_CONNECT_TIMEOUT;

/// Client name reported to the server when the caller does not choose one.
///
/// PulseAudio shows this string in mixers such as `pavucontrol`.
pub const DEFAULT_CLIENT_NAME: &str = "oxisound";

/// Connects to the PulseAudio (or `pipewire-pulse`) server and performs the handshake.
///
/// The socket path and cookie come from [`crate::env`]. A cookie-less server — the usual
/// `pipewire-pulse` configuration, and PulseAudio with `auth-anonymous=1` on a same-uid
/// unix socket — is supported: an empty cookie is sent and the server decides.
///
/// # Bounded by construction
///
/// `connect(2)` on a unix socket blocks while the server's listen backlog is full, and
/// nothing in the `pulseaudio` crate bounds it. The whole connect-plus-handshake sequence
/// therefore runs on a dedicated `oxisound-pulse-connect` worker thread while the caller
/// waits with `recv_timeout(PULSE_CONNECT_TIMEOUT)`; the handshake itself is additionally
/// bounded by `SO_RCVTIMEO`/`SO_SNDTIMEO` on the socket. On timeout the worker is
/// detached — it stays parked in the kernel until the call returns, then drops the
/// half-built client and exits. This mirrors `oxisound-cpal`'s `DUPLEX_OPEN_TIMEOUT`
/// handling of uninterruptible backend calls.
///
/// # Errors
///
/// - [`OxiSoundError::Device`] when no socket could be located or the server refused it.
/// - [`OxiSoundError::PermissionDenied`] when the socket or cookie is not readable.
/// - [`OxiSoundError::Timeout`] when [`PULSE_CONNECT_TIMEOUT`] elapses.
pub fn connect_client(client_name: &str) -> Result<Arc<PulseConnection>, OxiSoundError> {
    let env = ProcessEnv;
    let socket_path = resolve_server_socket(&env)?;
    let cookie = load_cookie_from_env(&env);
    let name = CString::new(client_name).map_err(|_| {
        OxiSoundError::Device(format!(
            "PulseAudio client name {client_name:?} contains an interior NUL byte"
        ))
    })?;

    log::debug!(
        "connecting to PulseAudio at {} (cookie: {})",
        socket_path.display(),
        cookie.as_ref().map_or("none", |_| "present")
    );

    let (tx, rx) = mpsc::channel();
    let display_path = socket_path.display().to_string();
    std::thread::Builder::new()
        .name("oxisound-pulse-connect".into())
        .spawn(move || {
            let result = connect_blocking(&socket_path, &name, cookie.as_deref());
            // The receiver is gone when the caller already timed out; that is expected.
            let _ = tx.send(result);
        })
        .map_err(|err| {
            OxiSoundError::Device(format!("could not spawn PulseAudio connect thread: {err}"))
        })?;

    match rx.recv_timeout(PULSE_CONNECT_TIMEOUT) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err(OxiSoundError::Timeout(format!(
            "connecting to the PulseAudio server at {display_path} did not complete within {:.1?}",
            PULSE_CONNECT_TIMEOUT
        ))),
        Err(RecvTimeoutError::Disconnected) => Err(OxiSoundError::Device(format!(
            "the PulseAudio connect worker for {display_path} exited without a result"
        ))),
    }
}

fn connect_blocking(
    path: &Path,
    name: &CStr,
    cookie: Option<&[u8]>,
) -> Result<Arc<PulseConnection>, OxiSoundError> {
    let socket = UnixStream::connect(path).map_err(|err| map_connect_io(path, &err))?;
    // Bound the blocking AUTH / SET_CLIENT_NAME handshake performed by `new_unix`.
    // `new_unix` switches the socket to non-blocking mode afterwards for the reactor,
    // which supersedes both timeouts.
    socket.set_read_timeout(Some(PULSE_CONNECT_TIMEOUT))?;
    socket.set_write_timeout(Some(PULSE_CONNECT_TIMEOUT))?;
    // Keep a dup of the socket before handing it to the client — see `PulseConnection`
    // for why the reactor thread cannot be stopped without it.
    let shutdown_handle = socket.try_clone()?;
    let client = Client::new_unix(name, socket, cookie).map_err(map_client_error)?;
    Ok(Arc::new(PulseConnection {
        client,
        shutdown_handle,
    }))
}

/// An owned connection to the PulseAudio server, shut down when the last handle drops.
///
/// # Why this type exists
///
/// The `pulseaudio` reactor loop is `poll(…, None)` → `recv` → `write_streams` →
/// `write_commands`, and its only exit is `write_commands` observing that the outgoing
/// channel has been disconnected. That check is reached *after* the unbounded `poll`, and
/// the mio waker that could interrupt the poll is owned by the reactor itself — so simply
/// dropping every `Client` clone does not wake it. The thread would park in `epoll_wait`
/// forever, holding the socket open, and each `enumerate()` / `default_output()` call
/// would leak one thread and one file descriptor.
///
/// `PulseConnection` keeps a `try_clone`'d handle on the same socket and calls
/// `shutdown(Both)` on it when the last reference drops. `shutdown(2)` acts on the socket,
/// not on the file descriptor, so the reactor's next `read` returns `Ok(0)`, which the
/// reactor maps to `ClientError::Disconnected`; `run()` returns, the thread exits, the
/// reactor's state is dropped, and with it the ring adapters — which is also what sets the
/// `is_disconnected()` flag on any surviving stream handle.
///
/// # Teardown ordering
///
/// Stream handles drop their `PlaybackStream` / `RecordStream` *before* their
/// `Arc<PulseConnection>`, so the `DELETE_*_STREAM` command is queued first. It is queued,
/// not flushed: the shutdown may beat it onto the wire. That is harmless — the server
/// reclaims every stream belonging to a disconnected client.
pub struct PulseConnection {
    client: Client,
    shutdown_handle: UnixStream,
}

impl std::fmt::Debug for PulseConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PulseConnection")
            .field(&self.client)
            .finish()
    }
}

impl PulseConnection {
    /// The underlying `pulseaudio` client, for issuing further commands on this socket.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl Drop for PulseConnection {
    fn drop(&mut self) {
        if let Err(err) = self.shutdown_handle.shutdown(Shutdown::Both) {
            // `NotConnected` just means the server already hung up.
            if err.kind() != io::ErrorKind::NotConnected {
                log::debug!("shutting down the PulseAudio socket failed: {err}");
            }
        }
    }
}

fn map_connect_io(path: &Path, err: &io::Error) -> OxiSoundError {
    match err.kind() {
        io::ErrorKind::PermissionDenied => OxiSoundError::PermissionDenied(format!(
            "cannot open the PulseAudio socket {}: {err}",
            path.display()
        )),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
            OxiSoundError::Device(format!(
                "no PulseAudio server listening on {}: {err}; is pulseaudio or pipewire-pulse \
                 running?",
                path.display()
            ))
        }
        _ => OxiSoundError::Device(format!(
            "could not connect to the PulseAudio socket {}: {err}",
            path.display()
        )),
    }
}

/// Translates a `pulseaudio` client error into the OxiSound error taxonomy.
///
/// Authentication and access failures become [`OxiSoundError::PermissionDenied`], a lost
/// reactor becomes [`OxiSoundError::Disconnected`], and a server-side timeout becomes
/// [`OxiSoundError::Timeout`], so callers can branch on `err.kind()` exactly as they do
/// with the cpal backend.
pub fn map_client_error(err: ClientError) -> OxiSoundError {
    match err {
        ClientError::ServerUnavailable => {
            OxiSoundError::Device("the PulseAudio server is unavailable".into())
        }
        ClientError::UnexpectedSequenceNumber => OxiSoundError::Device(
            "the PulseAudio server replied with an unexpected sequence number".into(),
        ),
        ClientError::Protocol(protocol_err) => map_protocol_error(protocol_err),
        ClientError::ServerError(server_err) => map_server_error(server_err),
        ClientError::Io(io_err) => OxiSoundError::Io(io_err),
        ClientError::Disconnected => {
            OxiSoundError::Disconnected("the PulseAudio connection was closed".into())
        }
    }
}

fn map_protocol_error(err: protocol::ProtocolError) -> OxiSoundError {
    match err {
        protocol::ProtocolError::Io(io_err) => OxiSoundError::Io(io_err),
        protocol::ProtocolError::ServerError(server_err) => map_server_error(server_err),
        protocol::ProtocolError::Timeout => {
            OxiSoundError::Timeout("the PulseAudio server reported a timeout".into())
        }
        other => OxiSoundError::Device(format!("PulseAudio protocol error: {other}")),
    }
}

fn map_server_error(err: protocol::PulseError) -> OxiSoundError {
    use protocol::PulseError as E;
    match err {
        E::AccessDenied | E::AuthKey => OxiSoundError::PermissionDenied(format!(
            "the PulseAudio server rejected the connection ({err:?}); check the cookie in \
             $PULSE_COOKIE or ~/.config/pulse/cookie"
        )),
        E::NoEntity => OxiSoundError::NoDevice,
        E::Timeout => OxiSoundError::Timeout(format!("the PulseAudio server timed out ({err:?})")),
        E::ConnectionTerminated | E::Killed => {
            OxiSoundError::Disconnected(format!("the PulseAudio stream ended ({err:?})"))
        }
        E::NotSupported => OxiSoundError::Unsupported(format!(
            "the PulseAudio server refused the request ({err:?})"
        )),
        E::Invalid | E::TooLarge | E::Version => {
            OxiSoundError::UnsupportedConfig(format!("the PulseAudio server rejected it ({err:?})"))
        }
        other => OxiSoundError::Device(format!("PulseAudio server error: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PULSE_MAX_CHANNELS, PULSE_MAX_SAMPLE_RATE};

    #[test]
    fn protocol_limits_match_the_pulseaudio_crate() {
        assert_eq!(
            u32::from(PULSE_MAX_CHANNELS),
            u32::from(protocol::sample_spec::MAX_CHANNELS)
        );
        assert_eq!(PULSE_MAX_SAMPLE_RATE, protocol::sample_spec::MAX_RATE);
    }

    #[test]
    fn client_errors_map_onto_the_oxisound_taxonomy() {
        assert_eq!(
            map_client_error(ClientError::Disconnected).kind(),
            "disconnected"
        );
        assert_eq!(
            map_client_error(ClientError::ServerUnavailable).kind(),
            "device"
        );
        assert_eq!(
            map_client_error(ClientError::ServerError(protocol::PulseError::AccessDenied)).kind(),
            "permission-denied"
        );
        assert_eq!(
            map_client_error(ClientError::ServerError(protocol::PulseError::NoEntity)).kind(),
            "no-device"
        );
        assert_eq!(
            map_client_error(ClientError::ServerError(protocol::PulseError::Timeout)).kind(),
            "timeout"
        );
        assert_eq!(
            map_client_error(ClientError::ServerError(protocol::PulseError::NotSupported)).kind(),
            "unsupported"
        );
        assert_eq!(
            map_client_error(ClientError::ServerError(protocol::PulseError::Invalid)).kind(),
            "unsupported-config"
        );
        assert_eq!(
            map_client_error(ClientError::Io(io::Error::other("boom"))).kind(),
            "io"
        );
    }

    #[test]
    fn connect_io_errors_are_classified() {
        let path = Path::new("/run/user/0/pulse/native");
        assert_eq!(
            map_connect_io(path, &io::Error::from(io::ErrorKind::PermissionDenied)).kind(),
            "permission-denied"
        );
        assert_eq!(
            map_connect_io(path, &io::Error::from(io::ErrorKind::NotFound)).kind(),
            "device"
        );
        assert_eq!(
            map_connect_io(path, &io::Error::from(io::ErrorKind::ConnectionRefused)).kind(),
            "device"
        );
    }

    #[test]
    fn client_name_with_interior_nul_is_rejected_before_any_socket_work() {
        let err = connect_client("bad\0name").expect_err("interior NUL must be rejected");
        assert_eq!(err.kind(), "device");
        assert!(
            err.to_string().contains("NUL"),
            "message should say why: {err}"
        );
    }

    /// The handshake round-trips over an in-memory socket pair: no PulseAudio server is
    /// needed, only a peer that speaks the protocol well enough to answer `AUTH` and
    /// `SET_CLIENT_NAME`.
    #[test]
    fn handshake_round_trips_over_a_unix_socket_pair() {
        use std::io::BufReader;

        let (client_side, server_side) = UnixStream::pair().expect("socket pair");
        let server = std::thread::spawn(move || -> Result<(u32, u32), String> {
            let mut sock = BufReader::new(server_side);
            let (auth_seq, auth_cmd) =
                protocol::read_command_message(&mut sock, protocol::MAX_VERSION)
                    .map_err(|e| e.to_string())?;
            assert!(
                matches!(auth_cmd, protocol::Command::Auth(_)),
                "expected AUTH"
            );
            protocol::write_reply_message(
                sock.get_mut(),
                auth_seq,
                &protocol::AuthReply {
                    version: protocol::MAX_VERSION,
                    use_shm: false,
                    use_memfd: false,
                },
                protocol::MAX_VERSION,
            )
            .map_err(|e| e.to_string())?;

            let (name_seq, name_cmd) =
                protocol::read_command_message(&mut sock, protocol::MAX_VERSION)
                    .map_err(|e| e.to_string())?;
            assert!(
                matches!(name_cmd, protocol::Command::SetClientName(_)),
                "expected SET_CLIENT_NAME"
            );
            protocol::write_reply_message(
                sock.get_mut(),
                name_seq,
                &protocol::SetClientNameReply { client_id: 7 },
                protocol::MAX_VERSION,
            )
            .map_err(|e| e.to_string())?;

            Ok((auth_seq, name_seq))
        });

        let name = CString::new(DEFAULT_CLIENT_NAME).expect("static name");
        let client = Client::new_unix(&name, client_side, Some(&[0u8; 256][..]))
            .expect("handshake should succeed against the stub server");
        let (auth_seq, name_seq) = server
            .join()
            .expect("server thread panicked")
            .expect("server side failed");
        assert_eq!(auth_seq, 0, "AUTH is sent with sequence 0");
        assert_eq!(name_seq, 1, "SET_CLIENT_NAME is sent with sequence 1");
        drop(client);
    }

    /// Regression test for the reactor-thread leak.
    ///
    /// The `pulseaudio` reactor parks in an unbounded `poll()`, so dropping every `Client`
    /// clone does not stop it. `PulseConnection` shuts the socket down instead; the peer
    /// must observe EOF, which is what makes the reactor exit.
    #[test]
    fn dropping_the_connection_shuts_the_socket_down_so_the_reactor_can_exit() {
        use std::io::{BufReader, Read};
        use std::time::Duration;

        let (client_side, server_side) = UnixStream::pair().expect("socket pair");
        server_side
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");

        let server = std::thread::spawn(move || -> Result<usize, String> {
            let mut sock = BufReader::new(server_side);
            let (auth_seq, _) = protocol::read_command_message(&mut sock, protocol::MAX_VERSION)
                .map_err(|e| e.to_string())?;
            protocol::write_reply_message(
                sock.get_mut(),
                auth_seq,
                &protocol::AuthReply {
                    version: protocol::MAX_VERSION,
                    use_shm: false,
                    use_memfd: false,
                },
                protocol::MAX_VERSION,
            )
            .map_err(|e| e.to_string())?;

            let (name_seq, _) = protocol::read_command_message(&mut sock, protocol::MAX_VERSION)
                .map_err(|e| e.to_string())?;
            protocol::write_reply_message(
                sock.get_mut(),
                name_seq,
                &protocol::SetClientNameReply { client_id: 7 },
                protocol::MAX_VERSION,
            )
            .map_err(|e| e.to_string())?;

            // Blocks until the client shuts the socket down; `Ok(0)` is EOF.
            let mut scratch = [0u8; 64];
            sock.get_mut().read(&mut scratch).map_err(|e| e.to_string())
        });

        let name = CString::new(DEFAULT_CLIENT_NAME).expect("static name");
        let shutdown_handle = client_side.try_clone().expect("dup the socket");
        let client = Client::new_unix(&name, client_side, Some(&[0u8; 256][..]))
            .expect("handshake should succeed");
        let connection = Arc::new(PulseConnection {
            client,
            shutdown_handle,
        });

        drop(connection);

        let observed = server
            .join()
            .expect("server thread panicked")
            .expect("server side read failed (a timeout means the socket was never shut down)");
        assert_eq!(
            observed, 0,
            "the peer must observe EOF once the connection guard drops"
        );
    }
}
