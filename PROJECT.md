# Local text tools fork

This branch adds local text tools and workspace archives on the official v0.9.5 base
`2ecf19a92fc3a7fff75b6ce3655ef04f4625ec01`. Protocol generation 1 and private
protocol 22 remain unchanged. Do not push implementation changes to the upstream
repository; this is a user-owned fork.

## Behavior and ownership

- Explicit workspace, final tab, and final pane closes save a per-server archive
  before closing. Archives retain the displayed workspace name and snapshot.
  Other explicit pane and tab closes save each affected pane as a separate
  archive that resumes into a new workspace.
  `workspace.archive.list` marks entries resumable only when every saved pane has
  a supported exact native agent identity. `workspace.archive.resume` rejects
  active duplicates and retains the archive. Other processes and historical
  snapshots are not recovered by this feature. A server without these methods
  cannot provide an archive list. When session persistence is disabled, list and
  resume return `archive_disabled` rather than reporting an empty archive.

- The default sidebar groups sessions under collapsible machines. Session names
  use bold rainbow colours; multi-pane workspaces retain separate pane targets.
  Each expanded machine row has a (+) control that creates a session on that machine
  after its connection is active. The current session has a bright rounded border. Archive and menu controls remain at the bottom. Priority order applies
  within each machine and preserves worktree groups. Custom agent views retain
  their dedicated list and configurable rows.
- Archive entries are scoped to the selected server. Explicitly closed workspaces
  and panes can be listed when session persistence is enabled. Resume is offered
  only when saved process identities can be resumed safely. Older sampled recovery
  snapshots are not imported into the archive.

- `src/client/shell/local_history.rs` owns bounded, immutable text snapshots,
  local line selection and local scroll. It never concatenates screen redraws.
- `local_compose.rs` and `local_compose_actions.rs` own explicit local drafts and
  ordinary pane Paste transport with retained drafts. They do not predict terminal echo.
- Existing terminal keys, mouse wheel and server output remain unchanged outside
  the explicit tools. Alternate-screen app history is not client terminal history.
- The reader starts from the received viewport or a prior cached snapshot. An
  explicit refresh uses advertised `pane.selection.read` on at most 1,000
  terminal-held rows, fenced by content revision. This does not scroll or alter
  remote selection. Hidden alternate-screen app history is not exposed.
  A completed snapshot can be read without a remote reply.

## Design and verification plan

1. Ground the affected subsystem. Done: endpoint surfaces contain viewport cells;
   pane.selection.read supplies revision-fenced terminal text.
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

Stable v0.9.5 excludes pane.read and pane.send_input from the client-shell lane.
Do not infer lane support from the CLI API. Compose uses the existing Paste input
event and retains its draft; queue admission is not remote acceptance.

## Normal interaction latency trace

The optional HERDR_LATENCY_TRACE path enables180seconds of numeric-only client
telemetry. The path must not exist. Data uses a bounded4096-event queue and a
separate writer thread; the stop record reports dropped events. No keys, text,
clipboard content, endpoint labels or request arguments enter this format.
Input tokens start before the Windows handoff queue. Command tokens span enqueue
and response. Writer batches report queue and socket-write duration with local or
remote identity. Server events distinguish full surface1, patch2, snapshot3,
command response4 and other0. Presentation scopes measure local output work.
Activation counts identify the epoch; local targets skip the remote source-off
acknowledgement. Input-to-surface causal identity is NOT on the stable wire:
timeline correlation across these stages is inference, not a measured RTT.

Performance procedure:
1. Capture the normal Sid-session baseline on the actual client.
2. Ground a hypothesis from queue, transport, command and frame timings.
3. Apply the smallest root fix and repeat the same live interaction.
4. Compare numeric artifacts with the trace analysis script.
5. Cite before/after evidence in the fix commit.
Architecture choice: bounded in-process stage tokens rather than another ETW-only
CPU sample, because the latter cannot identify input and RPC queue delay.
The prior local-mode experiment proves its explicit tools only, not ordinary
remote echo, remote application wheel response or cross-machine latency.

