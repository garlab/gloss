# Rich Markdown rendering

This document records the implementation direction. Features described here are
planned unless the [README](../README.md) says otherwise. The
[architecture](architecture.md) defines existing contracts; the
[performance baseline](performance-baseline.md) supplies the initial measurements.

## Priorities

| Order | Feature | Useful first version |
| --- | --- | --- |
| 1 | Code syntax highlighting | Language-aware fenced blocks, dark/light token colors, complete uncolored fallback. |
| 2 | Tables | Retained cell structure, column alignment, and wrapping within the reading width. |
| 3 | Images | Bounded local PNG, JPEG, and SVG decoding through a shared image placement path. |
| 4 | Mermaid | An optional renderer producing images through that same path, with a source fallback. |

Task lists, strikethrough, basic footnotes, and native TeX math already work.
GitHub alerts, inert front matter, an expanded details subset, validated terminal
links, and backslash math delimiters are implemented; see
[Markdown extensions](markdown-extensions.md) for current coverage. Opt-in paging,
literal text search, resize reflow, and file watching are implemented separately
from direct output; see [pager design and limits](pager.md). Heading navigation
remains a follow-up.

Every step must retain fast ordinary-document rendering, readable output on
terminals without graphics, and source-based copying and export. The core viewer
must remain usable without a browser or optional renderer installation.

## Syntax highlighting

The first implementation uses a focused Tree-sitter adapter in `src/highlight/`.
It reuses semantic capture mapping and language/query conventions, with a new
bounded interval algorithm in place of per-byte color storage. There is no
editor dependency or extra checkout. Native grammar support and cooperative
parse/query cancellation favored this choice over introducing a second syntax
engine. [Implementation notes and measurements](syntax-highlighting.md) record
the selected language set, limits, and dependency cost.

Preserve fenced-block language labels and source ranges in the document/layout
model. Highlight the logical code block before wrapping so multiline strings and
comments keep their state. Map semantic token kinds to the existing ANSI theme;
the parsing layer must not produce terminal escapes or GUI colors.

