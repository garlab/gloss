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
   PNG/JPEG decoding now shares `document-graphics` through its optional `images`
   feature; native codecs and the broader SVG worker are not enabled.
   Broader SVG/font support and images inside tables remain follow-up work.
4. An initial native Mermaid subset is implemented with a lazy reusable worker,
   bounded requests/cache/output, and complete code fallback. See
   [Mermaid support](mermaid.md) for coverage and measured backend costs.
   Latin/Greek/Cyrillic labels, ER attributes and five additional diagram families
   are implemented in the shared library. Broader script coverage, timeline
   sections and other families need compatibility and resource checks. Consider an optional official-renderer
   adapter when full Mermaid.js compatibility justifies its runtime cost.

Alerts, inert bounded front matter, an expanded details/summary subset, and
validated clickable links are implemented. See [Markdown extensions](markdown-extensions.md)
for coverage and limits; broaden HTML/detail forms only with source-preservation
checks. Preserve source-based
exports and benchmark absent, disabled, and enabled feature paths. Large inputs
and local measurement outputs stay outside the repository.

Track binary/dependency growth, helper processes, and encoded output as well as
latency and RSS. [Rendering optimizations](performance-optimizations.md) remove
retained event copies and reduce text-rendering allocations. Continue reducing
parser/layout memory beyond the CLI's completed-block streaming path. Bounded
math-raster caching and compressed graphics transport are implemented; evaluate
terminal-side image reuse separately. Current measurements are not resource ceilings.

ASCII wrapping and the benchmark's empty-selector timing fix are implemented;
see [text performance](text-performance.md) for measurements and validation.
Tall Mermaid diagrams now use readable width-based fitting and contiguous bands;
see [Mermaid support](mermaid.md). Routing optimizations ship through published
`document-graphics` 0.1.3 and its Mermaid backend 0.1.3.
Math transport now avoids added transparent cell padding while retaining native
pixels, baselines, and pager clipping. Pager reloads reuse bounded math/Mermaid
caches, and cancellation reaches the diagram worker; see [pager details](pager.md).
Further experiments include direct display-list rasterization in `katex-native`
and reducing repeated geometry work in dense flowchart routing. Validate live
Ghostty/Kitty placement alongside the protocol and pixel-equivalence checks.

## Deferred: native ABC music notation

ABC engraving is parked and does not block the initial release. Keep `abc`
fences as source. [Native ABC design](abc-native-design.md) records native parser
and renderer candidates, a possible standalone library, conformance fixtures,
and performance/resource requirements. Resume with a bounded native-library
evaluation and licensed reference corpus before selecting or forking a backend.
JavaScript is not part of the runtime plan.

## Reproducible builds and distribution

- `katex-native` and `document-graphics` 0.1.5 come from crates.io; normal builds
  need no local overrides. Shared native rendering lives in the standalone
  `document-graphics` library; keep its API and resource checks covered by both
  terminal and GUI integration tests.
- Add clean-checkout CI for formatting, Clippy,
  Rust tests, and Unix pseudo-terminal tests. Verify the declared minimum Rust
  version against the locked dependencies.
- Establish a tested platform matrix before adding release binaries or package
  distribution. The CLI remains unpublished; library releases are versioned
  independently.

A public checkout should build without private files or machine-specific
configuration before distribution is enabled.

## Terminal integration

- Add bounded capability queries and explicit override behavior while preserving
  document stdin and user input. Retain text fallbacks when queries fail.
- Add actual theme/color and geometry queries when input ownership permits.
  Environment theme hints, reported/environment geometry, `--theme`, and
  `--cell-size` overrides are implemented; see [terminal discovery](terminal-detection.md).
- Validate inline baselines, image placement, scrolling, and selection in Ghostty
  and Kitty across fonts, zoom levels, and narrow windows. Record tested versions
  and any terminal-specific limitations.
- Add multiplexer passthrough and additional graphics protocols only with
  capability handling and placement tests for each supported route.

## Markdown and equation layout

- Refine mixed nesting of lists, quotes, code, and display math. Improve tables
  and code layout through the ordered work above.
- Backslash-parenthesis and backslash-bracket math delimiters are implemented
  within bounded paragraph/table-cell spans, with escaping and code exclusions.
  Broaden supported contexts only with source-range and resource checks.
- Refine equation tag placement and the treatment of oversized formulas.
  Preserve individual source fallbacks and bounded raster allocation.
- Consider library-side raster limits and a reusable body/tag raster result in
  `katex-native` when shared semantics justify extraction. Keep general images,
  highlighting, terminal fitting, and placement policy in their own layers.

## Interactive viewing and copying

- The optional Unix pager implements navigation, literal search, resize reflow,
  bounded image reuse, and debounced file watching. See [pager implementation](pager.md).
  Add heading navigation and evaluate Windows terminal support separately.
  Keep direct output and pipe behavior available.
- Add an in-view source toggle and copying of Markdown or TeX from the document
  model. Refine source mappings where decoded entities prevent exact selection.
- Evaluate improvements to native mouse selection in Ghostty without making
  image-to-text recovery a requirement for reading or exporting documents.
