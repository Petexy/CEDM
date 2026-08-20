//! Background greetd conversation actor.

use crate::greeter::{AuthMessageType, Client, ErrorType, Response};
use crate::handoff::BackgroundHandoff;
use crate::sessions::Session;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use zeroize::Zeroizing;

pub type AttemptId = u64;

pub enum Command {
    Begin {
        attempt: AttemptId,
        username: String,
        session: Session,
    },
    Answer {
        attempt: AttemptId,
        answer: Zeroizing<String>,
    },
    Start {
        attempt: AttemptId,
        handoff: Option<BackgroundHandoff>,
    },
    Cancel {
        attempt: AttemptId,
    },
    Stop,
}

/// Which of the two kinds of refusal ended an attempt.
///
/// greetd draws this line itself and the protocol says why: an `auth_error`
/// "is not a fatal error, and is likely caused by incorrect credentials",
/// while a plain `error` is the machine reporting that something went wrong.
/// They are two different sentences to the person at the keyboard, and only
/// one of them is worth repeating verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// PAM would not accept the answer. A mistyped password is the ordinary
    /// cause; a locked, expired or otherwise unusable account arrives the
    /// same way, because PAM deliberately does not say which.
    Rejected,
    /// Anything else: greetd, the socket, this worker, the session launch.
    Service,
}

#[derive(Debug)]
pub enum Event {
    Prompt {
        attempt: AttemptId,
        message: String,
        secret: bool,
    },
    Status {
        attempt: AttemptId,
        message: String,
    },
    Authenticated {
        attempt: AttemptId,
    },
    Started {
        attempt: AttemptId,
    },
    Failed {
        attempt: AttemptId,
        failure: Failure,
        message: String,
    },
    Cancelled {
        attempt: AttemptId,
    },
}

impl Event {
    pub fn attempt(&self) -> AttemptId {
        match self {
            Self::Prompt { attempt, .. }
            | Self::Status { attempt, .. }
            | Self::Authenticated { attempt }
            | Self::Started { attempt }
            | Self::Failed { attempt, .. }
            | Self::Cancelled { attempt } => *attempt,
        }
    }
}

pub struct Actor {
    commands: Sender<Command>,
    events: Receiver<Event>,
    interrupt: Arc<Mutex<InterruptState>>,
}

#[derive(Default)]
struct InterruptState {
    active: Option<(AttemptId, UnixStream)>,
    latest_attempt: Option<AttemptId>,
}

impl Actor {
    pub fn spawn() -> Self {
        Self::spawn_inner(None)
    }

    fn spawn_inner(socket: Option<PathBuf>) -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let interrupt = Arc::new(Mutex::new(InterruptState::default()));
        let actor_interrupt = interrupt.clone();
        thread::Builder::new()
            .name("cedm-greetd".to_string())
            .spawn(move || run(command_rx, event_tx, actor_interrupt, socket))
            .expect("could not create greetd conversation thread");
        Self {
            commands: command_tx,
            events: event_rx,
            interrupt,
        }
    }

    pub fn send(&self, command: Command) -> bool {
        interrupt_for_command(&self.interrupt, &command);
        self.commands.send(command).is_ok()
    }
    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    #[cfg(test)]
    fn spawn_at(socket: &std::path::Path) -> Self {
        Self::spawn_inner(Some(socket.to_path_buf()))
    }
}

fn session_environment(session: &Session, handoff: Option<&BackgroundHandoff>) -> Vec<String> {
    let mut environment = session.environment();
    environment.retain(|entry| !entry.starts_with(&format!("{}=", crate::handoff::ENV)));
    if session.line_xin_bar {
        if let Some(handoff) = handoff {
            environment.push(handoff.environment());
        }
    }
    environment
}

impl Drop for Actor {
    fn drop(&mut self) {
        shutdown_active(&self.interrupt, None);
        let _ = self.commands.send(Command::Stop);
    }
}

