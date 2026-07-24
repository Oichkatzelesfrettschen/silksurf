//! Envelope framing and control-message serialization.
//!
//! A message is a ten-byte header (magic, wire version, kind, message type,
//! body length) followed by the encoded body. Decoding validates the header,
//! bounds the body length, decodes exactly the declared bytes, and rejects any
//! shortfall or surplus. Every command and event round-trips through here; a
//! malformed body is a typed error, never a panic.

use super::codec::{ByteReader, ByteWriter, MAX_DAMAGE_RECTS, MAX_MESSAGE_BYTES, ProtocolError};
use super::ids::{FrameGeneration, ProfileId, RequestId, ViewId};
use super::message::{
    Command, CrashReason, CursorKind, DamageRect, EngineMetrics, Event, FrameHandle,
    FrameTransport, ImeKind, InputEvent, KeyKind, LoadState, Modifiers, MouseButton,
    NavigationRequest, PermissionDecision, PermissionKind, PointerKind, Viewport,
};
use super::version::Capabilities;

/// Envelope format version. Distinct from the negotiated `ProtocolVersion`.
pub const WIRE_VERSION: u8 = 1;

const MAGIC: [u8; 2] = [0x53, 0x53];
const KIND_COMMAND: u8 = 0;
const KIND_EVENT: u8 = 1;

mod cmd {
    pub const CREATE_VIEW: u16 = 1;
    pub const CLOSE_VIEW: u16 = 2;
    pub const NAVIGATE: u16 = 3;
    pub const RELOAD: u16 = 4;
    pub const STOP: u16 = 5;
    pub const RESIZE: u16 = 6;
    pub const SET_VISIBLE: u16 = 7;
    pub const INPUT: u16 = 8;
    pub const PERMISSION_DECISION: u16 = 9;
    pub const RELEASE_FRAME: u16 = 10;
    pub const SHUTDOWN: u16 = 11;
}

mod evt {
    pub const VIEW_CREATED: u16 = 1;
    pub const VIEW_CLOSED: u16 = 2;
    pub const LOAD_STATE_CHANGED: u16 = 3;
    pub const URL_CHANGED: u16 = 4;
    pub const TITLE_CHANGED: u16 = 5;
    pub const CURSOR_CHANGED: u16 = 6;
    pub const STATUS_CHANGED: u16 = 7;
    pub const PROGRESS_CHANGED: u16 = 8;
    pub const PERMISSION_REQUESTED: u16 = 9;
    pub const DOWNLOAD_REQUESTED: u16 = 10;
    pub const FILE_CHOOSER_REQUESTED: u16 = 11;
    pub const NEW_VIEW_REQUESTED: u16 = 12;
    pub const FRAME_READY: u16 = 13;
    pub const CRASHED: u16 = 14;
    pub const HANG: u16 = 15;
    pub const CAPABILITY_MISMATCH: u16 = 16;
    pub const METRICS: u16 = 17;
}

/// A decoded control message: either a command or an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// A shell-to-engine command.
    Command(Command),
    /// An engine-to-shell event.
    Event(Event),
}

impl Message {
    /// Serializes this message to a framed envelope.
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        match self {
            Self::Command(command) => command.encode(),
            Self::Event(event) => event.encode(),
        }
    }

    /// Decodes one framed envelope, requiring it to consume the whole buffer.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let mut reader = ByteReader::new(bytes);
        if [reader.get_u8()?, reader.get_u8()?] != MAGIC {
            return Err(ProtocolError::BadMagic);
        }
        let wire = reader.get_u8()?;
        if wire != WIRE_VERSION {
            return Err(ProtocolError::UnsupportedWireVersion(wire));
        }
        let kind = reader.get_u8()?;
        let message_type = reader.get_u16()?;
        let body_len = reader.get_u32()? as usize;
        if body_len > MAX_MESSAGE_BYTES {
            return Err(ProtocolError::MessageTooLarge {
                len: body_len,
                max: MAX_MESSAGE_BYTES,
            });
        }
        let body = reader.get_slice(body_len)?;
        reader.expect_end()?;

        let mut body_reader = ByteReader::new(body);
        let message = match kind {
            KIND_COMMAND => Self::Command(decode_command(message_type, &mut body_reader)?),
            KIND_EVENT => Self::Event(decode_event(message_type, &mut body_reader)?),
            other => return Err(ProtocolError::UnknownKind(other)),
        };
        body_reader.expect_end()?;
        Ok(message)
    }
}

