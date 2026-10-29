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
            if byte >= 128:
                character = output[offset:offset + 3].decode()
                assert character in "│─┼•", "screen fixtures use ASCII and single-cell borders"
                offset += 2
            else:
                assert 32 <= byte < 127, "unexpected control character"
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
