# Architecture

mdviewer separates the original document, layout, raster generation, and terminal
output. The Rust library API is not yet stable; these boundaries guide further
implementation.

## Data flow

```text
UTF-8 Markdown
├── exact source output
└── Document: original source → optional syntax projection → events + original ranges
    └── Completed blocks: text + code + table cells + math + local images
        ├── plain text export with source TeX
        └── reading column + semantic styles + cell-aware lines
            ├── themed terminal text with source TeX
            └── math/local image rasters → Kitty graphics and native text
```

The CLI chooses an output mode before rendering. Non-terminal stdout always uses
text export unless exact source output was requested. Math is laid out only when
graphics are selected.
Local image blocks are built only when image graphics are enabled. Disabled or
unsupported graphics retain the existing flat image references and never open
image files. Text and exact source exports bypass all image decoding.

`Document::events()` parses on iteration and borrows text from the source or an
optional syntax projection. Each call starts a fresh parser. Extension candidates
can require preliminary range discovery and an offset map; exact source bytes
remain untouched. There is no retained event vector.

`layout::visit()` emits completed blocks. The CLI renders with one-block lookahead
for quote rails, then releases each block. Plain export writes completed blocks
directly too. `layout::layout()` and `render::write()` remain available to callers
that need a retained model. Code blocks and formatted tables remain whole; plain tables stream rows. Compact
optional source ranges keep link metadata from enlarging every retained run.
The full source and parser working storage still scale with input.

## Module boundaries

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Arguments, input, output policy, errors, and diagnostics. |
| `src/document.rs` | CommonMark events, original Markdown, and source ranges; normalize fenced math. |
| `src/document/extensions.rs` | Bounded front matter/details/math projections with original-source offset maps. |
| `src/links.rs` | Validate bounded web/mail hyperlink targets without opening them. |
| `src/layout.rs` | Convert events into text runs, code/table/image blocks, indentation context, and display math blocks. |
| `src/layout/table.rs` | Retain rows, cells, alignments, inline runs, and source spans. |
| `src/highlight/` | Lazily compile bundled language queries and produce bounded semantic UTF-8 token spans. |
| `src/layout/lines.rs` | Wrap borrowed word fragments at grapheme boundaries and measure inline baselines, independently of escape sequences. |
| `src/math.rs` | Adapt `katex-native`, fit bounded rasters, combine equation tags, track numbering, and cache fitted rasters within a byte budget. |
| `src/raster.rs` | Terminal-independent dimensions and straight RGBA pixels. |
| `src/images.rs` | Resolve local references, bound file/decode work, and cache fitted rasters by content. |
| `src/mermaid.rs` | Own the terminal invocation budget and fitted diagram cache; delegate rendering to `document-graphics`. |
| `src/terminal.rs` | Observe stdout, environment hints, multiplexers, and cell geometry. |
| `src/export.rs` | Generate readable plain text or return the original source. |
| `src/render/mod.rs` | Coordinate rendering and per-formula fallbacks. |
| `src/render/text.rs` | Emit ANSI attribute changes, reset line styles, and filter input control characters. |
| `src/render/code.rs` | Highlight one code block before line wrapping, preserving source spans and uncolored fallbacks. |
| `src/render/diagram.rs` | Offer Mermaid fences to the worker before code rendering; retain source fallback and optional selectable code. |
| `src/render/table.rs` | Estimate bounded column widths and render one row at a time, with numbered-cell fallback. |
| `src/render/inline.rs` | Format logical lines, preserve list/quote prefixes, and arrange text and images within reserved rows. |
| `src/render/theme.rs` | Map semantic styles to dark/light terminal palettes and equation ink. |
| `src/render/viewport.rs` | Bound the reading width and adapt margins to terminal geometry. |
| `src/render/kitty.rs` | Pad images, compress beneficial payloads, and base64-encode fixed-size graphics chunks. |

