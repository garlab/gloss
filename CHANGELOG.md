# Changelog

## Unreleased

- Keep vertical bars inside Markdown table formulas, including absolute values
  and conditional probabilities. Following text and cells stay visible.
- Render unbraced square roots such as `\sqrt x` with the same spacing as
  `\sqrt{x}`.

## 0.2.3 (2026-11-22)

- Hide empty HTML anchor targets such as `<a id="section"></a>` and
  `<a name="section"></a>` in rendered Markdown and text exports.
- Highlight Probl files and fenced code blocks. Use `--language probl` to
  select the language explicitly.
- Render supported LaTeX TikZ pictures in Kitty and Ghostty on macOS.
  Use `--tikz off` to show their source.
- Show the typeset title from `\texorpdfstring` in LaTeX headings, including
  inline math.

## 0.2.2 (2026-11-16)

- Highlight Terraform/HCL files and code fences, including interpolated strings,
  heredocs, template directives, comments, and expressions. Detect `.tf`,
  `.tfvars`, `.hcl`, and `.tfbackend` files automatically.

## 0.2.1 (2026-11-11)

- Render common README HTML like the equivalent Markdown: `<h1>` to `<h6>`
  headings, `<b>`/`<strong>`, `<i>`/`<em>`, `<s>`/`<del>`, `<code>`, and `<hr>`,
  alongside the existing `<p>`, `<div>`, `<a>`, `<br>`, and `<img>`. A centered
  title and tagline no longer show as source. Other HTML still does.

## 0.2.0 (2026-11-09)

- Render LaTeX documents (`.tex`, `.latex`, `.ltx`, or `--input-format latex`)
  natively: sections, lists, theorems, tables, footnotes, references, and math
  with amsmath numbering and scoped document macros. Included files, listings,
  `.bbl` bibliographies, and figures without extensions are read in place, and
  `--watch` follows included files. Unknown commands stay visible as source. No
  TeX installation is needed.
- Show the pager's first screen as soon as it is rendered, before the rest of a
  long document; the footer marks the row count with `+` until it completes.
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