fn run(
    commands: Receiver<Command>,
    events: Sender<Event>,
    interrupt: Arc<Mutex<InterruptState>>,
    socket: Option<PathBuf>,
) {
    let mut client: Option<Client> = None;
    let mut selected: Option<Session> = None;
    let mut current_attempt: Option<AttemptId> = None;
    while let Ok(command) = commands.recv() {
        match command {
            Command::Begin {
                attempt,
                username,
                session,
            } => {
                // Dropping the previous connection is the interruptible
                // cancellation primitive. Sending a synchronous CancelSession
                // here could itself wait behind an unhealthy greetd daemon.
                client = None;
                selected = None;
                current_attempt = Some(attempt);
                let connected = socket
                    .as_deref()
                    .map(Client::connect)
                    .unwrap_or_else(Client::connect_from_environment);
                match connected {
                    Ok(mut greetd) => {
                        if let Err(error) = install_interrupt(&interrupt, attempt, &greetd) {
                            let _ = events.send(Event::Failed {
                                attempt,
                                failure: Failure::Service,
                                message: error.to_string(),
                            });
                            clear_interrupt(&interrupt, attempt);
                            continue;
                        }
                        let cancelled = || is_cancelled(&interrupt, attempt);
                        match open_session(&mut greetd, &username, &cancelled) {
                            Ok(response) => {
                                selected = Some(session);
                                if is_cancelled(&interrupt, attempt) {
                                    current_attempt = None;
                                    clear_interrupt(&interrupt, attempt);
                                } else if drive(attempt, response, &mut greetd, &events, &interrupt)
                                    == Conversation::Open
                                {
                                    client = Some(greetd);
                                } else {
                                    selected = None;
                                    current_attempt = None;
                                    clear_interrupt(&interrupt, attempt);
                                }
                            }
                            Err(error) => {
                                let _ = events.send(Event::Failed {
                                    attempt,
                                    failure: Failure::Service,
                                    message: error.to_string(),
                                });
                                current_attempt = None;
                                clear_interrupt(&interrupt, attempt);
                            }
                        }
                    }
                    Err(error) => {
                        let _ = events.send(Event::Failed {
                            attempt,
                            failure: Failure::Service,
                            message: error.to_string(),
                        });
                        current_attempt = None;
                        clear_interrupt(&interrupt, attempt);
                    }
                }
            }
            Command::Answer { attempt, answer } => {
                if current_attempt != Some(attempt) {
                    continue;
                }
                let Some(greetd) = client.as_mut() else {
                    let _ = events.send(Event::Failed {
                        attempt,
                        failure: Failure::Service,
                        message: "No authentication conversation is active".to_string(),
                    });
                    continue;
                };
                match greetd.answer_cancellable(Some(&answer), || is_cancelled(&interrupt, attempt))
                {
                    Ok(response) => {
                        if !is_cancelled(&interrupt, attempt)
                            && drive(attempt, response, greetd, &events, &interrupt)
                                == Conversation::Closed
                        {
                            client = None;
                            selected = None;
                            current_attempt = None;
                            clear_interrupt(&interrupt, attempt);
                        }
                    }
                    Err(error) => {
                        let _ = events.send(Event::Failed {
                            attempt,
                            failure: Failure::Service,
                            message: error.to_string(),
                        });
                        client = None;
                        selected = None;
                        current_attempt = None;
                        clear_interrupt(&interrupt, attempt);
                    }
                }
                if is_cancelled(&interrupt, attempt) {
                    client = None;
                    selected = None;
                    current_attempt = None;
                }
            }
            Command::Start { attempt, handoff } => {
                if current_attempt != Some(attempt) {
                    continue;
                }
                let (Some(greetd), Some(session)) = (client.as_mut(), selected.as_ref()) else {
                    let _ = events.send(Event::Failed {
                        attempt,
                        failure: Failure::Service,
                        message: "No authenticated session is ready".to_string(),
                    });
                    continue;
                };
                let environment = session_environment(session, handoff.as_ref());
                // Resolved here rather than once at start-up: the wrapper is a
                // file on disk, and an upgrade that installs it should not need
                // the greeter restarted before it is used.
                let command = session.launch_command(crate::sessions::launcher().as_deref());
                match greetd.start_session_cancellable(&command, &environment, || {
                    is_cancelled(&interrupt, attempt)
                }) {
                    Ok(Response::Success) => {
                        let _ = events.send(Event::Started { attempt });
                        client = None;
                        selected = None;
                        current_attempt = None;
                        clear_interrupt(&interrupt, attempt);
                    }
                    Ok(Response::Error {
                        error_type,
                        description,
                    }) => {
                        report_failure(attempt, error_type, &description, &events);
                        client = None;
                        selected = None;
                        current_attempt = None;
                        clear_interrupt(&interrupt, attempt);
                    }
                    Ok(Response::AuthMessage { .. }) => {
                        let _ = events.send(Event::Failed {
                            attempt,
                            failure: Failure::Service,
                            message: "greetd requested authentication after session launch"
                                .to_string(),
                        });
                        client = None;
                        selected = None;
                        current_attempt = None;
                        clear_interrupt(&interrupt, attempt);
                    }
                    Err(error) => {
                        let _ = events.send(Event::Failed {
                            attempt,
                            failure: Failure::Service,
                            message: error.to_string(),
                        });
                        client = None;
                        selected = None;
                        current_attempt = None;
                        clear_interrupt(&interrupt, attempt);
                    }
                }
            }
            Command::Cancel { attempt } => {
                if current_attempt != Some(attempt) {
                    clear_interrupt(&interrupt, attempt);
                    continue;
                }
                client = None;
                selected = None;
                current_attempt = None;
                clear_interrupt(&interrupt, attempt);
                let _ = events.send(Event::Cancelled { attempt });
            }
            Command::Stop => {
                shutdown_active(&interrupt, None);
                break;
            }
        }
    }
}

