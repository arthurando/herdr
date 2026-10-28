# Local text tools

These tools provide local scroll and text entry when a remote connection is slow.
They do not change ordinary remote application controls.

## Read received text

1. Press your prefix, then Shift+U, or choose **local text** in the global menu.
2. Use the wheel, arrows, Page Up or Page Down to move through the local snapshot.
3. Press V to start a line selection. Move the cursor, then press Y to copy.
4. Press R to fetch terminal-held rows from the selected pane.
5. Press Esc to return to the live pane.

The reader opens immediately with the received viewport or a previous snapshot.
It keeps up to 1,000 fetched lines, within a 1 MiB snapshot limit. Eight snapshots
can remain in memory. Snapshots do not update while you read them. New output
continues in the live pane; R explicitly replaces the snapshot.

The snapshot label identifies its source and revision. A viewport is only the
visible screen, not the full history. Herdr never joins alternate-screen redraws
and presents them as conversation history. The fetch reads terminal-held rows without a scroll or selection change on the
remote application. Alternate-screen applications can retain their older history
privately; those rows are not available through this reader. The fetch still
needs the remote connection. After receipt, scroll, line
selection and copy need no remote response.

## Compose text locally

1. Press your prefix, then Shift+I, or choose **local compose** in the global menu.
2. Type or paste your draft. Enter adds a new line.
3. Press Ctrl+Enter to send the text to the original pane.
4. Confirm a multiline send with a second Ctrl+Enter.
5. Press Esc to return to the pane. Drafts remain in client memory.

Each draft has a 128 KiB limit. Text stays local until you send it. The server uses
the application's normal paste mode. No extra Enter key is appended. However,
line breaks can execute commands in applications that do not enable bracketed
paste. The multiline confirmation identifies the target and warns about this.

The draft remains after a send. The status says that the paste was queued, not
that the server or application accepted it. Check the remote pane before another
send. A second send requires explicit confirmation. Repeated key events cannot
send another copy. Clear the draft yourself after you verify delivery; Ctrl+E then Ctrl+U
clears the text. Do not use visible local drafts for passwords.

These tools do not predict terminal echo, infer password prompts, or change
normal terminal input. Ordinary remote typing and application scroll still
depend on connection latency. Cached reader scroll and local draft edits do not.

Explicit configured prefix bindings take precedence over these two shortcuts.
The global menu remains available when a shortcut is overridden.
