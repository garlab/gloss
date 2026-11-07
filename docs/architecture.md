# Architecture

gloss separates the original document, layout, raster generation, and terminal
output. The Rust library API is not yet stable; these boundaries guide further
implementation.

## Data flow

```text
Markdown / code / diagram text / image path
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
Input format and code language are resolved once from explicit options or the
named-file extension; stdin defaults to Markdown. `--language` selects code mode
and overrides extension-based language detection. Standalone code/Mermaid/SGF
retain their exact UTF-8 source and yield literal code events without synthetic
Markdown fences. Code mode forwards only supported syntax labels to the renderer,
so selecting a diagram language cannot enable graphics in this mode. Standalone
images yield image events from a native path, without loading image bytes into
the text source model. They export a reference; binary source output is rejected.
Markdown still uses its original parser and syntax projections.
Filename detection also recognizes `Cargo.lock` as TOML and Dockerfile/Containerfile
names as code using the bundled Containerfile grammar. Explicit formats take precedence;
recognized Markdown and image extensions keep their formats.
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
| `src/input.rs` | Resolve input formats/languages, build standalone documents, and read bounded regular text files for code, diagrams, and reloads. |
| `src/document.rs` | CommonMark or literal input events, original source, and source ranges; normalize fenced math. |
| `src/document/extensions.rs` | Bounded front matter/details/math projections with original-source offset maps. |
| `src/links.rs` | Validate bounded web/mail hyperlink targets without opening them. |
| `src/layout.rs` | Convert events into text runs, code/table/image blocks, indentation context, and display math blocks. |
| `src/layout/table.rs` | Retain rows, cells, alignments, inline runs, and source spans. |
| `src/highlight/` | Lazily compile bundled language queries and produce bounded semantic UTF-8 token spans. |
| `src/layout/lines.rs` | Wrap borrowed word fragments at grapheme boundaries and measure inline baselines, independently of escape sequences. |
| `src/math.rs` | Adapt `katex-native`, fit bounded rasters, combine equation tags, track numbering, and cache fitted rasters within a byte budget. |
| `src/raster.rs` | Terminal-independent dimensions and straight RGBA pixels. |
| `src/images.rs` | Resolve local references, enforce invocation budgets, and cache fitted shared-library rasters by content. |
| `src/mermaid.rs` | Own the terminal invocation budget and fitted diagram cache; delegate rendering to `document-graphics`. |
| `src/sgf.rs` | Select a collection game/main-line position and cache bounded native Go previews. |
| `src/terminal.rs` | Observe stdout, environment hints, multiplexers, and cell geometry. |
| `src/export.rs` | Generate readable plain text or return the original source. |
| `src/render/mod.rs` | Coordinate rendering, bounded reusable graphics caches, cancellation, and per-formula fallbacks. |
| `src/render/text.rs` | Emit ANSI attribute changes, reset line styles, and filter input control characters. |
| `src/render/code.rs` | Highlight one code block before line wrapping, preserving source spans and uncolored fallbacks. |
| `src/render/diagram.rs` | Place Mermaid/SGF previews before code rendering, with source fallback, optional code and SGF captions. |
| `src/render/table.rs` | Estimate bounded column widths and render one row at a time, with numbered-cell fallback. |
| `src/render/inline.rs` | Format logical lines, preserve list/quote prefixes, and arrange text and images within reserved rows. |
| `src/render/theme.rs` | Map semantic styles to dark/light terminal palettes and equation ink. |
| `src/render/viewport.rs` | Bound the reading width and adapt margins to terminal geometry. |
| `src/render/kitty.rs` | Place native math pixels or cell-aligned image bands, compress payloads, and base64-encode fixed-size chunks. |
| `src/pager/record.rs` | Record the renderer's own styled rows, source anchors, and image payloads in bounded anonymous temporary files. |
| `src/pager/mod.rs` | Navigation, incremental text search, viewport redraw, and owned Kitty image lifecycle. |
| `src/pager/tty.rs` | Controlling-terminal input, bounded key decoding, alternate screen, signals, and terminal restoration. |
| `src/pager/worker.rs` | One active render and one replaceable pending request, with cooperative cancellation. |
| `src/pager/watch.rs` | Debounced metadata polling, including atomic replacement. |

`katex-native` owns TeX parsing, equation layout, fonts, and rasterization.
`document-graphics` is a separate library repository. Its optional `images`
module owns bounded PNG/JPEG thumbnail decoding; gloss does not enable its
native image codecs or image worker. The strict `svg` entry point retains
bundled fonts and rejects external resources. The library also owns Mermaid
validation/layout, bounded SVG/font rasterization, worker protocol, cancellation
and process supervision. Its optional `sgf` module owns parsing, Go position
reconstruction and board rasterization. gloss selects the displayed position
and retains its own cache and invocation budgets.
File resolution, allowed URI schemes and invocation limits remain here.
It returns owned RGBA buffers and logical sizes; consumers move those buffers into
their image types. Terminal caches, total time/output budgets and fitting remain
in gloss. The shared crate contains no global queue or application lifecycle.
Terminal-specific behavior belongs here. Additional graphics protocols should
reuse the document model and layout rather than introduce separate parsers.

Code blocks retain their language label, optional fence attributes, original source
span, and decoded text runs. Terminal rendering highlights one block at a time, retaining multiline
syntax state without storing token runs for the whole document. Plain/source
export, disabled highlighting, and disabled color bypass the highlighter.
Standalone code uses one block and the same syntax budgets; files above the
128 KiB block limit remain complete uncolored code. Bounded readers cap code,
diagram, and pager text at 64 MiB. Extension detection initializes no parsers,
queries, or graphics resources.
Dockerfile highlighting reuses the published grammar's highlight and injection
queries. One nonrecursive layer handles independent Bash command/`RUN` heredoc
regions and JSON/YAML/TOML `COPY` heredocs. All regions share the outer block's
capture and time budgets. Disjoint ranges keep the total embedded source length
within the outer block's length; each tree is released before the next region.
Sorted span overlays clear parent colors inside supported regions without a
per-byte color buffer. Other embedded languages retain the parent syntax colors.
Code line numbers are terminal decoration. The CLI enables them automatically
for standalone code; `--line-numbers on` includes Markdown blocks. The renderer
counts newlines across existing code runs without copying the block, then adds
numbers to each logical line's frame before wrapping. Continuations reserve the
same gutter without repeating the number; narrow frames omit it. Numbers do not
enter syntax queries, document events, source spans, or text/source exports.

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
Tall diagrams retain width-based fitting within the shared raster caps. The
picture painter reserves at most `terminal rows - 2` rows per band (minimum one),
preserving pixels, contiguous placement, quote rails, and a single list marker.
It charges the complete cell-padded picture before emitting any band, and only
the top of the whole picture receives vertical cell padding. The pager records
and crops these ordinary image placements through its existing bounded spool.
Timeouts/crashes disable further worker requests, while cached images and all
remaining document text remain available. [Mermaid support](mermaid.md) defines
coverage, cache identity, limits, worker cleanup, and measurements.

SGF uses the same code-block and picture-placement path. CLI defaults and optional
`game`/`move` fence attributes select one main-line position. Parsing, replay and
rasterization call the shared library in-process, off the input thread in pager
mode. A separate bounded source/pixel cache survives reloads, with per-document
work budgets and cooperative cancellation. [SGF support](sgf.md) defines position
semantics and limits. No board or raster is retained for every game node.

Text rendering consumes logical lines with one-line lookahead for quote context.
Wrapping borrows word fragments and allocates text for the resulting physical
lines, rather than allocating every grapheme. The writer shares ANSI state
between spans within a physical line and resets it before graphics and at the
line boundary. OSC 8 targets travel independently of text styling and close after
each painted span. Source, optional projections, parser working storage, large
individual blocks, and very long logical lines can still scale with input; this
is not a constant-memory document reader.

Math rasters use an 8 MiB/64-entry LRU cache. Keys include TeX, inline/display
mode, font size, color, bounds, and equation-numbering state. Shared pixels avoid
cache-hit copies; numbering is advanced exactly as on a miss. The pager retains
this cache and the bounded Mermaid/SGF caches between renders, but resets equation
numbering and all per-document work/output budgets. File-image loaders remain
fresh so manual reloads still reread changed assets.

Equations retain their layout dimensions and baselines, but transport their
original rasters without adding transparent cell padding. Cursor movement and
Kitty's within-cell vertical offset position native pixels; omitting the target
cell rectangle prevents scaling. The pager records that offset and clips against
viewport pixel boundaries. Ordinary image/diagram bands remain cell-aligned.
Kitty transport applies fast zlib compression when it saves bytes, then encodes
3,072-byte chunks into a 4,096-byte stack buffer. Transmissions still use
terminal-allocated IDs, preserving earlier scrollback. The optional pager
records that transport before it reaches stdout. It uploads visible images by
image number, retains them while visible, crops partial rows in source pixels,
and frees only its own images on eviction, reload, suspension, and exit.

The pager records only the internal renderer's escape vocabulary, after input
filtering. It is not a terminal emulator for arbitrary ANSI input. Completed rows
and encoded image payloads spill to anonymous temporary files; only row indexes,
image metadata, and a small editable row window stay resident. An optional block
observer associates rows with original source offsets. Reflow uses the same
layout and renderer as direct output; exports never read back this representation.

Rendering runs off the input thread. Completion wakes the terminal input wait
through a nonblocking socket pair. Requests coalesce into one pending slot,
and a cancelled render is discarded before publication. Cancellation reaches the
Mermaid supervisor, which kills and reaps an in-flight helper; every completed
render also releases its helper while retaining bounded cache entries. The last complete page
remains usable while a watched replacement renders. The terminal guard unwinds
before joining the render worker, including on errors. Idle watch mode polls file
metadata rather than rereading content or retaining renderer processes.
Viewport geometry is refreshed before each redraw as well as during idle polling,
so input immediately after a resize cannot redraw a page at the old width.
Reload requests retain the resolved input format across atomic saves. Text
reloads compare content; standalone image reloads rebuild after a metadata change
even though the source-model path is unchanged. Native image paths bypass URL
decoding while reusing the existing bounded file loader and codecs.
[Pager implementation](pager.md) defines the limits and interaction contract.

The [feature design](feature-design.md) records rendering decisions, performance
requirements, and shared-library boundaries.

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
  baseline. Equation pixels use native-size placements; image bands use padded
  cell rectangles. Space is reserved before drawing near the bottom of the terminal.
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
checks image/text placement. `tests/pager_smoke.py` uses a controlling PTY to test
navigation, reflow, image ownership/cropping, watch recovery, and signal cleanup.
These suites do not establish visual compatibility with
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
[Text wrapping measurements](text-performance.md) cover the ASCII fast path and
the version 2 benchmark runner's correction to short-process wall timing.