`katex-native` owns TeX parsing, equation layout, fonts, and rasterization.
`document-graphics` is a separate library repository with `mermaid`, `svg` and
`worker` modules. It owns shared Mermaid validation/layout, bounded SVG/font
rasterization, worker protocol, cancellation and process supervision.
It returns owned RGBA buffers and logical sizes; consumers move those buffers into
their image types. Terminal caches, total time/output budgets and fitting remain
in mdviewer. The shared crate contains no global queue or application lifecycle.
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

Image blocks retain alt text, the original target, source range, and list/quote
context. They render one at a time with selectable captions, including on decode
failure. The loader keeps a bounded invocation-local cache of encoded content
and fitted pixels. It registers bundled SVG text fonts only when required.
The line painter accepts shared rasters with optional baseline/source metadata;
equations and ordinary images reuse row reservation, cell padding, quote rails,
and transport. Table separators and quote rails cover every reserved image row.
Images within table cells remain references. [Local images](images.md) defines
the supported formats and limits.

Mermaid stays in the existing code-block model, including its source range and
indentation. The terminal renderer recognizes the fence language before generic
highlighting. A cache miss can start one child of the same executable, using a
bounded private request/reply protocol. The child parses/layouts native diagrams
and rasterizes through `document-graphics` with bundled fonts. Only that generated
SVG path admits bounded, nonrecursive arrow markers; local SVG restrictions
remain unchanged. The parent validates returned dimensions and byte counts,
fits the usual reading column, and shares the local-image output budget.
Timeouts/crashes disable further worker requests, while cached images and all
remaining document text remain available. [Mermaid support](mermaid.md) defines
coverage, cache identity, limits, worker cleanup, and measurements.

Text rendering consumes logical lines with one-line lookahead for quote context.
Wrapping borrows word fragments and allocates text for the resulting physical
lines, rather than allocating every grapheme. The writer shares ANSI state
between spans within a physical line and resets it before graphics and at the
line boundary. OSC 8 targets travel independently of text styling and close after
each painted span. Source, optional projections, parser working storage, large
individual blocks, and very long logical lines can still scale with input; this
is not a constant-memory document reader.

Math rasters use an invocation-local 8 MiB/64-entry LRU cache. Keys include TeX,
inline/display mode, font size, color, bounds, and equation-numbering state.
Shared pixels avoid cache-hit copies; numbering is advanced exactly as on a miss.
Kitty transport applies fast zlib compression when it saves bytes, then encodes
3,072-byte chunks into a 4,096-byte stack buffer. Transmissions still use
terminal-allocated IDs, preserving earlier scrollback. Terminal-side image reuse
and its lifecycle are separate follow-up work.

The [feature design](feature-design.md) describes additional rendering features
and math-library extraction boundaries.

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
  and equations continue rendering. Failed local images keep their captions;
  failed diagrams keep complete code.
  Image dimensions and raster allocation are
  bounded. Inline rendering does not advance display equation numbering.
- **Layout respects terminal cells.** Wrapping preserves graphemes and keeps
  images whole. Text and equations share the same width and margins, including
  when graphics are disabled. Inline layout reserves rows above and below the
  baseline. Images are padded to cell dimensions to avoid stretching, and space
  is reserved before drawing near the bottom of the terminal.
- **Terminal discovery does not consume document input.** Current discovery uses
  environment hints, geometry, and explicit overrides only; see
  [terminal discovery](terminal-detection.md). Any future query handling must
  preserve stdin and user input and have bounded timeouts.
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
reading layout and the selected math/image modes.

Use the fixtures for manual checks at normal and narrow widths, with tall
equations, light and dark backgrounds, and output near the bottom of the screen.
Check text selection and selectable TeX as well as appearance. Development
commands are in the [README](../README.md).

The [performance baseline](performance-baseline.md) records timing, first output,
peak RSS, and output volume. The feature design defines regression review
triggers and the additional measurements needed for images and helper processes.
The [rendering optimization report](performance-optimizations.md) records the
current allocation and output improvements against the initial highlighter.
