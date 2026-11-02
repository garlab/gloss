"""Exercise interactive input, viewport graphics, reloads, and terminal cleanup.

Build first: python3 tests/pager_smoke.py target/debug/mdviewer
Uses a controlling Unix PTY; no GUI or third-party Python dependencies.
"""
import base64
import errno
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unicodedata
import unittest
import zlib

BINARY = Path(sys.argv.pop(1) if len(sys.argv) > 1 else "target/debug/mdviewer").resolve()
CSI = re.compile(rb"\x1b\[([?0-9;]*)([@-~])")


def terminal_settings(fd):
    settings = termios.tcgetattr(fd)
    # The kernel may set PENDIN while restoring canonical input; it is transient.
    settings[3] &= ~getattr(termios, "PENDIN", 0)
    return settings


class Screen:
    def __init__(self, columns, rows):
        self.columns, self.rows = columns, rows
        self.cells = {}
        self.row = self.column = 0
        self.pending = bytearray()
        self.images, self.placements = {}, {}
        self.header, self.payload = None, bytearray()
        self.uploads = self.frames = 0
        self.alt = False
        self.entered = self.left = False
        self.cropped = False
        self.join_next = False

    def text(self, footer=True):
        return "\n".join("".join(self.cells.get((r, c), " ") for c in range(self.columns)).rstrip()
                         for r in range(self.rows if footer else self.rows - 1))

    def graphic(self, packet):
        control, data = packet.split(b";", 1)
        fields = dict(pair.split(b"=", 1) for pair in control.split(b","))
        action = fields.get(b"a")
        if action == b"d":
            assert fields[b"d"] in (b"n", b"N"), "only owned image numbers may be deleted"
            number = int(fields[b"I"])
            self.placements.pop(number, None)
            if fields[b"d"] == b"N":
                self.images.pop(number, None)
        elif action == b"p":
            number = int(fields[b"I"])
            width, height = self.images[number]
            y, h, w, columns, rows = (int(fields[key]) for key in (b"y", b"h", b"w", b"c", b"r"))
            assert 0 <= y < height and 0 < h <= height - y
            assert w == width == columns * 10 and h == rows * 20
            assert 0 <= self.row and self.row + rows <= self.rows - 1, "image covers footer"
            assert 0 <= self.column and self.column + columns < self.columns
            self.cropped |= y > 0 or h < height
            self.placements[number] = (self.row, self.column, rows, columns)
        else:
            if action is not None:
                assert action == b"t" and self.header is None
                self.header = fields
            assert len(data) <= 4096 and len(data) % 4 == 0
            self.payload.extend(data)
            if fields[b"m"] == b"0":
                header = self.header
                rgba = base64.b64decode(self.payload, validate=True)
                if header.get(b"o") == b"z":
                    rgba = zlib.decompress(rgba)
                width, height = int(header[b"s"]), int(header[b"v"])
                assert len(rgba) == width * height * 4
                self.images[int(header[b"I"])] = (width, height)
                self.uploads += 1
                self.header, self.payload = None, bytearray()

    def feed(self, data):
        self.pending.extend(data)
        while self.pending:
            if self.pending[0] == 27:
                if len(self.pending) < 2:
                    return
                if self.pending[1] in (ord("_"), ord("]")):
                    end = self.pending.find(b"\x1b\\", 2)
                    if end < 0:
                        return
                    command = bytes(self.pending[:end + 2])
                    del self.pending[:end + 2]
                    if command.startswith(b"\x1b_G"):
                        self.graphic(command[3:-2])
                    else:
                        assert command.startswith(b"\x1b]8;;")
                    continue
                match = CSI.match(self.pending)
                if not match:
                    if len(self.pending) < 64:
                        return
                    raise AssertionError(f"unexpected escape {self.pending[:64]!r}")
                params, op = match.groups()
                del self.pending[:match.end()]
                if params.startswith(b"?"):
                    if params == b"?1049":
                        self.alt = op == b"h"
                        self.entered |= self.alt
                        self.left |= not self.alt
                    else:
                        assert params in (b"?25", b"?2004"), "mouse tracking must remain off"
                    continue
                values = [int(p or b"0") for p in params.split(b";")]
                if op == b"H":
                    self.row, self.column = values[0] - 1, (values[1] if len(values) > 1 else 1) - 1
                elif op == b"G":
                    self.column = values[0] - 1
                elif op == b"K":
                    assert values == [2]
                    self.cells = {pos: char for pos, char in self.cells.items() if pos[0] != self.row}
                else:
                    assert op == b"m", f"unexpected CSI {params!r} {op!r}"
                    if values == [0] and self.row == self.rows - 1 and self.column > 0:
                        self.frames += 1
                continue
            first = self.pending[0]
            count = 1 if first < 128 else 2 if first < 224 else 3 if first < 240 else 4
            if len(self.pending) < count:
                return
            ch = bytes(self.pending[:count]).decode()
            del self.pending[:count]
            assert unicodedata.category(ch) != "Cc", f"unexpected control {ch!r}"
            if ch == "\u200d":
                self.join_next = True
                continue
            width = 0 if self.join_next or unicodedata.category(ch) in ("Mn", "Me") else 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
            self.join_next = False
            assert 0 <= self.row < self.rows and self.column + width <= self.columns, "text outside viewport"
            if width:
                self.cells[self.row, self.column] = ch
                self.column += width


