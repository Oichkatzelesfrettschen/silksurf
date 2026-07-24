# Engine Protocol v1

The process-neutral, view-oriented message boundary between the SilkSurf
shell and a page engine. The shell owns chrome, tabs, profiles, input
routing, and frame composition. An engine owns document loading, script
execution, layout, and paint. This document specifies the messages, IDs,
lifecycle state machines, wire framing, error taxonomy, version negotiation,
and limits that cross the boundary.

The types live in `silksurf_core::engine_protocol`. The decision to introduce
the boundary is recorded in `ARCHITECTURE-DECISIONS.md` AD-027.

## Boundary rule

The boundary carries browser-view operations, never engine internals. It does
not expose `Dom`, `NodeId`, Boa values, CSS structures, Taffy nodes, or
display-list entries. A backend that is not the native SilkSurf engine (WPE,
Wry, Servo, CEF) satisfies the same message contract without sharing any of
those types. This is what lets the backend verdict stay open (issue #50,
DG-1..DG-3).

## Two planes

The protocol separates a control plane from a frame plane, because they have
different transport lifetimes.

- Control plane: commands and events. Small, structured, fully serialized and
  decoded by this crate. A malformed control message is rejected with a typed
  `ProtocolError` and never panics the receiver.
- Frame plane: `FrameHandle`. A rendered frame is large and its transport is
  platform-coupled (sealed shared memory first, DMA-BUF later; issue #53).
  This crate models the handle as an abstract descriptor carrying a view id,
  a monotonic generation, and an opaque transport token plus a byte length.
  The concrete transport representation (shared-memory name, file descriptor)
  is bound at the native-runtime extraction spike (#53) and is deliberately
  not fixed here.

## Identifiers

All identifiers are opaque `u64` newtypes. They are meaningful only to the
allocator that mints them; the peer treats them as tokens.

- `EngineInstanceId` -- one running engine process.
- `ViewId` -- one browsing view (future tab) inside an engine.
- `ProfileId` -- one persistent profile (cookies, storage, history root).
- `RequestId` -- one outstanding request/response pair (permission, download,
  file chooser, new view).
- `FrameGeneration` -- monotonic per view; identifies a produced frame and
  gates release so a stale engine cannot overwrite a presented frame.

## Version negotiation

`ProtocolVersion { major: u16, minor: u16 }`. The current version is
`CURRENT`. Each peer advertises a `VersionRange { min, max }` of versions it
speaks. Negotiation succeeds when the majors match and the ranges overlap; the
agreed version is the highest common minor within the shared major.

- Different majors -> `VersionError::MajorMismatch`. Majors are incompatible by
  construction; a new major is a breaking change.
- Same major, disjoint minor ranges -> `VersionError::NoCommonVersion`.
- Otherwise -> agreed `ProtocolVersion` at `min(local.max.minor,
  remote.max.minor)`.

Capabilities are negotiated alongside the version. `Capabilities` is a bitset a
backend advertises (DMA-BUF frames, downloads, file chooser, IME, permissions,
new-view, streaming fetch, websocket, accessibility, devtools). The negotiated
capability set is the intersection. A command that needs an unadvertised
capability is answered with `Event::CapabilityMismatch`, not a panic.

## Commands (shell -> engine)

- `CreateView { view, profile, viewport }`
- `CloseView { view }`
- `Navigate { view, request }` -- `NavigationRequest { url }`
- `Reload { view }`
- `Stop { view }`
- `Resize { view, viewport }` -- `Viewport { width, height, scale_permille }`
- `SetVisible { view, visible }`
- `Input { view, event }` -- `InputEvent` (below)
- `PermissionDecision { request, decision }`
- `ReleaseFrame { view, generation }` -- returns frame ownership to the engine
- `Shutdown` -- begin clean drain

## Input events

`InputEvent` is the union of pointer, keyboard, text, IME, and focus input.
Coordinates are physical device pixels relative to the view origin.

- `Pointer { kind, x, y, button, wheel_x, wheel_y }` with `PointerKind`
  (Down, Up, Move, Enter, Leave, Wheel).
- `Key { kind, key_code, modifiers }` with `KeyKind` (Down, Up).
- `Text { text }` -- committed text (post-IME).
- `Ime { kind, text, cursor_begin, cursor_end }` with `ImeKind`
  (Preedit, Commit, Cancel).
- `Focus { gained }`.

## Events (engine -> shell)

- `ViewCreated { view }` / `ViewClosed { view }`
- `LoadStateChanged { view, state }` -- `LoadState`
- `UrlChanged { view, url }`
- `TitleChanged { view, title }`
- `CursorChanged { view, cursor }` -- `CursorKind`
- `StatusChanged { view, status }`
- `ProgressChanged { view, permille }`
- `PermissionRequested { request, view, kind, origin }`
- `DownloadRequested { request, view, url, suggested_name }`
- `FileChooserRequested { request, view, multiple }`
- `NewViewRequested { request, source_view, url }`
- `FrameReady { frame, damage }` -- `FrameHandle` + bounded `DamageRect` list
- `Crashed { view, reason }` -- `CrashReason`
- `Hang { view, elapsed_ms }`
- `CapabilityMismatch { view, needed }`
- `Metrics { view, sample }` -- `EngineMetrics`

## Wire framing

Every control message is a length-prefixed envelope. The header is fixed at
ten bytes, big-endian:

```
offset size field
0      2    magic  = 0x53 0x53 ("SS")
2      1    wire_version (envelope format; distinct from ProtocolVersion)
3      1    kind (0 = command, 1 = event)
4      2    message_type (u16 discriminant; see MessageType)
6      4    body_len (u32)
10     N    body (body_len bytes)
```

The body is the encoded command or event. Strings are length-prefixed
(`u32` byte length + UTF-8 bytes). Vectors are length-prefixed (`u32` count +
elements). Enums encode a `u8` discriminant.

## Limits

Decoding enforces hard limits so a hostile or buggy peer cannot exhaust the
receiver:

- `MAX_MESSAGE_BYTES` bounds the envelope body.
- `MAX_STRING_BYTES` bounds any single string (URL, title, status, text).
- `MAX_DAMAGE_RECTS` bounds the `FrameReady` damage list.
- `MAX_VEC_LEN` bounds any other length-prefixed sequence.

A length that exceeds its limit is a decode error, not an allocation.

## Error taxonomy

`ProtocolError` classifies every rejection at the boundary:

- `UnsupportedWireVersion(u8)` -- envelope format not understood.
- `BadMagic` -- header does not start with the magic bytes.
- `Truncated { need, have }` -- fewer bytes than the header or body require.
- `TrailingBytes(usize)` -- body decoded but bytes remain.
- `MessageTooLarge { len, max }` -- body_len exceeds `MAX_MESSAGE_BYTES`.
- `UnknownMessageType(u16)` -- discriminant not in `MessageType`.
- `UnknownKind(u8)` -- kind byte is neither command nor event.
- `BadDiscriminant { message_type, value }` -- enum byte out of range.
- `InvalidUtf8` -- string bytes are not UTF-8.
- `LimitExceeded { limit, value }` -- a sequence or string exceeds its bound.

An `UnknownMessageType` from a newer minor peer is recoverable: a receiver may
skip the whole envelope by its `body_len` and continue, which is why the length
prefix precedes the body.

## Lifecycle state machines

State transitions are validated by explicit tables. An illegal transition
returns `IllegalTransition { from, to }` rather than mutating state.

Engine:

```
Starting -> Ready -> Draining -> Exited
Starting -> Failed
Ready    -> Failed
Draining -> Failed
```

View:

```
Creating -> Loading -> Interactive -> Closing -> Closed
Creating -> Failed
Loading  -> Failed
Loading  -> Interactive -> Loading   (re-navigation)
Interactive -> Failed
Failed   -> Closing -> Closed
```

Frame:

```
Produced -> Transferred -> Presented -> Released
Produced -> Transferred -> Discarded -> Released
```

A frame reaches `Released` from `Presented` or `Discarded` only; the shell
returns ownership with `ReleaseFrame`, and the generation prevents a stale
engine from presenting over a live frame.

## Scope boundary

This landing delivers the control plane in full: every command and event
serializes, decodes, and validates, and the malformed-message and
version-negotiation tests exercise that decode path directly. The frame-plane
transport (shared memory vs DMA-BUF) is bound at the native-runtime extraction
spike, issue #53. No process is spawned here; this is the contract that #53
builds the transport under.

## Home crate and split candidate

The module lives in `silksurf-core` for dependency correctness: both the shell
(`silksurf-app`) and the native engine (`silksurf-engine`) already depend on
`silksurf-core`, so the protocol types reach both sides without dragging engine
internals into the shell. The module is a split candidate: once #53 measures
ownership, it extracts to a dedicated `silksurf-engine-protocol` crate.
