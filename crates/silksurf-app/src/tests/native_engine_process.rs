use super::*;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use crate::browser_types::BrowserPagePayload;
use silksurf_core::engine_protocol::{
    DamageRect, FrameGeneration, FrameHandle, FrameTransport, MAX_DAMAGE_RECTS,
    NavigationRequest, MAX_STRING_BYTES,
};

const TEST_DEADLINE: Duration = Duration::from_secs(10);

fn encoded(event: Event) -> Vec<u8> {
    Message::Event(event).encode().expect("event encodes")
}

fn command(command: ProtocolCommand) -> WorkerMessage {
    WorkerMessage::Command(command)
}

fn fixture_payload(
    request: BrowserNavigationRequest,
    config: BrowserRenderConfig,
) -> NavigationResult {
    Ok(BrowserPagePayload {
        url: request.url,
        html: "<!doctype html><html><body><p>worker page</p></body></html>".to_string(),
        css_text: "html, body { display: block; } body { margin: 0; }".to_string(),
        script_texts: Vec::new(),
        module_texts: Vec::new(),
        images: Vec::new(),
        render_config: config,
        parsed_document: None,
    })
}

fn fixture_loader() -> NavigationLoader {
    Arc::new(|request, config, _image_cache| fixture_payload(request, config))
}

fn actor_transport(
    loader: NavigationLoader,
) -> (
    SyncSender<WorkerMessage>,
    EventIngress,
    JoinHandle<Result<(), NativeEngineProcessError>>,
) {
    let (sender, receiver) = mpsc::sync_channel(WORKER_QUEUE_DEPTH);
    let actor_sender = sender.clone();
    let (event_reader, mut event_writer) = io::pipe().expect("event pipe");
    let actor = thread::spawn(move || {
        run_runtime_actor(receiver, actor_sender, &mut event_writer, loader)
    });
    (sender, EventIngress::spawn(event_reader), actor)
}

fn receive(ingress: &EventIngress) -> Event {
    ingress.receive_timeout(TEST_DEADLINE).expect("event")
}

fn create_view(view: ViewId) -> ProtocolCommand {
    ProtocolCommand::CreateView {
        view,
        profile: ProfileId::new(1),
        viewport: Viewport {
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
            scale_permille: 1000,
        },
    }
}

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

fn maximal_title_changed(view: ViewId) -> Event {
    Event::TitleChanged {
        view,
        title: "t".repeat(MAX_STRING_BYTES),
    }
}

#[test]
fn runtime_actor_builds_and_owns_a_navigated_page() {
    let view = ViewId::new(7);
    let (sender, mut ingress, actor) = actor_transport(fixture_loader());

    sender.send(command(create_view(view))).expect("create command");
    assert_eq!(receive(&ingress), Event::ViewCreated { view });

    sender
        .send(command(ProtocolCommand::Navigate {
            view,
            request: NavigationRequest {
                url: "about:blank#fixture".to_string(),
            },
        }))
        .expect("navigate command");

    assert_eq!(
        receive(&ingress),
        Event::LoadStateChanged {
            view,
            state: LoadState::Started,
        }
    );
    assert_eq!(
        receive(&ingress),
        Event::UrlChanged {
            view,
            url: "about:blank#fixture".to_string(),
        }
    );
    assert_eq!(
        receive(&ingress),
        Event::LoadStateChanged {
            view,
            state: LoadState::Committed,
        }
    );
    assert_eq!(
        receive(&ingress),
        Event::LoadStateChanged {
            view,
            state: LoadState::Interactive,
        }
    );
    assert_eq!(
        receive(&ingress),
        Event::LoadStateChanged {
            view,
            state: LoadState::Complete,
        }
    );

    sender
        .send(command(ProtocolCommand::Shutdown))
        .expect("shutdown command");
    assert_eq!(receive(&ingress), Event::ViewClosed { view });
    actor
        .join()
        .expect("actor thread")
        .expect("actor completes");
    ingress.join();
}

