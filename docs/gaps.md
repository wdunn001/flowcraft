# Gaps

The ranked work list. Take work from the top.

1. Verify the control envelope against a live app. The builder follows WordCraft's
   `docs/control-protocol.md` (checked 2026-11-15); run a flow against a running app with
   `--targets` and confirm auth, ids and error handling end to end.
2. Land the flow lane in the CollabCraft service. The endpoints in
   `crates/connectors/src/cloud.rs` are a draft contract; the service must grow matching
   routes before any cloud step can run for real.
3. Trigger delivery. `app-event` triggers need an event source (the CollabCraft relay is the
   natural one); `schedule` triggers need a small runner that starts flows on an interval.
4. E-signature steps. `request-signature` and `signature-status` must track the suite's
   e-signature tooling as its API settles.
5. A real CollabCraft transport for the CLI, with OIDC device-flow sign-in. That transport
   lets `run` execute cloud steps. It belongs in the CLI or in a shared craft-libs crate,
   never in the library crates here.
6. An egui flow canvas (`apps/flowcraft`): drag steps, draw edges, edit conditions, run with
   live status. Build it like the suite's other apps, with `craft-ui` where it fits.
7. A browser build. The canvas app should build for wasm32 and serve from CollabCraft like
   the other Craft apps.
8. Run history. A run report store would let a flow's past runs be inspected and retried.
9. Suite merge chores: `contributors/people.toml`, packaging, release workflow and the rest
   of the app-repo furniture, when the repository moves into the storytold org.
