"""Exercise the real CLI through Unix pseudo-terminals, without a GUI.

Build first, then run: python3 tests/terminal_smoke.py target/debug/gloss
These checks validate output policy and cell placement, not emulator appearance.
"""

import base64
import errno
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import sys
import termios
import tempfile
import time
import unittest
import unicodedata
import zlib


BINARY = Path(sys.argv.pop(1)).resolve() if len(sys.argv) > 1 else Path("target/debug/gloss").resolve()
SAMPLE = "Above.\n\nBefore $\\frac{a}{b}$, between $x^2$ and $\\sqrt{1+x}$ after.\n\nBelow.\n"
GRAPHICS = re.compile(rb"\x1b_G(.*?)\x1b\\", re.DOTALL)
OSC = re.compile(rb"\x1b\]8;;[^\x1b\x07]*(?:\x1b\\|\x07)")
CSI = re.compile(rb"\x1b\[([0-9;]*)([A-Za-z])")


def terminal(source=SAMPLE, args=(), columns=80, rows=24, variables=None, file=None, cwd=None):
    master, slave = pty.openpty()
    # Capture renderer bytes without platform-dependent newline conversion.
    settings = termios.tcgetattr(slave)
    settings[1] &= ~termios.OPOST
    termios.tcsetattr(slave, termios.TCSANOW, settings)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, columns * 10, rows * 20))
    env = dict(os.environ, TERM="xterm-ghostty", TERM_PROGRAM="ghostty")
    for name in ("TMUX", "STY", "ZELLIJ", "NO_COLOR", "COLORFGBG"):
        env.pop(name, None)
    env.update(variables or {})
    process = subprocess.Popen(
        [str(BINARY), *args, str(file) if file else "-"], stdin=subprocess.PIPE, stdout=slave, stderr=subprocess.PIPE, env=env, cwd=cwd
    )
    os.close(slave)
    output = bytearray()
    try:
        if file is None:
            process.stdin.write(source.encode())
        process.stdin.close()
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.1)
            if ready:
                try:
                    chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                if not chunk:
                    break
                output.extend(chunk)
            elif process.poll() is not None:
                break
        else:
            raise TimeoutError("terminal output timed out")
        status = process.wait(timeout=5)
        error = process.stderr.read().decode()
        if status:
            raise AssertionError(f"CLI exited with {status}: {error}")
        return bytes(output), error
    finally:
        os.close(master)
        if process.poll() is None:
            process.kill()
            process.wait()
        process.stderr.close()


def plain(output):
    return CSI.sub(b"", OSC.sub(b"", GRAPHICS.sub(b"", output))).replace(b"\r", b"").decode()


def decoded_graphics(output):
    header, payload = None, bytearray()
    for match in GRAPHICS.finditer(output):
        control, chunk = match.group(1).split(b";", 1)
        fields = dict(pair.split(b"=", 1) for pair in control.split(b","))
        if b"a" in fields:
            header = fields
        payload.extend(chunk)
        if fields[b"m"] == b"0":
            rgba = base64.b64decode(payload, validate=True)
            if header.get(b"o") == b"z":
                rgba = zlib.decompress(rgba)
            width, height = int(header[b"s"]), int(header[b"v"])
            assert len(rgba) == width * height * 4
            yield width, height, rgba
            header, payload = None, bytearray()
    assert header is None


def attributes_at(output, needle=None):
    """Interpret SGR state at visible text, independent of escape encoding."""
    end = len(output) if needle is None else output.index(needle)
    state = dict(foreground=None, background=None, bold=False, italic=False,
                 underline=False, strike=False)
    flags = {1: ("bold", True), 22: ("bold", False),
             3: ("italic", True), 23: ("italic", False),
             4: ("underline", True), 24: ("underline", False),
             9: ("strike", True), 29: ("strike", False)}
    for match in CSI.finditer(output, 0, end):
        if match.group(2) != b"m":
            continue
        codes = iter(int(code or b"0") for code in match.group(1).split(b";"))
        for code in codes:
            if code == 0:
                state.update(foreground=None, background=None, bold=False,
                             italic=False, underline=False, strike=False)
            elif code in flags:
                key, value = flags[code]
                state[key] = value
            elif code in (38, 48):
                assert next(codes) == 5
                state["foreground" if code == 38 else "background"] = next(codes)
            elif code in (39, 49):
                state["foreground" if code == 39 else "background"] = None
            else:
                raise AssertionError(f"unexpected SGR code: {code}")
    return state


def screen(output, columns, return_cells=False, cell_width=10, cell_height=20):
    """Interpret single-cell test text and the emitted cursor/graphics subset.

    Rows are absolute scrollback rows. This checks that all image rectangles fit
    the viewport width and that no ordinary text cell is covered by an image.
    """
    output = OSC.sub(b"", output)
    row = column = offset = 0
    text_cells = []
    glyphs = {}
    images = []
    header = None
    payload = bytearray()
    while offset < len(output):
        if output.startswith(b"\x1b_G", offset):
            match = GRAPHICS.match(output, offset)
            assert match, "unfinished graphics command"
            control, chunk = match.group(1).split(b";", 1)
            fields = dict(pair.split(b"=", 1) for pair in control.split(b","))
            assert len(chunk) <= 4096 and len(chunk) % 4 == 0
            if b"a" in fields:
                assert header is None
                header = fields
            payload.extend(chunk)
            if fields[b"m"] == b"0":
                assert header is not None
                rgba = base64.b64decode(payload, validate=True)
                if header.get(b"o") == b"z":
                    rgba = zlib.decompress(rgba)
                width, height = int(header[b"s"]), int(header[b"v"])
                assert len(rgba) == width * height * 4
                if b"c" in header or b"r" in header:
                    image_columns, image_rows = int(header[b"c"]), int(header[b"r"])
                    assert width == image_columns * cell_width and height == image_rows * cell_height
                else:
                    y = int(header[b"Y"])
                    assert 0 <= y < cell_height
                    image_columns = (width + cell_width - 1) // cell_width
                    image_rows = (y + height + cell_height - 1) // cell_height
                assert header[b"C"] == b"1" and header[b"q"] == b"2"
                assert 0 <= column and column + image_columns < columns
                assert any(rgba[index] for index in range(3, len(rgba), 4))
                images.append((column, row, image_columns, image_rows))
                header, payload = None, bytearray()
            offset = match.end()
            continue
        if output.startswith(b"\x1b[", offset):
            match = CSI.match(output, offset)
            assert match, "unrecognized cursor sequence"
            amount = int(match.group(1) or b"1") if match.group(2) != b"m" else 0
            operation = match.group(2)
            if operation == b"A":
                row -= amount
            elif operation == b"B":
                row += amount
            elif operation == b"C":
                column += amount
            else:
                assert operation == b"m"
            assert row >= 0
            offset = match.end()
            continue
        byte = output[offset]
        if byte == 13:
            column = 0
        elif byte == 10:
            # terminal() captures raw bytes; model the terminal's usual ONLCR.
            row += 1
            column = 0
        else:
            if byte >= 128:
                size = 2 if byte < 0xe0 else 3 if byte < 0xf0 else 4
                character = output[offset:offset + size].decode()
                assert unicodedata.category(character) not in ('Cc', 'Mn', 'Me')
                assert unicodedata.east_asian_width(character) not in ('W', 'F'), 'screen fixtures use single-cell text'
                offset += size - 1
            else:
                assert 32 <= byte < 127, "unexpected control character"
                character = chr(byte)
            assert column < columns, "text unexpectedly relied on terminal autowrap"
            if byte != 32:
                text_cells.append((column, row))
                glyphs[column, row] = character
            column += 1
        offset += 1
    assert header is None
    for x, y in text_cells:
        for left, top, width, height in images:
            assert not (left <= x < left + width and top <= y < top + height), "text overlaps equation"
    return (images, glyphs) if return_cells else images


