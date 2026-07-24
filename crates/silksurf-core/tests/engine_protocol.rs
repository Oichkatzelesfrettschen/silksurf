//! Engine protocol v1 conformance: round-trip, malformed-message rejection,
//! version negotiation, and lifecycle transitions.
//!
//! These are the exit-criterion tests for issue #52: the control plane
//! serializes and decodes losslessly, a malformed message decodes to a typed
//! error and never panics, versions negotiate by the documented policy, and
//! lifecycle transitions follow the tables in AD-027.

use silksurf_core::engine_protocol::{
    Capabilities, Command, CrashReason, CursorKind, DamageRect, Endpoint, EngineMetrics,
    EngineState, Event, FrameGeneration, FrameHandle, FrameState, FrameTransport, ImeKind,
    InputEvent, KeyKind, LoadState, Message, Modifiers, MouseButton, NavigationRequest,
    PermissionDecision, PermissionKind, PointerKind, ProfileId, ProtocolError, ProtocolVersion,
    RequestId, VersionError, VersionRange, ViewId, ViewState, WIRE_VERSION, negotiate,
    negotiate_version,
};

const MAGIC0: u8 = 0x53;

fn sample_commands() -> Vec<Command> {
    let view = ViewId::new(7);
    let viewport = silksurf_core::engine_protocol::Viewport {
        width: 1280,
        height: 800,
        scale_permille: 1500,
    };
    vec![
        Command::CreateView {
            view,
            profile: ProfileId::new(3),
            viewport,
        },
        Command::CloseView { view },
        Command::Navigate {
            view,
            request: NavigationRequest {
                url: "https://example.com/path?q=1".to_owned(),
            },
        },
        Command::Reload { view },
        Command::Stop { view },
        Command::Resize { view, viewport },
        Command::SetVisible {
            view,
            visible: false,
        },
        Command::Input {
            view,
            event: InputEvent::Pointer {
                kind: PointerKind::Wheel,
                x: -12,
                y: 340,
                button: MouseButton::None,
                wheel_x: 0,
                wheel_y: -3,
            },
        },
        Command::Input {
            view,
            event: InputEvent::Key {
                kind: KeyKind::Down,
                key_code: 65,
                modifiers: Modifiers::CTRL.with(Modifiers::SHIFT),
            },
        },
        Command::Input {
            view,
            event: InputEvent::Text {
                text: "hello".to_owned(),
            },
        },
        Command::Input {
            view,
            event: InputEvent::Ime {
                kind: ImeKind::Preedit,
                text: "wo".to_owned(),
                cursor_begin: 0,
                cursor_end: 2,
            },
        },
        Command::Input {
            view,
            event: InputEvent::Focus { gained: true },
        },
        Command::PermissionDecision {
            request: RequestId::new(42),
            decision: PermissionDecision::Deny,
        },
        Command::ReleaseFrame {
            view,
            generation: FrameGeneration::new(9),
        },
        Command::Shutdown,
    ]
}

