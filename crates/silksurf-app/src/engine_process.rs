//! Shell side of the engine protocol v1 control plane over a child process.
//!
//! The browser binary is both shell and engine: `NativeEngineProcess::spawn`
//! re-execs `std::env::current_exe()` with `--silksurf-native-engine-worker`,
//! and `run_internal_engine_process_mode` claims that flag before
//! `parse_app_options` sees it. Framed protocol-v1 envelopes carry commands
//! down the child's stdin and events back up its stdout;
//! `silksurf_core::engine_protocol` owns the envelope layout and every bound.
//!
//! An `EventIngress` thread owns the event pipe, so the engine may write
//! whenever it has something to say. Both directions writing at once would
//! otherwise wedge on a full pipe buffer: `MAX_DAMAGE_RECTS` rectangles alone
//! encode to 65_536 bytes against a 64 KiB Linux pipe. Insertion into the
//! queue is nonblocking, because a reader thread that blocks on a full
//! channel moves that same deadlock from the pipe into the queue; an overflow
//! ends the transport and the supervisor kills the worker. The queue bounds
//! both the number of queued events and the wire bytes they retain, because
//! `Event` owns strings and rectangle vectors that a count bound leaves
//! unbounded.
//!
//! The worker still writes stdout from its command loop. A second producer
//! arrives with `BrowserPageRuntime`, and that slice adds the event-writer
//! thread that serializes them.
//!
//! The frame plane stays out. `Event::FrameReady` describes a `FrameHandle`
//! whose transport is a shared-memory descriptor, and descriptor passing
//! needs `recvmsg` with a control-message buffer rather than these pipes.

