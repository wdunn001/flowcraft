# Roadmap

Stage: M1 of the program plan, 0.1.x. The flow format, the engine, the integration seams,
the CLI and the incubating flowchart kernel exist with tests. Nothing ships yet. The ranked
work list lives in [docs/gaps.md](docs/gaps.md).

## Done

- Flow document model with validation, caps and format versioning.
- Engine with deterministic ordering, conditional edges and `$ref` resolution behind a
  `Connector` trait.
- Control-port envelope builder matching the apps' `docs/control-protocol.md`, with a TCP
  transport in the CLI.
- Draft CollabCraft binding for files, sharing, notifications and signatures.
- `crates/flowchart` (`craft-flowchart`): the flowchart kernel incubating for craft-libs.
  Connector routing and mind-map layout lifted from FigCraft's boards, a layered layout for
  generated flows and a diagram document. FlowCraft is the first consumer.
- `flowcraft-cli diagram`: a flow drawn as a diagram, auto-arranged.

## M1, the rest

- The egui designer (`apps/flowcraft`): canvas on the kernel, step property panel,
  validation pinned to nodes, simulate over the diagram.

## M2

- The run journal (`crates/run`): event-sourced, resumable, idempotency keys per step
  attempt; waits for tasks, envelopes, timers and webhooks.
- The flow lane as an optional crate mounted by the CollabCraft service: run store,
  trigger intake, task inbox.
- The `form-submitted` trigger from FormCraft; the GridCraft work-queue projection.

## M3

- The e-sign contract workflow end to end: FormCraft signature placements, `craft-esign`,
  acceptance task, invoice, payment stub.

## M4

- RFC-0042 in craft-contrib; the kernel moves to craft-libs; FigCraft cuts over and
  deletes its copies; EffectCraft adopts the layered layout.

## M5

- The Stripe adapter, AI-agent steps behind approval gates, the full client-onboarding
  showcase, browser builds.