fn frame(kind: u8, message_type: u16, body: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let len = body.len();
    if len > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::MessageTooLarge {
            len,
            max: MAX_MESSAGE_BYTES,
        });
    }
    let body_len = u32::try_from(len).map_err(|_| ProtocolError::MessageTooLarge {
        len,
        max: MAX_MESSAGE_BYTES,
    })?;
    let mut out = ByteWriter::new();
    out.put_bytes(&MAGIC);
    out.put_u8(WIRE_VERSION);
    out.put_u8(kind);
    out.put_u16(message_type);
    out.put_u32(body_len);
    out.put_bytes(body);
    Ok(out.into_bytes())
}

// --- shared field helpers ---------------------------------------------------

fn put_bool(writer: &mut ByteWriter, value: bool) {
    writer.put_u8(u8::from(value));
}

fn get_bool(reader: &mut ByteReader, message_type: u16) -> Result<bool, ProtocolError> {
    match reader.get_u8()? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(ProtocolError::BadDiscriminant {
            message_type,
            value,
        }),
    }
}

fn get_enum<T>(
    reader: &mut ByteReader,
    message_type: u16,
    parse: impl Fn(u8) -> Option<T>,
) -> Result<T, ProtocolError> {
    let value = reader.get_u8()?;
    parse(value).ok_or(ProtocolError::BadDiscriminant {
        message_type,
        value,
    })
}

fn put_viewport(writer: &mut ByteWriter, viewport: &Viewport) {
    writer.put_u32(viewport.width);
    writer.put_u32(viewport.height);
    writer.put_u16(viewport.scale_permille);
}

fn get_viewport(reader: &mut ByteReader) -> Result<Viewport, ProtocolError> {
    Ok(Viewport {
        width: reader.get_u32()?,
        height: reader.get_u32()?,
        scale_permille: reader.get_u16()?,
    })
}

fn put_input(writer: &mut ByteWriter, event: &InputEvent) -> Result<(), ProtocolError> {
    match event {
        InputEvent::Pointer {
            kind,
            x,
            y,
            button,
            wheel_x,
            wheel_y,
        } => {
            writer.put_u8(0);
            writer.put_u8(kind.to_u8());
            writer.put_i32(*x);
            writer.put_i32(*y);
            writer.put_u8(button.to_u8());
            writer.put_i32(*wheel_x);
            writer.put_i32(*wheel_y);
        }
        InputEvent::Key {
            kind,
            key_code,
            modifiers,
        } => {
            writer.put_u8(1);
            writer.put_u8(kind.to_u8());
            writer.put_u32(*key_code);
            writer.put_u8(modifiers.bits());
        }
        InputEvent::Text { text } => {
            writer.put_u8(2);
            writer.put_str(text)?;
        }
        InputEvent::Ime {
            kind,
            text,
            cursor_begin,
            cursor_end,
        } => {
            writer.put_u8(3);
            writer.put_u8(kind.to_u8());
            writer.put_str(text)?;
            writer.put_u32(*cursor_begin);
            writer.put_u32(*cursor_end);
        }
        InputEvent::Focus { gained } => {
            writer.put_u8(4);
            put_bool(writer, *gained);
        }
    }
    Ok(())
}

fn read_pointer(reader: &mut ByteReader, message_type: u16) -> Result<InputEvent, ProtocolError> {
    Ok(InputEvent::Pointer {
        kind: get_enum(reader, message_type, PointerKind::from_u8)?,
        x: reader.get_i32()?,
        y: reader.get_i32()?,
        button: get_enum(reader, message_type, MouseButton::from_u8)?,
        wheel_x: reader.get_i32()?,
        wheel_y: reader.get_i32()?,
    })
}

fn read_ime(reader: &mut ByteReader, message_type: u16) -> Result<InputEvent, ProtocolError> {
    Ok(InputEvent::Ime {
        kind: get_enum(reader, message_type, ImeKind::from_u8)?,
        text: reader.get_str()?,
        cursor_begin: reader.get_u32()?,
        cursor_end: reader.get_u32()?,
    })
}

