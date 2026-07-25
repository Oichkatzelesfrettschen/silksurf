# Native-Engine Process Module Ownership in the Browser Binary

**Date**: 2026-07-25
**Last verified**: 2026-07-25 (half-duplex constraint retired the same day)
**Evidence class**: crate source (rank 4) plus workspace test oracle (rank 2).
No live-run or GUI-frame evidence; the control plane carries no frame bytes.
**Mechanism**: `crates/silksurf-app/src/engine_process.rs` holds the shell side
of engine protocol v1 over a child process. `NativeEngineProcess::spawn`
re-execs `std::env::current_exe()` with `--silksurf-native-engine-worker`, and
`run_internal_engine_process_mode` dispatches that flag to the worker loop.
**Question**: does the shipped browser binary carry the process modes its own
supervisor re-execs?

## Verdict

It did not. The module reached the build only through
`include!("../engine_process.rs")` inside `src/bin/native_engine_process_probe.rs`;
`main.rs` declared no `mod engine_process`. Two mechanisms follow from that,
and a third was latent behind them.

## Re-exec resolved to a browser window, not a worker

`NativeEngineProcess::spawn` names the worker by re-exec, so the spawned
executable must recognize the flag. Under `include!` only the probe binary did.
`parse_app_options` never rejects an unknown argument: `positional_url_arg`
discards anything matching `_ if arg.starts_with('-')`, and the URL falls back
to `https://example.com`. A re-exec from the browser binary would therefore
have opened a second full browser window on the default URL while the parent
waited for `ViewCreated`, and the parent's first `receive` would have blocked
until that window closed. The probe passed throughout, because the probe was
the only process that ever ran the worker.

`main.rs` now declares the module and dispatches before `parse_app_options`.
The probe is a flag on the browser binary, so `current_exe()` names an
executable that handles the flag by construction, and
`crates/silksurf-app/tests/native_engine_process.rs` drives
`CARGO_BIN_EXE_silksurf-app` for both the lifecycle round trip and the
worker-flag claim.

## The envelope layout had two owners

`silksurf_core::engine_protocol::wire` defines the ten-byte envelope. The app
re-derived it: a private `ENVELOPE_HEADER_BYTES` and a body length read from
`header[6..10]`. Widening a header field in `wire.rs` compiles clean across the
workspace and misframes the stream at runtime, because no type connects the
two.

`wire.rs` now exports `ENVELOPE_HEADER_BYTES`, derived from the field widths,
and `envelope_body_len`, which validates magic and wire version and bounds the
declared length by `MAX_MESSAGE_BYTES` before a transport allocates a body.
`envelope_body_len_splits_every_encoded_message_at_the_header` asserts that the
header plus the declared body covers every encoded sample command and event, so
an encoder change that skipped the accessor fails a test rather than a stream.

The app keeps its own read loop. `FrameHandle` names a shared-memory or DMA-BUF
descriptor, and descriptor passing needs `recvmsg` with a control-message
buffer; `Read::read` cannot receive a descriptor. The durable shared artifact
is the header layout, not the loop.

## The transport was half-duplex by construction

Retired 2026-07-25 by `EventIngress`, which owns the event pipe on its own
thread behind a bounded queue with nonblocking insertion. The reasoning below
is what forced that ordering: asynchronous event ingress precedes worker-owned
navigation, because navigation is the first producer of unsolicited events.


The parent writes one command and blocks in `NativeEngineProcess::receive`
until the matching event arrives; the worker writes only in response to a
command. Neither side fills a pipe while the other is also writing, so one
thread per process suffices and the current control messages cannot deadlock.

That invariant is what unsolicited events break. `Event::FrameReady`,
`Event::Crashed`, and `Event::Hang` all originate at the engine, so enabling
any of them retires the argument above and makes deadlock reachable. It
becomes deterministic once one event exceeds the free pipe capacity or enough
queued events fill it: a maximal `FrameReady` carries `MAX_DAMAGE_RECTS`
(4096) rectangles at sixteen bytes each, which is 65_536 bytes before the
frame handle, the length prefix, and the envelope, against a 64 KiB Linux
pipe buffer. `MAX_STRING_BYTES` puts `UrlChanged` and `TitleChanged` in the
same range, and `MAX_MESSAGE_BYTES` allows 1 MiB. A single small `Crashed`
event fits and does not wedge anything by itself; repeated asynchronous
events reach the same backpressure failure.

`NativeEngineProcess::shutdown` called `Child::wait` with no deadline, so a
wedged worker held the shell. `shutdown_within` now polls `try_wait` and
escalates to `Child::kill` at the deadline, reaping on every path.

## Falsifiers

- A worker that emits an event without a preceding command retires the
  half-duplex invariant and makes the reader thread load-bearing at once,
  whether or not that first event is small enough to fit the pipe.
- A single control message above the free pipe capacity turns the reachable
  deadlock into a deterministic one, with no protocol change.
- Restoring a second binary that re-execs itself reopens the flag-ownership
  defect, so any new process mode belongs on the browser binary.