## Sid VM native Claude recognition

The stock Sid VM binary dated 2026-10-26 does not recognize `claude-bin`.
The existing fix is commit `42ffed3039c2965a32cb951657db2c965e6b55f8`.
It adds the exact alias and its unit assertion. No new recognition patch is required.
Official `herdr:claude` lifecycle reports carry session identity only. They cannot
force the missing runtime agent label. Never spoof pane environment or reported state.

The verified Linux candidate is `0.9.5-local.sid-claude-bin-42ffed30`, protocol 22,
SHA-256 `1102e4f291619692b4c474246116f0127fbf9d85bd68bf74f094ed96a298c974`.
It was built on permanent Helsinki in a 3 GiB capped offload container, with
Rust 1.96.1, Zig 0.16.0 and one Cargo job. Its dynamic dependencies are libc and libm.
The optimized full application test compile was aborted at the 3 GiB cap after
memory reclaim stalled progress. It did not pass. An isolated test compiled the
unchanged parser function and its helpers from the selected source and passed.
The production release build then passed with cached dependencies and unchanged flags.

Linux handoff uses the supported `server.live_handoff` JSON API with an explicit
candidate path, exact expected version and protocol. It transfers existing PTY
descriptors and waits for the replacement before commit. Keep the old binary for rollback.
Do not replace this with a server stop or service restart while jobs are active.

For a systemd `Type=simple` unit with `KillMode=control-group`, first verify and
reload `ExitType=cgroup` without restarting the unit. Otherwise an old main PID exit
can kill the replacement and its PTYs. The owned systemd fixture proved old-to-new
and new-to-old handoffs: the shell PID, foreground job PID, advancing heartbeat,
pane ID and native session identity/source stayed intact. The old binary returned
`agent_not_found`; the candidate recognized the same controlled `claude-bin` process.
Fixture metadata is synthetic. It does not prove an actual session read feedback.

`ExitType=cgroup` keeps the unit active with `MainPID=0` after handoff. It does not
provide replacement main-process supervision. Treat that recovery limit explicitly.
Native handoff can regenerate terminal IDs. Re-read the verified native session
and current terminal ID before refreshing a receiver binding. Actual Sid activation
also retained the original Claude and shell PIDs, their start times and the native
session identity. The fixed server then recognized the real Claude session as idle.

Recipes: `scripts/smoke_cross_version_systemd_handoff.py` and
`scripts/native_handoff_rpc.py`. Evidence is under
`root:outputs/2026-11-17-herdr-native-runtime/` on Sid VM. The lead performs actual activation,
then verifies the real agent identity, retained jobs and feedback delivery separately.


## Endpoint activation latency

When leaving a remote endpoint, the client sends its surface release without waiting
for an acknowledgement before it starts another endpoint. Target input stays fenced
until its coherent surface and presentation effects arrive. This avoids one stalled
source blocking a healthy target; it does not make remote terminal echo local.

Workspace focus shows a local selection while its request is pending. Typed events
are held in memory against the endpoint, boot, generation and selected workspace.
Only a matching focused pane and coherent surface can release them. A failed or
superseded selection leaves the input unsent. The global menu's pending-input view
masks content, lists unsent and uncertain entries, and permits explicit discard.
Once a transport send is attempted, delivery is uncertain and is not retried
automatically. Closing the client loses held input. Simultaneous selection and
input in one batch are refused with a notice. Pointer input is not buffered.
Pane and tab focus requests also bind held input to the requested pane or tab.
Input waits for the matching snapshot and surface. A disconnect or retired
endpoint disarms held input; the recovery view keeps it for explicit action.

The client pipelines the presentation-effects fence after the second surface-sync request on the same ordered transport. This starts only after the initial coherent target commit. Input remains frozen until both the coherent revision/geometry frame and the matching effects token arrive. Each resync uses unique request and fence identifiers, so resize and focus changes reject stale acknowledgements. This removes one serial network round trip from machine activation; it does not reduce ordinary remote typing echo time. The delayed fake-endpoint test compares sequential and overlapping response schedules; live verification remains separate.


