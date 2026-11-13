"""Capture an isolated Windows client and verify its machine (+) control.

Requires pywinpty, pyte and Pillow. No desktop input is injected.
Usage: python scripts/test_machine_workspace_ui.py EXE OUTPUT_DIR [--baseline]
"""
import sys, os, time, json, threading, subprocess, uuid
from pathlib import Path
from winpty import PtyProcess
import pyte
from PIL import Image, ImageDraw, ImageFont
exe = str(Path(sys.argv[1]).resolve())
root = Path(sys.argv[2]).resolve()
mode = 'monitor' if '--monitor-snapshot' in sys.argv[3:] else ('before' if '--baseline' in sys.argv[3:] else 'after')
root.mkdir(parents=True, exist_ok=False)
env = os.environ.copy()
for key in list(env):
    if key.startswith('HERDR_'):
        del env[key]
env.update(XDG_CONFIG_HOME=str(root / 'config'), XDG_STATE_HOME=str(root / 'state'), HERDR_SESSION='buttons-test-' + uuid.uuid4().hex[:12])
config = root / 'test.toml'
config.write_text('onboarding = false\n[terminal]\ndefault_shell = "cmd.exe"\n[ui]\nmouse_capture = true\nsidebar_start_collapsed = false\nprompt_new_workspace_name = false\n')
env['HERDR_CONFIG_PATH'] = str(config)
monitor_file = None
if mode == 'monitor':
    source = Path(sys.argv[sys.argv.index('--monitor-snapshot') + 1])
    monitor_file = root / 'monitor.json'
    monitor_file.write_bytes(source.read_bytes())
    env['HERDR_MONITOR_FILE'] = str(monitor_file)

def cli(*args):
    r = subprocess.run([exe, *args], env=env, cwd=root, capture_output=True, text=True, encoding='utf-8', creationflags=subprocess.CREATE_NO_WINDOW, timeout=15)
    if r.returncode:
        raise RuntimeError(str(args) + ': ' + r.stdout + r.stderr)
    return r.stdout
