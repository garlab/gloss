# Roadmap

This tracks remaining implementation work. Current behavior and limitations are
documented in the [README](../README.md); [architecture](architecture.md) records
the boundaries and output contracts that should remain intact.

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

- Refine mixed nesting of lists, quotes, code, and display math. Add table column
  sizing and code-block syntax highlighting.
- Add backslash-parenthesis and backslash-bracket math delimiters with coverage
  for escaping, code spans, and code fences.
- Refine equation tag placement and the treatment of oversized formulas.
  Preserve individual source fallbacks and bounded raster allocation.

## Interactive viewing and copying

- Introduce an optional pager with navigation, resize-driven reflow, and explicit
  image lifecycle management. Keep direct output and pipe behavior available.
- Add an in-view source toggle and copying of Markdown or TeX from the document
  model. Refine source mappings where decoded entities prevent exact selection.
- Evaluate improvements to native mouse selection in Ghostty without making
  image-to-text recovery a requirement for reading or exporting documents.
