// Module split from the browser binary. This file owns the first DG-1
// process boundary: bounded protocol framing over child stdin/stdout, an
// internal worker mode, and a lifecycle probe. Page-runtime and frame-plane
// ownership move here in later slices of issue #53.

use std::collections::HashSet;
use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::process::{
    Child, ChildStdin, ChildStdout, Command as ProcessCommand, ExitStatus, Stdio,
};

use silksurf_core::engine_protocol::{
    Command as ProtocolCommand, CrashReason, Event, Message, ProfileId, ProtocolError, ViewId,
    Viewport, MAX_MESSAGE_BYTES,
};

const ENVELOPE_HEADER_BYTES: usize = 10;
const NATIVE_ENGINE_WORKER_FLAG: &str = "--silksurf-native-engine-worker";
const NATIVE_ENGINE_PROBE_FLAG: &str = "--silksurf-native-engine-supervisor-probe";

#[derive(Debug)]
pub(crate) enum NativeEngineProcessError {
    Io(io::Error),
    Protocol(ProtocolError),
    MissingPipe(&'static str),
    MissingChildHandle,
    UnexpectedDirection,
    UnexpectedEvent(&'static str),
    UnsupportedCommand(&'static str),
    ChildFailed(Option<i32>),
}

impl fmt::Display for NativeEngineProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Protocol(error) => write!(formatter, "protocol error: {error}"),
            Self::MissingPipe(name) => write!(formatter, "child process has no {name} pipe"),
            Self::MissingChildHandle => formatter.write_str("child process handle is absent"),
            Self::UnexpectedDirection => formatter.write_str(
                "received an event on the command stream or a command on the event stream",
            ),
            Self::UnexpectedEvent(expected) => {
                write!(formatter, "received an unexpected event; expected {expected}")
            }
            Self::UnsupportedCommand(command) => {
                write!(
                    formatter,
                    "native engine worker command is not bound yet: {command}"
                )
            }
            Self::ChildFailed(code) => {
                write!(formatter, "native engine worker exited with {code:?}")
            }
        }
    }
}

impl std::error::Error for NativeEngineProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Protocol(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for NativeEngineProcessError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ProtocolError> for NativeEngineProcessError {
    fn from(error: ProtocolError) -> Self {
        Self::Protocol(error)
    }
}

/// Runs one of the internal process-boundary modes before normal CLI parsing.
/// Returns an exit code when an internal mode matched, or `None` for the normal
/// browser entry point.
pub(crate) fn run_internal_engine_process_mode(args: &[String]) -> Option<i32> {
    if args
        .iter()
        .any(|argument| argument == NATIVE_ENGINE_WORKER_FLAG)
    {
        return Some(run_worker_stdio());
    }
    if args
        .iter()
        .any(|argument| argument == NATIVE_ENGINE_PROBE_FLAG)
    {
        return Some(run_supervisor_probe());
    }
    None
}

fn run_worker_stdio() -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    match run_native_engine_worker(&mut reader, &mut writer) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("[SilkSurf] native engine worker failed: {error}");
            2
        }
    }
}

fn run_supervisor_probe() -> i32 {
    match supervisor_probe() {
        Ok(()) => {
            eprintln!("[SilkSurf] Native engine supervisor probe: OK");
            0
        }
        Err(error) => {
            eprintln!("[SilkSurf] Native engine supervisor probe failed: {error}");
            1
        }
    }
}

fn supervisor_probe() -> Result<(), NativeEngineProcessError> {
    let executable = std::env::current_exe()?;
    let mut engine = NativeEngineProcess::spawn(&executable)?;
    let view = ViewId::new(1);

    engine.send(ProtocolCommand::CreateView {
        view,
        profile: ProfileId::new(1),
        viewport: Viewport {
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
            scale_permille: 1000,
        },
    })?;
    expect_view_created(engine.receive()?, view)?;

    engine.send(ProtocolCommand::CloseView { view })?;
    expect_view_closed(engine.receive()?, view)?;

    let status = engine.shutdown()?;
    if !status.success() {
        return Err(NativeEngineProcessError::ChildFailed(status.code()));
    }
    Ok(())
}