A portable Tree-sitter core is a promising reuse option: language lookup, cached
queries, cancellable parsing, and semantic byte-range output can be isolated
from an editor. A lightweight lexical fallback is also useful, but a line-local
lexer must be called per line and must not be presented as a full grammar parser.
[Syntect](https://docs.rs/syntect/latest/syntect/highlighting/index.html) remains
an alternative if future measurements or language coverage justify reconsidering
the engine; no direct Syntect benchmark has been performed.

Evaluate startup, first highlighted output, peak RSS, binary size, grammar
coverage, minimum Rust version, and license notices. Start with commonly used
languages and explicit aliases; avoid loading every grammar or compiling every
query on startup. If the core needs a shared crate, give it an independent
manifest and focused API. Do not introduce an editor framework or an additional
private checkout requirement.

The highlighter needs bounds on source size, captures, query work, and cached
state. Parse cancellation alone does not bound highlight queries. Avoid
allocating a token slot for every input byte or retaining every capture for an
unbounded block. Validate spans against UTF-8 boundaries and preserve the
renderer’s control-character filtering.

Unknown languages, invalid input, and exhausted budgets retain all code as
ordinary text. Plain/source export and disabled color bypass highlighting.
Documents without code must not initialize highlighting resources. Language
labels and line numbers are later presentation options, independent of token
parsing.

## Tables

The initial implementation is in `src/layout/table.rs` and `src/render/table.rs`.
[Table rendering](tables.md) records the width, row, and raster limits,
numbered-cell fallback, export compatibility, and measurements.

Retain rows, cells, and alignment markers instead of flattening them into text.
Measure terminal-cell widths using the existing Unicode and inline-math rules.
Choose columns within the reading width and wrap cell contents without splitting
graphemes or images.

Bound the rows/cells examined for width estimation and the working memory needed
to lay out one table. A narrow terminal or excessive column count should use a
readable row-based fallback. All cell content must survive the fallback; a
measurement budget must not silently truncate the document.

Plain export should remain readable and independent of viewport width. Define
its table representation with tests before changing current output.

## Shared images

The initial implementation uses `src/raster.rs` for dimensions and straight RGBA
pixels, `src/images.rs` for bounded local decoding, and the existing line painter
and Kitty transport for placement. Math retains its baseline and equation
metadata separately. [Local images](images.md) records the supported SVG subset,
resource limits, cache behavior, and measurements.

Start with local PNG, JPEG, and SVG references resolved relative to the Markdown
file. Preserve alt text and source references when images cannot be displayed.
Define the base directory explicitly for stdin. Remote retrieval is a separate,
opt-in feature; rendering a document must not imply fetching every URL.

Check encoded byte size and dimensions before decoding. Bound decoded pixels,
intermediate allocations, SVG complexity/work, and cache bytes. Reject nested or
external SVG resource loading. The initial implementation lazily registers a
small set of already bundled fonts for SVG text; it does not scan system fonts.
Any broader font discovery must remain lazy and be measured separately.

Cache keys must describe content, size, and any theme/font inputs that affect
pixels. Persistent caches also need the renderer version; the current cache is
invocation-local, with fixed fonts and no recoloring. Use byte-accounted
eviction, reject entries larger than the cache budget, and avoid simultaneous copies of raw, padded, and encoded
pixels where possible. Pipes, plain export, source export, and text-only
fallbacks must not decode images.

## Mermaid

The initial implementation uses pinned `mermaid-rs-renderer` with its CLI/PNG
features disabled, running in one lazy worker from the mdviewer executable.
[Mermaid support](mermaid.md) records the selected subset, compatibility limits,
bounded worker/cache behavior, and native/official renderer measurements.
Broader dialect support remains future work.

Recognize Mermaid fences before generic code rendering. Rendered output uses the
shared image path; unsupported terminals, missing renderers, errors, and limits
fall back to the complete fenced source.

The official
[Mermaid CLI](https://github.com/mermaid-js/mermaid-cli) produces SVG/PNG using a
Node.js and Puppeteer/browser stack. An optional adapter remains a compatibility
option; the native implementation avoids that installation and process cost for
the supported subset. Recheck startup, process memory, installation size, and
compatibility before adding another backend.

If using a helper, start it only for a cache miss requiring a diagram. Bound
execution time and output size, process diagrams with one worker initially,
reuse the worker within an invocation where supported, and terminate it when
done. Do not create a persistent background service by default. Cancellation
must clean up descendants and temporary files.

Cache by diagram source, renderer version, configuration, and theme. Measure
cold startup, repeated diagrams, and cache hits separately. Report the complete
helper process tree's memory alongside process-reported RSS. The benchmark
runner's optional `--process-tree` sampler records concurrent descendant RSS;
run timing separately because process sampling adds overhead and can miss brief
allocation peaks.

## ABC music notation (deferred)

Native engraving of `abc` fences is a future feature, independent of the initial
release. [Native ABC design](abc-native-design.md) records the parser/layout
reuse candidates, standalone library boundaries, reference-suite strategy, and
resource gates. Evaluate existing Rust rendering before choosing a parser fork
and new engraver. ABC remains ordinary fenced source until that work resumes;
the runtime design excludes JavaScript.

## Other Markdown extensions

- **Alerts:** recognize the common
  [GitHub alert forms](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#alerts)
  as styled quotes with textual labels that also work without color.
- **Front matter:** recognize leading metadata, starting with the
  [YAML convention](https://jekyllrb.com/docs/step-by-step/03-front-matter/).
  Keep it distinguishable from a thematic break. Use bounded parsing with inert
  tags/aliases and source fallback for malformed or oversized input; do not
  execute metadata or silently discard it.
- **Details:** display
  [details/summary sections](https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/organizing-information-with-collapsed-sections)
  expanded initially. Folding requires an interactive viewer and should not hide
  content in direct terminal output or exports.
- **Math delimiters:** add `\(...\)` and `\[...\]` with explicit escaping rules;
  never recognize them inside code spans or fences.
- **Links:** consider clickable terminal labels using a validated URI policy.
  Preserve readable targets in text export and reject terminal control injection.

This remains a Markdown viewer, not an HTML browser. Extensions should preserve
source and degrade to readable text without executing document content.

## Resource and performance requirements

Treat wall time, time to first output, peak RSS, encoded output volume, dependency
footprint, and helper processes as separate costs. No single speed comparison
with another viewer is a sufficient acceptance criterion.

For every feature:

1. Re-run the relevant baseline cases on the same machine and build profile.
   Include a document where the feature is absent, a feature-disabled run, and
   feature-heavy and pathological inputs. Keep large generated inputs external.
2. On unchanged rendering paths, investigate a repeatable wall-time/first-output
   increase above the larger of 10% or 5 ms, or peak-RSS increase above the larger
   of 10% or 8 MiB. These are initial review triggers, not portable CI thresholds
   or permission to spend the allowance on every change. Compare against both
   the previous accepted version and the fixed initial baseline.
3. Report feature-enabled cost separately. Re-run suspicious results with more
   samples; three measurements are a reference, not a statistical guarantee.
   Add a focused benchmark before accepting a new expensive renderer.
4. Set explicit per-item work/allocation limits and per-invocation cache limits
   before enabling each feature. Calibrate numeric limits with measurements;
   document them with the implementation. Entry counts alone do not bound memory.
5. Make optional resources lazy and release temporary allocations promptly.
   Budget exhaustion removes decoration, never source content. Verify disabled
   paths do not launch helpers or initialize decoders/highlighters.

The initial 10 MiB mixed document reaches about 332 MiB peak RSS; 5,000 equations
produce about 184 MiB of graphics output. These are optimization targets, not
acceptable resource ceilings. The CLI now streams completed blocks, caches bounded math rasters, and compresses
beneficial graphics payloads. The [initial rendering optimizations](performance-optimizations.md)
remain a historical reference. Source, optional extension projections, parser
working storage, and whole code/table blocks still scale with input. Continue
evaluating those costs and terminal-side duplicate image reuse against memory,
latency, scrollback behavior, and output volume.

The PTY baseline excludes terminal image decoding, GPU work, and scrollback.
Graphics changes also need real-terminal checks for scrolling, selection,
responsiveness, and terminal-process memory. Keep renderer and emulator
measurements distinct.

## Dependency boundaries

Keep highlighting, Markdown parsing, front matter, general image loading, Mermaid,
terminal capability handling, ANSI styles, and Kitty transport outside
`katex-native`.

Two math-specific improvements are reasonable shared-library candidates:

- Raster dimension/allocation limits enforced before expensive work, with clear
  errors. Caller-side fitting remains useful, but every consumer benefits from
  library-side bounds.
- A body/tag raster result helper that preserves each baseline and avoids
  duplicated rasterization plumbing. Return the pieces without deciding terminal
  margins or tag placement.

Document equation-numbering state, terminal fitting, placement choices, and cache
budgets stay with the viewer. Move a helper only when its semantics are shared
and the library can test it independently. General SVG loading does not belong
in the math library simply because both paths use a rasterizer.
