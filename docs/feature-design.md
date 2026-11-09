# Rich Markdown rendering

This document records rendering decisions and requirements for future changes.
The [README](../README.md) describes supported behavior; the
[architecture](architecture.md) defines module boundaries and output contracts.
The [roadmap](roadmap.md) lists remaining work.

## Rendering choices

| Area | Decision | Implementation reference |
| --- | --- | --- |
| Syntax highlighting | Native grammars, semantic token spans, complete uncolored fallback. | [Highlighting](syntax-highlighting.md) |
| Tables | Retain cells and alignment; wrap within the reading width with bounded measurement. | [Tables](tables.md) |
| Images | Decode local PNG/JPEG/SVG with explicit limits and shared raster placement. | [Images](images.md) |
| Mermaid | Use a native worker for the supported subset; retain complete source on failure. | [Mermaid](mermaid.md) |

Changes must retain fast ordinary-document rendering, readable output on
terminals without graphics, and source-based copying and export. The core viewer
must remain usable without a browser or optional renderer installation.

## Syntax highlighting

Tree-sitter supplies native grammars and cooperative parse/query cancellation.
The adapter in `src/highlight/` maps semantic captures to bounded byte ranges;
it does not allocate a color slot for every input byte. Preserve fence language
labels and source ranges, and highlight before wrapping so multiline strings and
comments keep their state. ANSI styling belongs in the terminal renderer.

Bound source size, captures, query work, and cached state. Parse cancellation
alone does not bound highlight queries. Validate spans against UTF-8 boundaries
and preserve control-character filtering. Unknown languages, invalid input, and
exhausted budgets retain all code as ordinary text. Plain/source export,
disabled color, and documents without code must not initialize highlighting.

Evaluate additional grammars and embedded languages against startup, first
output, peak RSS, binary size, minimum Rust version, and licensing. Keep grammar
and query initialization lazy. Extract a shared core only when its language
lookup, parsing, and semantic-span contracts are useful independently of the
terminal application. Code line numbers remain renderer-only decoration.

## Tables

Retain rows, cells, and alignment markers. Measure terminal-cell widths using
the existing Unicode and inline-math rules, and wrap without splitting graphemes
or images. Bound width estimation and the temporary storage used for one table.

Narrow terminals and excessive column counts use a readable row-based fallback.
All content must survive; a measurement budget must not truncate cells. Plain
export remains independent of viewport width. Define any change to its table
representation with compatibility tests.

## Shared images

`document-graphics` owns bounded PNG/JPEG/SVG decoding. Gloss owns local-file
policy, fitting, invocation budgets, and caches. Math retains its baseline and
equation metadata separately while sharing image placement and transport.

Resolve local references relative to the document, with an explicit base for
stdin. Preserve alt text and source references on fallback. Remote retrieval
would need a separate opt-in design; rendering must not implicitly fetch URLs.

Check encoded bytes and dimensions before decoding. Bound decoded pixels,
intermediate allocations, SVG complexity, and cache bytes. Reject nested or
external SVG resources. Bundled SVG fonts load lazily; broader font support
needs its own startup, footprint, and compatibility measurements.

Cache identity must include content and fitting bounds, plus any theme/font
inputs that affect pixels. Persistent caches would also need renderer versions.
Use byte-accounted eviction and avoid simultaneous raw, padded, and encoded
copies. Pipes and text/source exports must not decode images.

## Mermaid

`document-graphics` uses a pinned `document-graphics-mermaid` backend with its
CLI/PNG features disabled. One lazy child of the Gloss executable handles native
rendering. Recognize Mermaid before generic code highlighting and reuse the
shared image path. Unsupported syntax, terminals, errors, and resource limits
retain complete source.

The official [Mermaid CLI](https://github.com/mermaid-js/mermaid-cli) is a possible
optional adapter for broader compatibility. Its Node.js/browser runtime would
need separate startup, process-memory, installation-size, and compatibility
measurements before adoption. No such adapter is required by the native path.

Start a worker only on an eligible cache miss. Bound execution time, request and
response sizes, and aggregate work. Reuse it within a render and release it when
done; cancellation must kill and reap in-flight work. Do not add a persistent
background service by default.

Cache keys include source, fitting bounds, scale, and appearance. Measure cold
startup, repeated diagrams, and cache hits separately. Report process-tree RSS
alongside process-reported RSS. Run the benchmark's optional `--process-tree`
sampler separately from timing: sampling adds overhead and can miss brief peaks.

## LaTeX documents

LaTeX is read natively, never converted to Markdown or another markup that is
parsed again, and without a TeX engine: previews must be fast on large
documents and need no TeX installation. `latex-reader` owns reading, numbering,
references, and macro scoping; it has no file access or KaTeX dependency.
gloss owns the file policy for included files and figures, presentation, math
rendering and caches, and watching. Changes to how LaTeX is read belong in the
crate, with tests there; gloss tests the presentation and integration.

## ABC music notation (deferred)

Native engraving of `abc` fences is independent of the initial release.
[Native ABC design](abc-native-design.md) records parser/layout candidates,
standalone library boundaries, reference suites, and resource requirements.
Evaluate existing native rendering before choosing a parser fork and new
engraver. ABC remains ordinary fenced source; the runtime plan excludes
JavaScript.

## Other Markdown extensions

[Markdown extensions](markdown-extensions.md) documents the implemented alerts,
front matter, expanded details, clickable links, and backslash math delimiters.
Preserve these contracts when broadening coverage:

- Keep alert meaning readable without color.
- Treat front matter as inert source, distinguish it from thematic breaks, and
  retain malformed or oversized content. Never execute metadata or discard it.
- Keep details content visible in direct output and exports. Interactive folding
  needs a separate presentation design.
- Preserve math source ranges and escaping; do not recognize delimiters in code.
- Validate hyperlink targets, retain readable text exports, and reject terminal
  control injection.

HTML extensions must preserve source and readable fallbacks without executing
content.

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

The [fixed baseline](performance-baseline.md) records about 332 MiB peak RSS for
10 MiB of mixed Markdown and about 184 MiB of output for 5,000 equations before
later optimizations. These observations are not resource ceilings. Completed-block
streaming, bounded raster caches, and compression reduce costs, but source,
parser storage, and whole code/table blocks still scale with input.

PTY measurements exclude emulator image decoding, GPU work, and scrollback.
Graphics changes also need real-terminal checks for responsiveness, selection,
scrolling, and terminal-process memory. Keep renderer and emulator measurements
separate.

## Dependency boundaries

Keep highlighting, Markdown parsing, front matter, general image loading, Mermaid,
terminal capability handling, ANSI styles, and Kitty transport outside
`katex-native`.

Two math-specific improvements remain shared-library candidates:

- Raster dimension/allocation limits enforced before expensive work, with clear
  errors. Caller-side fitting remains useful alongside library-side bounds.
- A body/tag raster helper that preserves each baseline without deciding terminal
  margins or tag placement.

Equation numbering, terminal fitting, placement, and cache budgets stay with the
viewer. Move a helper only when its semantics are shared and independently
testable. General SVG loading does not belong in the math library simply because
both paths use a rasterizer.