fn expect_view_created(event: Event, expected: ViewId) -> Result<(), NativeEngineProcessError> {
    match event {
        Event::ViewCreated { view } if view == expected => Ok(()),
        _ => Err(NativeEngineProcessError::UnexpectedEvent("ViewCreated")),
    }
}

fn expect_view_closed(event: Event, expected: ViewId) -> Result<(), NativeEngineProcessError> {
    match event {
        Event::ViewClosed { view } if view == expected => Ok(()),
        _ => Err(NativeEngineProcessError::UnexpectedEvent("ViewClosed")),
    }
}

struct NativeEngineProcess {
    child: Option<Child>,
    command_writer: Option<ChildStdin>,
    event_reader: BufReader<ChildStdout>,
}

impl NativeEngineProcess {
    fn spawn(executable: &std::path::Path) -> Result<Self, NativeEngineProcessError> {
        let mut child = ProcessCommand::new(executable)
            .arg(NATIVE_ENGINE_WORKER_FLAG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let command_writer = child
            .stdin
            .take()
            .ok_or(NativeEngineProcessError::MissingPipe("stdin"))?;
        let event_reader = child
            .stdout
            .take()
            .ok_or(NativeEngineProcessError::MissingPipe("stdout"))?;
        Ok(Self {
            child: Some(child),
            command_writer: Some(command_writer),
            event_reader: BufReader::new(event_reader),
        })
    }

    fn send(&mut self, command: ProtocolCommand) -> Result<(), NativeEngineProcessError> {
        let writer = self
            .command_writer
            .as_mut()
            .ok_or(NativeEngineProcessError::MissingPipe("stdin"))?;
        write_engine_message(writer, &Message::Command(command))
    }

    fn receive(&mut self) -> Result<Event, NativeEngineProcessError> {
        match read_engine_message(&mut self.event_reader)? {
            Some(Message::Event(event)) => Ok(event),
            Some(Message::Command(_)) => Err(NativeEngineProcessError::UnexpectedDirection),
            None => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "native engine event stream closed",
            )
            .into()),
        }
    }

    fn shutdown(mut self) -> Result<ExitStatus, NativeEngineProcessError> {
        self.send(ProtocolCommand::Shutdown)?;
        self.command_writer.take();
        let mut child = self
            .child
            .take()
            .ok_or(NativeEngineProcessError::MissingChildHandle)?;
        Ok(child.wait()?)
    }
}

impl Drop for NativeEngineProcess {
    fn drop(&mut self) {
        self.command_writer.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn run_native_engine_worker<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> Result<(), NativeEngineProcessError> {
    let mut views = HashSet::new();
    while let Some(message) = read_engine_message(reader)? {
        let Message::Command(command) = message else {
            return Err(NativeEngineProcessError::UnexpectedDirection);
        };
        match command {
            ProtocolCommand::CreateView { view, .. } => {
                if views.insert(view) {
                    write_event(writer, Event::ViewCreated { view })?;
                } else {
                    write_event(
                        writer,
                        Event::Crashed {
                            view,
                            reason: CrashReason::ProtocolViolation,
                        },
                    )?;
                }
            }
            ProtocolCommand::CloseView { view } => {
                if views.remove(&view) {
                    write_event(writer, Event::ViewClosed { view })?;
                } else {
                    write_event(
                        writer,
                        Event::Crashed {
                            view,
                            reason: CrashReason::ProtocolViolation,
                        },
                    )?;
                }
            }
            ProtocolCommand::Shutdown => {
                close_all_views(writer, &mut views)?;
                return Ok(());
            }
            other => {
                return Err(NativeEngineProcessError::UnsupportedCommand(command_name(
                    &other,
                )));
            }
        }
    }
    Ok(())
}

fn close_all_views<W: Write>(
    writer: &mut W,
    views: &mut HashSet<ViewId>,
) -> Result<(), NativeEngineProcessError> {
    let mut ordered: Vec<ViewId> = views.drain().collect();
    ordered.sort_unstable_by_key(|view| view.get());
    for view in ordered {
        write_event(writer, Event::ViewClosed { view })?;
    }
    Ok(())
}

fn command_name(command: &ProtocolCommand) -> &'static str {
    match command {
        ProtocolCommand::CreateView { .. } => "CreateView",
        ProtocolCommand::CloseView { .. } => "CloseView",
        ProtocolCommand::Navigate { .. } => "Navigate",
        ProtocolCommand::Reload { .. } => "Reload",
        ProtocolCommand::Stop { .. } => "Stop",
        ProtocolCommand::Resize { .. } => "Resize",
        ProtocolCommand::SetVisible { .. } => "SetVisible",
        ProtocolCommand::Input { .. } => "Input",
        ProtocolCommand::PermissionDecision { .. } => "PermissionDecision",
        ProtocolCommand::ReleaseFrame { .. } => "ReleaseFrame",
        ProtocolCommand::Shutdown => "Shutdown",
    }
}

fn write_event<W: Write>(writer: &mut W, event: Event) -> Result<(), NativeEngineProcessError> {
    write_engine_message(writer, &Message::Event(event))
}

fn write_engine_message<W: Write>(
    writer: &mut W,
    message: &Message,
) -> Result<(), NativeEngineProcessError> {
    let bytes = message.encode()?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

fn read_engine_message<R: Read>(
    reader: &mut R,
) -> Result<Option<Message>, NativeEngineProcessError> {
    let Some(header) = read_header(reader)? else {
        return Ok(None);
    };
    let body_len = u32::from_be_bytes([header[6], header[7], header[8], header[9]]) as usize;
    if body_len > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::MessageTooLarge {
            len: body_len,
            max: MAX_MESSAGE_BYTES,
        }
        .into());
    }

