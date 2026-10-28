# Local text tools fork

This branch adds client-only text tools on the official v0.9.5 base
`2ecf19a92fc3a7fff75b6ce3655ef04f4625ec01`. Protocol generation 1 and private
protocol 22 remain unchanged. Do not push implementation changes to the upstream
repository; this is a user-owned fork.

## Behavior and ownership

- `src/client/shell/local_history.rs` owns bounded, immutable text snapshots,
  local line selection and local scroll. It never concatenates screen redraws.
- `local_compose.rs` and `local_compose_actions.rs` own explicit local drafts and
  acknowledged `pane.send_input` submission. They do not predict terminal echo.
- Existing terminal keys, mouse wheel and server output remain unchanged outside
  the explicit tools. Alternate-screen app history is not client terminal history.
- The reader starts from the received viewport or a prior cached snapshot. An
  explicit refresh uses existing `pane.read recent` and may ask an idle remote
  app to scroll while the server collects text. Do not turn this into automatic
  background polling. A completed snapshot can be read without a remote reply.

## Design and verification plan

1. Ground the affected subsystem. Done: endpoint surfaces contain viewport cells;
   pane.read supplies authoritative revisioned text.
2. Run the architect step. Chosen: immutable endpoint/boot/pane snapshots and
   explicit local modals. Rejected: concatenate frame redraws, which invents
   chronology and duplicates alternate-screen controls.
3. Throughput checkpoint:
   - Blocking first steps: pin source and create leased worktree; establish tools.
   - Independent workstreams: compose editor, reader integration, build tools.
   - Shared mutable state: only integration owner edits shell wiring.
   - Smallest safe decomposition: isolated compose module plus one wiring owner.
4. Implement against the named data shape. Snapshot cache: eight entries, each
   at most 1 MiB retained line storage. Drafts: eight saved plus one active,
   each at most 128 KiB. Cache and drafts are memory-only, scoped by endpoint,
   server boot and pane. No new server protocol fields.
5. Verify on real surface: unit tests plus isolated ConPTY client attached to
   the unmodified stable server. Never use the user's panes as a test fixture.
6. Commit coherent units after affected checks.
7. Independent review: lead reads the diff; no additional review gate.
8. Delivery: lead owns candidate activation and live verification.

## Windows candidate handoff

Build using official Rust 1.96.1, Zig 0.16.0 and MSVC/Windows SDK.
Use the release profile: debug builds intentionally select `herdr-dev` state
and cannot attach to the installed stable namespace.
Set `HERDR_BUILD_CHANNEL=local` and `HERDR_BUILD_ID=received-text-20261028`
for a distinct candidate identity. Run tests through the harness reuse command.

Use the candidate executable's `client` subcommand to connect to an existing
server. `src/main.rs` dispatches this directly to `client::run_client`; it bypasses
server auto-start. Do not run `server stop`, `update`, or `live-handoff`.
The normal auto-detect path also validates generation compatibility instead of
replacing a compatible running server (`src/server/autodetect.rs`). Windows does
not support live server handoff (`src/platform/mod.rs`).

Keep the installed executable intact. Detach the old client with its configured
prefix then `q`; launch the candidate client in the same terminal. Detach closes
the presentation client, not the server/panes. Only the lead performs this
user-session handoff. Retain the stable executable for client-only rollback.