class TerminalSmoke(unittest.TestCase):
    def test_markdown_extensions_render_and_keep_sources(self):
        source = "> [!WARNING]\n> Be careful.\n\n<details>\n<summary>More</summary>\n\nBefore \\(x^2\\) after.\n\n</details>\n"
        output, error = terminal(source, args=["--show-math-source"])
        self.assertFalse(error)
        visible = plain(output)
        self.assertIn("Warning", visible)
        self.assertIn("More", visible)
        self.assertNotIn("<details>", visible)
        self.assertIn(r"TeX: \(x^2\)", visible)
        self.assertEqual(len(screen(output, 80)), 1)

    def test_links_are_scoped_and_modes_respect_fallbacks(self):
        source = "[A link](https://example.com) after. [bad](javascript:alert)"
        output, error = terminal(source)
        self.assertFalse(error)
        self.assertIn(b"\x1b]8;;https://example.com\x1b\\", output)
        self.assertNotIn(b"\x1b]8;;javascript:", output)
        screen(output, 80)
        for args, variables in [(["--links", "off"], {}), ([], {"TERM": "dumb"}),
                                ([], {"TMUX": "test"}), (["--output", "text"], {})]:
            output, error = terminal(source, args=args, variables=variables)
            self.assertFalse(error)
            self.assertNotIn(b"\x1b]8;", output)
            self.assertIn("https://example.com", plain(output))
        output, _ = terminal(source, args=["--links", "always"], variables={"TERM": "xterm", "TERM_PROGRAM": "unknown"})
        self.assertIn(b"\x1b]8;", output)

    def test_file_rendering_leaves_pending_stdin_untouched(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.md"
            path.write_text("Hello.\n")
            master, slave = pty.openpty()
            read_fd, write_fd = os.pipe()
            try:
                sentinel = b"pending shell input\n"
                os.write(write_fd, sentinel)
                os.close(write_fd)
                result = subprocess.run([str(BINARY), str(path)], stdin=read_fd, stdout=slave,
                    stderr=subprocess.PIPE, env=dict(os.environ, TERM="xterm-ghostty", TERM_PROGRAM="ghostty"), timeout=5)
                self.assertEqual(result.returncode, 0)
                self.assertFalse(result.stderr)
                self.assertEqual(os.read(read_fd, 1024), sentinel)
            finally:
                os.close(read_fd)
                os.close(slave)
                os.close(master)

    def test_theme_hints_and_explicit_theme_precedence(self):
        light, _ = terminal("Paragraph.\n", variables={"COLORFGBG": "0;15"})
        dark, _ = terminal("Paragraph.\n", args=["--theme", "dark"], variables={"COLORFGBG": "0;15"})
        explicit, _ = terminal("Paragraph.\n", args=["--theme", "light"])
        alias, _ = terminal("Paragraph.\n", args=["--light"])
        self.assertEqual(light, explicit)
        self.assertEqual(alias, explicit)
        self.assertEqual(plain(dark), plain(light))
        self.assertNotEqual(attributes_at(light, b"Paragraph"), attributes_at(dark, b"Paragraph"))

    def test_geometry_environment_fills_missing_dimensions_without_overriding_ioctl(self):
        source = "word " * 50 + "\n"
        fallback, error = terminal(source, args=["--width", "0"], columns=0, rows=0,
            variables={"COLUMNS": "50", "LINES": "20"})
        self.assertFalse(error)
        self.assertTrue(all(len(line) < 50 for line in plain(fallback).splitlines()))
        reported, error = terminal(source, args=["--width", "0"], columns=80, rows=24,
            variables={"COLUMNS": "12", "LINES": "8"})
        self.assertFalse(error)
        self.assertTrue(any(len(line) > 50 for line in plain(reported).splitlines()))

    def test_cell_size_override_uses_matching_pixel_rectangles(self):
        for size in ("8x16", "12x28", "20x40"):
            width, height = map(int, size.split("x"))
            output, error = terminal(args=["--cell-size", size])
            self.assertFalse(error)
            self.assertEqual(len(screen(output, 80, cell_width=width, cell_height=height)), 3)

    def test_auto_ghostty_renders_inline_math_without_replacing_prose(self):
        output, error = terminal()
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 3)
        self.assertIn("Before ", plain(output))
        self.assertIn(" after.", plain(output))
        self.assertIn("Below.", plain(output))
        self.assertNotIn(r"\frac{a}{b}", plain(output))

    def test_narrow_terminal_wraps_without_covering_text(self):
        output, error = terminal(columns=20, rows=12)
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 20)), 3)

    def test_tall_inline_formulas_leave_surrounding_lines_clear(self):
        source = "Above.\n\nA $\\displaystyle\\frac{1}{1+\\frac{1}{x}}$ and $\\displaystyle\\sum_{i=0}^{100}x_i$ here.\n\nBelow.\n"
        output, error = terminal(source, columns=40, rows=10)
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 40)), 2)

    def test_tables_align_headers_and_preserve_colors_without_color(self):
        source = "| Name | Center | Score |\n| :--- | :---: | ---: |\n| A | B | 42 |\n"
        dark, error = terminal(source, args=("--math", "tex"))
        light, _ = terminal(source, args=("--math", "tex", "--light"))
        uncolored, _ = terminal(source, args=("--math", "tex"), variables={"NO_COLOR": "1"})
        self.assertFalse(error)
        self.assertEqual(plain(dark), plain(light))
        self.assertEqual(plain(dark), plain(uncolored))
        self.assertNotIn(b"\x1b", uncolored)
        self.assertEqual(attributes_at(dark, b"Name")["foreground"], 81)
        self.assertEqual(attributes_at(light, b"Name")["foreground"], 25)
        self.assertTrue(attributes_at(dark, b"Name")["bold"])
        self.assertFalse(attributes_at(dark, b"42")["bold"])
        self.assertIn("A    │   B    │    42", plain(dark))
        screen(dark, 80)

    def test_table_equations_share_rows_without_covering_other_cells(self):
        source = "Above.\n\n| Left | Right |\n| :---: | ---: |\n| $\\frac{a}{b}$ | $\\sqrt{x}$ |\n| Text | Tail |\n\nBelow.\n"
        for columns in (24, 80):
            with self.subTest(columns=columns):
                output, error = terminal(source, columns=columns)
                self.assertFalse(error)
                images = screen(output, columns)
                self.assertEqual(len(images), 2)
                self.assertLessEqual(images[0][0] + images[0][2], images[1][0])
                self.assertIn("Below.", plain(output))
        output, error = terminal(source, args=("--show-math-source",))
        self.assertFalse(error)
        self.assertIn(r"$\frac{a}{b}$", plain(output))
        self.assertIn(r"$\sqrt{x}$", plain(output))
        self.assertEqual(len(screen(output, 80)), 2)

    def test_table_borders_span_every_reserved_equation_row(self):
        body = "| Left | Right |\n| --- | --- |\n| $\\displaystyle\\frac{a+b}{c}$ | $\\sqrt{x}$ |\n"
        for prefix in ("", "> "):
            for no_color in (False, True):
                with self.subTest(prefix=prefix, no_color=no_color):
                    source = "Above.\n\n" + "\n".join(prefix + line for line in body.splitlines()) + "\n\nBelow.\n"
                    output, error = terminal(source, rows=8, variables={"NO_COLOR": "1"} if no_color else {})
                    self.assertFalse(error)
                    images, cells = screen(output, 80, return_cells=True)
                    self.assertEqual(len(images), 2)
                    top = min(image[1] for image in images)
                    bottom = max(image[1] + image[3] for image in images)
                    self.assertGreater(bottom - top, 1)
                    columns = {column for (column, row), char in cells.items() if char == "│" and top <= row < bottom}
                    self.assertEqual(len(columns), 2 if prefix else 1)
                    for column in columns:
                        for row in range(top, bottom):
                            self.assertEqual(cells.get((column, row)), "│", (column, row))
                    self.assertIn("Below.", plain(output))

    def test_table_fallbacks_keep_source_and_do_not_block_later_graphics(self):
        source = "| First | Second | Third |\n| --- | --- | --- |\n| $x$ | beta | gamma |\n"
        narrow, error = terminal(source, columns=14)
        self.assertFalse(error)
        self.assertFalse(GRAPHICS.search(narrow))
        self.assertIn("1: $x$", plain(narrow))
        self.assertIn("3: gamma", plain(narrow))
        screen(narrow, 14)
        source = "| Many |\n| --- |\n| " + "$x$ " * 33 + " |\n| $y$ |\n"
        output, error = terminal(source)
        self.assertFalse(error)
        self.assertEqual(plain(output).count("$x$"), 33)
        self.assertEqual(len(screen(output, 80)), 1)
        source = "| Bad | Good |\n| --- | --- |\n| $\\definitelynotacommand{x}$ | $x^2$ |\n"
        output, error = terminal(source)
        self.assertIn("showing TeX", error)
        self.assertIn(r"\definitelynotacommand{x}", plain(output))
        self.assertEqual(len(screen(output, 80)), 1)

    def test_nested_tables_keep_quote_rails_and_list_markers(self):
        source = "> Before\n>\n> | Name | Value |\n> | --- | ---: |\n> | Q | 1 |\n>\n> After\n\n- | Name | Value |\n  | --- | ---: |\n  | L | 2 |\n\nEnd.\n"
        output, error = terminal(source, args=("--math", "tex"))
        self.assertFalse(error)
        value = plain(output)
        self.assertIn("│ Q    │     1", value)
        self.assertIn("• Name │ Value", value)
        self.assertIn("    L    │     2", value)
        self.assertEqual(value.count("•"), 1)
        self.assertIn("│ After", value)
        screen(output, 80)

    def test_local_images_resolve_against_the_document_and_preserve_captions(self):
        fixture = Path(__file__).resolve().parent / "fixtures/images.md"
        output, error = terminal(file=fixture, cwd="/", args=("--show-math-source",))
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 7)
        self.assertIn("[image: A blue color grid]", plain(output))
        self.assertIn("[image: A centered grid] (assets/color-grid.png)", plain(output))
        self.assertIn("[image: Remote banner] (https://example.com/banner.gif)", plain(output))
        self.assertNotIn("<img", plain(output))
        self.assertIn("assets/gradient.jpg", plain(output))
        self.assertIn("Remote example", plain(output))
        self.assertNotIn("TeX:", plain(output))
        self.assertEqual(plain(output).count("•"), 1)

    def test_image_modes_are_independent_of_math_and_respect_terminal_fallbacks(self):
        base = Path(__file__).resolve().parent / "fixtures"
        source = "Before.\n\n![Blue](assets/color-grid.png)\n\nAfter $x$.\n"
        arguments = ("--base-dir", str(base))
        output, error = terminal(source, args=(*arguments, "--math", "tex"))
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 1)
        self.assertIn("$x$", plain(output))
        output, error = terminal(source, args=(*arguments, "--images", "off"))
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 1)
        self.assertNotIn("$x$", plain(output))
        for variables in ({"TERM_PROGRAM": "", "TERM": "xterm-256color"}, {"TMUX": "test"}, {"TERM": "dumb"}):
            output, error = terminal(source, args=arguments, variables=variables)
            self.assertFalse(error)
            self.assertFalse(GRAPHICS.search(output))
            self.assertIn("[image: Blue] (assets/color-grid.png)", plain(output))
        output, error = terminal(source, args=(*arguments, "--images", "kitty"), variables={"TERM_PROGRAM": "", "TERM": "xterm-256color"})
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 1)

    def test_image_fitting_quote_rails_and_stdin_base_directory(self):
        base = Path(__file__).resolve().parent / "fixtures"
        source = "> ![Diagram](assets/diagram.svg)\n\nAfter.\n"
        for columns, rows in ((22, 8), (80, 24)):
            output, error = terminal(source, cwd=base, columns=columns, rows=rows, variables={"NO_COLOR": "1"})
            self.assertFalse(error)
            images, cells = screen(output, columns, return_cells=True)
            self.assertEqual(len(images), 1)
            left, top, width, height = images[0]
            self.assertLessEqual(height, rows - 1)
            rails = {x for (x, y), char in cells.items() if char == "│" and top <= y < top + height}
            self.assertEqual(len(rails), 1)
            for x in rails:
                self.assertLess(x, left)
                for y in range(top, top + height):
                    self.assertEqual(cells.get((x, y)), "│")
            self.assertIn("After.", plain(output))

    def test_missing_unsupported_and_table_images_keep_their_references(self):
        base = Path(__file__).resolve().parent / "fixtures"
        source = "![Missing](missing.png)\n\n![Remote](https://example.com/image.png)\n\n| Icon |\n| --- |\n| ![Cell](assets/color-grid.png) |\n\n![Good](assets/color-grid.png)\n\nAfter.\n"
        output, error = terminal(source, args=("--base-dir", str(base)))
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 1)
        for label in ("Missing", "Remote", "Cell", "Good", "After."):
            self.assertIn(label, plain(output))
        for control in ("![a\x1b[2J](missing.png)\n", '<img src="missing.png" alt="a&#x1b;[2J">\n',
                        '<img src="missing.png" alt="a\x1b[2J">\n', '<p><img src="missing.png" alt="a\x9b2J"></p>\n'):
            output, error = terminal(control)
            self.assertFalse(error)
            self.assertNotIn(b"\x1b[2J", output)
            self.assertNotIn("\x9b".encode(), output)

    def test_mermaid_native_subset_and_selectable_source(self):
        fixture = Path(__file__).resolve().parent / "fixtures/mermaid.md"
        output, error = terminal(file=fixture)
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 15)  # 13 diagrams; two need a second band.
        self.assertIn('kanban', plain(output))
        self.assertNotIn('A[Read Markdown]', plain(output))
        output, error = terminal(file=fixture, args=("--show-mermaid-source",))
        self.assertFalse(error)
        self.assertEqual(output.count(b"\x1b_Ga=T,"), 15)
        for text in ('A[Read Markdown]', 'Reader->>Viewer:', 'class Document', '[*] --> Reading', 'DOCUMENT ||--o{ BLOCK'):
            self.assertIn(text, plain(output))

    def test_mermaid_unicode_tables_and_charts_fit_and_preserve_exports(self):
        fixture = Path(__file__).resolve().parent / "fixtures/mermaid.md"
        for columns in (24, 100):
            for theme in ("dark", "light"):
                output, error = terminal(file=fixture, columns=columns, args=("--theme", theme))
                self.assertFalse(error)
                self.assertEqual(len(screen(output, columns)), 13 if columns == 24 else 15)
        source = fixture.read_text()
        for mode in ("off", "auto", "kitty"):
            result = subprocess.run([str(BINARY), "--mermaid", mode, "--output", "source", str(fixture)], capture_output=True, check=True)
            self.assertEqual(result.stdout, source.encode())
            self.assertFalse(result.stderr)

    def test_gantt_dates_labels_and_fallbacks_use_the_existing_mermaid_path(self):
        fixture = Path(__file__).resolve().parent / "fixtures/gantt.md"
        for columns in (24, 100):
            for theme in ("dark", "light"):
                output, error = terminal(file=fixture, columns=columns, args=("--theme", theme))
                self.assertFalse(error)
                self.assertEqual(len(screen(output, columns)), 3)
                self.assertIn("Meeting", plain(output))
                self.assertIn("missing", plain(output))
                self.assertNotIn("after build docs", plain(output))
                for _, _, rgba in decoded_graphics(output):
                    self.assertIn(0, rgba[3::4])
        for args, variables in [(("--mermaid", "off"), {}),
                                ((), {"TERM": "xterm-256color", "TERM_PROGRAM": ""})]:
            output, error = terminal(file=fixture, args=args, variables=variables)
            self.assertFalse(error)
            self.assertFalse(GRAPHICS.search(output))
            self.assertIn("after build docs", plain(output))
        output, error = terminal(file=fixture, columns=100, args=("--show-mermaid-source",))
        self.assertFalse(error)
        self.assertIn("after build docs", plain(output))
        result = subprocess.run([str(BINARY), "--output", "source", str(fixture)], capture_output=True, check=True)
        self.assertEqual(result.stdout, fixture.read_bytes())

    def test_standalone_gantt_matches_fenced_input_and_resource_fallback_recovers(self):
        diagram = "gantt\nA: a, 2026-01-02, 1d\nB: after a, 1w\n"
        fenced = "```mermaid\n" + diagram + "```\n"
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "schedule.mmd"
            file.write_text(diagram)
            standalone, error = terminal(file=file)
            self.assertFalse(error)
            markdown, error = terminal(fenced)
            self.assertFalse(error)
            self.assertEqual(list(decoded_graphics(standalone)), list(decoded_graphics(markdown)))
        oversized = "gantt\nStart: 2026-01-01, 1d\n" + "\n".join(f"Task {i}: 1d" for i in range(128))
        output, error = terminal("```mermaid\n" + oversized + "\n```\n\n" + fenced)
        self.assertFalse(error)
        self.assertIn("Task 127", plain(output))
        self.assertEqual(len(screen(output, 80)), 1)

    def test_sub_day_gantt_formats_render_and_preserve_source_fallbacks(self):
        fixture = Path(__file__).resolve().parent / "fixtures/gantt-time.md"
        for columns in (24, 100):
            for theme in ("dark", "light"):
                output, error = terminal(file=fixture, columns=columns, args=("--theme", theme))
                self.assertFalse(error)
                self.assertEqual(len(screen(output, columns)), 4)
                self.assertIn("Zoned", plain(output))
                self.assertIn("Undated", plain(output))
                self.assertNotIn("after build notes", plain(output))
        output, error = terminal(file=fixture, columns=100, args=("--show-mermaid-source", "--cell-size", "20x40"))
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 100, cell_width=20, cell_height=40)), 4)
        self.assertIn("after build notes", plain(output))
        for mode in ("off", "auto", "kitty"):
            result = subprocess.run([str(BINARY), "--mermaid", mode, "--output", "source", str(fixture)], capture_output=True, check=True)
            self.assertEqual(result.stdout, fixture.read_bytes())
        output, error = terminal(file=fixture, args=("--mermaid", "off"))
        self.assertFalse(error)
        self.assertFalse(GRAPHICS.search(output))
        self.assertIn("1.5h", plain(output))

    def test_standalone_millisecond_gantt_matches_markdown(self):
        diagram = "gantt\ndateFormat YYYY-MM-DD HH:mm:ss.SSS\naxisFormat %S.%L\nA: 9999-01-01 12:00:00.001, 1ms\nB: 2ms\n"
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "stages.mmd"
            file.write_text(diagram)
            standalone, error = terminal(file=file, columns=100)
            self.assertFalse(error)
            fenced, error = terminal("```mermaid\n" + diagram + "```\n", columns=100)
            self.assertFalse(error)
            self.assertEqual(len(screen(standalone, 100)), 1)
            self.assertEqual(list(decoded_graphics(standalone)), list(decoded_graphics(fenced)))

    def test_mermaid_transparency_and_text_scale_follow_terminal_cells(self):
        source = "```mermaid\nflowchart LR; A[Read Markdown]-->B[Show document]\n```\nAfter.\n"
        for theme in ("dark", "light"):
            sizes = []
            for cw, ch in ((10, 20), (20, 40)):
                output, error = terminal(source, columns=100,
                                         args=("--theme", theme, "--cell-size", f"{cw}x{ch}"))
                self.assertFalse(error)
                self.assertEqual(len(screen(output, 100, cell_width=cw, cell_height=ch)), 1)
                (width, height, rgba), = decoded_graphics(output)
                alpha = rgba[3::4]
                self.assertGreater(alpha.count(0), len(alpha) // 3)
                self.assertGreater(alpha.count(255), len(alpha) // 10)
                self.assertTrue(any(0 < value < 255 for value in alpha))
                self.assertIn("After.", plain(output))
                sizes.append((width, height))
            # Retina cells need twice the physical pixels, including label ink.
            self.assertLessEqual(abs(sizes[1][0] - 2 * sizes[0][0]), 20)
            self.assertLessEqual(abs(sizes[1][1] - 2 * sizes[0][1]), 40)

    def test_standalone_mermaid_matches_fenced_diagrams_and_explicit_stdin(self):
        source = 'flowchart LR\n A[Read] --> B[Show]\n'
        expected, error = terminal(f'```mermaid\n{source}```\n', columns=100)
        self.assertFalse(error)
        expected = list(decoded_graphics(expected))
        self.assertEqual(len(expected), 1)
        with tempfile.TemporaryDirectory() as root:
            for suffix in ('.mmd', '.MERMAID'):
                file = Path(root) / ('diagram' + suffix)
                file.write_text(source)
                output, error = terminal(file=file, columns=100)
                self.assertFalse(error)
                self.assertEqual(list(decoded_graphics(output)), expected)
                output, error = terminal(file=file, args=('--input-format', 'markdown'))
                self.assertFalse(error)
                self.assertFalse(list(decoded_graphics(output)))
        output, error = terminal(source, columns=100, args=('--input-format', 'mermaid'))
        self.assertFalse(error)
        self.assertEqual(list(decoded_graphics(output)), expected)

    def test_standalone_images_use_native_paths_and_the_existing_decoder(self):
        assets = Path(__file__).parent / 'fixtures/assets'
        with tempfile.TemporaryDirectory() as root:
            for name in ('color-grid.png', 'gradient.jpg', 'diagram.svg'):
                target = Path(root) / ('café [100%] #?:' + Path(name).suffix)
                target.write_bytes((assets / name).read_bytes())
                output, error = terminal(file=target)
                self.assertFalse(error)
                self.assertEqual(len(screen(output, 80)), 1)
                self.assertEqual(len(list(decoded_graphics(output))), 1)
                self.assertIn(target.name, plain(output))
                disabled, error = terminal(file=target, args=('--images', 'off'))
                self.assertFalse(error)
                self.assertFalse(list(decoded_graphics(disabled)))
                self.assertIn('[image:', plain(disabled))
            target = Path(root) / 'image-without-extension'
            target.write_bytes((assets / 'color-grid.png').read_bytes())
            output, error = terminal(file=target, args=('--input-format', 'image'))
            self.assertFalse(error)
            self.assertEqual(len(list(decoded_graphics(output))), 1)

    def test_sgf_selection_fallbacks_and_standalone_equivalence(self):
        source = '(;FF[4]GM[1]SZ[9];B[cc](;W[];B[dd])(;W[ee]))(;GM[1]SZ[9];B[ff])'
        for theme in ('dark', 'light'):
            outputs = []
            for attrs, expected in [('', 'game 1/2 · move 3'), (' move=1', 'game 1/2 · move 1'),
                                    (' move=0', 'game 1/2 · move 0'), (' game=2', 'game 2/2 · move 1')]:
                out, error = terminal(f'```sgf{attrs}\n{source}\n```\nAfter.\n', args=('--theme', theme))
                self.assertFalse(error)
                self.assertEqual(len(screen(out, 80)), 1)
                self.assertIn(expected, plain(out))
                self.assertIn('After.', plain(out))
                outputs.append(list(decoded_graphics(out)))
            self.assertNotEqual(outputs[0], outputs[1])
            raw, error = terminal(source, args=('--input-format', 'sgf', '--theme', theme))
            self.assertFalse(error)
            self.assertEqual(list(decoded_graphics(raw)), outputs[0])
            selected, error = terminal(source, args=('--input-format', 'sgf', '--theme', theme, '--sgf-move', '1'))
            self.assertFalse(error)
            self.assertEqual(list(decoded_graphics(selected)), outputs[1])
        for attrs in (' move=99', ' game=3', ' move=-1', ' game=0', ' unknown=1', ' move=1 move=2'):
            out, error = terminal(f'```sgf{attrs}\n{source}\n```\nAfter.\n')
            self.assertFalse(error)
            self.assertFalse(list(decoded_graphics(out)))
            self.assertIn(source, plain(out).replace('\n', '').replace(' ', ''))
            self.assertIn('After.', plain(out))
        with tempfile.TemporaryDirectory() as root:
            file = Path(root) / 'record.sgf'
            file.write_text(source)
            out, error = terminal(file=file, args=('--sgf-game', '2'))
            self.assertFalse(error)
            self.assertIn('game 2/2 · move 1', plain(out))
            self.assertEqual(len(list(decoded_graphics(out))), 1)

    def test_sgf_modes_containers_and_limits_keep_readable_source(self):
        source = '(;FF[4]GM[1]SZ[9];B[cc];W[dd])'
        markdown = f'- ```sgf\n  {source}\n  ```\n\nAfter.\n'
        out, error = terminal(markdown, args=('--show-sgf-source', '--images', 'off', '--mermaid', 'off', '--math', 'tex'))
        self.assertFalse(error)
        self.assertEqual(len(screen(out, 80)), 1)
        self.assertIn('game 1/1 · move 2', plain(out))
        self.assertIn(source, plain(out))
        self.assertEqual(plain(out).count('•'), 1)
        for args, variables in [(('--sgf', 'off'), None), ((), {'TERM_PROGRAM': '', 'TERM': 'xterm-256color'})]:
            out, error = terminal(markdown, args=args, variables=variables)
            self.assertFalse(error)
            self.assertFalse(list(decoded_graphics(out)))
            self.assertIn(source, plain(out))
        for columns in (18, 80):
            out, error = terminal(f'> ```sgf\n> {source}\n> ```\n\nAfter.\n', columns=columns)
            self.assertFalse(error)
            self.assertEqual(len(screen(out, columns)), 1)
        repeated = (f'```sgf\n{source}\n```\n\n' * 100) + 'After.\n'
        out, error = terminal(repeated)
        self.assertFalse(error)
        self.assertEqual(len(list(decoded_graphics(out))), 100)
        self.assertIn('After.', plain(out))
        too_large = '(;GM[1]SZ[9]C[' + ('x' * (256 * 1024)) + '])'
        out, error = terminal(f'```sgf\n{too_large}\n```\n```sgf\n{source}\n```\nAfter.\n')
        self.assertFalse(error)
        self.assertEqual(len(list(decoded_graphics(out))), 1)
        self.assertIn('After.', plain(out))

    def test_mermaid_modes_quotes_and_list_markers(self):
        source = "- ```mermaid\n  flowchart LR\n  A[Start]-->B[Done]\n  ```\n\nAfter $x$.\n"
        output, error = terminal(source, args=("--images", "off", "--math", "tex", "--show-mermaid-source"))
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 1)
        self.assertEqual(plain(output).count("•"), 1)
        self.assertIn("$x$", plain(output))
        output, error = terminal(source, args=("--mermaid", "off"))
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 1)  # Only the equation.
        self.assertIn("A[Start]", plain(output))
        for variables in ({"TERM_PROGRAM": "", "TERM": "xterm-256color"}, {"TMUX": "test"}, {"TERM": "dumb"}):
            output, error = terminal(source, variables=variables)
            self.assertFalse(error)
            self.assertFalse(GRAPHICS.search(output))
            self.assertIn("A[Start]", plain(output))
        output, error = terminal(source, args=("--mermaid", "kitty", "--math", "tex"), variables={"TERM_PROGRAM": "", "TERM": "xterm-256color"})
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 1)
        quoted = "> ```mermaid\n> flowchart LR; A-->B\n> ```\n\nAfter.\n"
        for light in ((), ("--light",)):
            output, error = terminal(quoted, columns=26, rows=10, args=light, variables={"NO_COLOR": "1"})
            self.assertFalse(error)
            images, cells = screen(output, 26, return_cells=True)
            self.assertEqual(len(images), 1)
            left, top, width, height = images[0]
            rails = {x for (x, y), char in cells.items() if char == "│" and top <= y < top + height}
            self.assertEqual(len(rails), 1)
            for x in rails:
                self.assertLess(x, left)
                for y in range(top, top + height): self.assertEqual(cells.get((x, y)), "│")
            self.assertIn("After.", plain(output))

    def test_tall_mermaid_bands_keep_readable_pixels_rails_and_one_bullet(self):
        diagram = "flowchart TD\n" + "\n".join(f"N{i}[Step {i}] --> N{i+1}[Step {i+1}]" for i in range(12))
        for prefix, continuation in [("", ""), ("> ", "> "), ("- ", "  ")]:
            source = prefix + "```mermaid\n" + "".join(continuation + line + "\n" for line in diagram.splitlines())
            source += continuation + "```\n\nAfter.\n"
            output, error = terminal(source, rows=12, variables={"NO_COLOR": "1"})
            self.assertFalse(error)
            images, cells = screen(output, 80, return_cells=True)
            self.assertGreater(len(images), 2)
            for left, top, width, height in images:
                self.assertLessEqual(height, 10)
                self.assertGreaterEqual(width, 10)
                if prefix == "> ":
                    for y in range(top, top + height):
                        self.assertEqual(cells.get((2, y)), "│")
            for previous, current in zip(images, images[1:]):
                self.assertEqual(previous[0], current[0])
                self.assertEqual(previous[1] + previous[3], current[1])
            self.assertEqual(plain(output).count("•"), int(prefix == "- "))
            self.assertIn("After.", plain(output))

    def test_mermaid_failures_and_budget_keep_complete_code(self):
        blocks = ["flowchart LR\nA[Broken", "flowchart LR; A[中文]-->B", "gantt\nSlice: 2026-01-01, 30q", "%%{init: {}}%%\nflowchart LR; A-->B", "flowchart LR; A-->B\nclick A \"https://example.com\""]
        source = "\n\n".join("```mermaid\n" + block + "\n```" for block in blocks)
        source += "\n\n```mermaid\nflowchart LR; A[Valid]-->B\n```\nAfter.\n"
        output, error = terminal(source)
        self.assertFalse(error)
        self.assertEqual(output.count(b"\x1b_Ga=T,"), 1)
        for text in ('A[Broken', '中文', 'Slice', 'init:', 'https://example.com', 'After.'):
            self.assertIn(text, plain(output))
        source = "".join(f"```mermaid\nflowchart LR; A[Diagram {i}]-->B\n```\n\n" for i in range(34))
        source += "```mermaid\nflowchart LR; A[Diagram 31]-->B\n```\n"
        output, error = terminal(source)
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 33)  # 32 misses, then a cache hit.
        self.assertIn('Diagram 32', plain(output))
        self.assertIn('Diagram 33', plain(output))

    def test_unknown_terminal_keeps_tex(self):
        output, error = terminal(variables={"TERM": "xterm-256color", "TERM_PROGRAM": ""})
        self.assertFalse(error)
        self.assertFalse(GRAPHICS.search(output))
        self.assertIn(r"$\frac{a}{b}$", plain(output))

    def test_explicit_tex_overrides_detection(self):
        output, error = terminal(args=("--math", "tex"))
        self.assertFalse(error)
        self.assertFalse(GRAPHICS.search(output))
        self.assertIn(r"$\frac{a}{b}$", plain(output))

    def test_explicit_kitty_overrides_unknown_terminal(self):
        output, error = terminal(args=("--math", "kitty"), variables={"TERM": "xterm-256color", "TERM_PROGRAM": ""})
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 3)

    def test_multiplexer_and_dumb_terminal_fall_back(self):
        for variables in ({"TMUX": "test-session"}, {"TERM": "dumb"}):
            with self.subTest(variables=variables):
                output, error = terminal(variables=variables)
                self.assertFalse(error)
                self.assertFalse(GRAPHICS.search(output))
                self.assertIn(r"$\frac{a}{b}$", plain(output))

    def test_selectable_source_is_printed_below_equations(self):
        output, error = terminal(args=("--show-math-source",))
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 3)
        self.assertIn(r"TeX: $\frac{a}{b}$", plain(output))
        self.assertEqual(plain(output).count(r"\frac{a}{b}"), 1)

    def test_no_color_disables_styles_without_disabling_images(self):
        output, error = terminal("**Text** and $x$.\n", variables={"NO_COLOR": "1"})
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 80)), 1)
        self.assertFalse(re.search(rb"\x1b\[[0-9;]*m", output))

    def test_text_output_disables_styles_and_images_on_a_terminal(self):
        output, error = terminal("**Text** and $x$.\n", args=("--output", "text"))
        self.assertFalse(error)
        self.assertNotIn(b"\x1b", output)
        self.assertIn("Text and $x$.", plain(output))

    def test_unicode_survives_wrapping_in_the_cli(self):
        output, error = terminal("界界 👩‍💻👩‍💻 éé $x$.\n", columns=12)
        self.assertFalse(error)
        text = plain(output)
        self.assertIn("界界", text)
        self.assertIn("👩‍💻👩‍💻", text)
        self.assertIn("éé", text)


    def test_terminal_width_and_margin_apply_without_graphics(self):
        source = ("A paragraph with several ordinary words that should wrap predictably. " * 5) + "\n"
        output, error = terminal(
            source, args=("--width", "32", "--margin", "4"), columns=120,
            variables={"TERM": "xterm-256color", "TERM_PROGRAM": ""},
        )
        self.assertFalse(error)
        lines = [line for line in plain(output).splitlines() if line.strip()]
        self.assertGreater(len(lines), 5)
        self.assertTrue(all(line.startswith("    ") and len(line) <= 36 for line in lines))
        self.assertFalse(GRAPHICS.search(output))
        self.assertEqual(" ".join(line.strip() for line in lines), source.strip())

    def test_default_width_is_bounded_and_zero_uses_available_space(self):
        source = "word " * 100 + "\n"
        bounded, _ = terminal(source, args=("--math", "tex"), columns=120)
        full, _ = terminal(source, args=("--math", "tex", "--width", "0"), columns=120)
        self.assertLessEqual(max(map(len, plain(bounded).splitlines())), 82)
        self.assertGreater(max(map(len, plain(full).splitlines())), 82)

    def test_wrapped_lists_and_quotes_keep_their_prefixes(self):
        source = (
            "- A long list item with enough words to require multiple lines at this width.\n\n"
            "> A long quotation with enough words to require multiple lines at this width.\n"
            ">\n"
            "> Another paragraph in the quotation.\n"
        )
        output, error = terminal(source, args=("--math", "tex", "--width", "28"), columns=100)
        self.assertFalse(error)
        groups = [group.splitlines() for group in plain(output).strip("\n").split("\n\n")]
        self.assertTrue(groups[0][0].startswith("  • "))
        self.assertTrue(all(line.startswith("    ") for line in groups[0][1:]))
        quoted = [line for group in groups[1:] for line in group]
        self.assertTrue(all(line.startswith("  │") for line in quoted))
        self.assertIn("  │ ", quoted)
        self.assertTrue(all(len(line) <= 30 for group in groups for line in group))

    def test_code_panels_preserve_blank_lines_and_indentation(self):
        source = "```text\nfirst\n\n    indented\nlast\n```\n"
        output, error = terminal(source, args=("--width", "24", "--math", "tex"), columns=80)
        self.assertFalse(error)
        lines = [line.rstrip() for line in plain(output).splitlines() if line.strip()]
        self.assertEqual(lines, ["  │ first", "  │", "  │     indented", "  │ last"])

    def test_syntax_colors_preserve_multiline_code_and_wrapping(self):
        source = "```rust\n/* first\ncontinued */\nfn café() { let value = \"中文 and 👩‍💻\"; }\n```\n"
        args = ("--width", "24", "--math", "tex")
        colored, error = terminal(source, args=args)
        disabled, other_error = terminal(source, args=(*args, "--no-highlight"))
        light, light_error = terminal(source, args=(*args, "--light"))
        self.assertFalse(error or other_error or light_error)
        self.assertEqual(plain(colored), plain(disabled))
        self.assertEqual(plain(colored), plain(light))
        self.assertNotEqual(colored, disabled)
        self.assertEqual(attributes_at(colored, b"continued */")["foreground"], 245)
        self.assertEqual(attributes_at(colored, b"continued */")["background"], 236)
        self.assertEqual(attributes_at(colored, b"fn")["foreground"], 176)
        self.assertEqual(attributes_at(light, b"fn")["foreground"], 90)
        self.assertEqual(attributes_at(light, b"fn")["background"], 254)
        self.assertIn("👩‍💻", plain(colored))

    def test_styles_are_cleared_between_spans_and_after_the_document(self):
        source = "**bold** plain1 *italic* plain2 ~~gone~~ plain3 [linked](https://example.com) plain4 `code` plain5\n"
        output, error = terminal(source, args=("--math", "tex"))
        self.assertFalse(error)
        for word in [b"plain1", b"plain2", b"plain3", b"plain4", b"plain5"]:
            self.assertEqual(attributes_at(output, word), dict(
                foreground=252, background=None, bold=False, italic=False,
                underline=False, strike=False))
        self.assertTrue(attributes_at(output, b"bold")["bold"])
        self.assertTrue(attributes_at(output, b"italic")["italic"])
        self.assertTrue(attributes_at(output, b"gone")["strike"])
        self.assertTrue(attributes_at(output, b"linked")["underline"])
        self.assertEqual(attributes_at(output, b"code")["background"], 236)
        self.assertEqual(attributes_at(output), dict(
            foreground=None, background=None, bold=False, italic=False,
            underline=False, strike=False))

    def test_standalone_yaml_reuses_fenced_colors_and_keeps_comments_literal(self):
        file = Path(__file__).resolve().parent / "fixtures" / "code.yaml"
        source = file.read_text()
        colored, error = terminal(file=file)
        fenced, fence_error = terminal(f"````yaml\n{source}````\n", args=("--line-numbers", "on"))
        stdin, stdin_error = terminal(source, args=("--language", "YML"))
        disabled, disabled_error = terminal(file=file, args=("--no-highlight",))
        uncolored, uncolored_error = terminal(file=file, variables={"NO_COLOR": "1"})
        self.assertFalse(error or fence_error or stdin_error or disabled_error or uncolored_error)
        self.assertEqual(colored, fenced)
        self.assertEqual(colored, stdin)
        self.assertEqual(plain(colored), plain(disabled))
        self.assertEqual(plain(colored), plain(uncolored))
        self.assertNotEqual(colored, disabled)
        self.assertNotIn(b"\x1b", uncolored)
        self.assertEqual(attributes_at(colored, b"# Deployment settings")["foreground"], 245)
        self.assertFalse(attributes_at(colored, b"# Deployment settings")["bold"])
        self.assertIn("**bold** and $x^2$ stay literal", plain(colored))
        self.assertNotIn(b"\x1b_G", colored)

    def test_code_line_numbers_count_logical_lines_and_align_wrapped_continuations(self):
        source = "\r\n".join(["abcdefghijklmnopqrst", "", "    x", "é 👩‍💻"] +
                               [f"row {n}" for n in range(5, 13)])
        args = ("--input-format", "code", "--width", "12")
        for ending in ("", "\r\n"):
            with self.subTest(ending=ending):
                output, error = terminal(source + ending, args=args)
                self.assertFalse(error)
                lines = [line.rstrip() for line in plain(output).splitlines() if line.strip()]
                self.assertEqual(lines[:4], ["   1 │ abcdefg", "     │ hijklmn", "     │ opqrst", "   2 │"])
                self.assertIn("   3 │     x", lines)
                self.assertIn("   4 │ é 👩‍💻", lines)
                self.assertEqual([int(m[1]) for line in lines
                                  if (m := re.match(r"\s*(\d+) │", line))], list(range(1, 13)))
                self.assertTrue(all(len(line) <= 14 for line in lines))
                self.assertEqual(attributes_at(output, "1 │".encode())["foreground"], 245)
        light, error = terminal(source, args=(*args, "--light"))
        self.assertFalse(error)
        self.assertEqual(attributes_at(light, "1 │".encode())["foreground"], 243)

    def test_code_line_number_controls_preserve_nested_markdown_prefixes(self):
        source = "- ```text\n  first\n  second\n  ```\n\n> ```text\n> third\n> fourth\n> ```\n"
        numbered, error = terminal(source, args=("--line-numbers", "on"))
        default, default_error = terminal(source)
        disabled, off_error = terminal(source, args=("--line-numbers", "off"))
        self.assertFalse(error or default_error or off_error)
        self.assertEqual(default, disabled)
        lines = [line.rstrip() for line in plain(numbered).splitlines() if line.strip()]
        self.assertEqual(lines, ["  • 1 │ first", "    2 │ second", "  │ 1 │ third", "  │ 2 │ fourth"])

    def test_code_line_numbers_yield_space_to_content_in_narrow_terminals(self):
        source = "界\ne\u0301\n"
        for columns in range(2, 9):
            with self.subTest(columns=columns):
                args = ("--input-format", "code", "--width", "0", "--margin", "0")
                output, error = terminal(source, args=args, columns=columns, variables={"NO_COLOR": "1"})
                disabled, other_error = terminal(source, args=(*args, "--line-numbers", "off"),
                                                columns=columns, variables={"NO_COLOR": "1"})
                self.assertFalse(error or other_error)
                self.assertIn("界", plain(output))
                self.assertIn("e\u0301", plain(output))
                for line in plain(output).splitlines():
                    width = sum(0 if unicodedata.combining(ch) else
                                2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1 for ch in line)
                    self.assertLessEqual(width, columns)
                if columns < 7:
                    self.assertEqual(output, disabled)
                else:
                    self.assertIn("1 │ 界", plain(output))
                    self.assertIn("2 │ e\u0301", plain(output))

    def test_special_code_filenames_get_numbers_and_keep_comments_literal(self):
        with tempfile.TemporaryDirectory() as root:
            for name, source in [
                ("Cargo.lock", '# Generated lockfile\nversion = 4\n'),
                ("Dockerfile", '# Build image\nFROM scratch\n'),
            ]:
                with self.subTest(name=name):
                    file = Path(root) / name
                    file.write_text(source)
                    output, error = terminal(file=file)
                    disabled, other_error = terminal(file=file, args=("--line-numbers", "off"))
                    self.assertFalse(error or other_error)
                    self.assertIn("  1 │ # ", plain(output))
                    self.assertIn("  │ # ", plain(disabled))
                    self.assertNotIn("  1 │", plain(disabled))
                    if name == "Cargo.lock":
                        fenced, fenced_error = terminal(f"```toml\n{source}```\n", args=("--line-numbers", "on"))
                        self.assertFalse(fenced_error)
                        self.assertEqual(output, fenced)
                    self.assertNotIn(b"\x1b_G", output)

    def test_standalone_code_fallbacks_do_not_start_diagrams_or_math(self):
        for language, source in [
            ("unknown", "# Comment\n**literal** and $x$\n"),
            ("mermaid", "flowchart LR; A-->B\n"),
            ("sgf", "(;GM[1]FF[4]SZ[9];B[dd])\n"),
            ("math", "x^2\n"),
            ("yaml", "# " + "x" * (128 * 1024) + "\nlast: kept\n"),
        ]:
            with self.subTest(language=language):
                args = ("--input-format", "code", "--language", language)
                colored, error = terminal(source, args=args)
                disabled, disabled_error = terminal(source, args=(*args, "--no-highlight"))
                self.assertFalse(error or disabled_error)
                self.assertEqual(colored, disabled)
                self.assertNotIn(b"\x1b_G", colored)
                self.assertIn(source.splitlines()[-1], plain(colored))

    def test_dockerfile_and_containerfile_share_colors_in_files_fences_and_stdin(self):
        source = (Path(__file__).resolve().parent / "fixtures" / "Dockerfile").read_text()
        fenced, fence_error = terminal(f"````containerfile\n{source}````\n", args=("--line-numbers", "on"))
        stdin, stdin_error = terminal(source, args=("--language", "docker"))
        self.assertFalse(fence_error or stdin_error)
        self.assertEqual(stdin, fenced)
        with tempfile.TemporaryDirectory() as root:
            for name in ("Dockerfile", "Containerfile", "Dockerfile.dev", "build.containerfile"):
                with self.subTest(name=name):
                    file = Path(root) / name
                    file.write_text(source)
                    colored, error = terminal(file=file)
                    disabled, disabled_error = terminal(file=file, args=("--no-highlight",))
                    uncolored, plain_error = terminal(file=file, variables={"NO_COLOR": "1"})
                    self.assertFalse(error or disabled_error or plain_error)
                    self.assertEqual(colored, fenced)
                    self.assertEqual(plain(colored), plain(disabled))
                    self.assertEqual(plain(colored), plain(uncolored))
                    self.assertNotEqual(colored, disabled)
                    self.assertNotIn(b"\x1b", uncolored)
                    self.assertNotIn(b"\x1b_G", colored)
                    for token, color in [(b"FROM", 176), (b"CoPy", 176), (b"--chown=1000:1000", 117),
                                         (b"printf", 81), (b"if", 176), (b"42", 215)]:
                        self.assertEqual(attributes_at(colored, token)["foreground"], color, token)

    def test_dockerfile_limits_keep_complete_source_without_affecting_next_block(self):
        source = "RUN echo hello\n" * 129
        args = ("--language", "containerfile")
        colored, error = terminal(source, args=args)
        disabled, disabled_error = terminal(source, args=(*args, "--no-highlight"))
        self.assertFalse(error or disabled_error)
        self.assertEqual(colored, disabled)
        self.assertEqual(plain(colored).count("RUN echo hello"), 129)
        markdown = f"```dockerfile\n{source}```\n\n```dockerfile\nFROM scratch\n```\n"
        output, error = terminal(markdown)
        self.assertFalse(error)
        self.assertEqual(plain(output).count("RUN echo hello"), 129)
        self.assertEqual(attributes_at(output, b"FROM")["foreground"], 176)

    def test_code_fallbacks_and_no_color_keep_the_complete_content(self):
        for language, body in [
            ("unknown", "fn main() { let value = 42; }\n"),
            ("rust", "// " + "x" * (128 * 1024) + "\n"),
            ("", "raw $math$ and code\n"),
        ]:
            source = f"```{language}\n{body}```\n"
            colored, error = terminal(source, args=("--math", "tex"))
            disabled, other_error = terminal(source, args=("--math", "tex", "--no-highlight"))
            self.assertFalse(error or other_error)
            self.assertEqual(colored, disabled)
        source = "```rust\nfn main() { let value = 42; }\n```\n"
        output, error = terminal(source, variables={"NO_COLOR": "1"})
        self.assertFalse(error)
        self.assertNotIn(b"\x1b", output)
        self.assertIn("fn main()", plain(output))

    def test_highlighted_code_cannot_inject_terminal_controls(self):
        source = "```python\nprint(\"\\x1b]52;c;payload\\x07\")\n```\n".replace("\\x1b", "\x1b").replace("\\x07", "\x07")
        output, error = terminal(source, args=("--math", "tex"))
        self.assertFalse(error)
        self.assertNotIn(b"\x1b]", output)
        self.assertNotIn(b"\x07", output)
        self.assertIn("payload", plain(output))

    def test_code_inside_lists_and_quotes_keeps_its_prefix(self):
        source = "- ```rust\n  fn main() {}\n  ```\n\n> ```python\n> def hello():\n>     pass\n> ```\n"
        output, error = terminal(source, args=("--math", "tex"))
        self.assertFalse(error)
        lines = [line.rstrip() for line in plain(output).splitlines() if line.strip()]
        self.assertEqual(lines, ["  • │ fn main() {}", "  │ │ def hello():", "  │ │     pass"])

    def test_quote_rails_continue_across_code_block_boundaries(self):
        source = "> Before\n>\n> ```rust\n> fn main() {}\n> ```\n>\n> After\n"
        output, error = terminal(source, args=("--math", "tex"))
        self.assertFalse(error)
        lines = [line.rstrip() for line in plain(output).strip("\n").splitlines()]
        self.assertEqual(lines, ["  │ Before", "  │", "  │ │ fn main() {}", "  │", "  │ After"])

    def test_light_theme_changes_colors_without_changing_text_layout(self):
        source = "# Heading\n\nText with **emphasis**, `code`, and a [link](https://example.com).\n"
        dark, error = terminal(source, args=("--math", "tex"))
        light, light_error = terminal(source, args=("--math", "tex", "--light"))
        self.assertFalse(error or light_error)
        self.assertEqual(plain(dark), plain(light))
        self.assertNotEqual(CSI.findall(dark), CSI.findall(light))
        self.assertIn(b"48;5;", dark)
        self.assertIn(b"48;5;", light)

    def test_inline_and_display_images_stay_inside_the_reading_column(self):
        source = (
            "Text $x^2$ and $\\frac{a}{b}$ here.\n\n"
            "$$\n\\underbrace{a+b+c+d+e+f+g+h}_{\\text{a wide equation}}\n$$\n"
        )
        output, error = terminal(source, args=("--width", "24", "--margin", "3"), columns=120)
        self.assertFalse(error)
        images = screen(output, 120)
        self.assertEqual(len(images), 3)
        self.assertTrue(all(3 <= left and left + width <= 27 for left, _, width, _ in images))

    def test_heading_background_and_large_margins_preserve_math_placement(self):
        output, error = terminal("# Energy $E=mc^2$\n", args=("--width", "30", "--margin", "9"), columns=100)
        self.assertFalse(error)
        self.assertEqual(len(screen(output, 100)), 1)
        narrow, error = terminal(args=("--width", "0", "--margin", "1000"), columns=20, rows=12)
        self.assertFalse(error)
        self.assertEqual(len(screen(narrow, 20)), 3)

if __name__ == "__main__":
    unittest.main()
