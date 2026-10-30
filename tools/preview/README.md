# Herdr HTML preview companion (Windows)

Open an interactive HTML preview beside the terminal:

```powershell
tools\preview\herdr-preview.cmd C:\path\to\report.html
tools\preview\herdr-preview.cmd http://localhost:3000
```

The launcher needs Node.js and the installed Microsoft Edge. It does not download software.
The requested Edge app window is visible. Native helper processes remain hidden.
Copy this directory to a stable installation folder and put that folder on PATH to use
`herdr-preview <file-or-url>` from a Herdr shell. Do not copy only the `.cmd` wrapper.

This is a separate browser app window, not a panel inside the character-cell terminal.
It uses available space beside the active terminal. If that space is too small, it opens
at the right edge of the monitor and can overlap the terminal. It never resizes or closes
the terminal. Move or resize the preview as needed. Initial placement does not track later
terminal movement. Close the preview window to stop its process and local file server.
The shell waits while the preview is open; launch it from a spare Herdr pane.

For files, select an HTML file in a dedicated preview directory. The browser can request
HTML, CSS, JavaScript, images and fonts from that directory and its subdirectories.
Absolute `/asset` paths are not supported; use relative asset paths. The server blocks
directory listings, hidden files, non-asset files (including JSON/ENV/key files), traversal
and symlinks outside the directory. Do not put secrets in HTML or asset files in this
directory. Refresh the browser with Ctrl+R after an edit. There is no automatic reload.
The loopback server uses an unpredictable URL prefix, checks Host, and does not enable CORS.
Local files have a restrictive content policy; external scripts and API requests are blocked.
Use an explicit localhost development URL for an app that needs its own server/API policy.

Each launch uses Edge guest mode and a fresh browser profile under `%LOCALAPPDATA%\herdr\preview\profiles`.
Guest mode prevents the Windows account sign-in prompt from covering each new preview.
It does not use your signed-in browser profile, extensions, cookies or native credentials.
No native JavaScript bridge exists. Profiles remain after close so the launcher never deletes
files automatically. They can be removed after their preview windows close. Only localhost,
127.0.0.1 and ::1 HTTP(S) URL inputs are accepted. Apps on those URLs retain their own browser
navigation/network behavior; this is not an offline browser or a sandbox for hostile apps.

## Architecture and checks

Herdr 0.9.5 uses Rust, ratatui and crossterm. Its terminal host renders cells, not HTML.
The complete source supplies this architecture evidence; native REA decompilation is unnecessary.
A native WebView2 wrapper would need a GUI host/toolchain and a new integration layer.
The installed Edge app host gives an interactive preview without changing Herdr's renderer,
protocol, credentials or active sessions. No Rust source or Cargo files change.

Data shape: input is either an HTML file or a validated loopback URL. A local file becomes
one `{url, close}` server handle. One browser child owns one isolated profile. Its exit closes
the server. The native placement helper returns `{x,y,width,height}` only.

Rigor: **Model the Domain** separates file/server lifecycle from URL launch.
**Prove It Works** requires real browser DOM, CSS and interaction evidence, not syntax alone.
Throughput: source inspection first; preview files are independent from performance changes;
there is no shared mutable Herdr state; one worker owns the small companion.

```powershell
node ~/.claude/scripts/harness/reuse.mjs -- node --test tools/preview/preview.test.mjs
node ~/.claude/scripts/harness/reuse.mjs -- node tools/preview/verify-render.mjs <evidence-directory>
```

The first check covers file boundaries, URL restrictions, refresh, Host, methods and shutdown.
The second opens headless Edge and verifies rendered CSS plus a JavaScript button interaction.
It saves a screenshot and DOM. Visible placement and window-close lifecycle must also be checked
at installation. Headless evidence alone does not establish those window-manager behaviors.
