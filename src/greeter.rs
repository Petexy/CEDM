//! Minimal synchronous client for greetd's length-prefixed JSON protocol.
//!
//! Authentication and session creation stay in greetd/PAM. This process only
//! displays each PAM conversation message and returns the user's answer; it
//! never needs root privileges and never implements authentication policy.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;
use zeroize::Zeroizing;

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const IO_POLL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMessageType {
    Visible,
    Secret,
    Info,
    Error,
}

impl AuthMessageType {
    pub fn needs_answer(self) -> bool {
        matches!(self, Self::Visible | Self::Secret)
    }

    pub fn is_secret(self) -> bool {
        self == Self::Secret
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success,
    Error {
        error_type: ErrorType,
        description: String,
    },
    AuthMessage {
        auth_message_type: AuthMessageType,
        auth_message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    AuthError,
    Error,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request<'a> {
    CreateSession {
        username: &'a str,
    },
    PostAuthMessageResponse {
        response: Option<&'a str>,
    },
    StartSession {
        cmd: &'a [String],
        env: &'a [String],
    },
    CancelSession,
}

pub struct Client {
    stream: UnixStream,
}

impl Client {
    pub fn connect_from_environment() -> anyhow::Result<Self> {
        let path = env::var_os("GREETD_SOCK").context("GREETD_SOCK is not set")?;
        Self::connect(Path::new(&path))
    }

    pub fn connect(path: &Path) -> anyhow::Result<Self> {
        let stream = UnixStream::connect(path)
            .with_context(|| format!("could not connect to greetd at {}", path.display()))?;
        stream
            .set_read_timeout(Some(IO_POLL))
            .context("could not set greetd read timeout")?;
        stream
            .set_write_timeout(Some(IO_POLL))
            .context("could not set greetd write timeout")?;
        Ok(Self { stream })
    }

    /// A clone used only to interrupt a blocked exchange from the UI thread.
    /// Shutting it down wakes reads/writes on the actor's copy without sharing
    /// protocol I/O between threads.
    pub(crate) fn interrupt_handle(&self) -> anyhow::Result<UnixStream> {
        self.stream
            .try_clone()
            .context("could not clone the greetd connection for cancellation")
    }

    pub fn create_session(&mut self, username: &str) -> anyhow::Result<Response> {
        self.create_session_cancellable(username, || false)
    }

    pub(crate) fn create_session_cancellable(
        &mut self,
        username: &str,
        cancelled: impl Fn() -> bool,
    ) -> anyhow::Result<Response> {
        if username.is_empty() || username.contains('\0') {
            bail!("greetd username is empty or contains NUL");
        }
        self.exchange(&Request::CreateSession { username }, cancelled)
    }

    pub fn answer(&mut self, answer: Option<&str>) -> anyhow::Result<Response> {
        self.answer_cancellable(answer, || false)
    }

    pub(crate) fn answer_cancellable(
        &mut self,
        answer: Option<&str>,
        cancelled: impl Fn() -> bool,
    ) -> anyhow::Result<Response> {
        if answer.is_some_and(|answer| answer.contains('\0')) {
            bail!("greetd authentication response contains NUL");
        }
        self.exchange(
            &Request::PostAuthMessageResponse { response: answer },
            cancelled,
        )
    }

    pub fn start_session(
        &mut self,
        command: &[String],
        environment: &[String],
    ) -> anyhow::Result<Response> {
        self.start_session_cancellable(command, environment, || false)
    }

    pub(crate) fn start_session_cancellable(
        &mut self,
        command: &[String],
        environment: &[String],
        cancelled: impl Fn() -> bool,
    ) -> anyhow::Result<Response> {
        validate_session_request(command, environment)?;
        self.exchange(
            &Request::StartSession {
                cmd: command,
                env: environment,
            },
            cancelled,
        )
    }

    pub fn cancel(&mut self) -> anyhow::Result<Response> {
        self.cancel_cancellable(|| false)
    }

    /// Discard the session greetd currently has under configuration.
    ///
    /// The protocol requires this of a greeter that stops answering an
    /// authentication message, whether it stopped because the user gave up or
    /// because PAM refused them: `auth_message` "must be answered with either
    /// post_auth_message_response or cancel_session".
    pub(crate) fn cancel_cancellable(
        &mut self,
        cancelled: impl Fn() -> bool,
    ) -> anyhow::Result<Response> {
        self.exchange(&Request::CancelSession, cancelled)
    }

    fn exchange(
        &mut self,
        request: &Request<'_>,
        cancelled: impl Fn() -> bool,
    ) -> anyhow::Result<Response> {
        // Authentication responses are copied by JSON serialization. Keep the
        // copy in an RAII guard so it is zeroized on success and on every
        // early-return/write-error path.
        {
            let frame = request_frame(request)?;
            write_all_cancellable(&mut self.stream, frame.as_slice(), &cancelled)
                .context("could not write greetd request frame")?;
            self.stream
                .flush()
                .context("could not flush greetd request")?;
        } // Zeroize the serialized secret before waiting for greetd's reply.

        let mut raw_length = [0_u8; 4];
        read_exact_cancellable(&mut self.stream, &mut raw_length, &cancelled)
            .context("could not read greetd response length")?;
        let length = response_length(raw_length)?;
        let mut response = vec![0_u8; length];
        read_exact_cancellable(&mut self.stream, &mut response, &cancelled)
            .context("could not read complete greetd response")?;
        serde_json::from_slice(&response).context("greetd returned invalid JSON")
    }
}

fn write_all_cancellable(
    stream: &mut UnixStream,
    mut bytes: &[u8],
    cancelled: &impl Fn() -> bool,
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        if cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "greetd exchange cancelled",
            ));
        }
        match stream.write(bytes) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(written) => bytes = &bytes[written..],
            Err(error) if retryable_timeout(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn read_exact_cancellable(
    stream: &mut UnixStream,
    mut bytes: &mut [u8],
    cancelled: &impl Fn() -> bool,
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        if cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "greetd exchange cancelled",
            ));
        }
        match stream.read(bytes) {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => bytes = &mut bytes[read..],
            Err(error) if retryable_timeout(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn retryable_timeout(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

fn request_frame(request: &Request<'_>) -> anyhow::Result<Zeroizing<Vec<u8>>> {
    // Keep both the serializer's payload and the final wire frame guarded: the
    // former remains a distinct allocation after it is copied into the frame.
    let payload = Zeroizing::new(serde_json::to_vec(request)?);
    if payload.len() > MAX_REQUEST_BYTES {
        bail!("greetd request exceeds the 64-KiB safety limit");
    }
    let length = u32::try_from(payload.len()).context("greetd request exceeds u32")?;
    let mut frame = Zeroizing::new(Vec::with_capacity(4 + payload.len()));
    frame.extend_from_slice(&length.to_ne_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn response_length(raw: [u8; 4]) -> anyhow::Result<usize> {
    let length = u32::from_ne_bytes(raw) as usize;
    if length == 0 {
        bail!("greetd returned an empty response frame");
    }
    if length > MAX_RESPONSE_BYTES {
        bail!("greetd response exceeds the one-megabyte safety limit");
    }
    Ok(length)
}

fn validate_session_request(command: &[String], environment: &[String]) -> anyhow::Result<()> {
    if command.first().is_none_or(String::is_empty) {
        bail!("greetd session command is empty");
    }
    if command.iter().any(|argument| argument.contains('\0')) {
        bail!("greetd session command contains NUL");
    }
    for assignment in environment {
        let Some((name, _)) = assignment.split_once('=') else {
            bail!("greetd environment entry is not NAME=value");
        };
        if name.is_empty() || name.contains('\0') || assignment.contains('\0') {
            bail!("greetd environment entry has an invalid name or contains NUL");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Shutdown;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    #[test]
    fn frames_native_length_prefixed_json() {
        let frame = request_frame(&Request::CreateSession { username: "alex" }).unwrap();
        let length = u32::from_ne_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(length, frame.len() - 4);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&frame[4..]).unwrap(),
            serde_json::json!({"type":"create_session","username":"alex"})
        );
    }

    #[test]
    fn bounds_requests_before_writing_and_rejects_bad_session_argv() {
        let oversized = "s".repeat(MAX_REQUEST_BYTES);
        assert!(request_frame(&Request::PostAuthMessageResponse {
            response: Some(&oversized)
        })
        .is_err());
        assert!(validate_session_request(&[], &[]).is_err());
        assert!(validate_session_request(
            &["session".to_string()],
            &["not-an-assignment".to_string()]
        )
        .is_err());
    }

    #[test]
    fn refuses_an_oversized_response_before_allocating_it() {
        assert!(response_length(((MAX_RESPONSE_BYTES + 1) as u32).to_ne_bytes()).is_err());
        assert!(response_length(0_u32.to_ne_bytes()).is_err());
    }

    #[test]
    fn an_interrupt_handle_shuts_down_the_actor_connection() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        let mut client = Client { stream };
        let interrupt = client.interrupt_handle().unwrap();
        if let Err(error) = interrupt.shutdown(Shutdown::Both) {
            // Some syscall sandboxes deny shutdown(2). Production still has
            // the bounded I/O timeout as its fallback cancellation path.
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                return;
            }
            panic!("could not interrupt socket pair: {error}");
        }
        let mut byte = [0_u8; 1];
        assert_eq!(peer.read(&mut byte).unwrap(), 0);
        assert_eq!(client.stream.read(&mut byte).unwrap(), 0);
    }

    #[test]
    fn a_cancellation_flag_interrupts_a_silent_daemon() {
        let (stream, _silent_peer) = UnixStream::pair().unwrap();
        for result in [
            stream.set_read_timeout(Some(IO_POLL)),
            stream.set_write_timeout(Some(IO_POLL)),
        ] {
            if let Err(error) = result {
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    return;
                }
                panic!("could not configure socket-pair timeout: {error}");
            }
        }
        let mut client = Client { stream };
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = cancelled.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(25));
            signal.store(true, Ordering::Release);
        });
        let started = Instant::now();
        assert!(client
            .create_session_cancellable("alex", || cancelled.load(Ordering::Acquire))
            .is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
