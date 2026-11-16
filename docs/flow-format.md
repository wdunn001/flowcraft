# The flow file format

A flow is one JSON document. `format_version` is 1; a reader refuses newer versions and a
document over 4 MiB. `flowcraft-cli validate` checks everything on this page.

## Top level

```json
{
  "format_version": 1,
  "id": "signed-to-archive",
  "name": "Archive signed documents",
  "description": "",
  "trigger": {"type": "manual"},
  "steps": [],
  "edges": []
}
```

Unknown top-level fields are refused. A new field means a new format version.

## Triggers

- `{"type": "manual"}` starts from a person or the CLI.
- `{"type": "app-event", "app": "pdfcraft", "event": "document.signed"}` starts from a Craft
  app event. The event's payload is readable through `$ref` values.
- `{"type": "schedule", "every_minutes": 60}` starts on an interval.

## Steps

Every step has a unique non-empty `id`, an optional `name` and one `action`.

- `app-command` runs one engine command in a Craft app over its control port:
  `{"type": "app-command", "app": "wordcraft", "command": "text.insert", "params": {...}}`.
- `cloud` calls CollabCraft. Operations: `store-file`, `share`, `notify`,
  `request-signature`, `signature-status`.
- `compose` builds an output object from literal values and `$ref` values without calling
  anything.

## Edges and conditions

An edge orders two steps: `{"from": "a", "to": "b"}`. The edges must form a DAG. A step runs
when it has no incoming edges, or when at least one incoming edge comes from a step that ran
and whose `when` condition holds. Everything else is skipped.

Conditions are structured, with no expression strings: `eq`, `ne`, `gt`, `lt`, `contains`,
`all`, `any`, `not`. Operands are JSON values and may be `$ref` objects. `gt` and `lt` need
numbers on both sides. `contains` works on strings, arrays and object keys.

## References

Anywhere a value appears, an object with the single key `$ref` is replaced at run time:

```json
{"$ref": {"from": "trigger", "path": ["document", "url"]}}
{"$ref": {"from": "step", "step": "export", "path": ["file", 0]}}
```

`path` walks object keys and array indexes. A missing path fails the step. A reference to a
step that was skipped fails the step. A malformed `$ref` body is an error. A typo cannot
slip through as literal data.

## Caps

Flow documents are hostile input. The caps: 4096 steps, 16384 edges, condition nesting 64,
value nesting 128, document size 4 MiB, control line 1 MiB.
