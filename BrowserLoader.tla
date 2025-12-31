---------------- MODULE BrowserLoader ----------------
EXTENDS Naturals, Sequences

(* --algorithm AsyncFetch
variables
    resource_state = "empty",  \* empty, loading, loaded, failed
    dom_node_alive = TRUE;     \* TRUE, FALSE (if user closed tab)

process RenderThread = "Renderer"
begin
RenderLoop:
    while dom_node_alive do
        \* Simulate user closing the tab or navigating away
        if resource_state = "loading" then
            either
                dom_node_alive := FALSE;
            or
                skip;
            end either;
        end if;
    end while;
end process;

process NetworkThread = "Network"
begin
Fetch:
    resource_state := "loading";
FetchNext:
    \* Simulate network delay
    resource_state := "loaded";
    
    \* CRITICAL SECTION: Trying to update DOM
UpdateDOM:
    if dom_node_alive then
        \* Successful update
        skip;
    else
        \* Resource returned but node is gone (handled safely?)
        skip;
    end if;
end process;

end algorithm; *)
\* BEGIN TRANSLATION (chksum(pcal) = "958486fc" /\ chksum(tla) = "cbff9c14")
VARIABLES resource_state, dom_node_alive, pc

vars == << resource_state, dom_node_alive, pc >>

ProcSet == {"Renderer"} \cup {"Network"}

Init == (* Global variables *)
        /\ resource_state = "empty"
        /\ dom_node_alive = TRUE
        /\ pc = [self \in ProcSet |-> CASE self = "Renderer" -> "RenderLoop"
                                        [] self = "Network" -> "Fetch"]

RenderLoop == /\ pc["Renderer"] = "RenderLoop"
              /\ IF dom_node_alive
                    THEN /\ IF resource_state = "loading"
                               THEN /\ \/ /\ dom_node_alive' = FALSE
                                       \/ /\ TRUE
                                          /\ UNCHANGED dom_node_alive
                               ELSE /\ TRUE
                                    /\ UNCHANGED dom_node_alive
                         /\ pc' = [pc EXCEPT !["Renderer"] = "RenderLoop"]
                    ELSE /\ pc' = [pc EXCEPT !["Renderer"] = "Done"]
                         /\ UNCHANGED dom_node_alive
              /\ UNCHANGED resource_state

RenderThread == RenderLoop

Fetch == /\ pc["Network"] = "Fetch"
         /\ resource_state' = "loading"
         /\ pc' = [pc EXCEPT !["Network"] = "FetchNext"]
         /\ UNCHANGED dom_node_alive

FetchNext == /\ pc["Network"] = "FetchNext"
             /\ resource_state' = "loaded"
             /\ pc' = [pc EXCEPT !["Network"] = "UpdateDOM"]
             /\ UNCHANGED dom_node_alive

UpdateDOM == /\ pc["Network"] = "UpdateDOM"
             /\ IF dom_node_alive
                   THEN /\ TRUE
                   ELSE /\ TRUE
             /\ pc' = [pc EXCEPT !["Network"] = "Done"]
             /\ UNCHANGED << resource_state, dom_node_alive >>

NetworkThread == Fetch \/ FetchNext \/ UpdateDOM

(* Allow infinite stuttering to prevent deadlock on termination. *)
Terminating == /\ \A self \in ProcSet: pc[self] = "Done"
               /\ UNCHANGED vars

Next == RenderThread \/ NetworkThread
           \/ Terminating

Spec == Init /\ [][Next]_vars

Termination == <>(\A self \in ProcSet: pc[self] = "Done")

\* END TRANSLATION 
======================================================
