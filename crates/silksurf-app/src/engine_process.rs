//! Supervised native-engine process boundary and worker runtime actor.
//!
//! The browser binary re-executes itself with `--silksurf-native-engine-worker`.
//! A command-reader thread owns child stdin and feeds a bounded actor queue. The
//! runtime actor owns the resident view map, every `BrowserPageRuntime`, and the
//! sole stdout event serializer. Navigation fetches run on worker threads; their
//! payloads return to the actor before JavaScript, DOM, layout, paint, or event
//! state changes. `Stop` and `Shutdown` remain readable during fetch. Page build
//! stays actor-owned and non-preemptible until the builder exposes checkpoints.
//!
//! The shell side owns child lifecycle and an `EventIngress` thread. Ingress
//! bounds both queued event count and exact encoded wire bytes, records terminal
//! failure before disconnecting, and leaves worker termination to the supervisor.
//!
//! Frame bytes stay outside this control plane. `FrameReady` carries descriptor
//! metadata; sealed memfd transfer uses a Unix-domain socket and `SCM_RIGHTS`
//! outside this pipe transport.

use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::process::{Child, Command as ProcessCommand, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::browser_types::{
    BrowserFrameBuffers, BrowserNavigationRequest, BrowserPage, BrowserRenderConfig,
    FRAME_HEIGHT, FRAME_WIDTH, ImageResourceCache, NavigationResult,
};
use crate::{build_browser_page_with_buffers_for_height, load_navigation_payload};
use silksurf_core::engine_protocol::{
    Command as ProtocolCommand, CrashReason, ENVELOPE_HEADER_BYTES, Event, LoadState,
    MAX_MESSAGE_BYTES, MAX_STRING_BYTES, Message, ProfileId, ProtocolError, ViewId, Viewport,
    envelope_body_len,
};

const NATIVE_ENGINE_WORKER_FLAG: &str = "--silksurf-native-engine-worker";
const NATIVE_ENGINE_PROBE_FLAG: &str = "--silksurf-native-engine-supervisor-probe";

/// Queued events the shell may fall behind by before the transport fails.
const EVENT_QUEUE_DEPTH: usize = 256;

/// One protocol-maximum envelope is the liveness floor for event delivery.
const EVENT_QUEUE_BYTE_BUDGET: usize = ENVELOPE_HEADER_BYTES + MAX_MESSAGE_BYTES;

/// Commands and navigation completions retained inside one native worker.
const WORKER_QUEUE_DEPTH: usize = 8;

/// The current loader has no mid-flight cancellation, so one worker admits one
/// fetch at a time. `Stop` invalidates its generation and the slot reopens when
/// the background fetch returns.
const MAX_INFLIGHT_NAVIGATIONS: usize = 1;

/// Grace period between `Shutdown` and `Child::kill`.
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(5);

/// Exit poll interval while a worker drains its command loop.
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Bound on a blocking shell receive.
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
                    "native engine worker command is not bound: {command}"
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

/// Runs an internal process mode before normal browser option parsing.
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
    let (sender, receiver) = mpsc::sync_channel(WORKER_QUEUE_DEPTH);
    let command_sender = sender.clone();
    let reader = thread::Builder::new()
        .name("silksurf-engine-command-reader".to_string())
        .spawn(move || {
            let stdin = io::stdin();
            let mut source = stdin.lock();
            read_commands_until_closed(&mut source, &command_sender);
        });
    if let Err(error) = reader {
        eprintln!("[SilkSurf] native engine command reader failed: {error}");
        return 2;
    }

    let stdout = io::stdout();
    let mut writer = stdout.lock();
    match run_runtime_actor(receiver, sender, &mut writer, production_navigation_loader()) {
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

struct QueuedEvent {
    event: Event,
    wire_bytes: usize,
}

/// Exact encoded bytes retained by queued events.
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

    fn release(&self, bytes: usize) {
        let _ = self
            .outstanding
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                Some(held.saturating_sub(bytes))
            });
    }
}

/// Owns the event pipe and hands decoded events to the shell through bounded
/// count and wire-byte budgets.
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

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "bound by the shell event pump with view routing")
    )]
    fn try_receive(&self) -> Result<Option<Event>, NativeEngineProcessError> {
        match self.events.try_recv() {
            Ok(queued) => Ok(Some(self.release(queued))),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(self.terminal_failure()),
        }
    }

    fn terminal_failure(&self) -> NativeEngineProcessError {
        match self.failure.lock() {
            Ok(mut slot) => slot
                .take()
                .unwrap_or(NativeEngineProcessError::EventStreamClosed),
            Err(_) => NativeEngineProcessError::EventStreamClosed,
        }
    }

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
                if charge.reserve(wire_bytes) {
                    match sender.try_send(QueuedEvent { event, wire_bytes }) {
                        Ok(()) => continue,
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
        record_terminal_failure(failure, outcome);
        return;
    }
}