    let mut body = vec![0u8; body_len];
    read_exact_protocol(reader, &mut body)?;
    let mut envelope = Vec::with_capacity(ENVELOPE_HEADER_BYTES + body_len);
    envelope.extend_from_slice(&header);
    envelope.extend_from_slice(&body);
    Ok(Some(Message::decode(&envelope)?))
}

fn read_header<R: Read>(
    reader: &mut R,
) -> Result<Option<[u8; ENVELOPE_HEADER_BYTES]>, NativeEngineProcessError> {
    let mut header = [0u8; ENVELOPE_HEADER_BYTES];
    let mut offset = 0usize;
    while offset < header.len() {
        let read = reader.read(&mut header[offset..])?;
        if read == 0 {
            if offset == 0 {
                return Ok(None);
            }
            return Err(ProtocolError::Truncated {
                need: header.len(),
                have: offset,
            }
            .into());
        }
        offset += read;
    }
    Ok(Some(header))
}

fn read_exact_protocol<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
) -> Result<(), NativeEngineProcessError> {
    let mut offset = 0usize;
    while offset < buffer.len() {
        let read = reader.read(&mut buffer[offset..])?;
        if read == 0 {
            return Err(ProtocolError::Truncated {
                need: buffer.len(),
                have: offset,
            }
            .into());
        }
        offset += read;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_lifecycle_round_trip_uses_bounded_wire_messages() {
        let view = ViewId::new(7);
        let commands = [
            Message::Command(ProtocolCommand::CreateView {
                view,
                profile: ProfileId::new(3),
                viewport: Viewport {
                    width: 640,
                    height: 480,
                    scale_permille: 1000,
                },
            }),
            Message::Command(ProtocolCommand::CloseView { view }),
            Message::Command(ProtocolCommand::Shutdown),
        ];
        let mut input = Vec::new();
        for command in commands {
            input.extend(command.encode().unwrap());
        }
        let mut output = Vec::new();
        run_native_engine_worker(&mut input.as_slice(), &mut output).unwrap();

        let mut output = output.as_slice();
        assert_eq!(
            read_engine_message(&mut output).unwrap(),
            Some(Message::Event(Event::ViewCreated { view }))
        );
        assert_eq!(
            read_engine_message(&mut output).unwrap(),
            Some(Message::Event(Event::ViewClosed { view }))
        );
        assert_eq!(read_engine_message(&mut output).unwrap(), None);
    }

    #[test]
    fn truncated_stream_is_a_protocol_error() {
        let bytes = [0x53, 0x53, 1, 0, 0];
        let error = read_engine_message(&mut bytes.as_slice()).unwrap_err();
        assert!(matches!(
            error,
            NativeEngineProcessError::Protocol(ProtocolError::Truncated { .. })
        ));
    }
}