fn sample_events() -> Vec<Event> {
    let view = ViewId::new(11);
    vec![
        Event::ViewCreated { view },
        Event::ViewClosed { view },
        Event::LoadStateChanged {
            view,
            state: LoadState::Interactive,
        },
        Event::UrlChanged {
            view,
            url: "https://a.test/".to_owned(),
        },
        Event::TitleChanged {
            view,
            title: "Title".to_owned(),
        },
        Event::CursorChanged {
            view,
            cursor: CursorKind::Grabbing,
        },
        Event::StatusChanged {
            view,
            status: "loading".to_owned(),
        },
        Event::ProgressChanged {
            view,
            permille: 640,
        },
        Event::PermissionRequested {
            request: RequestId::new(1),
            view,
            kind: PermissionKind::Camera,
            origin: "https://a.test".to_owned(),
        },
        Event::DownloadRequested {
            request: RequestId::new(2),
            view,
            url: "https://a.test/file.zip".to_owned(),
            suggested_name: "file.zip".to_owned(),
        },
        Event::FileChooserRequested {
            request: RequestId::new(3),
            view,
            multiple: true,
        },
        Event::NewViewRequested {
            request: RequestId::new(4),
            source_view: view,
            url: "https://b.test/".to_owned(),
        },
        Event::FrameReady {
            frame: FrameHandle {
                view,
                generation: FrameGeneration::FIRST,
                transport: FrameTransport::SharedMemory {
                    token: 0xDEAD_BEEF,
                    len: 4_096_000,
                },
            },
            damage: vec![
                DamageRect {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10,
                },
                DamageRect {
                    x: 5,
                    y: 5,
                    width: 20,
                    height: 20,
                },
            ],
        },
        Event::FrameReady {
            frame: FrameHandle {
                view,
                generation: FrameGeneration::FIRST.next(),
                transport: FrameTransport::Platform {
                    token: 1,
                    len: 8192,
                },
            },
            damage: Vec::new(),
        },
        Event::Crashed {
            view,
            reason: CrashReason::ProtocolViolation,
        },
        Event::Hang {
            view,
            elapsed_ms: 2500,
        },
        Event::CapabilityMismatch {
            view,
            needed: Capabilities::DMABUF_FRAMES.with(Capabilities::IME),
        },
        Event::Metrics {
            view,
            sample: EngineMetrics {
                input_to_submit_us: 117,
                rss_bytes: 25_000_000,
                frame_copies: 1,
            },
        },
    ]
}

#[test]
fn commands_round_trip() {
    for command in sample_commands() {
        let message = Message::Command(command.clone());
        let bytes = message.encode().expect("encode command");
        let decoded = Message::decode(&bytes).expect("decode command");
        assert_eq!(decoded, message, "round-trip mismatch for {command:?}");
    }
}

#[test]
fn events_round_trip() {
    for event in sample_events() {
        let message = Message::Event(event.clone());
        let bytes = message.encode().expect("encode event");
        let decoded = Message::decode(&bytes).expect("decode event");
        assert_eq!(decoded, message, "round-trip mismatch for {event:?}");
    }
}

#[test]
fn every_message_type_is_covered() {
    // 11 commands + 17 events are specified; the samples exercise each.
    let command_types: std::collections::BTreeSet<u16> = sample_commands()
        .iter()
        .map(Command::message_type)
        .collect();
    assert_eq!(command_types.len(), 11);
    let event_types: std::collections::BTreeSet<u16> =
        sample_events().iter().map(Event::message_type).collect();
    assert_eq!(event_types.len(), 17);
}

fn a_valid_message() -> Vec<u8> {
    Message::Command(Command::Navigate {
        view: ViewId::new(1),
        request: NavigationRequest {
            url: "https://example.com/".to_owned(),
        },
    })
    .encode()
    .expect("encode")
}

#[test]
fn empty_buffer_is_truncated() {
    assert!(matches!(
        Message::decode(&[]),
        Err(ProtocolError::Truncated { .. })
    ));
}

#[test]
fn bad_magic_is_rejected() {
    let mut bytes = a_valid_message();
    bytes[0] = 0x00;
    assert_eq!(Message::decode(&bytes), Err(ProtocolError::BadMagic));
}

#[test]
fn unsupported_wire_version_is_rejected() {
    let mut bytes = a_valid_message();
    bytes[2] = WIRE_VERSION.wrapping_add(9);
    assert_eq!(
        Message::decode(&bytes),
        Err(ProtocolError::UnsupportedWireVersion(
            WIRE_VERSION.wrapping_add(9)
        ))
    );
}

#[test]
fn unknown_kind_is_rejected() {
    let mut bytes = a_valid_message();
    bytes[3] = 9;
    assert_eq!(Message::decode(&bytes), Err(ProtocolError::UnknownKind(9)));
}

