"""Exercise the real CLI through Unix pseudo-terminals, without a GUI.

Build first, then run: python3 tests/terminal_smoke.py target/debug/mdviewer
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
import time
import unittest


BINARY = Path(sys.argv.pop(1)).resolve() if len(sys.argv) > 1 else Path("target/debug/mdviewer").resolve()
SAMPLE = "Above.\n\nBefore $\\frac{a}{b}$, between $x^2$ and $\\sqrt{1+x}$ after.\n\nBelow.\n"
GRAPHICS = re.compile(rb"\x1b_G(.*?)\x1b\\", re.DOTALL)
CSI = re.compile(rb"\x1b\[([0-9;]*)([A-Za-z])")


def terminal(source=SAMPLE, args=(), columns=80, rows=24, variables=None):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, columns * 10, rows * 20))
    env = dict(os.environ, TERM="xterm-ghostty", TERM_PROGRAM="ghostty")
    for name in ("TMUX", "STY", "ZELLIJ", "NO_COLOR"):
        env.pop(name, None)
    env.update(variables or {})
    process = subprocess.Popen(
        [str(BINARY), *args, "-"], stdin=subprocess.PIPE, stdout=slave, stderr=subprocess.PIPE, env=env
    )
    os.close(slave)
    output = bytearray()
    try:
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
    return CSI.sub(b"", GRAPHICS.sub(b"", output)).replace(b"\r", b"").decode()


def screen(output, columns):
    """Interpret the emitted ASCII test text and cursor/graphics subset.

    Rows are absolute scrollback rows. This checks that all image rectangles fit
    the viewport width and that no ordinary text cell is covered by an image.
    """
    row = column = offset = 0
    text_cells = []
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
                width, height = int(header[b"s"]), int(header[b"v"])
                image_columns, image_rows = int(header[b"c"]), int(header[b"r"])
                assert len(rgba) == width * height * 4
                assert width == image_columns * 10 and height == image_rows * 20
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
            row += 1
        else:
            assert 32 <= byte < 127, "the screen-model fixture must be ASCII"
            assert column < columns, "text unexpectedly relied on terminal autowrap"
            if byte != 32:
                text_cells.append((column, row))
            column += 1
        offset += 1
    assert header is None
    for x, y in text_cells:
        for left, top, width, height in images:
            assert not (left <= x < left + width and top <= y < top + height), "text overlaps equation"
    return images


class TerminalSmoke(unittest.TestCase):
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
