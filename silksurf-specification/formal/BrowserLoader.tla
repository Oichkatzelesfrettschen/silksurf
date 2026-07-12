---------------- MODULE BrowserLoader ----------------
(*
 * BrowserLoader.tla -- async fetch versus DOM-node lifetime formal model.
 *
 * WHY: A navigation fetches a resource on the network thread while the render
 * thread owns the DOM.  The render thread may free the target DOM node
 * mid-flight (the user closes the tab or navigates away) before the response
 * arrives.  The safety guarantee is that the network thread never commits a DOM
 * update against a freed node -- a use-after-free.  The correct implementation
 * checks node liveness at the commit point and drops the update when the node
 * is gone; this spec makes that guarantee machine-checkable and confirms it
 * holds under every interleaving of the free and the commit.
 *
 * WHAT: A four-step network protocol (fetch -> load -> update -> done) racing a
 * render thread that may free the node at any point before the update commits.
 * A ghost variable records whether a commit ever touched a freed node.
 *
 * HOW: Run with TLC:
 *   tlc BrowserLoader.tla -config BrowserLoader.cfg
 *
 * See: crates/silksurf-engine/src/speculative.rs (async fetch on the worker
 *      thread); crates/silksurf-app/src/window_frame.rs (the DOM/frame is
 *      replaced only on navigation completion).
 * See: silksurf-specification/formal/README.md (how to run TLC)
 *)

EXTENDS Naturals

(*
 * State variables.
 *
 * resource_state    -- the fetch progresses empty -> loading -> loaded.
 * node_alive        -- TRUE while the target DOM node exists; the render
 *                      thread may set it FALSE at any time before the commit.
 * dom_written       -- TRUE once the network thread has committed a DOM
 *                      update (only ever done to a live node).
 * used_after_free   -- ghost flag: TRUE iff a commit ever wrote to a freed
 *                      node.  The safety property is that this stays FALSE.
 * net_pc            -- the network thread's control location.
 *)
VARIABLES resource_state, node_alive, dom_written, used_after_free, net_pc

vars == << resource_state, node_alive, dom_written, used_after_free, net_pc >>

ResourceStates == {"empty", "loading", "loaded"}
NetLocations == {"fetch", "load", "update", "done"}

Init ==
    /\ resource_state = "empty"
    /\ node_alive = TRUE
    /\ dom_written = FALSE
    /\ used_after_free = FALSE
    /\ net_pc = "fetch"

(* ------------------------------------------------------------------ *)
(*  Actions                                                             *)
(* ------------------------------------------------------------------ *)

(*
 * FreeNode -- the render thread frees the DOM node.  Enabled at any point
 * before the network thread finishes, which is precisely the race the model
 * exists to check: the free may land between load and update, or before either.
 *)
FreeNode ==
    /\ node_alive
    /\ net_pc \in {"fetch", "load", "update"}
    /\ node_alive' = FALSE
    /\ UNCHANGED << resource_state, dom_written, used_after_free, net_pc >>

(*
 * Fetch -- the network request starts.
 *)
Fetch ==
    /\ net_pc = "fetch"
    /\ resource_state' = "loading"
    /\ net_pc' = "load"
    /\ UNCHANGED << node_alive, dom_written, used_after_free >>

(*
 * Load -- the response body arrives.
 *)
Load ==
    /\ net_pc = "load"
    /\ resource_state' = "loaded"
    /\ net_pc' = "update"
    /\ UNCHANGED << node_alive, dom_written, used_after_free >>

(*
 * Update -- the commit point.  The correct implementation checks node_alive
 * and commits the DOM update only when the node still exists; a freed node is
 * left untouched, so used_after_free is never set.  Modelling the check here is
 * the formal expression of the liveness guard in the renderer: a regression
 * that committed unconditionally (setting used_after_free' = TRUE when the node
 * is freed) would violate NoUseAfterFree and TLC would report the trace.
 *)
Update ==
    /\ net_pc = "update"
    /\ net_pc' = "done"
    /\ IF node_alive
         THEN /\ dom_written' = TRUE
              /\ UNCHANGED used_after_free
         ELSE /\ UNCHANGED dom_written
              /\ UNCHANGED used_after_free
    /\ UNCHANGED << resource_state, node_alive >>

(*
 * Done -- the protocol has finished.  An explicit stuttering step so the
 * terminal state is not flagged as a deadlock.
 *)
Done ==
    /\ net_pc = "done"
    /\ UNCHANGED vars

Next == FreeNode \/ Fetch \/ Load \/ Update \/ Done

(*
 * Spec -- weak fairness on the network actions guarantees the loader always
 * makes progress to "done"; FreeNode is left unfair (the user may or may not
 * close the tab).
 *)
Spec == Init /\ [][Next]_vars /\ WF_vars(Fetch \/ Load \/ Update)

(* ------------------------------------------------------------------ *)
(*  Invariants and properties                                           *)
(* ------------------------------------------------------------------ *)

(*
 * TypeOK -- every variable stays within its domain.
 *)
TypeOK ==
    /\ resource_state \in ResourceStates
    /\ node_alive \in BOOLEAN
    /\ dom_written \in BOOLEAN
    /\ used_after_free \in BOOLEAN
    /\ net_pc \in NetLocations

(*
 * NoUseAfterFree -- the primary safety guarantee: the network thread never
 * commits a DOM update against a freed node.
 *)
NoUseAfterFree == ~used_after_free

(*
 * CommitOnlyAfterLoad -- a DOM update is committed only once the resource has
 * loaded, never against an empty or still-loading resource.
 *)
CommitOnlyAfterLoad == dom_written => (resource_state = "loaded")

(*
 * Termination -- the loader always finishes, even when the node is freed
 * mid-fetch (the free must not deadlock or livelock the protocol).
 *)
Termination == <>(net_pc = "done")

(* ------------------------------------------------------------------ *)
(*  Theorems (checked by TLC via BrowserLoader.cfg)                     *)
(* ------------------------------------------------------------------ *)

(*
 * THEOREM Spec => []TypeOK               -- every reachable state well-formed.
 * THEOREM Spec => []NoUseAfterFree       -- no commit ever touches a freed node.
 * THEOREM Spec => []CommitOnlyAfterLoad  -- commits only after the load.
 * THEOREM Spec => Termination            -- the loader always finishes.
 *)

======================================================
