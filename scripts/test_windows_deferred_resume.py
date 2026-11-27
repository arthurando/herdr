"""Exercise deferred Codex restore in an isolated Windows PowerShell pane."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


SESSION_ID = "c5e10efb-43cd-4913-aede-88138a321154"
PANE_ID = "w1T:p1"


def make_probe(root):
    source = root / "Probe.cs"
    source.write_text(
        """
using System;
using System.IO;
class Probe {
    static void Main(string[] args) {
        string receipt = Environment.GetEnvironmentVariable("HERDR_TEST_ARGV");
        File.WriteAllLines(receipt + ".tmp", args);
        File.Move(receipt + ".tmp", receipt);
    }
}
""",
        encoding="ascii",
    )
    probe = root / "Custom Codex.exe"
    compiler = (
        Path(os.environ["WINDIR"])
        / "Microsoft.NET"
        / "Framework64"
        / "v4.0.30319"
        / "csc.exe"
    )
    subprocess.run(
        [compiler, "/nologo", f"/out:{probe}", source],
        check=True,
        creationflags=subprocess.CREATE_NO_WINDOW,
    )
    return probe


def write_session(config, cwd):
    pane = {
        "cwd": str(cwd),
        "label": "Resume probe",
        "agent_session": {
            "source": "herdr:codex",
            "agent": "codex",
            "kind": "id",
            "value": SESSION_ID,
        },
    }
    tab = {
        "layout": {"Pane": 1},
        "panes": {"1": pane},
        "zoomed": False,
        "focused": 1,
        "root_pane": 1,
    }
    workspace = {
        "id": "w1T",
        "custom_name": "Resume probe",
        "identity_cwd": str(cwd),
        "public_pane_numbers": {"1": 1},
        "next_public_pane_number": 2,
        "public_tab_numbers": [1],
        "next_public_tab_number": 2,
        "tabs": [tab],
        "active_tab": 0,
    }
    session = {"version": 3, "workspaces": [workspace], "active": 0, "selected": 0}
    (config / "session.json").write_text(json.dumps(session), encoding="utf-8")


def check_restore(herdr, root):
    config = root / "herdr"
    config.mkdir()
    probe = make_probe(root)
    receipt = root / "argv.txt"
    socket = config / "fixture.sock"
    settings = config / "config.toml"
    settings.write_text(
        'onboarding = false\n[terminal]\ndefault_shell = "powershell.exe"\n'
        f"codex_executable = {json.dumps(str(probe))}\n"
        "[update]\nversion_check = false\nmanifest_check = false\n",
        encoding="utf-8",
    )
    write_session(config, root)
    env = {key: value for key, value in os.environ.items() if not key.startswith("HERDR_")}
    env.update(
        XDG_CONFIG_HOME=str(root),
        XDG_STATE_HOME=str(root),
        HERDR_CONFIG_PATH=str(settings),
        HERDR_SOCKET_PATH=str(socket),
        HERDR_TEST_ARGV=str(receipt),
    )
    options = dict(cwd=root, env=env, creationflags=subprocess.CREATE_NO_WINDOW)

    def cli(*args):
        return subprocess.run(
            [herdr, *args], **options, capture_output=True,
            encoding="utf-8", errors="replace", timeout=8,
        ).stdout

    log_path = root / "server.log"
    with log_path.open("w") as log:
        server = subprocess.Popen([herdr, "server"], **options, stdout=log, stderr=log)
        running = False
        view = ""
        try:
            deadline = time.monotonic() + 20
            while server.poll() is None and time.monotonic() < deadline:
                status = cli("status", "server")
                running = "status: running" in status and str(socket) in status
                if running:
                    view = cli("pane", "read", PANE_ID, "--source", "recent", "--lines", "50", "--raw")
                    if receipt.exists() or "ParserError" in view:
                        break
                time.sleep(0.25)
            if not receipt.exists():
                server_log = log_path.read_text(encoding="utf-8", errors="replace")
                raise AssertionError(
                    f"Configured Codex did not run. Pane output:\n{view}"
                    f"\nIsolated server output:\n{server_log}"
                )
            actual = receipt.read_text(encoding="utf-8").splitlines()
            assert actual == ["resume", SESSION_ID], actual
            print(f"PASS: received {actual}")
        finally:
            try:
                if running and server.poll() is None:
                    cli("server", "stop")
                server.wait(timeout=8)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=8)


if __name__ == "__main__":
    if sys.platform != "win32" or len(sys.argv) != 2:
        raise SystemExit("Usage on Windows: python scripts/test_windows_deferred_resume.py <herdr.exe>")
    with tempfile.TemporaryDirectory(prefix="herdr-deferred-resume-") as directory:
        check_restore(Path(sys.argv[1]).resolve(strict=True), Path(directory))
