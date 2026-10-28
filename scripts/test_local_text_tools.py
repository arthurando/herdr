"""Windows isolated old-server/new-client qualification; never attach to user sessions.

Requires pywinpty and psutil. Stops only a verified isolated server. No desktop input.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import re
import subprocess
import tempfile
import threading
import time

import pyte
import psutil
from winpty import PtyProcess

parser = argparse.ArgumentParser()
parser.add_argument('--stable', required=True)
parser.add_argument('--candidate', required=True)
parser.add_argument('--output', required=True)
args = parser.parse_args()
root = Path(tempfile.mkdtemp(prefix='herdr-local-tools-'))
env = {k: v for k, v in os.environ.items() if not k.startswith('HERDR_')}
env.update(APPDATA=str(root/'roaming'), LOCALAPPDATA=str(root/'local'))
Path(env['APPDATA']).mkdir(); Path(env['LOCALAPPDATA']).mkdir()
config = root/'config.toml'
config.write_text('onboarding = false\n[terminal]\ndefault_shell = "cmd.exe"\n[update]\nversion_check = false\nmanifest_check = false\n[ui.toast]\ndelivery = "off"\n[ui.sound]\nenabled = false\n', encoding='utf-8')
env['HERDR_CONFIG_PATH'] = str(config)
session = 'local-tools-proof'
ansi = re.compile(r'\x1b\](?:[^\x1b\x07]|\x1b(?!\\))*(?:\x07|\x1b\\)|\x1b\[[0-?]*[ -/]*[@-~]')
receipt = {'root': str(root), 'candidate': args.candidate, 'checks': {}}
clients = []
server = None
suspended = False

def cli(*parts):
    result = subprocess.run([args.stable, '--session', session, *parts], env=env, cwd=root,
                            capture_output=True, text=True, timeout=15,
                            creationflags=subprocess.CREATE_NO_WINDOW)
    if result.returncode: raise RuntimeError(result.stderr)
    return result.stdout

class Client:
    def __init__(self, exe, extra=()):
        self.proc = PtyProcess.spawn([exe, '--session', session, *extra], env=env,
                                     cwd=str(root), dimensions=(46,134), backend=0)
        self.queue = queue.Queue(); self.raw = []
        self.screen = pyte.Screen(134, 46); self.stream = pyte.Stream(self.screen)
        clients.append(self)
        def read():
            try:
                while True:
                    data = self.proc.read(65536)
                    self.raw.append(data); self.queue.put((time.perf_counter(), data))
            except Exception: pass
        threading.Thread(target=read, daemon=True).start()
    def wait(self, pattern, timeout=15):
        started = time.perf_counter(); text = ''
        while time.perf_counter()-started < timeout:
            try: stamp, data = self.queue.get(timeout=.1)
            except queue.Empty: continue
            self.stream.feed(data)
            text = '\n'.join(self.screen.display)
            if re.search(pattern, text): return (stamp-started)*1000
        raise AssertionError('Timed out: '+pattern)
    def drain(self):
        while not self.queue.empty():
            _, data = self.queue.get_nowait(); self.stream.feed(data)
    def action(self, text, pattern):
        self.drain(); started=time.perf_counter(); self.proc.write(text)
        self.wait(pattern)
        return (time.perf_counter()-started)*1000

    def visible_change(self, text, row=3, timeout=5):
        self.drain(); before=self.screen.display[row]; started=time.perf_counter()
        self.proc.write(text)
        while time.perf_counter()-started < timeout:
            try: stamp,data=self.queue.get(timeout=.1)
            except queue.Empty: continue
            self.stream.feed(data)
            after=self.screen.display[row]
            if before != after:
                return {'ms': (stamp-started)*1000, 'before': before.strip(), 'after': after.strip()}
        raise AssertionError('No visible viewport change while server paused')

try:
    old = Client(args.stable)
    old.wait(re.escape(str(root)+'>'))
    status = cli('status')
    assert 'status: running' in status and str(root) in status
    log = root/'roaming/herdr/sessions'/session/'herdr-server.log'
    pid = int(re.search(r'event="app.startup"[^\n]*pid=(\d+)',log.read_text()).group(1))
    server = psutil.Process(pid)
    assert Path(server.exe()).resolve() == Path(args.stable).resolve()
    old.proc.write('\x02q')
    deadline=time.perf_counter()+5
    while old.proc.isalive() and time.perf_counter()<deadline: time.sleep(.05)
    assert not old.proc.isalive(), 'Old client did not detach'
    client = Client(args.candidate, ('client',))
    client.wait(re.escape(str(root)+'>'))
    assert server.is_running()
    receipt['checks']['stable_server_preserved_on_attach'] = pid
    client.action('for /L %i in (1,1,80) do @echo HISTORY_%i\r', r'HISTORY_80')
    client.action('\x02U', 'Local text snapshot')
    client.action('r', 'Fetched terminal rows')
    server.suspend(); suspended=True
    # Only the explicit isolated server is paused. Local read must still react.
    receipt['checks']['cached_pageup_with_server_suspended'] = client.visible_change('\x1b[5~')
    receipt['checks']['cached_wheel_with_server_suspended'] = client.visible_change('\x1b[<64;60;20M')
    client.action('\x1b', 'HISTORY_80')
    client.action('\x02I', 'Local compose')
    receipt['checks']['local_typing_with_server_suspended_ms'] = client.action('LOCAL_DRAFT_ONLY', 'LOCAL_DRAFT_ONLY')
    server.resume(); suspended=False
    remote_text=cli('pane','read','w1:p1','--source','visible')
    assert 'LOCAL_DRAFT_ONLY' not in remote_text, 'Local draft leaked into remote pane'
    receipt['checks']['draft_stays_local'] = True
    # A single-line paste stays in the draft and does not execute the command.
    client.action('\x15echo __LOCAL_SENT_%RANDOM%', '__LOCAL_SENT_')
    client.action('\x1b[13;5u', 'Paste queued')
    client.action('\x1b[13;5u', 'Verify remote text first')
    remote_text=cli('pane','read','w1:p1','--source','visible')
    assert '__LOCAL_SENT_%RANDOM%' in remote_text
    assert not re.search(r'__LOCAL_SENT_\d+', remote_text)
    assert remote_text.count('__LOCAL_SENT_%RANDOM%') == 1
    receipt['checks']['paste_retained_repeat_guarded_no_extra_enter'] = True
    client.action('\x1b', '__LOCAL_SENT_')
    client.action('\r', r'__LOCAL_SENT_\d+')
    receipt['checks']['remote_enter_still_executes_normally'] = True
    receipt['passed'] = True
except Exception as error:
    receipt['passed'] = False; receipt['error'] = str(error)
    raise
finally:
    if suspended and server is not None: server.resume()
    for index, client in enumerate(clients):
        (root/f'client-{index}.ansi').write_text(''.join(client.raw),encoding='utf-8')
        try: client.proc.write('\x02q')
        except Exception: pass
    status=cli('status')
    if str(root) in status: cli('server','stop')
    Path(args.output).write_text(json.dumps(receipt,indent=2),encoding='utf-8')
    print(json.dumps(receipt,indent=2))