use std::collections::HashSet;
use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::process::{Child, Command as ProcessCommand, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::browser_types::{FRAME_HEIGHT, FRAME_WIDTH};
use silksurf_core::engine_protocol::{
    Command as ProtocolCommand, CrashReason, ENVELOPE_HEADER_BYTES, Event, MAX_MESSAGE_BYTES,
    Message, ProfileId, ProtocolError, ViewId, Viewport, envelope_body_len,
};

const NATIVE_ENGINE_WORKER_FLAG: &str = "--silksurf-native-engine-worker";
const NATIVE_ENGINE_PROBE_FLAG: &str = "--silksurf-native-engine-supervisor-probe";

/// Queued events the shell may fall behind by before the transport fails.
const EVENT_QUEUE_DEPTH: usize = 256;

/// Owned wire bytes the event queue retains before the transport fails.
///
/// A count bound alone does not bound the working set: `Event` carries owned
/// strings and damage-rectangle vectors, so `EVENT_QUEUE_DEPTH` maximal
/// `FrameReady` events retain megabytes while the queue reports 256 entries.
/// One maximum legal envelope fits, which keeps every decodable message
/// deliverable, and a backlog of several does not.
const EVENT_QUEUE_BYTE_BUDGET: usize = ENVELOPE_HEADER_BYTES + MAX_MESSAGE_BYTES;

/// Grace period between `Shutdown` and `Child::kill`.
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(5);

/// Exit poll interval while a worker drains its command loop.
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Bound on a blocking `receive`, so a silent worker surfaces as a timeout.
const RECEIVE_DEADLINE: Duration = Duration::from_secs(10);

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
    EventStreamClosed,
    EventQueueOverflow,
    EventQueueByteOverflow,
    ReceiveTimeout(Duration),
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
                write!(
                    formatter,
                    "received an unexpected event; expected {expected}"
                )
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
            Self::EventStreamClosed => formatter.write_str("native engine event stream closed"),
            Self::EventQueueOverflow => write!(
                formatter,
                "shell fell more than {EVENT_QUEUE_DEPTH} events behind the native engine"
            ),
            Self::EventQueueByteOverflow => write!(
                formatter,
                "queued native engine events exceed {EVENT_QUEUE_BYTE_BUDGET} wire bytes"
            ),
            Self::ReceiveTimeout(deadline) => {
                write!(formatter, "no native engine event within {deadline:?}")
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
    expect_view_created(&engine.receive()?, view)?;

    engine.send(ProtocolCommand::CloseView { view })?;
    expect_view_closed(&engine.receive()?, view)?;

    let status = engine.shutdown()?;
    if !status.success() {
        return Err(NativeEngineProcessError::ChildFailed(status.code()));
    }
    Ok(())
}

fn expect_view_created(event: &Event, expected: ViewId) -> Result<(), NativeEngineProcessError> {
    match event {
        Event::ViewCreated { view } if *view == expected => Ok(()),
        _ => Err(NativeEngineProcessError::UnexpectedEvent("ViewCreated")),
    }
}

fn expect_view_closed(event: &Event, expected: ViewId) -> Result<(), NativeEngineProcessError> {
    match event {
        Event::ViewClosed { view } if *view == expected => Ok(()),
        _ => Err(NativeEngineProcessError::UnexpectedEvent("ViewClosed")),
    }
}

/// Owns the event pipe on a dedicated thread and hands decoded events to the
/// shell through a bounded queue.
///
/// The thread terminates on end of stream, on a decode failure, on a command
/// arriving in the event direction, and on queue overflow. It records the
/// cause before dropping its sender, so a receiver that observes the closed
/// channel always finds the reason already published.
struct QueuedEvent {
    event: Event,
    wire_bytes: usize,
}

/// Outstanding wire bytes held by queued events.
///
/// The reader reserves before it inserts and the receiver releases on removal,
/// so the counter tracks what the queue retains rather than what crossed the
/// pipe. Reservation is a compare-exchange loop, which keeps the budget exact
/// against a receiver draining concurrently on another thread.
#[derive(Default)]
struct QueueCharge {
    outstanding: AtomicUsize,
}

impl QueueCharge {
    fn reserve(&self, bytes: usize) -> bool {
        let mut held = self.outstanding.load(Ordering::Acquire);
        loop {
            let Some(next) = held
                .checked_add(bytes)
                .filter(|sum| *sum <= EVENT_QUEUE_BYTE_BUDGET)
            else {
                return false;
            };
            match self.outstanding.compare_exchange_weak(
                held,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(observed) => held = observed,
            }
        }
    }

    /// Saturates rather than wrapping, so a double release costs one event's
    /// budget instead of rejecting every later event as a byte overflow.
    fn release(&self, bytes: usize) {
        let _ = self
            .outstanding
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                Some(held.saturating_sub(bytes))
            });
    }
}

struct EventIngress {
    events: Receiver<QueuedEvent>,
    charge: Arc<QueueCharge>,
    failure: Arc<Mutex<Option<NativeEngineProcessError>>>,
    reader: Option<JoinHandle<()>>,
}

