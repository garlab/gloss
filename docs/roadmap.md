# Roadmap

This tracks remaining implementation work. Current behavior and limitations are
documented in the [README](../README.md); [architecture](architecture.md) records
the boundaries and output contracts that should remain intact.
[Feature design](feature-design.md) records the agreed rendering sequence and
resource requirements; [performance baseline](performance-baseline.md) provides
the fixed comparison data and reproduction tools.

## Rich Markdown, in order

1. Syntax highlighting is implemented for the initial language set, with retained
   fence languages, lazy queries, bounded work, and complete plain-code fallback.
   Evaluate additional grammars against binary size, startup, and query cost.
2. Table cells, alignment, wrapping, inline math, and numbered-cell fallbacks are
   implemented with bounded measurement and temporary row storage. See
   [table rendering](tables.md) for the limits and measurements.
3. Local PNG/JPEG/SVG images are implemented through shared raster placement and
   transport, with selectable captions, bounded decoding/cache/output, and text
   fallback. See [local images](images.md) for the SVG subset and measurements.
   Broader SVG/font support and images inside tables remain follow-up work.
4. Add optional Mermaid rendering through that image path. Select a backend
   after measuring startup, process memory, cache behavior, and compatibility.

Alongside these steps, add alerts, bounded front matter handling, expanded
details/summary sections, and validated clickable links. Preserve source-based
exports and benchmark absent, disabled, and enabled feature paths. Large inputs
and local measurement outputs stay outside the repository.

Track binary/dependency growth, helper processes, and encoded output as well as
latency and RSS. [Rendering optimizations](performance-optimizations.md) remove
retained event copies and reduce text-rendering allocations. Continue reducing
parser/layout memory and evaluate math image reuse/compression; current
measurements are not resource ceilings.

## Reproducible builds and distribution

- Make `katex-native` publicly available and replace the sibling path dependency
  with a versioned package or pinned public revision.
- Once that dependency is available, add clean-checkout CI for formatting, Clippy,
  Rust tests, and Unix pseudo-terminal tests. Verify the declared minimum Rust
  version against the locked dependencies.
- Establish a tested platform matrix before adding release binaries or package
  distribution. Keep crate publication disabled until dependencies are
  independently obtainable.

A public checkout should build without private files or machine-specific
configuration before distribution is enabled.

## Terminal integration

- Add bounded capability queries and explicit override behavior while preserving
  document stdin and user input. Retain text fallbacks when queries fail.
- Query theme colors and improve geometry discovery while preserving manual
  controls and sensible behavior when a terminal does not answer.
- Validate inline baselines, image placement, scrolling, and selection in Ghostty
  and Kitty across fonts, zoom levels, and narrow windows. Record tested versions
  and any terminal-specific limitations.
- Add multiplexer passthrough and additional graphics protocols only with
  capability handling and placement tests for each supported route.

## Markdown and equation layout

- Refine mixed nesting of lists, quotes, code, and display math. Improve tables
  and code layout through the ordered work above.
- Add backslash-parenthesis and backslash-bracket math delimiters with coverage
  for escaping, code spans, and code fences.
- Refine equation tag placement and the treatment of oversized formulas.
  Preserve individual source fallbacks and bounded raster allocation.
- Consider library-side raster limits and a reusable body/tag raster result in
  `katex-native` when shared semantics justify extraction. Keep general images,
  highlighting, terminal fitting, and placement policy in their own layers.

## Interactive viewing and copying

- Introduce an optional pager with navigation, resize-driven reflow, and explicit
  image lifecycle management. Add search and heading navigation; evaluate file
  watching separately. Keep direct output and pipe behavior available.
- Add an in-view source toggle and copying of Markdown or TeX from the document
  model. Refine source mappings where decoded entities prevent exact selection.
- Evaluate improvements to native mouse selection in Ghostty without making
  image-to-text recovery a requirement for reading or exporting documents.