columns, rows = (100, 42) if mode == 'monitor' else (100, 28)
screen = pyte.Screen(columns, rows)
stream = pyte.Stream(screen)
lock = threading.Lock()
raw = []
proc = None
server = None
try:
    log = (root / 'server.log').open('w')
    server = subprocess.Popen([exe, 'server'], env=env, cwd=root, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
    for i in range(40):
        try:
            before = cli('workspace', 'list')
            break
        except Exception:
            time.sleep(0.25)
    else:
        raise RuntimeError('fixture server did not start')
    (root / 'initial-workspaces.json').write_text(before)
    proc = PtyProcess.spawn([exe, 'client'], env=env, cwd=str(root), dimensions=(rows, columns))

    def read():
        while proc.isalive():
            try:
                data = proc.read(65536)
            except Exception:
                break
            raw.append(data)
            with lock:
                stream.feed(data)
    thread = threading.Thread(target=read, daemon=True)
    thread.start()
    time.sleep(5)

    def capture(name):
        with lock:
            lines = screen.display[:]
            img = Image.new('RGB', (columns * 10, rows * 20), (25, 25, 35))
            draw = ImageDraw.Draw(img)
            font = ImageFont.truetype(str(Path(os.environ['WINDIR']) / 'Fonts' / 'consola.ttf'), 17)
            colours = {'default': '#dddddd', 'black': '#000000', 'white': '#ffffff', 'red': '#cd3131', 'green': '#0dbc79', 'yellow': '#e5e510', 'blue': '#2472c8', 'magenta': '#bc3fbc', 'cyan': '#11a8cd'}
            for y in range(rows):
                for x in range(columns):
                    c = screen.buffer[y][x]

                    def colour(v, default):
                        return colours.get(v, '#' + v if len(v) == 6 else default)
                    bg = colour(c.bg, '#191923') if c.bg != 'default' else '#191923'
                    fg = colour(c.fg, '#dddddd')
                    draw.rectangle((x * 10, y * 20, x * 10 + 9, y * 20 + 19), fill=bg)
                    draw.text((x * 10, y * 20), c.data, font=font, fill=fg)
            img.save(root / (name + '.png'))
            (root / (name + '.txt')).write_text('\n'.join(lines), encoding='utf-8')
        return lines
    lines = capture('sidebar')
    machine_label = 'Local' if mode == 'before' else 'Omen'
    assert any(machine_label in line for line in lines), machine_label + ' machine row is absent'
    if mode == 'after':
        before = cli('workspace', 'list')
        machine_y = next(y for y, line in enumerate(lines) if 'Omen' in line and '(+)' in line)
        def disclosure():
            proc.write(f'\x1b[<0;2;{machine_y + 1}M\x1b[<0;2;{machine_y + 1}m')
            time.sleep(1)
        disclosure()
        hidden = capture('machine-collapsed')
        assert any('> Omen' in line for line in hidden), 'machine did not collapse'
        assert cli('workspace', 'list') == before, 'collapse changed runtime workspaces'
        disclosure()
        lines = capture('machine-expanded')
        assert any('v Omen' in line for line in lines), 'machine did not expand'
        assert cli('workspace', 'list') == before, 'expand changed runtime workspaces'
    if mode == 'monitor':
        assert not any('Sid VM' in line for line in lines), 'monitor should start collapsed'
        x, y = next((line.index('Monitor'), y) for y, line in enumerate(lines) if 'Monitor' in line)
        def click_monitor():
            proc.write(f'\x1b[<0;{x + 1};{y + 1}M\x1b[<0;{x + 1};{y + 1}m')
            time.sleep(2.5)
        click_monitor()
        expanded = capture('monitor-expanded')
        for label in ['Sid VM', 'VPS Helsinki', 'VPS Singapore', 'Temporary compute']:
            assert any(label in line for line in expanded), label + ' absent'
        assert any('RAM' in line for line in expanded), 'real RAM bars absent'
        assert 'menu' in expanded[y] and 'Monitor' in expanded[y], 'footer lost'
        click_monitor()
        collapsed = capture('monitor-collapsed')
        assert not any('VPS Helsinki' in line for line in collapsed), 'collapse failed'
        click_monitor()
        monitor_file.write_text('{"version":999}', encoding='utf-8')
        time.sleep(3)
        invalid = capture('monitor-invalid')
        assert any('Invalid monitor data' in line for line in invalid), 'invalid source not visible'
        assert not any('RAM' in line for line in invalid), 'invalid source retained healthy bars'
    if mode == 'before':
        assert any('[+] New' in line or ('Local' in line and '(+)' in line) for line in lines), 'Baseline session control is absent'
    if mode == 'after':
        matches = [(line.index('(+)') + 1, y) for y, line in enumerate(lines) if '(+)' in line and 'Omen' in line]
        assert matches, 'Omen (+) absent: ' + repr(lines)
        assert not any(('[+] New' in line for line in lines))
        x, y = matches[0]
        before = cli('workspace', 'list')
        proc.write(f'\x1b[<0;{x + 1};{y + 1}M\x1b[<0;{x + 1};{y + 1}m')
        time.sleep(1)
        prompted = cli('workspace', 'list')
        assert prompted == before, 'the naming popup must not create a session'
        popup = capture('name-popup')
        assert any('new session' in line for line in popup), 'name popup is absent'
        proc.write('\x1b')
        time.sleep(.5)
        assert cli('workspace', 'list') == before, 'cancel must not create a session'
        proc.write(f'\x1b[<0;{x + 1};{y + 1}M\x1b[<0;{x + 1};{y + 1}m')
        time.sleep(.5)
        proc.write('Machine session test\r')
        time.sleep(2)
        after = cli('workspace', 'list')
        (root / 'before-click.json').write_text(before)
        (root / 'after-click.json').write_text(after)
        old_ids = {item['workspace_id'] for item in json.loads(before)['result']['workspaces']}
        new_ids = {item['workspace_id'] for item in json.loads(after)['result']['workspaces']}
        assert old_ids < new_ids and len(new_ids - old_ids) == 1, 'click must add exactly one workspace'
        added = [item for item in json.loads(after)['result']['workspaces'] if item['workspace_id'] not in old_ids]
        assert added[0]['label'] == 'Machine session test', 'confirmed name was not saved'
        created = capture('created-session')
        assert any('\u256d' in line for line in created), 'current session border is absent'
    (root / 'raw.ansi').write_text(''.join(raw), encoding='utf-8')
    print(json.dumps({'mode': mode, 'result': 'PASS', 'artifact': str(root)}))
finally:
    if proc:
        try:
            proc.close(force=True)
        except Exception:
            pass
    if server:
        try:
            cli('server', 'stop')
        except Exception:
            server.terminate()
        try:
            server.wait(timeout=5)
        except Exception:
            server.kill()