## Direct input recovery

Ordinary key, committed text and paste events always use the terminal input path. The client no longer loads automatic-draft sidecars or offers automatic-draft preference toggles. Stale enabled `.drafts.json` files cannot open the composer. Local compose remains an explicit manual tool only. Ctrl+Shift+D is not a Herdr direct-input shortcut; the host terminal can reserve it.

The recovery preserves remote pane fallback rows and the activation fence optimization. Ordinary remote typing and application scroll still require the remote host. Existing live trace shows about400ms first-response gaps, with local input queues below2ms. This release restores direct behavior; it does not claim to remove transport latency.

The isolated client/server smoke writes stale enabled preferences and executes ordinary typed and pasted commands without compose. A remote-route regression uses exact matching profile/boot/pane enrollment and verifies key, paste and IME text independently reach terminal transport with no overlay. Production panes receive no test input.

## Sid relay operation

Sid uses runtime Tailscale preferred relay `20` (Hong Kong). Apply on Sid with `tailscale debug force-prefer-derp 20`, then `tailscale debug restun`. Restore automatic selection with preference `0` and `restun`. This preference ends when Tailscale restarts; no persistence task is installed.

The live route check on 2026-10-28 measured ten Hong Kong replies at 189-192ms (median190ms), versus the Nuremberg baseline median381ms. All replies arrived and Sid VM SSH remained available. This measures network RTT, not key-to-echo or application-scroll latency. Confirm `Self.Relay` and actual `DERP(hkg)` ping output after reapplication. The ping tool can exit nonzero because no direct peer connection forms even when every relay reply arrives.

The direct-input client is installed separately as `local.direct-input-20261028`. The original server stays running. Client-only activation uses the new executable with `client`; production client activation still requires the user's terminal. Do not replace the server to activate this presentation change.

## Windows remote file drop

A pasted absolute Windows drive path in a remote pane opens a choice. Press
Ctrl+U to upload; other input sends the original path to the pane.
Herdr transfers one file at a time over a separate noninteractive SSH process.
It creates a unique directory under `~/herdr-uploads`, verifies SHA-256, and
pastes a quoted path only when the original endpoint generation and pane remain
active. The limit is 1 GiB. A failed or interrupted transfer can leave a
partial file. If the pane changes, Herdr shows the verified path in a notice.
The terminal does not distinguish a file drop from an ordinary path paste.

## Machine session controls

The expanded machine shows the current workspace as its main card. When that
workspace contains multiple panes, only other panes appear as sub cards. The
focused pane stays in the main card and changes as focus moves.

The expanded machine row owns its (+) hit area. The click retains one endpoint
intent until the runtime finishes its existing activation fence. The active
machine then opens the same required naming prompt as the keyboard shortcut.
Confirm creates the session; cancel creates nothing. The legacy optional-name
setting cannot bypass this client prompt. A failed or superseded activation cancels the intent. Offline machines
show the existing unavailable notice. No endpoint protocol fields change.

Windows UI proof uses an isolated config, state folder and unique named server:
`python scripts/test_machine_workspace_ui.py <herdr.exe> <new-output-directory>`.
It captures the actual ConPTY frame and checks that a Local (+) click adds exactly
one workspace only after the name is confirmed. It also proves cancel has no
creation effect and captures the bright current-session border. `--baseline` captures the old client without clicking. The script
requires installed pywinpty, pyte and Pillow; it stops only its own fixture server.
`--sidebar-panes` creates an isolated two-pane workspace and checks that its
main card and one other-pane card appear in the captured frame.
`--sidebar-click` uses two tabs with distinct terminal text. It checks that a
sub-card click changes the visible terminal and active border, then that a
main-card click restores both. `--server-exe` can run the fixture against an
older local server; `--dimensions COLSxROWS` checks a shorter terminal.
Unit endpoint tests cover remote routing, prompts, offline targets and long labels.

