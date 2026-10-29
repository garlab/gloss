# Architecture

mdviewer separates the original document, layout, math rasterization, and terminal
output. The Rust library API is not yet stable; these boundaries guide further
implementation.

## Data flow

```text
UTF-8 Markdown
├── exact source output
└── Document: original source → borrowed event iterator + byte ranges
    └── Layout: text runs + code blocks + table cells + display math blocks
        ├── plain text export with source TeX
        └── reading column + semantic styles + cell-aware lines
            ├── themed terminal text with source TeX
            └── math rasters → Kitty graphics and native text
```

The CLI chooses an output mode before rendering. Non-terminal stdout always uses
text export unless exact source output was requested. Math is laid out only when
graphics are selected.

`Document::events()` parses on iteration and borrows unchanged text from the
source. Each call starts a new parser; the document does not retain an owned
event vector alongside the layout. Layout still owns its decoded text runs.

## Module boundaries

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Arguments, input, output policy, errors, and diagnostics. |
| `src/document.rs` | CommonMark events, original Markdown, and source ranges; normalize fenced math. |
| `src/layout.rs` | Convert events into text runs, code/table blocks, indentation context, and display math blocks. |
| `src/layout/table.rs` | Retain rows, cells, alignments, inline runs, and source spans. |
| `src/highlight/` | Lazily compile bundled language queries and produce bounded semantic UTF-8 token spans. |
| `src/layout/lines.rs` | Wrap borrowed word fragments at grapheme boundaries and measure inline baselines, independently of escape sequences. |
| `src/math.rs` | Adapt `katex-native`, fit bounded rasters, combine equation tags, and track numbering. |
| `src/terminal.rs` | Observe stdout, environment hints, multiplexers, and cell geometry. |
| `src/export.rs` | Generate readable plain text or return the original source. |
| `src/render/mod.rs` | Coordinate rendering and per-formula fallbacks. |
| `src/render/text.rs` | Emit ANSI attribute changes, reset line styles, and filter input control characters. |
| `src/render/code.rs` | Highlight one code block before line wrapping, preserving source spans and uncolored fallbacks. |
| `src/render/table.rs` | Estimate bounded column widths and render one row at a time, with numbered-cell fallback. |
| `src/render/inline.rs` | Format logical lines, preserve list/quote prefixes, and arrange text and images within reserved rows. |
| `src/render/theme.rs` | Map semantic styles to dark/light terminal palettes and equation ink. |
| `src/render/viewport.rs` | Bound the reading width and adapt margins to terminal geometry. |
| `src/render/kitty.rs` | Pad images to cell boundaries and transmit chunked graphics commands. |

`katex-native` owns TeX parsing, equation layout, fonts, and rasterization.
Terminal-specific behavior belongs here. Additional graphics protocols should
reuse the document model and layout rather than introduce separate parsers.

Code blocks retain their language label, original source span, and decoded text
runs. Terminal rendering highlights one block at a time, retaining multiline
syntax state without storing token runs for the whole document. Plain/source
export, disabled highlighting, and disabled color bypass the highlighter.

Tables retain cell runs and source spans instead of flattening their delimiters.
The renderer samples bounded text to choose column widths, then wraps and aligns
one row at a time. Cells reuse the inline text/math preparation and line painter,
including shared baselines and selectable TeX. Narrow or excessive tables and
over-limit rows use numbered cells with source TeX. Plain export still builds
flat runs, preserving its previous separators and avoiding table measurement or
cell storage. [Table rendering](tables.md) defines the limits.

Text rendering consumes logical lines with one-line lookahead for quote context.
Wrapping borrows word fragments and allocates text for the resulting physical
lines, rather than allocating every grapheme. The writer shares ANSI state
between spans within a physical line and resets it before graphics and at the
line boundary. The original source, full layout, parser working storage, and a
single very long logical line can still require memory proportional to input;
this is not a constant-memory document reader.

The [feature design](feature-design.md) describes shared image placement,
optional renderers, and math-library extraction
boundaries. These are future changes, not additional modules already present.

## Contracts to preserve

- **Source remains authoritative.** Original UTF-8 bytes survive independently of
  pixels and styles. Runs retain source spans and original TeX. Exact source
  output preserves line endings and final-newline state.
- **Text output is independent of graphics.** Plain export contains readable text
  and TeX without terminal controls or viewport-width wrapping. It never reads
  back rendered output. Diagnostics go to stderr.
- **Rendered input is filtered.** User-provided text cannot emit terminal control
  sequences through styled or plain rendering. Exact source output deliberately
  bypasses filtering.
- **Failures stay local.** Invalid formulas retain their source while later text
  and equations continue rendering. Image dimensions and raster allocation are
  bounded. Inline rendering does not advance display equation numbering.
- **Layout respects terminal cells.** Wrapping preserves graphemes and keeps
  images whole. Text and equations share the same width and margins, including
  when graphics are disabled. Inline layout reserves rows above and below the
  baseline. Images are padded to cell dimensions to avoid stretching, and space
  is reserved before drawing near the bottom of the terminal.
- **Terminal discovery does not consume document input.** Current discovery uses
  environment hints and geometry only. Any future query handling must preserve
  stdin and user input and have bounded timeouts.
- **Optional rendering has bounded cost.** New highlighters, decoders, helpers,
  and caches need explicit work/allocation limits and lazy initialization.
  Disabled or absent features must not burden plain/source export. Exhausted
  decoration budgets preserve complete source content in a readable fallback.

Source ranges currently identify whole runs. Precise selection within decoded
entities needs a finer mapping. Font baselines and missing cell dimensions are
estimated, which requires visual validation across terminals and fonts.

## Validation

`tests/integration.rs` covers parsing, exact source and readable text exports,
control-character filtering, formula fallbacks, numbering, and graphics payloads.
Unit tests cover line measurements, wrapping, and terminal hints.
`tests/terminal_smoke.py` exercises the CLI through Unix pseudo-terminals and
checks image/text placement. Neither suite establishes visual compatibility with
a particular terminal version.

Theme colors and decorative prefixes are applied only during terminal rendering.
Width, margins, heading markers, quote rails, and code padding never enter plain
text or exact source exports. `NO_COLOR` disables styling while keeping the
reading layout and the selected math mode.

Use the fixtures for manual checks at normal and narrow widths, with tall
equations, light and dark backgrounds, and output near the bottom of the screen.
Check text selection and selectable TeX as well as appearance. Development
commands are in the [README](../README.md).

The [performance baseline](performance-baseline.md) records timing, first output,
peak RSS, and output volume. The feature design defines regression review
triggers and the additional measurements needed for images and helper processes.
The [rendering optimization report](performance-optimizations.md) records the
current allocation and output improvements against the initial highlighter.
