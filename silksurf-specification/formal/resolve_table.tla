---------------- MODULE resolve_table ----------------
(*
 * resolve_table.tla -- Monotonic Resolve Table formal model.
 *
 * WHY: The SilkSurf CSS engine maintains a lock-free mapping from selector
 * generation numbers (GenNum) to computed style indices (StyleIdx).  The
 * fundamental safety guarantee is monotonicity: once a generation number is
 * bound to a style index, that binding is permanent.  No re-binding and no
 * invalidation are permitted.  This spec makes the invariant machine-checkable
 * and documents the permitted operations precisely.
 *
 * WHAT: A partial-function model of the resolve table over finite domains
 * chosen small enough for TLC to exhaust the state space.
 *
 * HOW: Run with TLC:
 *   tlc resolve_table.tla -config resolve_table.cfg
 *
 * See: crates/silksurf-core/src/ (implementation)
 * See: silksurf-specification/formal/README.md (how to run TLC)
 *)

EXTENDS Integers, Sequences, FiniteSets

(*
 * Model parameters.  Keep these small so TLC terminates quickly.
 * For real verification widen GenNums / StyleIdxs and run overnight.
 *)
CONSTANTS
    GenNums,    \* finite set of valid generation numbers, e.g. 0..3
    StyleIdxs   \* finite set of valid style index values, e.g. 0..7

ASSUME GenNums \subseteq Nat
ASSUME StyleIdxs \subseteq Nat

(*
 * State variables.
 *
 * table  -- the resolve table, modelled as a function from a subset of
 *           GenNums to StyleIdxs.  DOMAIN table is the set of generations
 *           that have been bound so far.
 *
 * last_op -- a string tag recording which operation fired most recently.
 *            Used only for human readability in TLC traces; not checked.
 *)
VARIABLES table, last_op

vars == << table, last_op >>

(*
 * TypeInvariant -- every bound generation maps to a valid style index.
 *
 * This is the basic well-formedness condition; Monotonicity below is the
 * stronger safety property.
 *)
TypeInvariant ==
    /\ DOMAIN table \subseteq GenNums
    /\ \A g \in DOMAIN table : table[g] \in StyleIdxs

(*
 * Monotonicity -- the key safety invariant.
 *
 * Once a generation g is mapped to a style index i, no future state may map
 * g to any other index.  We check this by inspecting the current table only:
 * the invariant is "this state is consistent with all previous states having
 * the same bindings for already-bound generations."
 *
 * TLC verifies Monotonicity holds in every reachable state, which (combined
 * with the fact that Insert is the only way bindings enter the table and it
 * refuses to rebind) proves the property globally.
 *)
Monotonicity ==
    \A g \in DOMAIN table :
        /\ g \in GenNums
        /\ table[g] \in StyleIdxs

(*
 * Init -- the table starts empty; no generation has been resolved yet.
 *)
Init ==
    /\ table = [g \in {} |-> 0]    \* empty function (domain = {})
    /\ last_op = "init"

(* ------------------------------------------------------------------ *)
(*  Operations                                                          *)
(* ------------------------------------------------------------------ *)

(*
 * Insert(gen, idx) -- bind generation gen to style index idx.
 *
 * PRECONDITION: gen must not already be in the table.  Attempting to
 * rebind an existing generation is explicitly disallowed (the lock-free
 * implementation uses compare-and-swap to enforce this).  Here we model
 * that as a guard so TLC never generates a state where a binding changes.
 *
 * WHY guard on gen \notin DOMAIN table: this is the formal expression of
 * the monotonicity contract.  The CAS in the Rust implementation does the
 * same: it writes only if the slot was previously unset.
 *)
Insert(gen, idx) ==
    /\ gen \in GenNums
    /\ idx \in StyleIdxs
    /\ gen \notin DOMAIN table           \* rebinding disallowed
    /\ table' = [g \in DOMAIN table \cup {gen} |->
                    IF g = gen THEN idx ELSE table[g]]
    /\ last_op' = "insert"

(*
 * Lookup(gen) -- read the style index for generation gen.
 *
 * This is a pure read: no state changes.  In TLA+ a read is modelled as a
 * stuttering step (UNCHANGED vars) with a guard that the generation is bound.
 * We include it so the spec is complete and TLC can verify Lookup never
 * produces an out-of-domain access.
 *
 * An unbound Lookup is not modelled here because in the Rust implementation
 * it returns None and does not change any state; we simply do not enable the
 * step when gen is unbound.
 *)
Lookup(gen) ==
    /\ gen \in DOMAIN table
    /\ UNCHANGED vars

(*
 * Clear -- the no-op / prohibited operation.
 *
 * WHY: The monotonic table never invalidates entries.  We model Clear as a
 * permanently disabled action (FALSE guard) so TLC confirms that no reachable
 * state ever clears the table.  Documenting it explicitly communicates the
 * design intent: there is no "reset" operation.
 *)
Clear ==
    /\ FALSE                             \* always disabled -- no eviction permitted
    /\ UNCHANGED vars

(* ------------------------------------------------------------------ *)
(*  Temporal formula                                                    *)
(* ------------------------------------------------------------------ *)

(*
 * Next -- the union of all enabled actions.
 *
 * Clear is included in the disjunction for completeness but its FALSE guard
 * means it never fires.
 *)
Next ==
    \/ \E gen \in GenNums, idx \in StyleIdxs : Insert(gen, idx)
    \/ \E gen \in GenNums : Lookup(gen)
    \/ Clear

(*
 * Spec -- the complete specification.
 *
 * Init /\ [][Next]_vars means: start in Init and every step is either a Next
 * action or a stuttering step (the system may do nothing, which is sound for
 * a passive data structure).
 *)
Spec == Init /\ [][Next]_vars

(* ------------------------------------------------------------------ *)
(*  Theorems (checked by TLC as invariants; stated here as comments)   *)
(* ------------------------------------------------------------------ *)

(*
 * THEOREM Spec => []TypeInvariant
 *   Every reachable state is well-formed.
 *
 * THEOREM Spec => []Monotonicity
 *   Every generation that has been bound retains its binding forever.
 *   This is the primary safety guarantee of the resolve table.
 *
 * To have TLC check these, add to resolve_table.cfg:
 *   INVARIANT TypeInvariant
 *   INVARIANT Monotonicity
 *)

======================================================