## Machine monitor

Monitor beside Menu expands a client-only panel at the bottom of the machine
sidebar. It starts collapsed and saves the choice with other client chrome.
The session list retains at least its header and several rows. The monitor has
its own wheel scroll when space is limited; Archive and Menu remain accessible.

The external collector owns `%LOCALAPPDATA%/herdr/monitor/snapshot.json`.
`HERDR_MONITOR_FILE` overrides the path for isolated checks. The version 1 JSON
contains generated_at, poll_seconds, hosts and temporary resource inventory.
Only Sid VM, VPS Helsinki and VPS Singapore appear. The consumer has no provider
credentials, network calls or server protocol changes. A bounded background
reader checks at two-second intervals; render uses only cached typed data.
The reader limits each file to 256 KiB and 32 temporary resources. A missing,
invalid or stale source cannot appear as fresh zero utilization or empty inventory.

Host CPU is the collector's interval percent. Memory uses bytes and displays GiB.
VPS load is the one-minute load alongside CPU count. Temporary inventory errors
remain visible. Rented CPU displays its server count and the longest elapsed time
across those servers. A zero-server inventory displays count 0 and no elapsed time;
an unknown worker age makes the longest elapsed time unknown. RunPod runtime is up.
Unknown temporary usage displays "usage unavailable"; unknown elapsed values display unknown.
Inset metric rows use small separated square segments within a visible battery track.
Host dividers and right-aligned capacity/load details separate each source.
Stopped rented CPU remains
visible because the rental still exists; stopped RunPod is excluded.

Design decision: the typed local snapshot keeps fleet monitoring separate from
session runtime. Adding server snapshot fields was rejected because it would
require server upgrades for a local presentation feature. Data collection and
UI changes have separate owners. The only shared boundary is version 1 JSON.
The client never performs file or network I/O from a render loop.

Native proof: `python scripts/test_machine_workspace_ui.py <candidate.exe>
<new-output-directory> --monitor-snapshot <fresh-collector-snapshot.json>`.
This starts its own isolated server/client, captures actual ConPTY output,
checks expand/collapse and rejects invalid data. It never sends input to user panes.

## Machine group presentation

Machine disclosure arrows hide or show that machine's sessions without changing the active pane.
The machine name retains activation and (+) retains session creation. Choices persist
by endpoint identity in client chrome preferences. Omen replaces the local machine label.
SID profiles follow other remote machines and start collapsed when no saved choice exists.
Keyboard session navigation can reveal a hidden target. No server or protocol change is required.

## Deferred agent restore

Deferred agent restore in a Windows PowerShell pane uses the same launcher
as explicit agent start. The launcher invokes the configured Codex executable
with its saved arguments, including when the executable path has spaces.

Verify with `python scripts/test_windows_deferred_resume.py <herdr.exe>`.
The test starts an isolated server and checks the arguments received by a
native probe executable.

## Native build cache in nested worktrees

Cargo merges .cargo/config.toml arrays from a task worktree and its parent repository.
A nested worktree can therefore duplicate the static CRT rustflag and miss the shared cache.
For native Windows checks and release builds, set `RUSTFLAGS=-C target-feature=+crt-static`
explicitly and keep `CARGO_BUILD_JOBS=1`. Initialize the installed Visual Studio vcvars64
before Cargo. Reuse the existing `CARGO_TARGET_DIR` and installed Zig executable.
This avoids a full dependency rebuild; it does not change product compiler settings.

Fast visual proof: set `HERDR_MONITOR_RENDER_OUTPUT` to a JSON output path and run
`cargo test --bin herdr monitor_export_native_render -- --ignored`. The test reads
the live local snapshot and exports the production Ratatui buffer. Render it with
`python scripts/render_monitor_buffer.py <cells.json> <output.png>` before an optimized build.