fn get_input(reader: &mut ByteReader, message_type: u16) -> Result<InputEvent, ProtocolError> {
    match reader.get_u8()? {
        0 => read_pointer(reader, message_type),
        1 => Ok(InputEvent::Key {
            kind: get_enum(reader, message_type, KeyKind::from_u8)?,
            key_code: reader.get_u32()?,
            modifiers: Modifiers::from_bits(reader.get_u8()?),
        }),
        2 => Ok(InputEvent::Text {
            text: reader.get_str()?,
        }),
        3 => read_ime(reader, message_type),
        4 => Ok(InputEvent::Focus {
            gained: get_bool(reader, message_type)?,
        }),
        value => Err(ProtocolError::BadDiscriminant {
            message_type,
            value,
        }),
    }
}

fn put_transport(writer: &mut ByteWriter, transport: &FrameTransport) {
    match transport {
        FrameTransport::SharedMemory { token, len } => {
            writer.put_u8(0);
            writer.put_u64(*token);
            writer.put_u64(*len);
        }
        FrameTransport::Platform { token, len } => {
            writer.put_u8(1);
            writer.put_u64(*token);
            writer.put_u64(*len);
        }
    }
}

fn get_transport(
    reader: &mut ByteReader,
    message_type: u16,
) -> Result<FrameTransport, ProtocolError> {
    match reader.get_u8()? {
        0 => Ok(FrameTransport::SharedMemory {
            token: reader.get_u64()?,
            len: reader.get_u64()?,
        }),
        1 => Ok(FrameTransport::Platform {
            token: reader.get_u64()?,
            len: reader.get_u64()?,
        }),
        value => Err(ProtocolError::BadDiscriminant {
            message_type,
            value,
        }),
    }
}

fn put_frame_handle(writer: &mut ByteWriter, handle: &FrameHandle) {
    writer.put_u64(handle.view.get());
    writer.put_u64(handle.generation.get());
    put_transport(writer, &handle.transport);
}

fn get_frame_handle(
    reader: &mut ByteReader,
    message_type: u16,
) -> Result<FrameHandle, ProtocolError> {
    Ok(FrameHandle {
        view: ViewId::new(reader.get_u64()?),
        generation: FrameGeneration::new(reader.get_u64()?),
        transport: get_transport(reader, message_type)?,
    })
}

// --- command codec ----------------------------------------------------------

impl Command {
    /// The wire discriminant for this command.
    pub fn message_type(&self) -> u16 {
        match self {
            Self::CreateView { .. } => cmd::CREATE_VIEW,
            Self::CloseView { .. } => cmd::CLOSE_VIEW,
            Self::Navigate { .. } => cmd::NAVIGATE,
            Self::Reload { .. } => cmd::RELOAD,
            Self::Stop { .. } => cmd::STOP,
            Self::Resize { .. } => cmd::RESIZE,
            Self::SetVisible { .. } => cmd::SET_VISIBLE,
            Self::Input { .. } => cmd::INPUT,
            Self::PermissionDecision { .. } => cmd::PERMISSION_DECISION,
            Self::ReleaseFrame { .. } => cmd::RELEASE_FRAME,
            Self::Shutdown => cmd::SHUTDOWN,
        }
    }

    /// Serializes this command to a framed envelope.
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut body = ByteWriter::new();
        self.encode_body(&mut body)?;
        frame(KIND_COMMAND, self.message_type(), &body.into_bytes())
    }

    fn encode_body(&self, writer: &mut ByteWriter) -> Result<(), ProtocolError> {
        match self {
            Self::CreateView {
                view,
                profile,
                viewport,
            } => {
                writer.put_u64(view.get());
                writer.put_u64(profile.get());
                put_viewport(writer, viewport);
            }
            Self::CloseView { view } | Self::Reload { view } | Self::Stop { view } => {
                writer.put_u64(view.get());
            }
            Self::Navigate { view, request } => {
                writer.put_u64(view.get());
                writer.put_str(&request.url)?;
            }
            Self::Resize { view, viewport } => {
                writer.put_u64(view.get());
                put_viewport(writer, viewport);
            }
            Self::SetVisible { view, visible } => {
                writer.put_u64(view.get());
                put_bool(writer, *visible);
            }
            Self::Input { view, event } => {
                writer.put_u64(view.get());
                put_input(writer, event)?;
            }
            Self::PermissionDecision { request, decision } => {
                writer.put_u64(request.get());
                writer.put_u8(decision.to_u8());
            }
            Self::ReleaseFrame { view, generation } => {
                writer.put_u64(view.get());
                writer.put_u64(generation.get());
            }
            Self::Shutdown => {}
        }
        Ok(())
    }
}