impl EventIngress {
    fn spawn<R: Read + Send + 'static>(source: R) -> Self {
        let (sender, events) = mpsc::sync_channel(EVENT_QUEUE_DEPTH);
        let failure = Arc::new(Mutex::new(None));
        let charge = Arc::new(QueueCharge::default());
        let reader_failure = Arc::clone(&failure);
        let reader_charge = Arc::clone(&charge);
        let reader = thread::spawn(move || {
            read_events_until_closed(source, &sender, &reader_charge, &reader_failure);
        });
        Self {
            events,
            charge,
            failure,
            reader: Some(reader),
        }
    }

    /// Releases the dequeued event's reservation before handing it to the shell.
    fn release(&self, queued: QueuedEvent) -> Event {
        self.charge.release(queued.wire_bytes);
        queued.event
    }

    fn receive_timeout(&self, deadline: Duration) -> Result<Event, NativeEngineProcessError> {
        match self.events.recv_timeout(deadline) {
            Ok(queued) => Ok(self.release(queued)),
            Err(RecvTimeoutError::Timeout) => {
                Err(NativeEngineProcessError::ReceiveTimeout(deadline))
            }
            Err(RecvTimeoutError::Disconnected) => Err(self.terminal_failure()),
        }
    }

    /// Drains one queued event without blocking. The shell event pump binds
    /// this when view routing lands; the transport contract is proved here.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "bound by the shell pump with view routing")
    )]
    fn try_receive(&self) -> Result<Option<Event>, NativeEngineProcessError> {
        match self.events.try_recv() {
            Ok(queued) => Ok(Some(self.release(queued))),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(self.terminal_failure()),
        }
    }

    /// Reports why the stream ended. The queue drains before the channel
    /// reports disconnection, so every event delivered precedes this.
    fn terminal_failure(&self) -> NativeEngineProcessError {
        match self.failure.lock() {
            Ok(mut slot) => slot
                .take()
                .unwrap_or(NativeEngineProcessError::EventStreamClosed),
            Err(_) => NativeEngineProcessError::EventStreamClosed,
        }
    }

    /// Joins the reader thread. The caller reaps the child first, because the
    /// thread runs until the write end of the event pipe closes.
    fn join(&mut self) {
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_events_until_closed<R: Read>(
    source: R,
    sender: &SyncSender<QueuedEvent>,
    charge: &Arc<QueueCharge>,
    failure: &Arc<Mutex<Option<NativeEngineProcessError>>>,
) {
    let mut source = BufReader::new(source);
    loop {
        let outcome = match read_engine_envelope(&mut source) {
            Ok(Some((Message::Event(event), wire_bytes))) => {
                // The charge is reserved before insertion and released by the
                // path that drops the event, so a rejected send leaves the
                // budget as it found it.
                if charge.reserve(wire_bytes) {
                    match sender.try_send(QueuedEvent { event, wire_bytes }) {
                        Ok(()) => continue,
                        // Blocking here would move the pipe deadlock into the queue.
                        Err(TrySendError::Full(queued)) => {
                            charge.release(queued.wire_bytes);
                            NativeEngineProcessError::EventQueueOverflow
                        }
                        Err(TrySendError::Disconnected(queued)) => {
                            charge.release(queued.wire_bytes);
                            return;
                        }
                    }
                } else {
                    NativeEngineProcessError::EventQueueByteOverflow
                }
            }
            Ok(Some((Message::Command(_), _))) => NativeEngineProcessError::UnexpectedDirection,
            Ok(None) => return,
            Err(error) => error,
        };
        if let Ok(mut slot) = failure.lock() {
            *slot = Some(outcome);
        }
        return;
    }
}

struct NativeEngineProcess {
    child: Option<Child>,
    command_writer: Option<Box<dyn Write + Send>>,
    ingress: EventIngress,
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
        let event_source = child
            .stdout
            .take()
            .ok_or(NativeEngineProcessError::MissingPipe("stdout"))?;
        Ok(Self::adopt(Some(child), command_writer, event_source))
    }

    /// Supervises an already-created transport. `spawn` builds the browser
    /// worker through this, and a test drives it with pipes.
    fn adopt<W, R>(child: Option<Child>, command_writer: W, event_source: R) -> Self
    where
        W: Write + Send + 'static,
        R: Read + Send + 'static,
    {
        Self {
            child,
            command_writer: Some(Box::new(command_writer)),
            ingress: EventIngress::spawn(event_source),
        }
    }

    fn send(&mut self, command: ProtocolCommand) -> Result<(), NativeEngineProcessError> {
        let writer = self
            .command_writer
            .as_mut()
            .ok_or(NativeEngineProcessError::MissingPipe("stdin"))?;
        write_engine_message(writer, &Message::Command(command))
    }

    fn receive(&mut self) -> Result<Event, NativeEngineProcessError> {
        self.ingress.receive_timeout(RECEIVE_DEADLINE)
    }

    fn shutdown(self) -> Result<ExitStatus, NativeEngineProcessError> {
        self.shutdown_within(SHUTDOWN_DEADLINE)
    }

    /// Requests shutdown, closes the command stream, and reaps the worker,
    /// killing it once `deadline` passes so a wedged engine cannot hold the
    /// shell.
    fn shutdown_within(
        mut self,
        deadline: Duration,
    ) -> Result<ExitStatus, NativeEngineProcessError> {
        let requested = self.send(ProtocolCommand::Shutdown);
        self.command_writer.take();
        let mut child = self
            .child
            .take()
            .ok_or(NativeEngineProcessError::MissingChildHandle)?;
        let status = reap_within(&mut child, deadline);
        self.ingress.join();
        requested?;
        status
    }
}

