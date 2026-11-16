# Roadmap

Stage: scaffold, 0.1.0. The flow format, the engine, the connector seams and the CLI exist with
tests. Nothing ships yet. The ranked work list lives in [docs/gaps.md](docs/gaps.md).

## Done

- Flow document model with validation, caps and format versioning.
- Engine with deterministic ordering, conditional edges and `$ref` resolution behind a
  `Connector` trait.
- Control-port envelope builder matching the apps' `docs/control-protocol.md`, with a TCP
  transport in the CLI.
- Draft CollabCraft binding for files, sharing, notifications and signatures.
- CLI: `new`, `validate`, `inspect`, `run` with dry-run.

## Next

- Verify the control envelope against each target app, then run a flow end to end against a
  live app.
- Land the flow lane in the CollabCraft service so the cloud operations have a real counterpart.
- Trigger delivery: app events through CollabCraft, schedules through a runner.

## Later

- An egui flow canvas (`apps/flowcraft`), built like the suite's other apps.
- A browser build served at `craft.quasarke.net`.
- E-signature steps tracking the suite's e-signature tooling as it lands.
