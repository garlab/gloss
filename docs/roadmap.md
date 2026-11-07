# Roadmap

This tracks remaining work. Current behavior and limits are in the
[README](../README.md) and [feature references](README.md#feature-references).
The [architecture](architecture.md) and [design decisions](feature-design.md)
define contracts that changes must preserve.

## Reproducible builds and distribution

- Complete the first hosted CI matrix and registry publication, then add the
  `gloss` formula to the separate Homebrew tap. The [release guide](releasing.md)
  covers the prepared workflows, Trusted Publishing, and package checks.
- Establish platform and terminal coverage before adding prebuilt binaries or
  broader distribution. Windows pager/watch support remains separate work.
- Keep shared libraries independently versioned and validate their API/resource
  changes in consumers. Release from a clean checkout with registry dependencies
  and no machine-local overrides.

## Rendering coverage

- Extend [syntax highlighting](syntax-highlighting.md) and embedded-language
  support only after checking grammar size, startup, query cost, and fallbacks.
- Broaden [local SVG/font support](images.md) and evaluate images inside tables.
- Expand [native Mermaid coverage](mermaid.md) where there is demand: additional
  scripts, timeline sections, and diagram families. [Gantt](gantt.md) follow-ups
  include timezones, other date formats, and advanced calendar options. An
  optional official-renderer adapter needs a separate runtime/resource evaluation.
- Add [SGF](sgf.md) move/variation navigation and evaluate legacy text encodings.
- Broaden HTML/details forms only with source-preservation checks; see
  [Markdown extensions](markdown-extensions.md).

## Performance

Continue reducing parser/layout memory beyond completed-block streaming.
Evaluate direct display-list rasterization in `katex-native`, repeated geometry
work in dense flowchart routing, and terminal-side image reuse in direct output.
Pager changes must retain bounded caches, cancellation, and idle resource use.

Measure absent, disabled, and enabled feature paths against the previous version
and [fixed baseline](performance-baseline.md). Track latency, first output, RSS,
binary/dependency size, helper processes, and encoded output separately; see the
[review thresholds](feature-design.md#resource-and-performance-requirements).
Keep generated inputs and local results outside the repository. Pair protocol
and pixel checks with live-terminal validation.

## Terminal integration

- Add bounded capability, theme/color, and geometry queries when input ownership
  permits. Preserve document stdin and navigation input, and retain fallbacks
  when queries fail. See [terminal discovery](terminal-detection.md).
- Validate inline baselines, image placement, scrolling, and selection in Ghostty
  and Kitty across fonts, zoom levels, and narrow windows. Record terminal
  versions and limitations.
- Add multiplexer passthrough or additional graphics protocols only with
  capability handling and placement checks for each route.

## Markdown and equation layout

- Refine mixed nesting of lists, quotes, code, tables, and display math.
- Broaden backslash-math contexts beyond the supported paragraph/table-cell
  spans, preserving escapes, code exclusions, and bounded source-range handling.
- Refine equation tags and oversized formulas while retaining source fallbacks
  and bounded raster allocation.
- Consider library-side raster limits and a reusable body/tag raster result in
  `katex-native` when shared semantics justify it. Keep terminal policy separate.

## Interactive viewing and copying

- Add heading navigation to the [pager](pager.md), and evaluate Windows terminal
  support separately. Preserve direct output and pipes.
- Add an in-view source toggle and copying of Markdown or TeX from the document
  model. Refine mappings where decoded entities prevent exact selection.
- Improve native mouse selection in Ghostty where possible; reading and exporting
  must remain useful without recovering text from image pixels.

## Deferred: native ABC music notation

ABC engraving does not block the initial release. Keep `abc` fences as source.
The [native ABC design](abc-native-design.md) preserves parser/renderer research,
standalone library boundaries, conformance strategy, and resource requirements.
Resume with a bounded native-library evaluation and licensed reference corpus
before choosing or forking a backend. JavaScript is not part of the runtime plan.
