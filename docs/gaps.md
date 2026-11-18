# Gaps

The ranked work list. Take work from the top.

1. The egui designer (`apps/flowcraft`), built like the suite's other apps on the storytold
   egui pins. The canvas renders `craft_flowchart::doc`, nodes drag, connectors reroute,
   a property panel edits the semantic step, validation pins to nodes and simulate
   animates a dry run over the diagram.
2. The run journal (`crates/run`). Event-sourced and resumable: run started, step started
   with an idempotency key per attempt, step completed or failed, wait armed (task,
   envelope, timer, webhook), wait resolved, run finished. Resume replays the journal and
   continues from unresolved waits. Journals carry references, never tokens.
3. The flow lane, an optional crate the CollabCraft service mounts (the e-sign lane
   pattern): Postgres run store, trigger intake, timer and webhook resumption, the task
   inbox. Routes disable themselves when unconfigured.
4. Cloud steps re-targeted onto the `craft_cloud` traits (the fork's crates by git rev
   until craft-libs PR 160 merges). The draft URL mapping in
   `crates/integrations/src/cloud.rs` retires; the CollabCraft client and
   `craft-cloud-local` become two adapters of one seam.
5. New step kinds: `task` (assignee, instructions, outcomes); `send-envelope` plus
   `wait-for-envelope` on `craft-esign`; `agent` (goal, tools, timeout, output schema)
   behind an approval task by default; `payment` on a vendor-neutral provider trait with a
   stub adapter. New triggers: `form-submitted` plus `webhook`.
6. The GridCraft work-queue projection: queue rows written over the control protocol, the
   journal staying authoritative.
7. RFC-0042 in craft-contrib proposing `craft-flowchart`, then the craft-libs PR and the
   FigCraft cutover that deletes `board_route.rs` and `board_mindmap.rs`. Read RFC-0016
   first and cross-reference it.
8. Verify the control envelope against a live app with `--targets`, end to end.
9. A browser build of the designer, served from CollabCraft like the other Craft apps.
