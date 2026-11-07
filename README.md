# gloss

The Cargo package is `gloss-cli`; the executable and Homebrew formula
are named `gloss`.

A terminal viewer for Markdown, math, diagrams, images, and code. Math is laid
out by `katex-native`; surrounding prose stays selectable terminal text. No
browser, Node.js, or external TeX installation is needed at runtime.

gloss prints into terminal scrollback and exits by default. Use `--pager` for
interactive reading or `--watch` for live file updates. Kitty graphics have text
fallbacks on other terminals.

## Install

With Homebrew, once the tap's initial bottle publication is complete:

```sh
brew install garlab/tap/gloss
```

Bottles cover Apple Silicon on macOS 26 or newer and Linux x86-64. Other
supported platforms build from source. The
[tap](https://github.com/garlab/homebrew-tap) follows stable crates.io releases
automatically.

## Build and run

A Rust 1.90 or newer toolchain is required. Math uses the published
[`katex-native`](https://crates.io/crates/katex-native) crate. Diagram, SVG and PNG/JPEG
rendering uses the [`document-graphics`](https://crates.io/crates/document-graphics)
library and its native `document-graphics-mermaid` backend. Cargo downloads these
dependencies from crates.io. gloss enables the library's
`images` and `sgf` features; native image codecs and image workers remain disabled.

Install the `gloss-cli` package to get the `gloss` executable. Cargo's binary
directory must be on your `PATH`; building also requires a C compiler for the
syntax grammars.

```sh
cargo install gloss-cli --locked
```

To build from a checkout and try the included examples, run these commands from
the `gloss` directory:

```sh
cargo install --path . --locked
gloss tests/fixtures/math.md
gloss --show-math-source tests/fixtures/inline.md
gloss tests/fixtures/style.md
gloss tests/fixtures/code.md
gloss tests/fixtures/tables.md
gloss tests/fixtures/images.md
gloss tests/fixtures/mermaid.md
gloss tests/fixtures/gantt-time.md
gloss tests/fixtures/sgf.md
gloss tests/fixtures/extensions.md
```

To run without installing, use `cargo run --locked -- <arguments>`. Omit the file
argument or pass `-` to read Markdown from stdin.

To develop document-graphics alongside gloss, place its checkout beside
gloss and add this optional override to an untracked `.cargo/config.toml`
(merging with any existing settings):

```toml
[patch.crates-io]
document-graphics = { path = "../document-graphics" }
# Optional while also developing the backend:
# document-graphics-mermaid = { path = "../document-graphics-mermaid" }
```

Run Cargo from the gloss root so it reads the local override. To return to
published dependencies, remove the overrides and run `cargo check` to restore
registry sources and checksums in `Cargo.lock`.

## Standalone inputs

Named `.mmd` / `.mermaid` files render as Mermaid, `.sgf` files as Go records,
and `.png` / `.jpg` / `.jpeg` / `.svg` files as images. Code files reuse the fenced
code renderer, so comments, Markdown delimiters, and math stay literal:

| Language | File extensions |
| --- | --- |
| YAML | `.yaml`, `.yml` |
| TOML | `.toml` |
| JSON | `.json` |
| Rust | `.rs` |
| Python | `.py`, `.pyi`, `.pyw` |
| JavaScript / JSX | `.js`, `.jsx`, `.mjs`, `.cjs` |
| TypeScript | `.ts`, `.mts`, `.cts` |
| TSX | `.tsx` |
| Go | `.go` |
| Bash / shell | `.sh`, `.bash` |

Extensions are case insensitive; other files and stdin default to Markdown.
All input formats work with `--pager` and named files support `--watch`.
`Cargo.lock` uses TOML highlighting. `Dockerfile`, `Dockerfile.dev`,
`build.Dockerfile`, and equivalent `Containerfile` names are recognized as code.
Dockerfile and Containerfile inputs share syntax colors, including shell commands
and supported heredocs.

```sh
gloss diagram.mmd
gloss --watch diagram.mermaid
gloss picture.png
gloss --pager record.sgf
gloss --sgf-move 42 record.sgf
gloss config.yaml
gloss --watch config.toml
gloss Cargo.lock
gloss Dockerfile
gloss --input-format code settings.conf
cat config.yaml | gloss --language yaml -
cat diagram.mmd | gloss --input-format mermaid -
```

Use `--input-format markdown|code|mermaid|sgf|image` to override detection.
`--language` selects code mode and overrides the detected syntax language; it
also works for stdin and extensionless files. Use it with automatic detection or
explicit code mode. Unknown languages use an uncolored code panel. Explicit code
mode always keeps source literal, including Mermaid/SGF files.

Standalone code is one block and uses the same highlighting limits as fenced
code: files above 128 KiB retain all content without syntax colors. Code,
Mermaid, SGF, and pager text inputs are limited to 64 MiB.

Image input requires a named local file; explicit image mode can recognize PNG/JPEG/SVG
without a recognized extension. `--base-dir` applies to Markdown image references,
while standalone image paths are used directly. Text inputs must be UTF-8.

Pipes and text-only terminals retain code/diagram source or an image reference.
`--output source` preserves standalone code/Mermaid/SGF text exactly. It rejects image
inputs; use the original image file for binary copying. Image stdin is not supported.

## Terminal rendering

Terminal output uses distinct heading levels, tinted inline
code, shaded code blocks, and colored links and list markers. Lists and quotes
keep their indentation when lines wrap. The default theme uses `COLORFGBG` when
available and otherwise selects dark. Use `--theme light` (or `--light`) and
`--theme dark` to choose explicitly.

GitHub alerts retain colored labels. Leading YAML/TOML-style front matter stays
visible as inert metadata. Supported details/summary sections expand, and
`\(...\)` / `\[...\]` join the existing math delimiters. Web/mail links are
clickable in recognized Ghostty/Kitty sessions, with visible targets retained.
Use `--links off` to disable hyperlinks. See [Markdown extensions](docs/markdown-extensions.md)
for syntax, limits, and fallback behavior.

Fenced code uses syntax colors for Rust, Python, JavaScript/JSX, TypeScript/TSX,
JSON, Bash/shell, Go, TOML, YAML, and Dockerfile/Containerfile. Common aliases such
as `rs`, `py`, `js`, `ts`, `sh`, `yml`, and `docker` are accepted without case
sensitivity. Multiline comments and strings retain their context before the code
is wrapped.

Dockerfile shell commands and `RUN` heredocs use Bash highlighting. `COPY` heredocs
targeting JSON, YAML, or TOML files use those grammars. Embedded regions share the
outer block's highlighting budget, with at most 128 regions; exhausted limits
keep the complete block uncolored. See [syntax highlighting](docs/syntax-highlighting.md).

Use `--no-highlight` to keep the rest of the theme while disabling syntax colors.
Unknown languages and indented code retain the normal code panel. Highlighting
has bounded work: oversized or expensive blocks fall back to uncolored code
without losing content. No grammar downloads or helper processes are needed.

Code line numbers default to `--line-numbers auto`: enabled for standalone code
and disabled for Markdown blocks. Use `--line-numbers on` to number all code
blocks or `--line-numbers off` to hide them. Each block starts at 1; blank source
lines count, and wrapped continuations leave the number column empty. Very narrow
views omit the number column to preserve space for code.

Numbers remain visible with `--no-highlight` or `NO_COLOR`. Pipes, `--output text`,
and `--output source` omit them. Use `--line-numbers off` for mouse selection
without the number column.

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

Mermaid fences render flowcharts, sequence, class and state diagrams, ER tables,
pie charts, mind maps, timelines, user journeys, quadrant charts and
[Gantt schedules with date and sub-day timing](docs/gantt.md). Embedded fonts
cover Latin (including accents), Greek and Cyrillic labels.
Diagrams use a transparent canvas and scale labels to terminal cell height,
including high-density displays, then shrink to fit the reading column.
Tall diagrams continue down the page in contiguous image bands, including inside
the pager, instead of shrinking to a single screen; raster and output limits still apply.
`--show-mermaid-source` keeps selectable code beneath the diagram;
`--mermaid off` keeps the code alone. Unsupported syntax, missing glyphs,
errors, and limits retain the complete code panel. The native
backend uses one lazy worker from the same executable, with no additional
installation. It does not provide full Mermaid.js compatibility. See
[Mermaid support](docs/mermaid.md) for the limits and measurements.

SGF fences render native Go boards, including setup stones, captures and board
annotations. By default they show the final position of the first game's main
line. `--sgf-game` and `--sgf-move` select another game or earlier move; fence
attributes such as `sgf game=2 move=12` override those defaults per board.
Move `0` selects the root, and `move=last` selects the final main-line node.
Every board has a selectable game/move caption. Use `--show-sgf-source` to retain
the record below it or `--sgf off` to show code alone. Invalid, unsupported or
over-limit records retain source. See [SGF support](docs/sgf.md).

The reading column is at most 80 cells wide, with two-column horizontal margins.
Text and equations share that column in both graphics and TeX modes. The width
fits the terminal, and margins shrink automatically in narrow windows.

```sh
gloss --width 100 --margin 3 notes.md
gloss --width 0 --margin 0 notes.md  # Use the available terminal width.
```

The default `--math auto` enables graphics in sessions identified as Ghostty or
Kitty by `TERM` or `TERM_PROGRAM`. This uses environment hints, assumes graphics
are enabled, and does not probe the terminal or read keyboard input.
`--images auto` uses the same detection independently of the math setting.
`--mermaid auto` and `--sgf auto` independently apply it to diagram fences and
standalone records.

Other terminals use selectable TeX. Recognized tmux, screen, and Zellij sessions,
`TERM=dumb`, and very small windows also use TeX. Multiplexer graphics passthrough
is not implemented.

| Option | Behavior |
| --- | --- |
| `--math tex` | Keep all equations as selectable source TeX. |
| `--math kitty` | Enable graphics in another compatible terminal; pipe, multiplexer, and small-window restrictions still apply. |
| `--show-math-source` | Print selectable TeX beneath rendered equations. |
| `--images off` | Keep image references as text and skip image content reads and decoding. |
| `--images kitty` | Enable local images in another compatible terminal, subject to the same graphics restrictions. |
| `--base-dir <directory>` | Override the directory used to resolve local image paths. |
| `--mermaid off` | Keep Mermaid fences as code and skip the diagram worker. |
| `--mermaid kitty` | Enable supported diagrams in another compatible terminal, subject to the same graphics restrictions. |
| `--show-mermaid-source` | Print selectable code beneath rendered Mermaid diagrams. |
| `--sgf auto\|off\|kitty` | Select native Go board rendering, independently of other graphics. |
| `--sgf-game <number>` | Select a one-based game in a collection; default `1`. |
| `--sgf-move <number>` | Select a main-line move; `0` is the root, and omission selects the final position. |
| `--show-sgf-source` | Print selectable SGF beneath rendered boards. |
| `-w, --width <columns>` | Set the maximum content width, excluding margins; `0` uses available space. |
| `--margin <columns>` | Set the horizontal margin, default `2`. |
| `--theme auto\|dark\|light` | Select the palette; auto uses an optional `COLORFGBG` hint. |
| `--light` | Alias for `--theme light`. |
| `--cell-size <width>x<height>` | Override terminal cell dimensions in pixels, from 1 to 512 each. |
| `--links auto\|off\|always` | Control validated OSC 8 hyperlinks; plain/source exports remain independent. |
| `--no-highlight` | Disable code syntax colors while retaining the theme and layout. |
| `--font-size <pixels>` | Override the equation font size, from 4 to 256 pixels. |

Equation size follows the terminal cell height by default, and oversized equations
shrink to fit. Cell dimensions are read on Unix when available, with an estimated
fallback. Font baselines are estimated; appearance can vary by font and terminal.
Theme hints are estimates and can be overridden. A nonempty `NO_COLOR` disables
text styling without disabling images or hyperlinks. See
[terminal discovery](docs/terminal-detection.md) for precedence and limitations.

## Pager and live updates

```sh
gloss --pager notes.md
gloss --watch notes.md
cat notes.md | gloss --pager -
```

`-p` / `--pager` opens an alternate screen on Unix terminals (including macOS).
Text reflows when the window changes size; equations, images, and diagrams scroll
with it. The previous terminal screen is restored on exit. Keyboard input uses
the controlling terminal, so piped Markdown remains usable.

| Key | Action |
| --- | --- |
| `j` / `k`, arrows, Enter | Scroll down/up one row; Enter scrolls down. |
| Space / `b`, Page Down / Page Up | Move a page, keeping one row of overlap. |
| `d` / `u`, Ctrl-D / Ctrl-U | Move half a page. |
| `g` / `G`, Home / End | Go to the beginning/end. |
| `/` / `?`, Enter | Search forward/backward for literal, case-sensitive rendered text. |
| `n` / `N` | Repeat/reverse the last search, wrapping through the document. |
| `r` | Reload the file and its local images; rerender piped input. |
| `w` / `h` | Cycle rendering warnings / show key hints. |
| `q`, Escape, Ctrl-C | Quit; Escape first cancels an open search prompt. |
| Ctrl-Z | Restore the terminal and suspend; `fg` resumes. |

`--watch` implies the pager and requires a named regular file and interactive
stdout. It detects in-place and atomic saves after a short debounce, retains the
last good page through temporary read/render errors, and preserves the current
source block when possible. Staying at the end follows newly appended content.
Only the Markdown file is watched; press `r` after changing an image alone.

Mouse tracking is disabled, preserving native text selection in Ghostty. Copying
uses the same source options described below. Search covers terminal text, not
letters inside rasterized equations or diagrams. Bracketed paste is ignored to
avoid treating pasted text as commands.

`--pager` is ignored for redirected output, `--output text`, `--output source`,
and `TERM=dumb`. `--watch` rejects those modes. Ordinary scrollback output is
unchanged. The pager has a 64 MiB input limit and bounded temporary storage;
oversized inputs receive an error instead of being truncated. See
[pager implementation](docs/pager.md) for resource limits and validation.

## Copying and pipes

Normal terminal selection copies prose. Selecting an equation image alone does
not recover its TeX; use `--show-math-source` or `--math tex` when copying math.
Image captions and source paths are selectable too; selecting pixels does not
copy an image file or recover text drawn inside it.
Use `--show-mermaid-source` or `--show-sgf-source` to copy diagram code through
terminal selection.

```sh
cat notes.md | gloss -
gloss --output text notes.md
gloss notes.md > notes.txt
gloss --output source notes.md | pbcopy  # macOS
```

Pipes receive readable text with source TeX, image references, and Mermaid/SGF code,
even with forced graphics: no ANSI
styles, graphics, added margins, or screen-width wrapping. `--output text`
selects that representation on a terminal too. Diagnostics go to stderr.

`--output source` preserves the original UTF-8 Markdown byte for byte, including
line endings and final-newline state. It bypasses parsing and control-character
filtering. Clipboard commands such as `pbcopy` are external; gloss does not
change the clipboard itself.

## Markdown and math

- Headings, emphasis, lists, quotes, code, links, task lists, strikethrough,
  footnotes, and aligned, wrapped tables.
- `$...$` / `\(...\)` inline math, `$$...$$` / `\[...\]` display math, and fenced `math` blocks.
- GitHub alerts, inert front matter, expanded details, and validated terminal links.
- Equation tags and automatic numbering, with tags placed after the formula.
- Bounded local PNG/JPEG/SVG images with selectable captions and text fallbacks.
- Native rendering of supported Mermaid fences, with optional selectable source.
- Unicode-aware terminal wrapping, preserving graphemes and keeping
  equations whole.
- Individual formula failures fall back to source TeX without interrupting the
  rest of the document.

Math support follows the KaTeX subset implemented by `katex-native`.

Complex combinations of nested blocks still need layout improvements.
Embedded-language highlighting is currently limited to Dockerfile shell commands
and the supported heredocs above; unsupported fences remain uncolored.
Remote image URLs remain text references. HTML is not executed.

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo +1.90.0 check --locked --all-targets
cargo build --locked
python3 tests/terminal_smoke.py target/debug/gloss  # Unix, Python 3
python3 tests/pager_smoke.py target/debug/gloss
```

Rust tests cover parsing, source preservation, wrapping, math, and output
contracts. Pseudo-terminal tests cover graphics selection, fallbacks, narrow
windows, Unicode, reading width, themes, and image/text placement. Pager tests
also cover input, search, image cropping/reuse, resize, atomic saves, and terminal
restoration. Visual checks
in real terminals are still needed to assess font alignment, scrolling, and
selection behavior.

[CI](.github/workflows/ci.yml) runs Rust checks on Linux, macOS, and Windows,
Unix pseudo-terminal checks on Linux and macOS, and minimum-Rust and package
verification on Linux. The pager and `--watch` require Unix; Windows uses direct
output. See [releasing](docs/releasing.md) for publishing to crates.io,
tag-triggered releases, and automated Homebrew bottles. Hosted platform checks must
pass before publication.

The [documentation index](docs/README.md) separates feature references, contributor
guides, and historical performance measurements. Start with the
[architecture](docs/architecture.md), [design decisions](docs/feature-design.md),
and [roadmap](docs/roadmap.md) when contributing.

## License

MIT; see [LICENSE](LICENSE). `katex-native` is MIT, and its bundled KaTeX fonts use
the SIL Open Font License 1.1; see the [font license](https://github.com/garlab/katex-native/blob/main/fonts/OFL.txt).