fn read_view(reader: &mut ByteReader) -> Result<ViewId, ProtocolError> {
    Ok(ViewId::new(reader.get_u64()?))
}

fn decode_command(message_type: u16, reader: &mut ByteReader) -> Result<Command, ProtocolError> {
    if message_type <= cmd::RESIZE {
        decode_command_lo(message_type, reader)
    } else {
        decode_command_hi(message_type, reader)
    }
}

fn decode_command_lo(message_type: u16, reader: &mut ByteReader) -> Result<Command, ProtocolError> {
    match message_type {
        cmd::CREATE_VIEW => Ok(Command::CreateView {
            view: read_view(reader)?,
            profile: ProfileId::new(reader.get_u64()?),
            viewport: get_viewport(reader)?,
        }),
        cmd::CLOSE_VIEW => Ok(Command::CloseView {
            view: read_view(reader)?,
        }),
        cmd::NAVIGATE => Ok(Command::Navigate {
            view: read_view(reader)?,
            request: NavigationRequest {
                url: reader.get_str()?,
            },
        }),
        cmd::RELOAD => Ok(Command::Reload {
            view: read_view(reader)?,
        }),
        cmd::STOP => Ok(Command::Stop {
            view: read_view(reader)?,
        }),
        cmd::RESIZE => Ok(Command::Resize {
            view: read_view(reader)?,
            viewport: get_viewport(reader)?,
        }),
        other => Err(ProtocolError::UnknownMessageType(other)),
    }
}

fn decode_command_hi(message_type: u16, reader: &mut ByteReader) -> Result<Command, ProtocolError> {
    match message_type {
        cmd::SET_VISIBLE => Ok(Command::SetVisible {
            view: read_view(reader)?,
            visible: get_bool(reader, message_type)?,
        }),
        cmd::INPUT => Ok(Command::Input {
            view: read_view(reader)?,
            event: get_input(reader, message_type)?,
        }),
        cmd::PERMISSION_DECISION => Ok(Command::PermissionDecision {
            request: RequestId::new(reader.get_u64()?),
            decision: get_enum(reader, message_type, PermissionDecision::from_u8)?,
        }),
        cmd::RELEASE_FRAME => Ok(Command::ReleaseFrame {
            view: read_view(reader)?,
            generation: FrameGeneration::new(reader.get_u64()?),
        }),
        cmd::SHUTDOWN => Ok(Command::Shutdown),
        other => Err(ProtocolError::UnknownMessageType(other)),
    }
}

// --- event codec ------------------------------------------------------------

impl Event {
    /// The wire discriminant for this event.
    pub fn message_type(&self) -> u16 {
        match self {
            Self::ViewCreated { .. } => evt::VIEW_CREATED,
            Self::ViewClosed { .. } => evt::VIEW_CLOSED,
            Self::LoadStateChanged { .. } => evt::LOAD_STATE_CHANGED,
            Self::UrlChanged { .. } => evt::URL_CHANGED,
            Self::TitleChanged { .. } => evt::TITLE_CHANGED,
            Self::CursorChanged { .. } => evt::CURSOR_CHANGED,
            Self::StatusChanged { .. } => evt::STATUS_CHANGED,
            Self::ProgressChanged { .. } => evt::PROGRESS_CHANGED,
            Self::PermissionRequested { .. } => evt::PERMISSION_REQUESTED,
            Self::DownloadRequested { .. } => evt::DOWNLOAD_REQUESTED,
            Self::FileChooserRequested { .. } => evt::FILE_CHOOSER_REQUESTED,
            Self::NewViewRequested { .. } => evt::NEW_VIEW_REQUESTED,
            Self::FrameReady { .. } => evt::FRAME_READY,
            Self::Crashed { .. } => evt::CRASHED,
            Self::Hang { .. } => evt::HANG,
            Self::CapabilityMismatch { .. } => evt::CAPABILITY_MISMATCH,
            Self::Metrics { .. } => evt::METRICS,
        }
    }

