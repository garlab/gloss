# mdviewer

A terminal Markdown viewer with native inline and display equations. Math is laid
out by `katex-native`; surrounding prose stays selectable terminal text. No
browser, Node.js, or external TeX installation is needed at runtime.

mdviewer prints into terminal scrollback and exits. It supports Kitty graphics
with text fallbacks; interactive paging and resize handling are planned.

## Build and run

A Rust 1.90 or newer toolchain and a separate `katex-native` checkout are
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
mdviewer tests/fixtures/style.md
mdviewer tests/fixtures/code.md
mdviewer tests/fixtures/tables.md
mdviewer tests/fixtures/images.md
mdviewer tests/fixtures/mermaid.md
```

To run without installing, use `cargo run --locked -- <arguments>`. Omit the file
argument or pass `-` to read Markdown from stdin.

## Terminal rendering

Terminal output uses a dark color palette, distinct heading levels, tinted inline
code, shaded code blocks, and colored links and list markers. Lists and quotes
keep their indentation when lines wrap. Use `--light` for a light background.

Fenced code uses syntax colors for Rust, Python, JavaScript/JSX, TypeScript/TSX,
JSON, Bash/shell, Go, TOML, and YAML. Common aliases such as `rs`, `py`, `js`,
`ts`, `sh`, and `yml` are accepted without case sensitivity. Multiline comments
and strings retain their context before the code is wrapped.

Use `--no-highlight` to keep the rest of the theme while disabling syntax colors.
Unknown languages and indented code retain the normal code panel. Highlighting
has bounded work: oversized or expensive blocks fall back to uncolored code
without losing content. No grammar downloads or helper processes are needed.

Tables retain left, center, and right alignment and wrap cells within the reading
column. Headers use accent colors; inline styles and equations remain supported.
Narrow windows or excessive column counts use numbered cells, with blank lines
between rows. Oversized rows use the same fallback, preserving all content and
selectable source TeX. Width estimation and temporary row storage have explicit
limits; see [table rendering](docs/tables.md).

Local PNG, JPEG, and SVG images render in graphics-capable sessions. Paths are
relative to the Markdown file, or the working directory for stdin; `--base-dir`
overrides that directory. Every image retains a selectable `[image: alt] (path)`
caption. Missing, unsupported, or over-limit images keep that reference.

Images fit the reading column and viewport without enlargement. Inline image
references become separate blocks; images inside tables remain text references.
SVG supports bounded shapes, gradients, clipping, and text using bundled fonts.
Filters, masks, reusable elements, nested images, external resources, and text
with unavailable glyphs fall back to the reference. No network requests, system
font scans, or helper processes are used. See [local images](docs/images.md) for
the supported subset and resource limits.

Mermaid fences render natively for an initial ASCII subset: flowcharts, sequence,
class and state diagrams, and ER relationships without attribute tables.
`--show-mermaid-source` keeps selectable code beneath the diagram;
`--mermaid off` keeps the code alone. Unsupported syntax, non-ASCII source,
missing glyphs, errors, and limits retain the complete code panel. The native
backend uses one lazy worker from the same executable, with no additional
installation. It does not provide full Mermaid.js compatibility. See
[Mermaid support](docs/mermaid.md) for the limits and measurements.

The reading column is at most 80 cells wide, with two-column horizontal margins.
Text and equations share that column in both graphics and TeX modes. The width
fits the terminal, and margins shrink automatically in narrow windows.

```sh
mdviewer --width 100 --margin 3 notes.md
mdviewer --width 0 --margin 0 notes.md  # Use the available terminal width.
```

The default `--math auto` enables graphics in sessions identified as Ghostty or
Kitty by `TERM` or `TERM_PROGRAM`. This uses environment hints, assumes graphics
are enabled, and does not probe the terminal or read keyboard input.
`--images auto` uses the same detection independently of the math setting.
`--mermaid auto` independently applies it to Mermaid fences.

Other terminals use selectable TeX. Recognized tmux, screen, and Zellij sessions,
`TERM=dumb`, and very small windows also use TeX. Multiplexer graphics passthrough
is not implemented.

| Option | Behavior |
| --- | --- |
| `--math tex` | Keep all equations as selectable source TeX. |
| `--math kitty` | Enable graphics in another compatible terminal; pipe, multiplexer, and small-window restrictions still apply. |
| `--show-math-source` | Print selectable TeX beneath rendered equations. |
| `--images off` | Keep image references as text and skip image file access. |
| `--images kitty` | Enable local images in another compatible terminal, subject to the same graphics restrictions. |
| `--base-dir <directory>` | Override the directory used to resolve local image paths. |
| `--mermaid off` | Keep Mermaid fences as code and skip the diagram worker. |
| `--mermaid kitty` | Enable supported diagrams in another compatible terminal, subject to the same graphics restrictions. |
| `--show-mermaid-source` | Print selectable code beneath rendered Mermaid diagrams. |
| `-w, --width <columns>` | Set the maximum content width, excluding margins; `0` uses available space. |
| `--margin <columns>` | Set the horizontal margin, default `2`. |
| `--light` | Use a light color palette for text and equations. |
| `--no-highlight` | Disable code syntax colors while retaining the theme and layout. |
| `--font-size <pixels>` | Override the equation font size, from 4 to 256 pixels. |

Equation size follows the terminal cell height by default, and oversized equations
shrink to fit. Cell dimensions are read on Unix when available, with an estimated
fallback. Font baselines are estimated; appearance can vary by font and terminal.
Background detection is not implemented. A nonempty `NO_COLOR` disables text
styling without disabling images.

## Copying and pipes

Normal terminal selection copies prose. Selecting an equation image alone does
not recover its TeX; use `--show-math-source` or `--math tex` when copying math.
Image captions and source paths are selectable too; selecting pixels does not
copy an image file or recover text drawn inside it.
Use `--show-mermaid-source` to copy diagram code through terminal selection.

```sh
cat notes.md | mdviewer -
mdviewer --output text notes.md
mdviewer notes.md > notes.txt
mdviewer --output source notes.md | pbcopy  # macOS
```

Pipes receive readable text with source TeX, image references, and Mermaid code,
even with forced graphics: no ANSI
styles, graphics, added margins, or screen-width wrapping. `--output text`
selects that representation on a terminal too. Diagnostics go to stderr.

`--output source` preserves the original UTF-8 Markdown byte for byte, including
line endings and final-newline state. It bypasses parsing and control-character
filtering. Clipboard commands such as `pbcopy` are external; mdviewer does not
change the clipboard itself.

## Markdown and math

- Headings, emphasis, lists, quotes, code, links, task lists, strikethrough,
  footnotes, and aligned, wrapped tables.
- `$...$` inline math, `$$...$$` display math, and fenced `math` blocks.
- Equation tags and automatic numbering, with tags placed after the formula.
- Bounded local PNG/JPEG/SVG images with selectable captions and text fallbacks.
- Native rendering of supported Mermaid fences, with optional selectable source.
- Unicode-aware terminal wrapping, preserving graphemes and keeping
  equations whole.
- Individual formula failures fall back to source TeX without interrupting the
  rest of the document.

Math support follows the KaTeX subset implemented by `katex-native`.
Backslash-parenthesis and backslash-bracket math delimiters are not recognized.

Complex combinations of nested blocks still need layout improvements.
Embedded languages inside code strings or HTML are not
highlighted separately; unsupported fences remain uncolored.
Remote image URLs remain text references. HTML is not executed.

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
windows, Unicode, reading width, themes, and image/text placement. Visual checks
in real terminals are still needed to assess font alignment, scrolling, and
selection behavior.

See [architecture](docs/architecture.md) for module boundaries and contracts,
[feature design](docs/feature-design.md) for planned rendering features and
resource limits, and [the roadmap](docs/roadmap.md) for implementation order.
The [performance baseline](docs/performance-baseline.md) records comparisons with
Glow and commands for repeating them without keeping large inputs in the repo.
The [syntax highlighting measurements](docs/syntax-highlighting.md) record its
implementation limits, dependency costs, and comparison with that baseline.
The [rendering optimizations](docs/performance-optimizations.md) record subsequent
reductions in time, memory, output volume, and executable size.
The [table implementation](docs/tables.md) records its limits and measured cost.
The [local image implementation](docs/images.md) records decoder, cache, output,
and SVG limits, together with its measurements.
The [Mermaid implementation](docs/mermaid.md) records native coverage, worker
limits, and comparisons with the official renderer.
Crate publication is disabled while `katex-native` uses a local path dependency.

## License

MIT; see [LICENSE](LICENSE). `katex-native` is MIT, and its bundled KaTeX fonts use
the SIL Open Font License 1.1; see `../katex-native/fonts/OFL.txt`.