fn interrupt_for_command(interrupt: &Mutex<InterruptState>, command: &Command) {
    let Ok(mut state) = interrupt.lock() else {
        return;
    };
    match command {
        Command::Cancel { attempt } => {
            if state.latest_attempt == Some(*attempt) {
                state.latest_attempt = None;
            }
            if state
                .active
                .as_ref()
                .is_some_and(|(active, _)| active == attempt)
            {
                if let Some((_, stream)) = &state.active {
                    let _ = stream.shutdown(Shutdown::Both);
                }
            }
        }
        Command::Begin { attempt, .. } => {
            state.latest_attempt = Some(*attempt);
            if state
                .active
                .as_ref()
                .is_some_and(|(active, _)| active != attempt)
            {
                if let Some((_, stream)) = &state.active {
                    let _ = stream.shutdown(Shutdown::Both);
                }
            }
        }
        Command::Stop => {
            state.latest_attempt = None;
            if let Some((_, stream)) = &state.active {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
        Command::Answer { .. } | Command::Start { .. } => {}
    }
}

fn install_interrupt(
    interrupt: &Mutex<InterruptState>,
    attempt: AttemptId,
    client: &Client,
) -> anyhow::Result<()> {
    let stream = client.interrupt_handle()?;
    let mut state = interrupt
        .lock()
        .map_err(|_| anyhow::anyhow!("greetd cancellation state is unavailable"))?;
    if state.latest_attempt != Some(attempt) {
        let _ = stream.shutdown(Shutdown::Both);
    }
    state.active = Some((attempt, stream));
    Ok(())
}

fn clear_interrupt(interrupt: &Mutex<InterruptState>, attempt: AttemptId) {
    let Ok(mut state) = interrupt.lock() else {
        return;
    };
    if state
        .active
        .as_ref()
        .is_some_and(|(active, _)| *active == attempt)
    {
        state.active = None;
    }
}

fn is_cancelled(interrupt: &Mutex<InterruptState>, attempt: AttemptId) -> bool {
    interrupt
        .lock()
        .map(|state| state.latest_attempt != Some(attempt))
        .unwrap_or(true)
}

fn shutdown_active(interrupt: &Mutex<InterruptState>, attempt: Option<AttemptId>) {
    let Ok(state) = interrupt.lock() else { return };
    if let Some((active, stream)) = &state.active {
        if attempt.is_none_or(|attempt| attempt == *active) {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

/// Whether the conversation on a connection can still be continued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Conversation {
    /// Parked on the user: a prompt is waiting to be answered, or the session
    /// is authenticated and waiting to be started.
    Open,
    /// Over. greetd has been told so, and the connection is of no further use.
    Closed,
}

/// Open a PAM conversation for `username`, clearing a stale one first.
///
/// greetd configures one session at a time and holds it until somebody
/// cancels it. It is not dropped when the greeter's connection goes away, so
/// an attempt that ended without a `cancel_session` — a refusal, a greeter
/// killed at its password prompt — leaves the daemon answering every later
/// `create_session` with the same refusal, and the machine cannot be logged
/// into at all until greetd itself is restarted.
///
/// [`drive`] cancels the sessions this greeter ends, which is where that has
/// to be done. This is the other half: a login being *started* is the one
/// moment where a session already under configuration is certainly nobody's,
/// and clearing it is the only way back in.
fn open_session(
    greetd: &mut Client,
    username: &str,
    cancelled: &impl Fn() -> bool,
) -> anyhow::Result<Response> {
    let response = greetd.create_session_cancellable(username, cancelled)?;
    let Response::Error {
        error_type: ErrorType::Error,
        description,
    } = &response
    else {
        return Ok(response);
    };
    if !leftover_session(description) {
        return Ok(response);
    }
    tracing::warn!(
        %description,
        "clearing a greetd session left behind by an earlier attempt"
    );
    greetd.cancel_cancellable(cancelled)?;
    greetd.create_session_cancellable(username, cancelled)
}

/// Whether greetd's refusal means "there is already one of those".
///
/// Read out of the sentence because the protocol offers nothing better: every
/// refusal that is not an `auth_error` is a plain `error` carrying prose. Kept
/// deliberately narrow — a session that is already *scheduled* belongs to a
/// login on its way out of this greeter, and cancelling that would be
/// cancelling somebody's successful sign-in.
fn leftover_session(description: &str) -> bool {
    let description = description.to_ascii_lowercase();
    description.contains("already being configured") || description.contains("already active")
}

/// Tell greetd that this conversation is over.
///
/// Best-effort by nature: the answer changes nothing here, and the reason for
/// sending it is the daemon's state rather than this greeter's.
fn end_session(client: &mut Client, attempt: AttemptId, interrupt: &Mutex<InterruptState>) {
    match client.cancel_cancellable(|| is_cancelled(interrupt, attempt)) {
        Ok(Response::Error { description, .. }) => {
            tracing::debug!(%description, "greetd had no session left to cancel");
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "could not cancel the refused greetd session"),
    }
}

fn failure_of(error_type: ErrorType) -> Failure {
    match error_type {
        ErrorType::AuthError => Failure::Rejected,
        ErrorType::Error => Failure::Service,
    }
}

/// Report a refusal from greetd, keeping its own words in the log.
///
/// The message travels with the event because a service failure has nothing
/// to describe it but greetd's sentence. What the interface does with it for
/// a rejected password is the interface's business; the log keeps the
/// original either way.
fn report_failure(
    attempt: AttemptId,
    error_type: ErrorType,
    description: &str,
    events: &Sender<Event>,
) {
    let failure = failure_of(error_type);
    tracing::info!(?failure, %description, "greetd ended the attempt");
    let _ = events.send(Event::Failed {
        attempt,
        failure,
        message: description.to_string(),
    });
}

fn drive(
    attempt: AttemptId,
    mut response: Response,
    client: &mut Client,
    events: &Sender<Event>,
    interrupt: &Mutex<InterruptState>,
) -> Conversation {
    loop {
        match response {
            Response::Success => {
                let _ = events.send(Event::Authenticated { attempt });
                return Conversation::Open;
            }
            Response::Error {
                error_type,
                description,
            } => {
                // The refusal ends the attempt for the user, but not for
                // greetd: the session stays under configuration until it is
                // cancelled, and the next `create_session` — the very next
                // thing a person who mistyped their password does — is
                // refused because of it. See [`open_session`].
                //
                // Told before the interface is, because the interface acting
                // on the failure is what interrupts this connection: a "Try
                // again" pressed the instant the message appears would cut
                // the cancellation off mid-exchange and leave behind exactly
                // the session it is here to clear.
                end_session(client, attempt, interrupt);
                report_failure(attempt, error_type, &description, events);
                return Conversation::Closed;
            }
            Response::AuthMessage {
                auth_message_type,
                auth_message,
            } if auth_message_type.needs_answer() => {
                let _ = events.send(Event::Prompt {
                    attempt,
                    message: auth_message,
                    secret: auth_message_type.is_secret(),
                });
                return Conversation::Open;
            }
            Response::AuthMessage {
                auth_message_type,
                auth_message,
            } => {
                // The word in front is the greeter's and is translated; what
                // follows is PAM's own and is not — see
                // [`crate::i18n::Strings::service_refused`] for why a module's
                // own sentence is left in the language it was written in.
                let prefix = if auth_message_type == AuthMessageType::Error {
                    crate::i18n::text().error_prefix
                } else {
                    ""
                };
                let _ = events.send(Event::Status {
                    attempt,
                    message: format!("{prefix}{auth_message}"),
                });
                match client.answer_cancellable(None, || is_cancelled(interrupt, attempt)) {
                    Ok(next) => response = next,
                    Err(error) => {
                        let _ = events.send(Event::Failed {
                            attempt,
                            failure: Failure::Service,
                            message: error.to_string(),
                        });
                        return Conversation::Closed;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, SystemTime};

    fn test_session(line_xin_bar: bool) -> Session {
        Session {
            id: if line_xin_bar { "lxb" } else { "plasma" }.into(),
            name: if line_xin_bar { "LineXinBar" } else { "Plasma" }.into(),
            comment: None,
            command: vec![if line_xin_bar {
                "lxb-session".into()
            } else {
                "startplasma-wayland".into()
            }],
            desktop_names: vec![if line_xin_bar {
                "LineXinBar".into()
            } else {
                "KDE".into()
            }],
            kind: crate::sessions::Kind::Wayland,
            source: if line_xin_bar {
                "/test/lxb.desktop".into()
            } else {
                "/test/plasma.desktop".into()
            },
            line_xin_bar,
        }
    }

    fn test_handoff() -> BackgroundHandoff {
        BackgroundHandoff {
            boot_id: "12345678-1234-1234-1234-123456789abc".into(),
            sample_ns: 2_000_000_000,
            scene_ns: 7_500_000_000,
            accent: "Blue".into(),
            theme: None,
        }
    }

    fn read_request(stream: &mut UnixStream) -> anyhow::Result<serde_json::Value> {
        use anyhow::Context;

        let mut raw_length = [0_u8; 4];
        stream
            .read_exact(&mut raw_length)
            .context("read mock greetd request length")?;
        let length = u32::from_ne_bytes(raw_length) as usize;
        let mut payload = vec![0_u8; length];
        stream
            .read_exact(&mut payload)
            .context("read mock greetd request")?;
        serde_json::from_slice(&payload).context("parse mock greetd request")
    }

    fn send(stream: &mut UnixStream, payload: &str) -> anyhow::Result<()> {
        stream.write_all(&(payload.len() as u32).to_ne_bytes())?;
        stream.write_all(payload.as_bytes())?;
        stream.flush()?;
        Ok(())
    }

    fn send_success(stream: &mut UnixStream) -> anyhow::Result<()> {
        send(stream, r#"{"type":"success"}"#)
    }

    fn mock_socket(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cedm-greetd-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn request_type(request: &serde_json::Value) -> String {
        request["type"].as_str().unwrap_or_default().to_string()
    }

    /// Some command sandboxes deny the socket syscalls these mocks need. The
    /// same protocol paths are exercised outside the sandbox during
    /// integration validation, so skipping is better than a false failure.
    fn sandboxed(event: &Event) -> bool {
        matches!(
            event,
            Event::Failed { message, .. } if message.to_lowercase().contains("permission denied")
        )
    }

    /// greetd holds the session it is configuring until somebody cancels it,
    /// and does not let go of it because a greeter went quiet or went away. A
    /// refusal that is not cancelled is therefore not the end of one attempt:
    /// it is the end of every attempt, because each later `create_session` is
    /// refused by the session still sitting there.
    #[test]
    fn a_refused_password_is_cancelled_so_the_next_attempt_is_possible() {
        let socket = mock_socket("refusal");
        let _ = fs::remove_file(&socket);
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("could not bind mock greetd socket: {error}"),
        };
        let server = std::thread::spawn(move || -> anyhow::Result<Vec<String>> {
            let mut seen = Vec::new();
            let (mut stream, _) = listener.accept()?;
            seen.push(request_type(&read_request(&mut stream)?));
            send(
                &mut stream,
                r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password:"}"#,
            )?;
            seen.push(request_type(&read_request(&mut stream)?));
            send(
                &mut stream,
                r#"{"type":"error","error_type":"auth_error","description":"authentication error: AUTH_ERR"}"#,
            )?;
            seen.push(request_type(&read_request(&mut stream)?));
            send_success(&mut stream)?;
            // The attempt made straight afterwards, on its own connection.
            let (mut retry, _) = listener.accept()?;
            seen.push(request_type(&read_request(&mut retry)?));
            send_success(&mut retry)?;
            Ok(seen)
        });

        let actor = Actor::spawn_at(&socket);
        assert!(actor.send(Command::Begin {
            attempt: 1,
            username: "alex".into(),
            session: test_session(false),
        }));
        let prompt = actor.events.recv_timeout(Duration::from_secs(2)).unwrap();
        if sandboxed(&prompt) {
            drop(actor);
            let _ = server.join();
            fs::remove_file(&socket).unwrap();
            return;
        }
        assert!(matches!(
            prompt,
            Event::Prompt {
                attempt: 1,
                secret: true,
                ..
            }
        ));

        assert!(actor.send(Command::Answer {
            attempt: 1,
            answer: Zeroizing::new("not the password".to_string()),
        }));
        match actor.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            Event::Failed {
                attempt: 1,
                failure: Failure::Rejected,
                message,
            } => assert!(message.contains("AUTH_ERR"), "{message}"),
            other => panic!("a refused password is not {other:?}"),
        }

        assert!(actor.send(Command::Begin {
            attempt: 2,
            username: "alex".into(),
            session: test_session(false),
        }));
        let second = actor.events.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            matches!(second, Event::Authenticated { attempt: 2 }),
            "the attempt made after a refusal is not {second:?}"
        );
        drop(actor);

        let seen = server.join().unwrap().unwrap();
        fs::remove_file(&socket).unwrap();
        assert_eq!(
            seen,
            [
                "create_session",
                "post_auth_message_response",
                "cancel_session",
                "create_session"
            ]
        );
    }

    /// And a daemon already holding one — left there by a greeter killed at
    /// its password prompt, or by an older build of this one — is cleared by
    /// the login that finds it, rather than being a machine that can no longer
    /// be signed into at all.
    #[test]
    fn a_session_left_under_configuration_is_cleared_by_the_next_login() {
        let socket = mock_socket("leftover");
        let _ = fs::remove_file(&socket);
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("could not bind mock greetd socket: {error}"),
        };
        let server = std::thread::spawn(move || -> anyhow::Result<Vec<String>> {
            let mut seen = Vec::new();
            let (mut stream, _) = listener.accept()?;
            seen.push(request_type(&read_request(&mut stream)?));
            send(
                &mut stream,
                r#"{"type":"error","error_type":"error","description":"a session is already being configured"}"#,
            )?;
            seen.push(request_type(&read_request(&mut stream)?));
            send_success(&mut stream)?;
            seen.push(request_type(&read_request(&mut stream)?));
            send_success(&mut stream)?;
            Ok(seen)
        });

        let actor = Actor::spawn_at(&socket);
        assert!(actor.send(Command::Begin {
            attempt: 1,
            username: "alex".into(),
            session: test_session(false),
        }));
        let event = actor.events.recv_timeout(Duration::from_secs(2)).unwrap();
        if sandboxed(&event) {
            drop(actor);
            let _ = server.join();
            fs::remove_file(&socket).unwrap();
            return;
        }
        assert!(
            matches!(event, Event::Authenticated { attempt: 1 }),
            "the login that found the stale session is not {event:?}"
        );
        drop(actor);

        let seen = server.join().unwrap().unwrap();
        fs::remove_file(&socket).unwrap();
        assert_eq!(seen, ["create_session", "cancel_session", "create_session"]);
    }

    #[test]
    fn only_a_session_still_under_configuration_is_cleared() {
        assert!(leftover_session("a session is already being configured"));
        assert!(leftover_session("session already active"));
        // Somebody's successful login, on its way out of this greeter.
        assert!(!leftover_session("a session is already scheduled"));
        assert!(!leftover_session("authentication error: AUTH_ERR"));
        assert_eq!(failure_of(ErrorType::AuthError), Failure::Rejected);
        assert_eq!(failure_of(ErrorType::Error), Failure::Service);
    }

    #[test]
    fn every_event_identifies_its_authentication_attempt() {
        let events = [
            Event::Prompt {
                attempt: 7,
                message: "Password".into(),
                secret: true,
            },
            Event::Status {
                attempt: 7,
                message: "Checking".into(),
            },
            Event::Authenticated { attempt: 7 },
            Event::Started { attempt: 7 },
            Event::Failed {
                attempt: 7,
                failure: Failure::Rejected,
                message: "Denied".into(),
            },
            Event::Cancelled { attempt: 7 },
        ];
        assert!(events.iter().all(|event| event.attempt() == 7));
    }

    #[test]
    fn non_lxb_environment_never_gets_the_handoff_key() {
        let environment = session_environment(&test_session(false), Some(&test_handoff()));
        assert!(environment
            .iter()
            .all(|entry| !entry.starts_with(crate::handoff::ENV)));
    }

    #[test]
    fn lxb_start_session_request_carries_the_exact_one_shot_handoff() {
        let socket = std::env::temp_dir().join(format!(
            "cedm-greetd-handoff-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_file(&socket);
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            // The syscall sandbox used by the repository test runner can
            // prohibit creating Unix sockets. The same protocol path is
            // exercised by the nested integration harness outside it.
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("could not bind mock greetd socket: {error}"),
        };
        let server = std::thread::spawn(move || -> anyhow::Result<serde_json::Value> {
            let (mut stream, _) = listener.accept()?;
            let create = read_request(&mut stream)?;
            anyhow::ensure!(create["type"] == "create_session");
            anyhow::ensure!(create["username"] == "alex");
            send_success(&mut stream)?;
            let start = read_request(&mut stream)?;
            send_success(&mut stream)?;
            Ok(start)
        });

        let actor = Actor::spawn_at(&socket);
        assert!(actor.send(Command::Begin {
            attempt: 1,
            username: "alex".into(),
            session: test_session(true),
        }));
        let authenticated = actor.events.recv_timeout(Duration::from_secs(2)).unwrap();
        if let Event::Failed { message, .. } = &authenticated {
            if message.to_lowercase().contains("permission denied") {
                // Some command sandboxes deny the socket timeout syscall. The
                // same test is run outside that sandbox during integration
                // validation so the complete protocol path is still covered.
                drop(actor);
                let _ = server.join();
                fs::remove_file(&socket).unwrap();
                return;
            }
        }
        assert!(matches!(authenticated, Event::Authenticated { attempt: 1 }));

        let handoff = test_handoff();
        assert!(actor.send(Command::Start {
            attempt: 1,
            handoff: Some(handoff.clone()),
        }));
        assert!(matches!(
            actor.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Started { attempt: 1 }
        ));
        drop(actor);

        let request = server.join().unwrap().unwrap();
        fs::remove_file(&socket).unwrap();
        assert_eq!(request["type"], "start_session");
        // Whether the packaged wrapper is in front of it depends on whether
        // this is an installed machine or a bare checkout, so pin the part that
        // does not: the session's own argv is what greetd is asked to end up
        // running. `launch_command` covers the wrapping itself.
        let command = request["cmd"].as_array().unwrap();
        assert_eq!(command.last().unwrap(), "lxb-session");
        let environment = request["env"].as_array().unwrap();
        let handoff_assignment = handoff.environment();
        assert_eq!(
            environment
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|entry| entry.starts_with(crate::handoff::ENV))
                .collect::<Vec<_>>(),
            [handoff_assignment.as_str()]
        );
    }
}