class Pager:
    def __init__(self, source="", args=(), file=None, columns=80, rows=12, env=None):
        self.master, self.slave = pty.openpty()
        self.saved = terminal_settings(self.slave)
        self.screen = Screen(columns, rows)
        self.resize(columns, rows)
        variables = dict(os.environ, TERM="xterm-ghostty", TERM_PROGRAM="ghostty")
        for key in ("TMUX", "STY", "ZELLIJ", "NO_COLOR", "COLORFGBG"):
            variables.pop(key, None)
        variables.update(env or {})

        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(1, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen([str(BINARY), "--pager", *args, str(file) if file else "-"],
                                        stdin=subprocess.PIPE, stdout=self.slave, stderr=subprocess.PIPE,
                                        preexec_fn=controlling_terminal, env=variables)
        self.output = bytearray()
        if file is None:
            self.process.stdin.write(source.encode())
        self.process.stdin.close()

    def resize(self, columns, rows):
        self.screen.columns, self.screen.rows = columns, rows
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, columns * 10, rows * 20))

    def pump(self, seconds=0.05):
        if select.select([self.master], [], [], seconds)[0]:
            try:
                data = os.read(self.master, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                return
            self.output.extend(data)
            self.screen.feed(data)

    def until(self, predicate, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.pump()
            if predicate():
                return
            if self.process.poll() is not None:
                if predicate():
                    return
                raise AssertionError(f"pager exited: {self.process.stderr.read().decode()}\n{self.screen.text()}")
        raise AssertionError(f"pager timed out\n{self.screen.text()}")

    def ready(self, text=None):
        self.until(lambda: self.screen.entered and self.screen.frames > 0
                   and re.search(r"\d+-\d+/\d+", self.screen.text().splitlines()[-1])
                   and " rendering" not in self.screen.text().splitlines()[-1]
                   and (text is None or text in self.screen.text(footer=False)))

    def keys(self, keys, text=None):
        frames = self.screen.frames
        os.write(self.master, keys.encode())
        self.until(lambda: self.screen.frames > frames and (text is None or text in self.screen.text()))

    def close(self, key="q", status=0):
        if self.process.poll() is None:
            os.write(self.master, key.encode())
        self.until(lambda: self.process.poll() is not None)
        for _ in range(4):
            self.pump(0.01)
        assert self.process.returncode == status, self.process.stderr.read().decode()
        try:
            restored = terminal_settings(self.slave)
        except termios.error as error:
            # Darwin revokes the PTY when its controlling session leader exits.
            # The suspend test checks termios restoration while that leader lives.
            if sys.platform != "darwin" or error.args[0] != errno.ENOTTY:
                raise
        else:
            assert restored == self.saved, "terminal attributes not restored"
        assert self.screen.left and not self.screen.alt
        assert not self.screen.images and not self.screen.placements

    def cleanup(self):
        os.close(self.master)
        os.close(self.slave)
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=5)
        self.process.stderr.close()


class PagerSmoke(unittest.TestCase):
    def pager(self, *args, **kwargs):
        pager = Pager(*args, **kwargs)
        self.addCleanup(pager.cleanup)
        return pager

    def test_navigation_search_unicode_and_piped_input(self):
        p = self.pager("\n\n".join(f"Line {i:03} café" for i in range(80)))
        p.ready("Line 000")
        p.keys("G", "Line 079")
        p.keys("g", "Line 000")
        p.keys("/Line 050 café\r", "Found: Line 050 café")
        self.assertIn("Line 050", p.screen.text(False))
        p.keys("?Line 010\r", "Found: Line 010")
        p.keys("n", "Found: Line 010")
        p.keys("/absent\r", "Not found: absent")
        p.keys("\x1b[200~qGG\x1b[201~j")
        p.close()

    def test_graphics_cropped_cached_and_freed(self):
        p = self.pager("Start\n\nBefore $\\frac{a}{b}$ after.\n\n" + "\n\n".join(f"Line {i}" for i in range(20)), rows=10)
        p.ready("Start")
        self.assertGreater(p.screen.uploads, 0)
        uploaded = p.screen.uploads
        p.keys("j")
        self.assertEqual(p.screen.uploads, uploaded, "scrolling a visible image must reuse its data")
        for _ in range(4):
            p.keys("j")
        self.assertTrue(p.screen.cropped)
        p.keys("G", "Line 19")
        self.assertFalse(p.screen.images)
        p.keys("g", "Start")
        self.assertGreater(p.screen.uploads, uploaded)
        p.close()

    def test_resize_reflows_without_losing_position(self):
        p = self.pager("\n\n".join(f"Section {i:03}: " + "words " * 15 for i in range(40)), columns=90)
        p.ready("Section 000")
        p.keys("/Section 020\r", "Found: Section 020")
        p.resize(42, 16)
        p.until(lambda: "Section 020" in p.screen.text(False) and "rendering" not in p.screen.text()
                and p.screen.row == 15)
        p.close()

    def test_watch_atomic_save_errors_and_recovery(self):
        with tempfile.TemporaryDirectory() as root:
            file = Path(root) / "sample.md"
            file.write_text("Original\n\n" + "\n\n".join(f"Line {i:03}" for i in range(50)))
            p = self.pager(file=file, args=["--watch"], columns=120)
            p.ready("Original")
            p.keys("/Line 025\r", "Found: Line 025")
            updated = file.with_suffix(".new")
            updated.write_text("Introduction\n\n" * 5 + file.read_text())
            updated.replace(file)
            p.until(lambda: "Found:" not in p.screen.text() and "rendering" not in p.screen.text())
            self.assertIn("Line 025", p.screen.text(False))
            file.unlink()
            p.until(lambda: "Reload failed" in p.screen.text())
            self.assertIn("Line 025", p.screen.text(False))
            file.write_bytes(b"\xff")
            p.until(lambda: "UTF-8" in p.screen.text())
            file.write_text("Recovered\n")
            p.ready("Recovered")
            p.close()

    def test_manual_reload_and_plain_terminal(self):
        with tempfile.TemporaryDirectory() as root:
            file = Path(root) / "sample.md"
            file.write_text("Old $x^2$\n")
            p = self.pager(file=file, env={"TERM": "xterm-256color", "TERM_PROGRAM": "", "NO_COLOR": "1"})
            p.ready("Old $x^2$")
            file.write_text("New text\n")
            p.keys("r", "New text")
            self.assertEqual(p.screen.uploads, 0)
            self.assertNotIn(b"38;5;", p.output)
            p.close()

    def test_signal_and_ctrl_c_restore_the_terminal(self):
        for sig in (None, signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            with self.subTest(signal=sig):
                p = self.pager("Hello $x$\n")
                p.ready("Hello")
                if sig is not None:
                    p.process.send_signal(sig)
                p.close(key="\x03" if sig is None else "")

    def test_suspend_and_resume(self):
        p = self.pager("Suspend $x$\n")
        p.ready("Suspend")
        os.write(p.master, b"\x1a")
        p.until(lambda: p.screen.left)
        self.assertEqual(terminal_settings(p.slave), p.saved)
        p.screen.left = False
        p.process.send_signal(signal.SIGCONT)
        p.until(lambda: p.screen.alt and bool(p.screen.placements))
        p.close()

    def test_existing_rich_fixtures(self):
        for name in ("style", "code", "tables", "math", "extensions", "mermaid"):
            with self.subTest(fixture=name):
                p = self.pager(file=Path("tests/fixtures") / f"{name}.md")
                p.ready()
                p.keys("G")
                p.keys("g")
                p.close()

    def test_watch_rejects_fifo_and_recovers_after_oversized_input(self):
        with tempfile.TemporaryDirectory() as root:
            file = Path(root) / "sample.md"
            file.write_text("Original\n")
            p = self.pager(file=file, args=["--watch"], columns=120)
            p.ready("Original")
            file.unlink()
            os.mkfifo(file)
            p.until(lambda: "regular file" in p.screen.text())
            self.assertIn("Original", p.screen.text(False))
            file.unlink()
            with file.open("wb") as oversized:
                oversized.truncate(64 * 1024 * 1024 + 1)
            p.until(lambda: "64 MiB" in p.screen.text())
            file.write_text("Recovered\n")
            p.ready("Recovered")
            p.close()

    def test_quit_cancels_an_inflight_render(self):
        p = self.pager("A paragraph with **styles** and `code`.\n\n" * 100_000)
        p.until(lambda: p.screen.entered)
        start = time.monotonic()
        p.close()
        self.assertLess(time.monotonic() - start, 3)


if __name__ == "__main__":
    unittest.main()