fn record_terminal_failure(
    failure: &Arc<Mutex<Option<NativeEngineProcessError>>>,
    outcome: NativeEngineProcessError,
) {
    if let Ok(mut slot) = failure.lock() {
        *slot = Some(outcome);
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

type NavigationLoader = Arc<
    dyn Fn(
            BrowserNavigationRequest,
            BrowserRenderConfig,
            Arc<Mutex<ImageResourceCache>>,
        ) -> NavigationResult
        + Send
        + Sync,
>;

fn production_navigation_loader() -> NavigationLoader {
    Arc::new(|request, config, image_cache| {
        load_navigation_payload(&request, &config, &image_cache)
    })
}

enum WorkerMessage {
    Command(ProtocolCommand),
    CommandStreamClosed,
    CommandStreamFailed(NativeEngineProcessError),
    NavigationComplete {
        view: ViewId,
        generation: u64,
        result: Box<NavigationResult>,
    },
}

struct NativeEngineView {
    profile: ProfileId,
    render_config: BrowserRenderConfig,
    viewport: Viewport,
    requested_url: Option<String>,
    navigation_generation: u64,
    active_navigation: Option<u64>,
    page: Option<BrowserPage>,
    spare_buffers: BrowserFrameBuffers,
}

impl NativeEngineView {
    fn new(
        profile: ProfileId,
        render_config: BrowserRenderConfig,
        viewport: Viewport,
    ) -> Self {
        Self {
            profile,
            render_config,
            viewport,
            requested_url: None,
            navigation_generation: 0,
            active_navigation: None,
            page: None,
            spare_buffers: BrowserFrameBuffers::default(),
        }
    }

    fn reload_url(&self) -> Option<String> {
        self.page
            .as_ref()
            .map(|page| page.frame.url.clone())
            .or_else(|| self.requested_url.clone())
    }

    /// Drops the old runtime before constructing the replacement and reuses its
    /// large raster allocations. One view therefore never retains two page
    /// runtimes or two independent viewport-buffer sets during navigation.
    fn take_build_buffers(&mut self) -> BrowserFrameBuffers {
        if let Some(BrowserPage { frame, runtime }) = self.page.take() {
            return BrowserFrameBuffers {
                rgba: runtime.rgba,
                argb: frame.argb,
            };
        }
        std::mem::take(&mut self.spare_buffers)
    }
}

struct NativeEngineWorker {
    views: Vec<(ViewId, NativeEngineView)>,
    image_cache: Arc<Mutex<ImageResourceCache>>,
    sender: SyncSender<WorkerMessage>,
    loader: NavigationLoader,
    inflight_navigations: usize,
}

impl NativeEngineWorker {
    fn new(sender: SyncSender<WorkerMessage>, loader: NavigationLoader) -> Self {
        Self {
            views: Vec::with_capacity(1),
            image_cache: Arc::new(Mutex::new(ImageResourceCache::new())),
            sender,
            loader,
            inflight_navigations: 0,
        }
    }

    fn view(&self, id: ViewId) -> Option<&NativeEngineView> {
        self.views
            .iter()
            .find_map(|(view, entry)| (*view == id).then_some(entry))
    }

    fn view_mut(&mut self, id: ViewId) -> Option<&mut NativeEngineView> {
        self.views
            .iter_mut()
            .find_map(|(view, entry)| (*view == id).then_some(entry))
    }

    fn handle_command<W: Write>(
        &mut self,
        command: ProtocolCommand,
        writer: &mut W,
    ) -> Result<bool, NativeEngineProcessError> {
        match command {
            ProtocolCommand::CreateView {
                view,
                profile,
                viewport,
            } => self.create_view(view, profile, viewport, writer),
            ProtocolCommand::CloseView { view } => self.close_view(view, writer),
            ProtocolCommand::Navigate { view, request } => {
                self.start_navigation(view, request.url, writer)?;
                Ok(true)
            }
            ProtocolCommand::Reload { view } => {
                let Some(url) = self.view(view).and_then(NativeEngineView::reload_url) else {
                    return self.protocol_violation(view, writer);
                };
                self.start_navigation(view, url, writer)?;
                Ok(true)
            }
            ProtocolCommand::Stop { view } => self.stop_navigation(view, writer),
            command @ (ProtocolCommand::Resize { .. } | ProtocolCommand::SetVisible { .. }) => {
                Err(NativeEngineProcessError::UnsupportedCommand(command_name(&command)))
            }
            ProtocolCommand::Shutdown => {
                self.close_all_views(writer)?;
                Ok(false)
            }
            other => Err(NativeEngineProcessError::UnsupportedCommand(command_name(
                &other,
            ))),
        }
    }

    fn create_view<W: Write>(
        &mut self,
        view: ViewId,
        profile: ProfileId,
        viewport: Viewport,
        writer: &mut W,
    ) -> Result<bool, NativeEngineProcessError> {
        if self.view(view).is_some() {
            return self.protocol_violation(view, writer);
        }
        let render_config = self
            .views
            .iter()
            .find_map(|(_, entry)| {
                (entry.profile == profile).then(|| entry.render_config.clone())
            })
            .unwrap_or_default();
        self.views.push((
            view,
            NativeEngineView::new(profile, render_config, viewport),
        ));
        write_event(writer, Event::ViewCreated { view })?;
        Ok(true)
    }

    fn close_view<W: Write>(
        &mut self,
        view: ViewId,
        writer: &mut W,
    ) -> Result<bool, NativeEngineProcessError> {
        let Some(index) = self
            .views
            .iter()
            .position(|(candidate, _)| *candidate == view)
        else {
            return self.protocol_violation(view, writer);
        };
        drop(self.views.swap_remove(index));
        write_event(writer, Event::ViewClosed { view })?;
        Ok(true)
    }

    fn start_navigation<W: Write>(
        &mut self,
        view: ViewId,
        url: String,
        writer: &mut W,
    ) -> Result<(), NativeEngineProcessError> {
        if self.view(view).is_none() {
            self.protocol_violation(view, writer)?;
            return Ok(());
        }
        if self.inflight_navigations >= MAX_INFLIGHT_NAVIGATIONS {
            write_event(
                writer,
                Event::StatusChanged {
                    view,
                    status: "navigation worker busy".to_string(),
                },
            )?;
            return Ok(());
        }
        let Some(entry) = self.view_mut(view) else {
            return Ok(());
        };
        entry.navigation_generation = entry.navigation_generation.saturating_add(1);
        let generation = entry.navigation_generation;
        entry.active_navigation = Some(generation);
        entry.requested_url = Some(url.clone());
        let config = entry.render_config.clone();

        write_event(
            writer,
            Event::LoadStateChanged {
                view,
                state: LoadState::Started,
            },
        )?;

        let sender = self.sender.clone();
        let loader = Arc::clone(&self.loader);
        let image_cache = Arc::clone(&self.image_cache);
        let request = BrowserNavigationRequest::get(url);
        let fetch = thread::Builder::new()
            .name("silksurf-navigation-fetch".to_string())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    loader(request, config, image_cache)
                }))
                .unwrap_or_else(|_| Err("navigation fetch panicked".to_string()));
                let _ = sender.send(WorkerMessage::NavigationComplete {
                    view,
                    generation,
                    result: Box::new(result),
                });
            })?;
        self.inflight_navigations = self.inflight_navigations.saturating_add(1);
        drop(fetch);
        Ok(())
    }

    fn stop_navigation<W: Write>(
        &mut self,
        view: ViewId,
        writer: &mut W,
    ) -> Result<bool, NativeEngineProcessError> {
        let Some(entry) = self.view_mut(view) else {
            return self.protocol_violation(view, writer);
        };
        if entry.active_navigation.take().is_some() {
            write_event(
                writer,
                Event::LoadStateChanged {
                    view,
                    state: LoadState::Idle,
                },
            )?;
            write_event(
                writer,
                Event::StatusChanged {
                    view,
                    status: "stopped".to_string(),
                },
            )?;
        }
        Ok(true)
    }

    fn handle_navigation_complete<W: Write>(
        &mut self,
        view: ViewId,
        generation: u64,
        result: NavigationResult,
        writer: &mut W,
    ) -> Result<(), NativeEngineProcessError> {
        self.inflight_navigations = self.inflight_navigations.saturating_sub(1);
        let Some(entry) = self.view_mut(view) else {
            return Ok(());
        };
        if entry.active_navigation != Some(generation) {
            return Ok(());
        }
        entry.active_navigation = None;

        let payload = match result {
            Ok(payload) => payload,
            Err(error) => {
                write_navigation_failure(writer, view, error)?;
                return Ok(());
            }
        };
        let url = payload.url.clone();
        write_event(
            writer,
            Event::UrlChanged {
                view,
                url: url.clone(),
            },
        )?;
        write_event(
            writer,
            Event::LoadStateChanged {
                view,
                state: LoadState::Committed,
            },
        )?;

        let height = entry.viewport.height;
        let buffers = entry.take_build_buffers();
        match build_browser_page_with_buffers_for_height(payload, buffers, Some(height)) {
            Ok(page) => {
                entry.page = Some(page);
                write_event(
                    writer,
                    Event::LoadStateChanged {
                        view,
                        state: LoadState::Interactive,
                    },
                )?;
                write_event(
                    writer,
                    Event::LoadStateChanged {
                        view,
                        state: LoadState::Complete,
                    },
                )?;
            }
            Err(error) => {
                entry.spare_buffers = error.buffers;
                write_navigation_failure(writer, view, error.message)?;
            }
        }
        Ok(())
    }

    fn protocol_violation<W: Write>(
        &self,
        view: ViewId,
        writer: &mut W,
    ) -> Result<bool, NativeEngineProcessError> {
        write_event(
            writer,
            Event::Crashed {
                view,
                reason: CrashReason::ProtocolViolation,
            },
        )?;
        Ok(true)
    }

    fn close_all_views<W: Write>(
        &mut self,
        writer: &mut W,
    ) -> Result<(), NativeEngineProcessError> {
        self.views.sort_unstable_by_key(|(view, _)| view.get());
        for (view, _) in self.views.drain(..) {
            write_event(writer, Event::ViewClosed { view })?;
        }
        Ok(())
    }
}

