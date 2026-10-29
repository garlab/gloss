# mdviewer

A terminal Markdown viewer with native inline and display equations. Math is laid
out by `katex-native`; surrounding prose stays selectable terminal text. No
browser, Node.js, or external TeX installation is needed at runtime.

mdviewer prints into terminal scrollback and exits. It supports Kitty graphics
with text fallbacks; interactive paging and resize handling are planned.

## Build and run

A current stable Rust toolchain and a separate `katex-native` checkout are
required. The math dependency is not yet publicly available, so this repository
alone is not sufficient to build. Keep the checkouts beside one another:

```text
projects/
├── mdviewer/
└── katex-native/
```

From the `mdviewer` directory, install locally and try the included examples.
Cargo's binary directory must be on your `PATH`.

```sh
cargo install --path . --locked
mdviewer tests/fixtures/math.md
mdviewer --show-math-source tests/fixtures/inline.md
```

To run without installing, use `cargo run --locked -- <arguments>`. Omit the file
argument or pass `-` to read Markdown from stdin.

## Terminal rendering

The default `--math auto` enables graphics in sessions identified as Ghostty or
Kitty by `TERM` or `TERM_PROGRAM`. This uses environment hints, assumes graphics
are enabled, and does not probe the terminal or read keyboard input.

Other terminals use selectable TeX. Recognized tmux, screen, and Zellij sessions,
`TERM=dumb`, and very small windows also use TeX. Multiplexer graphics passthrough
is not implemented.

| Option | Behavior |
| --- | --- |
| `--math tex` | Keep all equations as selectable source TeX. |
| `--math kitty` | Enable graphics in another compatible terminal; pipe, multiplexer, and small-window restrictions still apply. |
| `--show-math-source` | Print selectable TeX beneath rendered equations. |
| `--light` | Use dark equation ink for a light background. |
| `--font-size <pixels>` | Override the equation font size, from 4 to 256 pixels. |

Equation size follows the terminal cell height by default, and oversized equations
shrink to fit. Cell dimensions are read on Unix when available, with an estimated
fallback. Font baselines are estimated; appearance can vary by font and terminal.
Background detection is not implemented. A nonempty `NO_COLOR` disables text
styling without disabling math images.

## Copying and pipes

Normal terminal selection copies prose. Selecting an equation image alone does
not recover its TeX; use `--show-math-source` or `--math tex` when copying math.

```sh
cat notes.md | mdviewer -
mdviewer --output text notes.md
mdviewer notes.md > notes.txt
mdviewer --output source notes.md | pbcopy  # macOS
```

Pipes receive readable text with source TeX, even with `--math kitty`: no ANSI
styles, graphics, or added screen-width wrapping. `--output text` selects that
representation on a terminal too. Diagnostics go to stderr.

`--output source` preserves the original UTF-8 Markdown byte for byte, including
line endings and final-newline state. It bypasses parsing and control-character
filtering. Clipboard commands such as `pbcopy` are external; mdviewer does not
change the clipboard itself.

## Markdown and math

- Headings, emphasis, lists, quotes, code, links, task lists, strikethrough,
  footnotes, and basic tables.
- `$...$` inline math, `$$...$$` display math, and fenced `math` blocks.
- Equation tags and automatic numbering, with tags placed after the formula.
- Unicode-aware wrapping in graphics mode, preserving graphemes and keeping
  equations whole.
- Individual formula failures fall back to source TeX without interrupting the
  rest of the document.

Math support follows the KaTeX subset implemented by `katex-native`.
Backslash-parenthesis and backslash-bracket math delimiters are not recognized.

Nested lists and quotes need layout improvements. Tables are not column-aligned,
code is not syntax-highlighted, and text-only output relies on terminal soft
wrapping. Markdown images are shown as text references and are not fetched.
HTML is not executed.

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 tests/terminal_smoke.py target/debug/mdviewer  # Unix, Python 3
```

Rust tests cover parsing, source preservation, wrapping, math, and output
contracts. Pseudo-terminal tests cover graphics selection, fallbacks, narrow
windows, Unicode, and image/text placement. Visual checks in real terminals are
still needed to assess font alignment, scrolling, and selection behavior.

See [architecture](docs/architecture.md) for module boundaries and contracts, and
[the roadmap](docs/roadmap.md) for remaining implementation work. Crate publication
is disabled while `katex-native` uses a local path dependency.

## License

MIT; see [LICENSE](LICENSE). `katex-native` is MIT, and its bundled KaTeX fonts use
the SIL Open Font License 1.1; see `../katex-native/fonts/OFL.txt`.