    /// Serializes this event to a framed envelope.
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut body = ByteWriter::new();
        self.encode_body(&mut body)?;
        frame(KIND_EVENT, self.message_type(), &body.into_bytes())
    }

    fn encode_body(&self, writer: &mut ByteWriter) -> Result<(), ProtocolError> {
        if self.message_type() <= evt::PROGRESS_CHANGED {
            self.encode_body_lo(writer)
        } else {
            self.encode_body_hi(writer)
        }
    }

    fn encode_body_lo(&self, writer: &mut ByteWriter) -> Result<(), ProtocolError> {
        match self {
            Self::ViewCreated { view } | Self::ViewClosed { view } => writer.put_u64(view.get()),
            Self::LoadStateChanged { view, state } => {
                writer.put_u64(view.get());
                writer.put_u8(state.to_u8());
            }
            Self::UrlChanged { view, url } => {
                writer.put_u64(view.get());
                writer.put_str(url)?;
            }
            Self::TitleChanged { view, title } => {
                writer.put_u64(view.get());
                writer.put_str(title)?;
            }
            Self::CursorChanged { view, cursor } => {
                writer.put_u64(view.get());
                writer.put_u8(cursor.to_u8());
            }
            Self::StatusChanged { view, status } => {
                writer.put_u64(view.get());
                writer.put_str(status)?;
            }
            Self::ProgressChanged { view, permille } => {
                writer.put_u64(view.get());
                writer.put_u16(*permille);
            }
            _ => {}
        }
        Ok(())
    }

    fn encode_body_hi(&self, writer: &mut ByteWriter) -> Result<(), ProtocolError> {
        match self {
            Self::PermissionRequested {
                request,
                view,
                kind,
                origin,
            } => {
                writer.put_u64(request.get());
                writer.put_u64(view.get());
                writer.put_u8(kind.to_u8());
                writer.put_str(origin)?;
            }
            Self::DownloadRequested {
                request,
                view,
                url,
                suggested_name,
            } => {
                writer.put_u64(request.get());
                writer.put_u64(view.get());
                writer.put_str(url)?;
                writer.put_str(suggested_name)?;
            }
            Self::FileChooserRequested {
                request,
                view,
                multiple,
            } => {
                writer.put_u64(request.get());
                writer.put_u64(view.get());
                put_bool(writer, *multiple);
            }
            Self::NewViewRequested {
                request,
                source_view,
                url,
            } => {
                writer.put_u64(request.get());
                writer.put_u64(source_view.get());
                writer.put_str(url)?;
            }
            Self::FrameReady { frame, damage } => {
                put_frame_handle(writer, frame);
                writer.put_len(damage.len(), MAX_DAMAGE_RECTS)?;
                for rect in damage {
                    writer.put_u32(rect.x);
                    writer.put_u32(rect.y);
                    writer.put_u32(rect.width);
                    writer.put_u32(rect.height);
                }
            }
            Self::Crashed { view, reason } => {
                writer.put_u64(view.get());
                writer.put_u8(reason.to_u8());
            }
            Self::Hang { view, elapsed_ms } => {
                writer.put_u64(view.get());
                writer.put_u64(*elapsed_ms);
            }
            Self::CapabilityMismatch { view, needed } => {
                writer.put_u64(view.get());
                writer.put_u64(needed.bits());
            }
            Self::Metrics { view, sample } => {
                writer.put_u64(view.get());
                writer.put_u64(sample.input_to_submit_us);
                writer.put_u64(sample.rss_bytes);
                writer.put_u32(sample.frame_copies);
            }
            _ => {}
        }
        Ok(())
    }
}

fn decode_event(message_type: u16, reader: &mut ByteReader) -> Result<Event, ProtocolError> {
    if message_type <= evt::PROGRESS_CHANGED {
        decode_event_lo(message_type, reader)
    } else if message_type <= evt::NEW_VIEW_REQUESTED {
        decode_event_mid(message_type, reader)
    } else {
        decode_event_hi(message_type, reader)
    }
}