fn write_navigation_failure<W: Write>(
    writer: &mut W,
    view: ViewId,
    error: String,
) -> Result<(), NativeEngineProcessError> {
    write_event(
        writer,
        Event::LoadStateChanged {
            view,
            state: LoadState::Failed,
        },
    )?;
    write_event(
        writer,
        Event::StatusChanged {
            view,
            status: bounded_protocol_string(error),
        },
    )
}

fn bounded_protocol_string(mut value: String) -> String {
    if value.len() <= MAX_STRING_BYTES {
        return value;
    }
    let mut boundary = MAX_STRING_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    value.truncate(boundary);
    value
}

fn read_commands_until_closed<R: Read>(source: &mut R, sender: &SyncSender<WorkerMessage>) {
    loop {
        let message = match read_engine_message(source) {
            Ok(Some(Message::Command(command))) => WorkerMessage::Command(command),
            Ok(Some(Message::Event(_))) => {
                WorkerMessage::CommandStreamFailed(NativeEngineProcessError::UnexpectedDirection)
            }
            Ok(None) => WorkerMessage::CommandStreamClosed,
            Err(error) => WorkerMessage::CommandStreamFailed(error),
        };
        let terminal = matches!(
            message,
            WorkerMessage::CommandStreamClosed | WorkerMessage::CommandStreamFailed(_)
        );
        if sender.send(message).is_err() || terminal {
            return;
        }
    }
}