#[test]
fn navigation_fetch_panic_releases_the_single_worker_slot() {
    let view = ViewId::new(8);
    let attempts = Arc::new(AtomicUsize::new(0));
    let loader_attempts = Arc::clone(&attempts);
    let loader: NavigationLoader = Arc::new(move |request, config, _image_cache| {
        if loader_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("synthetic navigation fetch panic");
        }
        fixture_payload(request, config)
    });
    let (sender, mut ingress, actor) = actor_transport(loader);

    sender.send(command(create_view(view))).expect("create command");
    assert_eq!(receive(&ingress), Event::ViewCreated { view });
    sender
        .send(command(ProtocolCommand::Navigate {
            view,
            request: NavigationRequest {
                url: "about:blank#panic".to_string(),
            },
        }))
        .expect("first navigate command");
    assert_eq!(
        receive(&ingress),
        Event::LoadStateChanged {
            view,
            state: LoadState::Started,
        }
    );
    assert_eq!(
        receive(&ingress),
        Event::LoadStateChanged {
            view,
            state: LoadState::Failed,
        }
    );
    assert_eq!(
        receive(&ingress),
        Event::StatusChanged {
            view,
            status: "navigation fetch panicked".to_string(),
        }
    );

    sender
        .send(command(ProtocolCommand::Navigate {
            view,
            request: NavigationRequest {
                url: "about:blank#recovered".to_string(),
            },
        }))
        .expect("second navigate command");
    assert_eq!(
        receive(&ingress),
        Event::LoadStateChanged {
            view,
            state: LoadState::Started,
        },
        "the failed fetch releases the single navigation slot"
    );
    for expected in [
        Event::UrlChanged {
            view,
            url: "about:blank#recovered".to_string(),
        },
        Event::LoadStateChanged {
            view,
            state: LoadState::Committed,
        },
        Event::LoadStateChanged {
            view,
            state: LoadState::Interactive,
        },
        Event::LoadStateChanged {
            view,
            state: LoadState::Complete,
        },
    ] {
        assert_eq!(receive(&ingress), expected);
    }

    sender
        .send(command(ProtocolCommand::Shutdown))
        .expect("shutdown command");
    assert_eq!(receive(&ingress), Event::ViewClosed { view });
    actor
        .join()
        .expect("actor thread")
        .expect("actor completes");
    ingress.join();
}

#[test]
fn stop_discards_a_stale_navigation_completion() {
    let view = ViewId::new(8);
    let barrier = Arc::new(Barrier::new(2));
    let loader_barrier = Arc::clone(&barrier);
    let loader: NavigationLoader = Arc::new(move |request, config, _image_cache| {
        loader_barrier.wait();
        fixture_payload(request, config)
    });
    let (sender, mut ingress, actor) = actor_transport(loader);

    sender.send(command(create_view(view))).expect("create command");
    assert_eq!(receive(&ingress), Event::ViewCreated { view });
    sender
        .send(command(ProtocolCommand::Navigate {
            view,
            request: NavigationRequest {
                url: "about:blank#stale".to_string(),
            },
        }))
        .expect("navigate command");
    assert_eq!(
        receive(&ingress),
        Event::LoadStateChanged {
            view,
            state: LoadState::Started,
        }
    );

    sender
        .send(command(ProtocolCommand::Stop { view }))
        .expect("stop command");
    assert_eq!(
        receive(&ingress),
        Event::LoadStateChanged {
            view,
            state: LoadState::Idle,
        }
    );
    assert_eq!(
        receive(&ingress),
        Event::StatusChanged {
            view,
            status: "stopped".to_string(),
        }
    );

    sender
        .send(command(ProtocolCommand::Navigate {
            view,
            request: NavigationRequest {
                url: "about:blank#too-soon".to_string(),
            },
        }))
        .expect("second navigate command");
    assert_eq!(
        receive(&ingress),
        Event::StatusChanged {
            view,
            status: "navigation worker busy".to_string(),
        },
        "a stopped but still running fetch holds the single worker slot"
    );

    barrier.wait();
    sender
        .send(command(ProtocolCommand::Shutdown))
        .expect("shutdown command");
    assert_eq!(
        receive(&ingress),
        Event::ViewClosed { view },
        "stale completion must not commit after Stop"
    );
    actor
        .join()
        .expect("actor thread")
        .expect("actor completes");
    ingress.join();
}

