# Changelog

## Unreleased

- Render LaTeX documents (`.tex`, `.latex`, `.ltx`, or `--input-format latex`)
  natively: sections, lists, theorems, tables, footnotes, references, and math
  with amsmath numbering and scoped document macros. Included files, listings,
  `.bbl` bibliographies, and figures without extensions are read in place, and
  `--watch` follows included files. Unknown commands stay visible as source. No
  TeX installation is needed.
- Reload documents with many equations much faster in the pager and with
  `--watch`: rendered equations are cached compressed, so up to 4,096 of them
  (previously 64) are reused within the same 8 MiB.
- Render HTML `<img>` markup like Markdown images, including inside `<p>`,
  `<div>`, `<a>`, and `<picture>`. Local images display; remote ones keep their
  reference and are never fetched. Other HTML still shows as source.

## 0.1.1 (2026-11-07)

- Publish each version as an immutable GitHub release with a source archive,
  checksums, and build provenance.
- Install with Homebrew from the `garlab/tap` tap. Bottles cover Apple Silicon on
  macOS 26 or newer and Linux x86-64.

Application behavior is unchanged.

## 0.1.0 (2026-11-07)

- Render Markdown with colors, readable margins, bounded line widths, tables,
  syntax highlighting, links, alerts, and details blocks.
- Display native math, Mermaid diagrams, local PNG/JPEG/SVG images, and SGF Go
  records through Kitty graphics, with source or text fallbacks.
- Open standalone code, diagrams, images, and Go records. Detect common code
  languages, `Cargo.lock`, Dockerfile, and Containerfile; show code line numbers.
- Read interactively with the Unix pager, literal search, resize reflow, and
  live file watching. Direct output remains the default.
- Export original source or plain text for copying and pipes. Bound expensive
  rendering work, image transport, caches, and worker lifetimes.

The Cargo package is `gloss-cli`; the executable is `gloss`. No browser,
JavaScript runtime, or external TeX installation is required.