fn run_runtime_actor<W: Write>(
    receiver: Receiver<WorkerMessage>,
    sender: SyncSender<WorkerMessage>,
    writer: &mut W,
    loader: NavigationLoader,
) -> Result<(), NativeEngineProcessError> {
    let mut worker = NativeEngineWorker::new(sender, loader);
    while let Ok(message) = receiver.recv() {
        match message {
            WorkerMessage::Command(command) => {
                if !worker.handle_command(command, writer)? {
                    return Ok(());
                }
            }
            WorkerMessage::CommandStreamClosed => return Ok(()),
            WorkerMessage::CommandStreamFailed(error) => return Err(error),
            WorkerMessage::NavigationComplete {
                view,
                generation,
                result,
            } => worker.handle_navigation_complete(view, generation, *result, writer)?,
        }
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

fn event_name(event: &Event) -> &'static str {
    match event {
        Event::ViewCreated { .. } => "ViewCreated",
        Event::ViewClosed { .. } => "ViewClosed",
        Event::LoadStateChanged { .. } => "LoadStateChanged",
        Event::UrlChanged { .. } => "UrlChanged",
        Event::TitleChanged { .. } => "TitleChanged",
        Event::CursorChanged { .. } => "CursorChanged",
        Event::StatusChanged { .. } => "StatusChanged",
        Event::ProgressChanged { .. } => "ProgressChanged",
        Event::PermissionRequested { .. } => "PermissionRequested",
        Event::DownloadRequested { .. } => "DownloadRequested",
        Event::FileChooserRequested { .. } => "FileChooserRequested",
        Event::NewViewRequested { .. } => "NewViewRequested",
        Event::FrameReady { .. } => "FrameReady",
        Event::Crashed { .. } => "Crashed",
        Event::Hang { .. } => "Hang",
        Event::CapabilityMismatch { .. } => "CapabilityMismatch",
        Event::Metrics { .. } => "Metrics",
    }
}

fn write_event<W: Write>(writer: &mut W, event: Event) -> Result<(), NativeEngineProcessError> {
    let name = event_name(&event);
    let bytes = Message::Event(event).encode()?;
    if std::env::var_os("SILKSURF_TRACE_ENGINE_EVENTS").is_some() {
        eprintln!(
            "[SilkSurf] engine event: type={name} wire_bytes={}",
            bytes.len()
        );
    }
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
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
#[path = "tests/native_engine_process.rs"]
mod tests;