/// Waits for `child` to exit, escalating to `Child::kill` at the deadline.
/// Every path reaps, so a killed worker leaves no zombie.
fn reap_within(
    child: &mut Child,
    deadline: Duration,
) -> Result<ExitStatus, NativeEngineProcessError> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() >= deadline {
            let _ = child.kill();
            return Ok(child.wait()?);
        }
        thread::sleep(SHUTDOWN_POLL_INTERVAL);
    }
}

impl Drop for NativeEngineProcess {
    fn drop(&mut self) {
        self.command_writer.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.ingress.join();
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
    Ok(read_engine_envelope(reader)?.map(|(message, _)| message))
}

/// Reads one envelope and reports the exact wire size that carried it.
///
/// `envelope_body_len` bounds the body before allocation, so the returned size
/// never exceeds `ENVELOPE_HEADER_BYTES + MAX_MESSAGE_BYTES`. A queue charges
/// against this measured size rather than against `size_of` a decoded `Event`,
/// whose owned strings and rectangle vectors live outside the enum.
fn read_engine_envelope<R: Read>(
    reader: &mut R,
) -> Result<Option<(Message, usize)>, NativeEngineProcessError> {
    let Some(header) = read_header(reader)? else {
        return Ok(None);
    };
    let body_len = envelope_body_len(&header)?;
    let mut body = vec![0u8; body_len];
    read_exact_protocol(reader, &mut body)?;
    let wire_bytes = ENVELOPE_HEADER_BYTES + body_len;
    let mut envelope = Vec::with_capacity(wire_bytes);
    envelope.extend_from_slice(&header);
    envelope.extend_from_slice(&body);
    Ok(Some((Message::decode(&envelope)?, wire_bytes)))
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

    use silksurf_core::engine_protocol::{
        DamageRect, FrameGeneration, FrameHandle, FrameTransport, MAX_DAMAGE_RECTS,
        MAX_STRING_BYTES,
    };

    const TEST_DEADLINE: Duration = Duration::from_secs(10);

    /// A `FrameReady` at the damage-rect cap: 4096 rectangles of four `u32`
    /// each exceed a 64 KiB pipe buffer on its own.
    fn maximal_frame_ready(view: ViewId) -> Event {
        Event::FrameReady {
            frame: FrameHandle {
                view,
                generation: FrameGeneration::FIRST,
                transport: FrameTransport::SharedMemory {
                    token: 0xFEED,
                    len: 4096,
                },
            },
            damage: (0..MAX_DAMAGE_RECTS as u32)
                .map(|index| DamageRect {
                    x: index,
                    y: index,
                    width: 1,
                    height: 1,
                })
                .collect(),
        }
    }

    fn encoded(event: Event) -> Vec<u8> {
        Message::Event(event).encode().expect("event encodes")
    }

    /// A `TitleChanged` at the string cap: legal on its own, and sixteen of
    /// them exceed the queue's wire budget while the count queue holds 256.
    fn maximal_title_changed(view: ViewId) -> Event {
        Event::TitleChanged {
            view,
            title: "t".repeat(MAX_STRING_BYTES),
        }
    }

    #[test]
    fn one_maximal_event_fits_the_queue_byte_budget() {
        let view = ViewId::new(7);
        let (reader, mut writer) = io::pipe().expect("pipe");
        let ingress = EventIngress::spawn(reader);

        let bytes = encoded(maximal_frame_ready(view));
        assert!(bytes.len() <= EVENT_QUEUE_BYTE_BUDGET);
        let engine_side = thread::spawn(move || {
            writer.write_all(&bytes).expect("write");
        });

        assert!(matches!(
            ingress.receive_timeout(TEST_DEADLINE).expect("event"),
            Event::FrameReady { .. }
        ));
        engine_side.join().expect("engine writer thread");
    }

    #[test]
    fn cumulative_wire_bytes_overflow_while_the_count_queue_has_room() {
        let view = ViewId::new(8);
        let bytes = encoded(maximal_title_changed(view));
        let capacity = EVENT_QUEUE_BYTE_BUDGET / bytes.len();
        assert!(
            capacity < EVENT_QUEUE_DEPTH,
            "the byte budget must bind before the count budget, got {capacity}"
        );

        let (reader, mut writer) = io::pipe().expect("pipe");
        let ingress = EventIngress::spawn(reader);
        // Writing stops at EPIPE once the reader fails closed and drops its end.
        let engine_side = thread::spawn(move || {
            for _ in 0..(2 * capacity) {
                if writer.write_all(&bytes).is_err() {
                    break;
                }
            }
        });
        // The shell drains nothing yet, so no release restores budget mid-fill
        // and the backlog reaches the byte bound rather than racing it.
        thread::sleep(Duration::from_millis(200));

        let mut delivered = 0usize;
        let terminal = loop {
            match ingress.receive_timeout(TEST_DEADLINE) {
                Ok(_) => delivered += 1,
                Err(error) => break error,
            }
        };
        engine_side.join().expect("engine writer thread");
        assert!(
            delivered < EVENT_QUEUE_DEPTH,
            "the count queue still had room at {delivered} of {EVENT_QUEUE_DEPTH}"
        );
        assert!(
            matches!(terminal, NativeEngineProcessError::EventQueueByteOverflow),
            "byte overflow must stay distinct from count overflow and EOF: {terminal:?}"
        );
    }

    #[test]
    fn dequeue_releases_the_exact_wire_charge() {
        let view = ViewId::new(9);
        let charge = QueueCharge::default();
        assert!(charge.reserve(EVENT_QUEUE_BYTE_BUDGET));
        assert!(!charge.reserve(1), "a full budget admits nothing further");
        charge.release(EVENT_QUEUE_BYTE_BUDGET);
        assert!(charge.reserve(EVENT_QUEUE_BYTE_BUDGET));

        // The same accounting drives the queue: draining every event restores
        // the whole budget, so a slow shell that catches up keeps the transport.
        let bytes = encoded(maximal_title_changed(view));
        let rounds = 3 * (EVENT_QUEUE_BYTE_BUDGET / bytes.len());
        let (reader, mut writer) = io::pipe().expect("pipe");
        let ingress = EventIngress::spawn(reader);
        let engine_side = thread::spawn(move || {
            for _ in 0..rounds {
                writer.write_all(&bytes).expect("write");
            }
        });

        for index in 0..rounds {
            assert!(
                matches!(
                    ingress.receive_timeout(TEST_DEADLINE),
                    Ok(Event::TitleChanged { .. })
                ),
                "event {index} of {rounds} must arrive once the charge is released"
            );
        }
        engine_side.join().expect("engine writer thread");
    }

    #[test]
    fn ingress_delivers_unsolicited_events_in_wire_order() {
        let view = ViewId::new(4);
        let (reader, mut writer) = io::pipe().expect("pipe");
        let ingress = EventIngress::spawn(reader);

        let sent: Vec<Event> = (1..=8)
            .map(|permille| Event::ProgressChanged { view, permille })
            .collect();
        for event in &sent {
            writer.write_all(&encoded(event.clone())).expect("write");
        }
        drop(writer);

        for expected in sent {
            assert_eq!(
                ingress.receive_timeout(TEST_DEADLINE).expect("event"),
                expected
            );
        }
    }

    #[test]
    fn ingress_drains_an_event_larger_than_the_pipe_buffer() {
        let view = ViewId::new(5);
        let large = encoded(maximal_frame_ready(view));
        assert!(
            large.len() > 64 * 1024,
            "fixture must exceed a pipe buffer, got {}",
            large.len()
        );

        let (event_reader, mut event_writer) = io::pipe().expect("event pipe");
        let (command_reader, command_writer) = io::pipe().expect("command pipe");
        let mut engine = NativeEngineProcess::adopt(None, command_writer, event_reader);

        // The engine writes both events before the shell reads either, which
        // the half-duplex transport could not survive.
        let follow = encoded(Event::ViewCreated { view });
        let engine_side = thread::spawn(move || {
            event_writer.write_all(&large).expect("write large event");
            event_writer.write_all(&follow).expect("write follow-up");
        });

        engine
            .send(ProtocolCommand::CloseView { view })
            .expect("command still writable while events are in flight");
        assert!(matches!(
            engine.receive().expect("large event"),
            Event::FrameReady { .. }
        ));
        assert_eq!(
            engine.receive().expect("follow-up event"),
            Event::ViewCreated { view }
        );
        engine_side.join().expect("engine writer thread");
        drop(command_reader);
    }

    #[test]
    fn malformed_envelope_ends_the_stream_with_a_typed_error() {
        let (reader, mut writer) = io::pipe().expect("pipe");
        let ingress = EventIngress::spawn(reader);
        writer
            .write_all(&[0xFF; ENVELOPE_HEADER_BYTES])
            .expect("write");
        drop(writer);

        assert!(matches!(
            ingress.receive_timeout(TEST_DEADLINE).unwrap_err(),
            NativeEngineProcessError::Protocol(ProtocolError::BadMagic)
        ));
    }

    #[test]
    fn queue_overflow_reports_overflow_rather_than_a_clean_close() {
        let view = ViewId::new(6);
        let (reader, mut writer) = io::pipe().expect("pipe");
        let ingress = EventIngress::spawn(reader);

        // Writing stops at EPIPE, which is the fail-closed path itself: the
        // reader terminates on overflow and drops the read end.
        for permille in 0..(EVENT_QUEUE_DEPTH as u16 * 2) {
            if writer
                .write_all(&encoded(Event::ProgressChanged { view, permille }))
                .is_err()
            {
                break;
            }
        }
        drop(writer);
        // Let the reader fill the queue before the shell drains any of it.
        thread::sleep(Duration::from_millis(200));

        let mut delivered = 0usize;
        let terminal = loop {
            match ingress.receive_timeout(TEST_DEADLINE) {
                Ok(_) => delivered += 1,
                Err(error) => break error,
            }
        };
        assert!(delivered <= EVENT_QUEUE_DEPTH, "delivered {delivered}");
        assert!(
            matches!(terminal, NativeEngineProcessError::EventQueueOverflow),
            "expected overflow, got {terminal}"
        );
    }

    #[test]
    fn closing_the_event_writer_closes_the_stream_once() {
        let (reader, writer) = io::pipe().expect("pipe");
        let mut ingress = EventIngress::spawn(reader);
        drop(writer);

        // Blocking first, so the reader thread has finished; then the
        // nonblocking path must report the same close rather than Empty.
        assert!(matches!(
            ingress.receive_timeout(TEST_DEADLINE).unwrap_err(),
            NativeEngineProcessError::EventStreamClosed
        ));
        assert!(matches!(
            ingress.try_receive().unwrap_err(),
            NativeEngineProcessError::EventStreamClosed
        ));
        ingress.join();
    }

    #[test]
    fn shutdown_kills_a_worker_that_ignores_it() {
        let mut child = ProcessCommand::new("sleep")
            .arg("300")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("sleep must spawn");
        let command_writer = child.stdin.take().expect("stdin");
        let event_source = child.stdout.take().expect("stdout");
        let engine = NativeEngineProcess::adopt(Some(child), command_writer, event_source);

        let start = Instant::now();
        let status = engine
            .shutdown_within(Duration::from_millis(200))
            .expect("supervisor reaps an unresponsive worker");
        let elapsed = start.elapsed();

        assert!(!status.success(), "killed worker reports {status:?}");
        assert!(
            elapsed < Duration::from_secs(5),
            "shutdown took {elapsed:?}"
        );
    }
}
