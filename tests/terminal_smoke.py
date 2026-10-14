#!/usr/bin/env python3
"""Exercise the picker and zsh widget through real pseudo-terminals."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import termios
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('--bin', required=True)
args = parser.parse_args()
binary = Path(args.bin).resolve()


def terminal():
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ,
                struct.pack('HHHH', 24, 120, 0, 0))
    return master, slave


def read_until(master, needle, timeout=10, cursor_row=1):
    transcript = b''
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if select.select([master], [], [], .1)[0]:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                break
            if not chunk:
                break
            transcript += chunk
            if b'\x1b[6n' in chunk:
                os.write(master, f'\x1b[{cursor_row};1R'.encode())
            if needle in transcript:
                return transcript
    raise AssertionError(f'Terminal did not show {needle!r}: {transcript[-1500:]!r}')


with tempfile.TemporaryDirectory() as temp:
    root = Path(temp).resolve()
    data = root / 'data'
    data.mkdir()
    alpha = root / 'alpha project'
    beta = root / 'beta'
    alpha.mkdir()
    beta.mkdir()
    subprocess.run(['git', '-C', str(alpha), 'init', '-b', 'feature/picker'], check=True, capture_output=True)
    entries = [(str(alpha), 20.0), (str(beta), 10.0)]
    contents = struct.pack('<IQ', 3, len(entries))
    for path, score in entries:
        encoded = path.encode()
        contents += struct.pack('<Q', len(encoded)) + encoded + struct.pack('<dQ', score, int(time.time()) - 720)
    (data / 'db.zo').write_bytes(contents)
    env = dict(os.environ, _ZO_DATA_DIR=str(data), TERM='xterm-256color', COLUMNS='120', LINES='24', PATH=f'{binary.parent}:{os.environ["PATH"]}')

    env.pop('NO_COLOR', None)
    env.pop('ZORT_LIMIT', None)

    for keys, expected in [(b'\r', alpha), (b'\x7f\r', alpha), (b'\x15\r', alpha), (b'\x10\r', beta), (b'beta\r', beta), (b'\x1b', None)]:
        master, slave = terminal()
        proc = subprocess.Popen([str(binary), 'pick'], stdin=slave, stderr=slave, stdout=subprocess.PIPE, env=env)
        os.close(slave)
        try:
            screen = read_until(master, b'Search', cursor_row=8)
            assert b'BRANCH' in screen and b'LAST ACCESSED' in screen and b'feature/picker' in screen
            assert b'12 minutes ago' in screen
            assert b'\x1b[?1049h' not in screen, 'Picker still uses full screen'
            assert b'\x1b[2J' not in screen, 'Picker clears terminal history'
            assert re.search(rb'\x1b\[14;1H[^\r\n]*Search', screen), 'Picker is not compact'
            assert screen.index(b'beta') < screen.index(b'alpha project'), 'Best result must be last'
            for ansi, indexed in [(b'32', b'2'), (b'34', b'4'), (b'35', b'5')]:
                pattern = rb'\x1b\[(?:' + ansi + rb'|38;5;' + indexed + rb')(?:;[0-9]+)*m'
                assert re.search(pattern, screen), f'Missing foreground color {ansi!r}'
            os.write(master, keys)
            output, _ = proc.communicate(timeout=10)
            assert proc.returncode == (0 if expected else 1), proc.returncode
            assert output.decode().rstrip('\n') == (str(expected) if expected else ''), output
            if expected is None:
                tail = b''
                while select.select([master], [], [], .1)[0]:
                    try:
                        chunk = os.read(master, 65536)
                        if not chunk:
                            break
                        tail += chunk
                    except OSError:
                        break
                assert b'Error:' not in tail, tail
        finally:
            if proc.poll() is None:
                proc.kill()
            os.close(master)
    print('Picker: rendering, timestamps, navigation, search, selection, cancellation passed')

    # More entries than the default cap: hidden entries must remain searchable.
    for i in range(12):
        directory = root / f'extra_{i:02}'
        directory.mkdir()
        subprocess.run(['zoxide', 'add', str(directory)], env=env, check=True)
    hidden = json.loads(subprocess.check_output([str(binary), 'list', '--json'], env=env))[-1]
    for options, shown in [([], 10), (['--limit', '5'], 5)]:
        master, slave = terminal()
        proc = subprocess.Popen([str(binary), 'pick', *options], stdin=slave, stderr=slave, stdout=subprocess.PIPE, env=env)
        os.close(slave)
        try:
            screen = read_until(master, b'Search')
            assert f'{shown} shown'.encode() in screen, 'Wrong result limit'
            assert len(re.findall(rb'\x1b\[38;5;2(?:;49)?m[a-zA-Z_]', screen)) == shown, 'Wrong rendered row count'
            assert hidden['name'].encode() not in screen
            os.write(master, hidden['name'].encode() + b'\r')
            deadline = time.monotonic() + 10
            while proc.poll() is None and time.monotonic() < deadline:
                if select.select([master], [], [], .1)[0]:
                    try:
                        chunk = os.read(master, 65536)
                    except OSError:
                        break
                    if not chunk:
                        break
            output, _ = proc.communicate(timeout=2)
            assert proc.returncode == 0
            assert output.decode().rstrip('\n') == hidden['path'], output
        finally:
            if proc.poll() is None:
                proc.kill()
            os.close(master)
    print('Picker: result limits and search across full history passed')


    # A partially typed command must survive the widget and run in the selected project.
    master, slave = terminal()
    def controlling_terminal():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    proc = subprocess.Popen(['zsh', '-f', '-i'], stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=controlling_terminal)
    os.close(slave)
    try:
        read_until(master, b'\x1b[?2004h')
        os.write(master, b"PS1='ZORT> '; unset RPS1\n")
        read_until(master, b'\x1b[?2004h')
        init = subprocess.check_output([str(binary), 'init', 'zsh'], env=env)
        script = root / 'init.zsh'
        script.write_bytes(init)
        os.write(master, f"source '{script}'; print READY\n".encode())
        read_until(master, b'\x1b[?2004h')
        os.write(master, b'print -r -- "BUFFER_OK:$PWD"')
        os.write(master, b'\x07')
        screen = read_until(master, b'Search')
        os.write(master, b'\x10\r')
        returned = read_until(master, b'ZORT> ')
        if b'Error:' in returned:
            raise AssertionError(returned)
        os.write(master, b'\r')
        read_until(master, f'BUFFER_OK:{beta}\r\n'.encode())
        print('Zsh: Ctrl+G switches directory and preserves the command buffer')
    finally:
        os.close(master)
        proc.kill()
        proc.wait(timeout=5)