#[test]
fn replacement_navigation_reuses_the_previous_viewport_buffers() {
    let request = BrowserNavigationRequest::get("about:blank#buffers".to_string());
    let payload = fixture_payload(request, BrowserRenderConfig::default())
        .expect("fixture payload");
    let mut page = build_browser_page_with_buffers_for_height(
        payload,
        BrowserFrameBuffers::default(),
        None,
    )
    .expect("fixture page builds");
    page.frame.argb.reserve(4096);
    page.runtime.rgba.reserve(4096);
    let argb_capacity = page.frame.argb.capacity();
    let rgba_capacity = page.runtime.rgba.capacity();

    let mut view = NativeEngineView::new(
        ProfileId::new(1),
        Viewport {
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
            scale_permille: 1000,
        },
    );
    view.page = Some(page);
    let buffers = view.take_build_buffers();

    assert!(view.page.is_none());
    assert!(buffers.argb.capacity() >= argb_capacity);
    assert!(buffers.rgba.capacity() >= rgba_capacity);
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

#[test]
fn one_maximal_event_fits_the_queue_byte_budget() {
    let view = ViewId::new(11);
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
    let view = ViewId::new(12);
    let bytes = encoded(maximal_title_changed(view));
    let capacity = EVENT_QUEUE_BYTE_BUDGET / bytes.len();
    assert!(capacity < EVENT_QUEUE_DEPTH);

    let (reader, mut writer) = io::pipe().expect("pipe");
    let ingress = EventIngress::spawn(reader);
    let engine_side = thread::spawn(move || {
        for _ in 0..(2 * capacity) {
            if writer.write_all(&bytes).is_err() {
                break;
            }
        }
    });
    thread::sleep(Duration::from_millis(200));

    let mut delivered = 0usize;
    let terminal = loop {
        match ingress.receive_timeout(TEST_DEADLINE) {
            Ok(_) => delivered += 1,
            Err(error) => break error,
        }
    };
    engine_side.join().expect("engine writer thread");
    assert!(delivered < EVENT_QUEUE_DEPTH);
    assert!(matches!(
        terminal,
        NativeEngineProcessError::EventQueueByteOverflow
    ));
}

#[test]
fn dequeue_releases_the_exact_wire_charge() {
    let view = ViewId::new(13);
    let charge = QueueCharge::default();
    assert!(charge.reserve(EVENT_QUEUE_BYTE_BUDGET));
    assert!(!charge.reserve(1));
    charge.release(EVENT_QUEUE_BYTE_BUDGET);
    assert!(charge.reserve(EVENT_QUEUE_BYTE_BUDGET));

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
            "event {index} of {rounds} must arrive"
        );
    }
    engine_side.join().expect("engine writer thread");
}

#[test]
fn ingress_delivers_unsolicited_events_in_wire_order() {
    let view = ViewId::new(14);
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
    let view = ViewId::new(15);
    let large = encoded(maximal_frame_ready(view));
    assert!(large.len() > 64 * 1024);

    let (event_reader, mut event_writer) = io::pipe().expect("event pipe");
    let (command_reader, command_writer) = io::pipe().expect("command pipe");
    let mut engine = NativeEngineProcess::adopt(None, command_writer, event_reader);

    let follow = encoded(Event::ViewCreated { view });
    let engine_side = thread::spawn(move || {
        event_writer.write_all(&large).expect("write large event");
        event_writer.write_all(&follow).expect("write follow-up");
    });

    engine
        .send(ProtocolCommand::CloseView { view })
        .expect("command remains writable");
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
    let view = ViewId::new(16);
    let (reader, mut writer) = io::pipe().expect("pipe");
    let ingress = EventIngress::spawn(reader);

    for permille in 0..(EVENT_QUEUE_DEPTH as u16 * 2) {
        if writer
            .write_all(&encoded(Event::ProgressChanged { view, permille }))
            .is_err()
        {
            break;
        }
    }
    drop(writer);
    thread::sleep(Duration::from_millis(200));

    let terminal = loop {
        match ingress.receive_timeout(TEST_DEADLINE) {
            Ok(_) => {}
            Err(error) => break error,
        }
    };
    assert!(matches!(
        terminal,
        NativeEngineProcessError::EventQueueOverflow
    ));
}

#[test]
fn closing_the_event_writer_closes_the_stream_once() {
    let (reader, writer) = io::pipe().expect("pipe");
    let mut ingress = EventIngress::spawn(reader);
    drop(writer);

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

    assert!(!status.success());
    assert!(elapsed < Duration::from_secs(5));
}