fn decode_event_lo(message_type: u16, reader: &mut ByteReader) -> Result<Event, ProtocolError> {
    match message_type {
        evt::VIEW_CREATED => Ok(Event::ViewCreated {
            view: ViewId::new(reader.get_u64()?),
        }),
        evt::VIEW_CLOSED => Ok(Event::ViewClosed {
            view: ViewId::new(reader.get_u64()?),
        }),
        evt::LOAD_STATE_CHANGED => Ok(Event::LoadStateChanged {
            view: ViewId::new(reader.get_u64()?),
            state: get_enum(reader, message_type, LoadState::from_u8)?,
        }),
        evt::URL_CHANGED => Ok(Event::UrlChanged {
            view: ViewId::new(reader.get_u64()?),
            url: reader.get_str()?,
        }),
        evt::TITLE_CHANGED => Ok(Event::TitleChanged {
            view: ViewId::new(reader.get_u64()?),
            title: reader.get_str()?,
        }),
        evt::CURSOR_CHANGED => Ok(Event::CursorChanged {
            view: ViewId::new(reader.get_u64()?),
            cursor: get_enum(reader, message_type, CursorKind::from_u8)?,
        }),
        evt::STATUS_CHANGED => Ok(Event::StatusChanged {
            view: ViewId::new(reader.get_u64()?),
            status: reader.get_str()?,
        }),
        evt::PROGRESS_CHANGED => Ok(Event::ProgressChanged {
            view: ViewId::new(reader.get_u64()?),
            permille: reader.get_u16()?,
        }),
        other => Err(ProtocolError::UnknownMessageType(other)),
    }
}

fn decode_event_mid(message_type: u16, reader: &mut ByteReader) -> Result<Event, ProtocolError> {
    match message_type {
        evt::PERMISSION_REQUESTED => Ok(Event::PermissionRequested {
            request: RequestId::new(reader.get_u64()?),
            view: read_view(reader)?,
            kind: get_enum(reader, message_type, PermissionKind::from_u8)?,
            origin: reader.get_str()?,
        }),
        evt::DOWNLOAD_REQUESTED => Ok(Event::DownloadRequested {
            request: RequestId::new(reader.get_u64()?),
            view: read_view(reader)?,
            url: reader.get_str()?,
            suggested_name: reader.get_str()?,
        }),
        evt::FILE_CHOOSER_REQUESTED => Ok(Event::FileChooserRequested {
            request: RequestId::new(reader.get_u64()?),
            view: read_view(reader)?,
            multiple: get_bool(reader, message_type)?,
        }),
        evt::NEW_VIEW_REQUESTED => Ok(Event::NewViewRequested {
            request: RequestId::new(reader.get_u64()?),
            source_view: read_view(reader)?,
            url: reader.get_str()?,
        }),
        other => Err(ProtocolError::UnknownMessageType(other)),
    }
}

fn decode_event_hi(message_type: u16, reader: &mut ByteReader) -> Result<Event, ProtocolError> {
    match message_type {
        evt::FRAME_READY => decode_frame_ready(message_type, reader),
        evt::CRASHED => Ok(Event::Crashed {
            view: read_view(reader)?,
            reason: get_enum(reader, message_type, CrashReason::from_u8)?,
        }),
        evt::HANG => Ok(Event::Hang {
            view: read_view(reader)?,
            elapsed_ms: reader.get_u64()?,
        }),
        evt::CAPABILITY_MISMATCH => Ok(Event::CapabilityMismatch {
            view: read_view(reader)?,
            needed: Capabilities::from_bits(reader.get_u64()?),
        }),
        evt::METRICS => Ok(Event::Metrics {
            view: read_view(reader)?,
            sample: EngineMetrics {
                input_to_submit_us: reader.get_u64()?,
                rss_bytes: reader.get_u64()?,
                frame_copies: reader.get_u32()?,
            },
        }),
        other => Err(ProtocolError::UnknownMessageType(other)),
    }
}

fn decode_frame_ready(message_type: u16, reader: &mut ByteReader) -> Result<Event, ProtocolError> {
    let frame = get_frame_handle(reader, message_type)?;
    let count = reader.get_len(MAX_DAMAGE_RECTS)?;
    let mut damage = Vec::with_capacity(count);
    for _ in 0..count {
        damage.push(DamageRect {
            x: reader.get_u32()?,
            y: reader.get_u32()?,
            width: reader.get_u32()?,
            height: reader.get_u32()?,
        });
    }
    Ok(Event::FrameReady { frame, damage })
}
