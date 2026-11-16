# FlowCraft

FlowCraft is a workflow builder for the Craft suite, in the product space of Microsoft Power
Automate and Google Apps Script. A flow is a trigger, steps and conditional edges. Steps drive
the Craft apps over their loopback control ports or call CollabCraft for files, sharing,
notifications and signatures. FlowCraft is the next phase of the CollabCraft service and the
suite's e-signature tooling.

The repository is an early scaffold. It is built to the conventions of the Craft apps at
[github.com/storytold](https://github.com/storytold) and is written to merge into that suite.

## What exists

- `crates/flow` holds the flow document: triggers, steps, edges, structured conditions, `$ref`
  values, caps and validation.
- `crates/engine` runs a flow. It orders the steps, evaluates edge conditions and resolves
  references. Every effect goes through a `Connector` trait.
- `crates/connectors` builds the Craft control-port envelope (`engine.execute`) and a draft
  CollabCraft binding. No sockets live in the library crates.
- `apps/flowcraft-cli` validates, inspects and runs flows. It owns the TCP transport to the
  apps' control ports.

## A flow

```json
{
  "format_version": 1,
  "id": "signed-to-archive",
  "name": "Archive signed documents",
  "trigger": {"type": "app-event", "app": "pdfcraft", "event": "document.signed"},
  "steps": [
    {"id": "store", "action": {"type": "cloud", "operation": {
      "type": "store-file", "path": "signed/latest.pdf",
      "content": {"$ref": {"from": "trigger", "path": ["document"]}}
    }}},
    {"id": "notify", "action": {"type": "cloud", "operation": {
      "type": "notify", "user": "owner",
      "message": {"$ref": {"from": "step", "step": "store", "path": []}}
    }}}
  ],
  "edges": [{"from": "store", "to": "notify"}]
}
```

`flowcraft-cli new starter.json` writes this document. `flowcraft-cli run starter.json` dry-runs
it. The format is specified in [docs/flow-format.md](docs/flow-format.md).

## Build

```sh
cargo xtask ci
```

The gate runs the format check, clippy with warnings denied, the test suite and the asset
attribution check. CI calls the same gate through the suite's shared workflow.

## License

Dual-licensed under the MIT License or the Apache License 2.0, at your option.