#[test]
fn unknown_command_type_is_rejected() {
    // kind = command (0), message_type = 0xFFFF, empty body.
    let bytes = vec![MAGIC0, MAGIC0, WIRE_VERSION, 0, 0xFF, 0xFF, 0, 0, 0, 0];
    assert_eq!(
        Message::decode(&bytes),
        Err(ProtocolError::UnknownMessageType(0xFFFF))
    );
}

#[test]
fn unknown_event_type_is_rejected() {
    // kind = event (1), message_type = 0x0100, empty body.
    let bytes = vec![MAGIC0, MAGIC0, WIRE_VERSION, 1, 0x01, 0x00, 0, 0, 0, 0];
    assert_eq!(
        Message::decode(&bytes),
        Err(ProtocolError::UnknownMessageType(0x0100))
    );
}

#[test]
fn oversized_body_length_is_rejected_before_allocation() {
    // Declared body_len = 0xFFFF_FFFF but no body present.
    let bytes = vec![
        MAGIC0,
        MAGIC0,
        WIRE_VERSION,
        0,
        0,
        3,
        0xFF,
        0xFF,
        0xFF,
        0xFF,
    ];
    assert!(matches!(
        Message::decode(&bytes),
        Err(ProtocolError::MessageTooLarge { .. })
    ));
}

#[test]
fn truncated_body_is_rejected() {
    let mut bytes = a_valid_message();
    bytes.pop();
    assert!(matches!(
        Message::decode(&bytes),
        Err(ProtocolError::Truncated { .. })
    ));
}

#[test]
fn trailing_bytes_are_rejected() {
    let mut bytes = a_valid_message();
    bytes.push(0x00);
    assert!(matches!(
        Message::decode(&bytes),
        Err(ProtocolError::TrailingBytes(1))
    ));
}

#[test]
fn bad_enum_discriminant_is_rejected() {
    // LoadStateChanged with an out-of-range state byte.
    let event = Event::LoadStateChanged {
        view: ViewId::new(1),
        state: LoadState::Idle,
    };
    let mut bytes = Message::Event(event).encode().expect("encode");
    let last = bytes.len() - 1;
    bytes[last] = 0x7F;
    assert!(matches!(
        Message::decode(&bytes),
        Err(ProtocolError::BadDiscriminant { .. })
    ));
}

#[test]
fn oversized_string_prefix_inside_body_is_rejected() {
    // Navigate body: view (8 bytes) + string len prefix set past the limit.
    let mut body = Vec::new();
    body.extend_from_slice(&1u64.to_be_bytes());
    body.extend_from_slice(&0x00FF_FFFFu32.to_be_bytes()); // 16 MiB claimed
    let mut bytes = vec![MAGIC0, MAGIC0, WIRE_VERSION, 0, 0, 3];
    let body_len = u32::try_from(body.len()).unwrap();
    bytes.extend_from_slice(&body_len.to_be_bytes());
    bytes.extend_from_slice(&body);
    assert!(matches!(
        Message::decode(&bytes),
        Err(ProtocolError::LimitExceeded { .. })
    ));
}

#[test]
fn decode_never_panics_on_truncation() {
    // Every prefix of every valid message must decode to Ok or Err, never
    // panic. This is the property the process boundary depends on.
    let mut corpus: Vec<Vec<u8>> = sample_commands()
        .into_iter()
        .map(|c| Message::Command(c).encode().expect("encode"))
        .collect();
    corpus.extend(
        sample_events()
            .into_iter()
            .map(|e| Message::Event(e).encode().expect("encode")),
    );
    for message in &corpus {
        for len in 0..=message.len() {
            let _ = Message::decode(&message[..len]);
        }
    }
}

#[test]
fn decode_never_panics_on_byte_flips() {
    let corpus = a_valid_message();
    for index in 0..corpus.len() {
        for bit in 0..8u32 {
            let mut mutated = corpus.clone();
            mutated[index] ^= 1u8 << bit;
            let _ = Message::decode(&mutated);
        }
    }
}

