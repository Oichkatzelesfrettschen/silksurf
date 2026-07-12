/*
 * net_queue turns blocking HTTP work into event-loop completions.
 *
 * A fetch() call allocates a request id, parks the promise's resolving
 * functions in the JS-side `__silksurfPendingNet` registry (GC-rooted, the
 * same idiom as the event-listener registry), and spawns a worker thread that
 * runs the blocking silksurf-net client. The worker sends a NetCompletion
 * over an mpsc channel; run_host_callbacks drains the channel on the next
 * tick and settles the promise through boa's job queue.
 *
 * The shared half (sender, id counter, in-flight count) lives behind
 * Rc<RefCell<...>> so fetch natives can capture it; only the Sender clone
 * crosses into worker threads (Rc never does). in_flight decrements when a
 * completion is CONSUMED, not when it is sent, so has_pending_host_callbacks
 * stays true across the send-to-drain window.
 *
 * Teardown: dropping the SilkContext drops the Receiver; worker sends then
 * fail silently and the threads exit. No explicit cancellation pass needed.
 */

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};

pub(super) enum NetPayload {
    Response(silksurf_net::HttpResponse),
    Error(String),
}

pub(super) struct NetCompletion {
    pub(super) id: u64,
    pub(super) payload: NetPayload,
}

pub(super) struct NetShared {
    tx: Sender<NetCompletion>,
    next_id: u64,
    in_flight: usize,
}

pub(super) type NetSharedRef = Rc<RefCell<NetShared>>;

pub(super) struct NetQueue {
    pub(super) shared: NetSharedRef,
    rx: Receiver<NetCompletion>,
}

impl NetQueue {
    pub(super) fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            shared: Rc::new(RefCell::new(NetShared {
                tx,
                next_id: 0,
                in_flight: 0,
            })),
            rx,
        }
    }

    /// Requests still awaiting drain. Drives the event-loop wake deadline.
    pub(super) fn in_flight(&self) -> usize {
        self.shared.borrow().in_flight
    }

    /// Pull every completion that has arrived; each consumes one in-flight slot.
    pub(super) fn drain(&mut self) -> Vec<NetCompletion> {
        let mut completions = Vec::new();
        while let Ok(completion) = self.rx.try_recv() {
            completions.push(completion);
        }
        let mut shared = self.shared.borrow_mut();
        shared.in_flight = shared.in_flight.saturating_sub(completions.len());
        completions
    }
}

impl NetShared {
    /// Allocate a request id and count it in flight. Returns the id and a
    /// Sender clone for the worker thread.
    pub(super) fn begin_request(&mut self) -> (u64, Sender<NetCompletion>) {
        let id = self.next_id;
        self.next_id += 1;
        self.in_flight += 1;
        (id, self.tx.clone())
    }
}

/// Run one blocking HTTP request on a worker thread, reporting completion.
pub(super) fn spawn_request(
    id: u64,
    tx: Sender<NetCompletion>,
    request: silksurf_net::HttpRequest,
) {
    std::thread::spawn(move || {
        use silksurf_net::{BasicClient, NetClient};
        let payload = match BasicClient::new().fetch(&request) {
            Ok(response) => NetPayload::Response(response),
            Err(err) => NetPayload::Error(err.message),
        };
        // A send failure means the SilkContext (and its realm) is gone;
        // the completion has no destination and the thread just exits.
        let _ = tx.send(NetCompletion { id, payload });
    });
}