#[test]
fn version_negotiates_highest_common_minor() {
    let local = VersionRange::new(ProtocolVersion::new(1, 0), ProtocolVersion::new(1, 4));
    let remote = VersionRange::new(ProtocolVersion::new(1, 2), ProtocolVersion::new(1, 9));
    assert_eq!(
        negotiate_version(local, remote),
        Ok(ProtocolVersion::new(1, 4))
    );
}

#[test]
fn version_identical_ranges_agree() {
    let range = VersionRange::current();
    assert_eq!(
        negotiate_version(range, range),
        Ok(ProtocolVersion::CURRENT)
    );
}

#[test]
fn version_major_mismatch_is_rejected() {
    let local = VersionRange::new(ProtocolVersion::new(1, 0), ProtocolVersion::new(1, 3));
    let remote = VersionRange::new(ProtocolVersion::new(2, 0), ProtocolVersion::new(2, 1));
    assert_eq!(
        negotiate_version(local, remote),
        Err(VersionError::MajorMismatch {
            local: 1,
            remote: 2
        })
    );
}

#[test]
fn version_disjoint_minors_have_no_common() {
    let local = VersionRange::new(ProtocolVersion::new(1, 0), ProtocolVersion::new(1, 2));
    let remote = VersionRange::new(ProtocolVersion::new(1, 5), ProtocolVersion::new(1, 9));
    assert_eq!(
        negotiate_version(local, remote),
        Err(VersionError::NoCommonVersion { major: 1 })
    );
}

#[test]
fn negotiate_intersects_capabilities() {
    let local = Endpoint {
        versions: VersionRange::current(),
        capabilities: Capabilities::DOWNLOADS
            .with(Capabilities::IME)
            .with(Capabilities::DMABUF_FRAMES),
    };
    let remote = Endpoint {
        versions: VersionRange::current(),
        capabilities: Capabilities::IME.with(Capabilities::WEBSOCKET),
    };
    let agreed = negotiate(local, remote).expect("negotiate");
    assert_eq!(agreed.version, ProtocolVersion::CURRENT);
    assert_eq!(agreed.capabilities, Capabilities::IME);
    assert!(!agreed.capabilities.contains(Capabilities::DOWNLOADS));
}

#[test]
fn engine_transitions_follow_the_table() {
    assert_eq!(
        EngineState::Starting.transition(EngineState::Ready),
        Ok(EngineState::Ready)
    );
    assert_eq!(
        EngineState::Ready.transition(EngineState::Draining),
        Ok(EngineState::Draining)
    );
    assert!(EngineState::Exited.transition(EngineState::Ready).is_err());
    let error = EngineState::Starting
        .transition(EngineState::Exited)
        .expect_err("skipping Ready is illegal");
    assert_eq!(error.from, "Starting");
    assert_eq!(error.to, "Exited");
}

#[test]
fn view_allows_renavigation_and_recovers_from_failure() {
    assert_eq!(
        ViewState::Interactive.transition(ViewState::Loading),
        Ok(ViewState::Loading)
    );
    assert_eq!(
        ViewState::Failed.transition(ViewState::Closing),
        Ok(ViewState::Closing)
    );
    assert!(ViewState::Closed.transition(ViewState::Loading).is_err());
}

#[test]
fn frame_releases_only_after_present_or_discard() {
    assert_eq!(
        FrameState::Produced.transition(FrameState::Transferred),
        Ok(FrameState::Transferred)
    );
    assert_eq!(
        FrameState::Presented.transition(FrameState::Released),
        Ok(FrameState::Released)
    );
    assert_eq!(
        FrameState::Discarded.transition(FrameState::Released),
        Ok(FrameState::Released)
    );
    assert!(
        FrameState::Produced
            .transition(FrameState::Released)
            .is_err()
    );
}

#[test]
fn can_transition_agrees_with_transition() {
    let states = [
        EngineState::Starting,
        EngineState::Ready,
        EngineState::Draining,
        EngineState::Exited,
        EngineState::Failed,
    ];
    for &from in &states {
        for &to in &states {
            assert_eq!(from.can_transition(to), from.transition(to).is_ok());
        }
    }
}
